import { describe, it, expect } from 'vitest'
import {
  DECODE_FILTERS,
  DecodeHistory,
  fmtUtc,
  MAX_HISTORY,
  MAX_ROWS,
  orderEntries,
  passesFilter,
  periodStartMs,
  renderWindow,
  rowTimeMs,
  RX_TOL_HZ,
  TIER_PERIOD_SECS,
} from './decodeHistory'
import type { DecodeRow, LateDecodes } from './types'

function row(p: Partial<DecodeRow> & Pick<DecodeRow, 'message' | 'freqHz'>): DecodeRow {
  return {
    from: p.from ?? 'W9XYZ',
    snr: p.snr ?? -10,
    dtSec: p.dtSec ?? 0.1,
    isCq: p.isCq ?? false,
    directedToMe: p.directedToMe ?? false,
    worked: p.worked ?? false,
    tier: p.tier ?? 'FT8',
    rv: p.rv ?? 0,
    ...p,
  }
}

describe('decode history — WSJT-X chronological flow', () => {
  it('renders oldest-first: earlier periods sort above later ones', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })], 100, 1_000)
    h.ingest([row({ message: 'CQ K2DEF FN20', freqHz: 800 })], 101, 16_000)
    const list = orderEntries(h.entries(), 'time')
    expect(list.map((d) => d.slot)).toEqual([100, 101])
    expect(list[0].message).toBe('CQ W9XYZ EN52') // oldest at the top
    expect(list[list.length - 1].message).toBe('CQ K2DEF FN20') // newest at the bottom
  })

  it('dedupes snapshot re-polls within a period, but appends a NEW row when the same station is re-heard next period', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    const cq = row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })
    h.ingest([cq], 100, 1_000)
    h.ingest([cq], 100, 2_000) // same period re-poll → still one row
    expect(h.entries()).toHaveLength(1)
    expect(h.entries()[0].at).toBe(1_000) // first-heard timestamp kept
    h.ingest([cq], 101, 16_000) // next period → a second line (WSJT-X)
    expect(h.entries()).toHaveLength(2)
  })

  it('own-TX rows key per transmit cycle (txAt), one row per call', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    const tx = row({ message: 'W9XYZ KD9TAW EN52', freqHz: 1500, mine: true, txAt: 60 })
    h.ingest([tx], 100, 61_000)
    h.ingest([tx], 100, 62_000) // re-emitted across polls → one row
    expect(h.entries()).toHaveLength(1)
    h.ingest([{ ...tx, txAt: 90 }], 102, 91_000) // next cycle → new row
    expect(h.entries()).toHaveLength(2)
  })

  it('an evicted own-TX row re-served later pins to ITS period, not the live one (#15)', () => {
    // The engine re-serves the whole own-TX ring every poll; on a busy band the
    // 300-row cap evicts the oldest own-TX rows, and the next poll re-added them
    // with the CURRENT ingest slot while keeping the original clock time — old
    // CQ calls rendering under the live period separator (m7jyfradio's PD2BS
    // calls). With no surviving entry, the slot now derives from the row's own
    // transmit time.
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    const tx = row({ message: 'PD2BS KD9TAW EN52', freqHz: 1500, mine: true, txAt: 60 })
    h.ingest([tx], 100, 61_000)
    expect(h.entries()[0].slot).toBe(100)
    // Fill past the cap so the own-TX row is evicted (insertion-oldest first). Driven off
    // MAX_HISTORY, not a hardcoded 320: the store was resized once (300 → 3000) and a literal
    // here silently stopped evicting, which turned this test green while testing nothing.
    for (let i = 0; i < MAX_HISTORY + 20; i++) {
      h.ingest([row({ message: `CQ T${i}ABC EN${i % 90} X`, freqHz: 400 + i * 5 })], 101, 75_000)
    }
    expect(h.entries().some((d) => d.mine)).toBe(false) // premise: evicted
    // Twenty periods later the engine's ring re-serves it: same txAt, live slot 120.
    h.ingest([tx], 120, 361_000)
    const mine = h.entries().find((d) => d.mine)!
    expect(mine.at).toBe(60_000) // original transmit time kept
    expect(mine.slot).toBe(100) // …and the PERIOD it names, never 120
  })
})

describe('an own transmission sorts after the decodes it answered', () => {
  // FT8 period P = slot 100 starts at 1,500,000 ms. Decodes ingested at slot P were HEARD in
  // P-1; the over sent in P answers them and must list below them, however the decode's arrival
  // time and the over's whole-second transmit stamp happen to fall.
  const P = 100
  const START = P * 15_000
  const rr73 = row({ message: 'KD9TAW W9XYZ RR73', freqHz: 1200, directedToMe: true })
  const my73 = row({ message: 'W9XYZ KD9TAW 73', freqHz: 1500, mine: true, txAt: START / 1000 })
  const order = (h: DecodeHistory) => orderEntries(h.entries(), 'time').map((d) => d.message)

  it('a decode that arrives after the over started still lists above it', () => {
    // The RR73 lands 1.8 s into P (a later decode pass); the 73 went out at P+0.
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([rr73], P, START + 1_800)
    h.ingest([rr73, my73], P, START + 2_000)
    expect(order(h)).toEqual(['KD9TAW W9XYZ RR73', 'W9XYZ KD9TAW 73'])
  })

  it('the whole-second transmit stamp cannot pull the over above the decode', () => {
    // RR73 at P+1.3 s, 73 released at P+1.6 s but stamped P+1 s (the engine's Unix seconds).
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([rr73], P, START + 1_300)
    h.ingest([rr73, { ...my73, txAt: START / 1000 + 1 }], P, START + 1_700)
    expect(order(h)).toEqual(['KD9TAW W9XYZ RR73', 'W9XYZ KD9TAW 73'])
  })

  it('the next period’s decodes still list below the over (boundary)', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    // The 73 is stamped after the RR73 arrived, so this holds on arrival order alone.
    const late73 = { ...my73, txAt: START / 1000 + 1 }
    h.ingest([rr73], P, START + 400)
    h.ingest([rr73, late73], P, START + 1_200)
    const next = row({ message: 'CQ K2DEF FN20', freqHz: 800, isCq: true })
    h.ingest([next, late73], P + 1, START + 15_400) // heard during P, arrives in P+1
    expect(order(h)).toEqual(['KD9TAW W9XYZ RR73', 'W9XYZ KD9TAW 73', 'CQ K2DEF FN20'])
  })

  it('within one side, rows keep time order: decodes first-heard, overs by transmit time', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    const cq = row({ message: 'CQ K2DEF FN20', freqHz: 800, isCq: true })
    const second = { ...my73, message: 'W9XYZ KD9TAW RR73', txAt: START / 1000 + 5 }
    h.ingest([second, my73], P, START + 5_500)
    h.ingest([second, my73, rr73], P, START + 6_000)
    h.ingest([second, my73, rr73, cq], P, START + 7_000)
    expect(order(h)).toEqual(['KD9TAW W9XYZ RR73', 'CQ K2DEF FN20', 'W9XYZ KD9TAW 73', 'W9XYZ KD9TAW RR73'])
  })

  it('only the time order changes: SNR order still ranks by SNR', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([{ ...rr73, snr: -20 }, { ...my73, snr: 0 }], P, START + 2_000)
    expect(orderEntries(h.entries(), 'snr').map((d) => d.message)).toEqual(['W9XYZ KD9TAW 73', 'KD9TAW W9XYZ RR73'])
  })
})

describe('Rx Frequency filter (rx)', () => {
  it('passes a directedToMe decode at a FAR audio offset (the missed-caller fix)', () => {
    const d = row({ message: 'KD9TAW JA1ABC -15', freqHz: 2400, directedToMe: true })
    expect(passesFilter(d, 'rx', 500)).toBe(true)
  })

  it('passes own TX and near-offset decodes; rejects far unrelated decodes', () => {
    expect(passesFilter(row({ message: 'TX', freqHz: 1500, mine: true }), 'rx', 500)).toBe(true)
    expect(passesFilter(row({ message: 'NEAR', freqHz: 500 + RX_TOL_HZ }), 'rx', 500)).toBe(true)
    expect(passesFilter(row({ message: 'FAR', freqHz: 2400 }), 'rx', 500)).toBe(false)
  })
})

describe('CQ+73 filter (cq73 — tester request: 73s alongside the CQs)', () => {
  // Classification is the ENGINE's: Msg::parse types each message and the row's
  // `signoff` flag (Rr73/Bye73) rides the DTO like `isCq` does — this predicate
  // only trusts the flag, so token-positional proof (DM73 grids, calls containing
  // 73) lives with the parser tests in tempo-core/tempo-app.
  it('passes CQ rows and signoff rows; rejects plain traffic', () => {
    expect(passesFilter(row({ message: 'CQ W1AW FN31', freqHz: 1200, isCq: true }), 'cq73', 500)).toBe(true)
    expect(passesFilter(row({ message: 'K2DEF W9XYZ RR73', freqHz: 1200, signoff: true }), 'cq73', 500)).toBe(true)
    expect(passesFilter(row({ message: 'K2DEF W9XYZ 73', freqHz: 1200, signoff: true }), 'cq73', 500)).toBe(true)
    // A grid message whose grid merely LOOKS like 73 — the engine left signoff off.
    expect(passesFilter(row({ message: 'K2DEF W9XYZ DM73', freqHz: 1200 }), 'cq73', 500)).toBe(false)
    expect(passesFilter(row({ message: 'K2DEF W9XYZ -10', freqHz: 1200 }), 'cq73', 500)).toBe(false)
  })

  it('plain CQ still EXCLUDES signoffs — zero change for existing operators', () => {
    expect(passesFilter(row({ message: 'K2DEF W9XYZ RR73', freqHz: 1200, signoff: true }), 'cq', 500)).toBe(false)
    expect(passesFilter(row({ message: 'K2DEF W9XYZ 73', freqHz: 1200, signoff: true }), 'cq', 500)).toBe(false)
  })

  it('the chip bar slots CQ+73 between CQ and To me', () => {
    expect(DECODE_FILTERS).toEqual(['all', 'cq', 'cq73', 'me', 'rx', 'b4', 'new'])
  })
})

describe('band / tier scope wipe', () => {
  it('a band change clears the history (stale old-band rows are a hazard)', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })], 100, 1_000)
    expect(h.entries()).toHaveLength(1)
    expect(h.setScope('40m', 'FT8')).toBe(true)
    expect(h.entries()).toHaveLength(0)
  })

  it('a tier change clears too; same scope is a no-op', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })], 100, 1_000)
    expect(h.setScope('20m', 'FT8')).toBe(false) // unchanged → keep history
    expect(h.entries()).toHaveLength(1)
    expect(h.setScope('20m', 'FT4')).toBe(true)
    expect(h.entries()).toHaveLength(0)
  })
})

describe('a period heard before a band or mode change (WSJT-X shows it)', () => {
  const late: LateDecodes = {
    band: '20m',
    dialMhz: 14.074,
    tier: 'FT8',
    periodStartMs: 8 * 15_000,
    slot: 9,
    rows: [row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })],
  }

  it('lands after the wipe, once, under its own slot, keeping what it was heard on', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([row({ message: 'CQ K2DEF FN20', freqHz: 800 })], 8)
    expect(h.setScope('40m', 'FT8')).toBe(true)
    h.ingestLate(late)
    h.ingestLate(late) // the next poll sends it again
    h.ingest([row({ message: 'CQ JA1XYZ PM95', freqHz: 900 })], 11)
    const list = orderEntries(h.entries(), 'time')
    expect(list.map((d) => [d.message, d.slot, d.heard?.band])).toEqual([
      ['CQ W9XYZ EN52', 9, '20m'],
      ['CQ JA1XYZ PM95', 11, undefined],
    ])
    expect(list[0].heard).toEqual({ band: '20m', dialMhz: 14.074, tier: 'FT8', periodStartMs: 120_000 })
  })

  it('the same line heard live in the same slot is a row of its own', () => {
    const h = new DecodeHistory()
    h.setScope('40m', 'FT8')
    h.ingestLate(late)
    h.ingest(late.rows, late.slot)
    expect(h.entries().map((d) => d.heard?.band ?? 'live')).toEqual(['20m', 'live'])
  })
})

describe('off-band excursions are not a band change (operator ruling 2026-08-13)', () => {
  // Listening off the ham bands is a supported use case, and the backend says so by sending
  // `radio.band` as the EMPTY STRING — "no ham-band claim". Keyed raw, that empty label was a
  // band NAME: leaving 40m for WWV wiped the pane, and coming back wiped it again, so a
  // 20-second listen off the band edge cost the whole band's history twice. It also undid at
  // the UI layer the backend's own rule that an off-band excursion is a round trip
  // (`off_band_from` remembers the band it left), not a QSY.
  const cq = () => [row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })]

  it('a trip off the band and back is a round trip — the pane survives both moves', () => {
    const h = new DecodeHistory()
    h.setScope('40m', 'FT8')
    h.ingest(cq(), 100, 1_000)
    expect(h.setScope('', 'FT8')).toBe(false) // dial to WWV on 5 MHz — unknown, not a new band
    expect(h.entries()).toHaveLength(1)
    expect(h.setScope('40m', 'FT8')).toBe(false) // …and back to where we were
    expect(h.entries()).toHaveLength(1)
  })

  it('every off-band frequency is ONE scope — 5 → 11 → 27 MHz never re-wipes', () => {
    // Deliberate, and the same rule as tuning WITHIN a band: moving the dial around off the
    // band plan is the same kind of move, so it does not throw the pane away.
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest(cq(), 100, 1_000)
    for (let i = 0; i < 3; i++) expect(h.setScope('', 'FT8')).toBe(false)
    expect(h.entries()).toHaveLength(1)
  })

  it('a GENUINE band change still wipes, even reached through an off-band excursion', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest(cq(), 100, 1_000)
    expect(h.setScope('', 'FT8')).toBe(false)
    expect(h.setScope('40m', 'FT8')).toBe(true) // 20m → 40m is a QSY however you got there
    expect(h.entries()).toHaveLength(0)
  })

  it('a TIER change wipes regardless — including while the dial is off the bands', () => {
    const h = new DecodeHistory()
    h.setScope('', 'FT8')
    h.ingest(cq(), 100, 1_000)
    expect(h.setScope('', 'FT4')).toBe(true)
    expect(h.entries()).toHaveLength(0)
  })
})

describe('period separator UTC', () => {
  it('derives the period start from slot × period (engine slots count from the epoch)', () => {
    // FT8 slot 4 = 60 s after the epoch = 00:01:00 UTC.
    expect(fmtUtc(periodStartMs(4, 'FT8'))).toBe('000100')
    // FT4 slot 9 = 67.5 s → period starts at 67.5 s = 00:01:07 UTC.
    expect(fmtUtc(periodStartMs(9, 'FT4'))).toBe('000107')
    // FT2 slot 16 = 60 s at a 3.75 s period. The FRACTIONAL period is the point:
    // FT2 is the only sub-4-second tier, so a table entry rounded to 4 (or left
    // at FT8's 15) would put every FT2 period separator on the wrong second.
    expect(TIER_PERIOD_SECS.FT2).toBe(3.75)
    expect(fmtUtc(periodStartMs(16, 'FT2'))).toBe('000100')
    // …and slot 17 lands on the 3.75 s grid, not a whole second.
    expect(periodStartMs(17, 'FT2')).toBe(63_750)
  })

  it('takes the live period over the table one (MSK144, FST4, Q65 and JS8 make it a setting)', () => {
    // MSK144 slot 4 at a 30 s period = 120 s; the table's 15 s would say 60 s.
    expect(periodStartMs(4, 'MSK144', 30)).toBe(120_000)
    expect(periodStartMs(4, 'MSK144')).toBe(60_000)
  })
})

describe('the time a row shows (rowTimeMs)', () => {
  it('a received row: the start of the period its audio came from, never when it arrived', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    // Ingested at boundary slot 5 (audio slot 4, 00:01:00), 34 s into the next period.
    h.ingest([row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })], 5, 94_000)
    const [d] = h.entries()
    expect(d.at).toBe(94_000)
    expect(rowTimeMs(d, 'FT8')).toBe(60_000)
    expect(rowTimeMs(d, 'FT8', 30)).toBe(120_000)
  })

  it('a row heard before a band or mode change: the period it was heard in', () => {
    // Heard on FT8 (boundary slot 4, the period at 45 s), shown after a change to FT4: its slot
    // comes in FT4's numbering (8), which would date it 52.5 s.
    const h = new DecodeHistory()
    h.setScope('20m', 'FT4')
    h.ingestLate(
      { band: '40m', dialMhz: 7.074, tier: 'FT8', periodStartMs: 45_000, slot: 8, rows: [row({ message: 'CQ K2DEF FN20', freqHz: 800 })] },
      94_000,
    )
    expect(rowTimeMs(h.entries()[0], 'FT4')).toBe(45_000)
  })

  it('an own transmission: when it was sent', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest([row({ message: 'CQ KD9TAW EN52', freqHz: 1500, mine: true, txAt: 75 })], 6, 94_000)
    expect(rowTimeMs(h.entries()[0], 'FT8')).toBe(75_000)
  })
})

// THE LOOK-BACK PROPERTY (operator, 2026-08-18: "it's good to be able to look back").
//
// Both panes ingest the SAME full decode list and filter at render, so the Rx-Frequency
// pane's history was spent almost entirely on band-wide rows it never displays. On a busy
// band an 80-decode period filled the old 300-row store in under four minutes, and the two
// or three rows on your own frequency — the ones the pane exists for — were evicted with
// them. The store is now sized for what a pane KEEPS; what it RENDERS is capped separately,
// so the DOM cost of a busy Band Activity pane is unchanged.
describe('history depth — a filtered pane can still look back', () => {
  const BUSY = 80

  /** N periods of a busy band, one decode per period on the RX frequency. */
  function busyBand(periods: number): DecodeHistory {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    for (let p = 0; p < periods; p++) {
      const slot = 1000 + p
      const batch = [
        // The one row this pane exists to show, tagged with its period.
        row({ message: `CQ ONFREQ${p} AA00`, freqHz: 1500, from: `ONFREQ${p}`, isCq: true }),
        // …and the rest of the band, which it will filter out at render.
        ...Array.from({ length: BUSY }, (_, i) =>
          row({
            message: `CQ FAR${p}_${i} BB11`,
            freqHz: 300 + ((i * 137) % 2600),
            from: `FAR${p}_${i}`,
            isCq: true,
          }),
        ),
      ]
      h.ingest(batch, slot, 1_722_500_000_000 + p * 15_000)
    }
    return h
  }

  it('keeps 30 periods of RX-frequency rows through a busy band (7½ minutes)', () => {
    const shown = busyBand(30)
      .entries()
      .filter((d) => passesFilter(d, 'rx', 1500))
    // Every period's on-frequency row survives — including the first.
    // EVERY period's own row, oldest included — not merely "some rows survived".
    const kept = new Set(shown.map((d) => d.from))
    const missing = Array.from({ length: 30 }, (_, p) => `ONFREQ${p}`).filter((c) => !kept.has(c))
    expect(missing, 'on-frequency rows evicted by band-wide traffic').toEqual([])
  })

  it('still bounds the store — an unbounded feed is its own bug', () => {
    const h = busyBand(120)
    expect(h.entries().length).toBeLessThanOrEqual(MAX_HISTORY)
  })

  it('caps what a pane RENDERS independently of what it stores', () => {
    // The DOM cost is the render cap, not the store: Band Activity sees every row.
    const all = busyBand(120)
      .entries()
      .filter((d) => passesFilter(d, 'all', 1500))
    const rendered = renderWindow(orderEntries(all, 'time'))
    expect(rendered.length).toBe(MAX_ROWS)
    // …and it is the NEWEST rows that survive the cap, never the oldest.
    const last = rendered[rendered.length - 1]
    expect(last).toEqual(all[all.length - 1] ?? last)
    expect(rendered[0].slot).toBeGreaterThan(all[0].slot)
  })
})

describe('erase and the engine own-TX ring', () => {
  // #178 (reopened) / #189: Erase wiped the pane and the next poll painted the same overs
  // straight back, so an operator's own transmissions could not be cleared at all short of
  // restarting Nexus. The engine keeps a 30-deep DISPLAY ring of overs already sent and
  // appends ALL of it to every snapshot — deliberately, so a band QSY performed under the
  // operator cannot lose their record of what they transmitted. Received decodes do not come
  // back because the engine's list is current-period; own-TX rows persist for 30 overs.
  //
  // So the pane has to remember that it was erased: an over already sent when Erase was
  // pressed stays gone, and the next one transmitted still appears.
  const mine = (txAt: number, message: string): DecodeRow =>
    row({ message, freqHz: 1500, from: 'KD9TAW', mine: true, txAt })

  it('an own-TX row already sent when Erase was pressed does not come back', () => {
    const h = new DecodeHistory()
    h.ingest([mine(1000, 'CQ KD9TAW EN61')], 0)
    expect(h.entries()).toHaveLength(1)

    h.erase(1000)
    // The very next poll re-serves the same ring — this is what shipped as un-erasable.
    h.ingest([mine(1000, 'CQ KD9TAW EN61')], 1)
    expect(h.entries()).toHaveLength(0)
  })

  it('an over transmitted AFTER the erase still appears', () => {
    const h = new DecodeHistory()
    h.ingest([mine(1000, 'CQ KD9TAW EN61')], 0)
    h.erase(1000)
    // The ring still carries the old over; a new one joins it.
    h.ingest([mine(1000, 'CQ KD9TAW EN61'), mine(1015, 'P29YY KD9TAW EN61')], 1)
    const msgs = h.entries().map((e) => e.message)
    expect(msgs).toEqual(['P29YY KD9TAW EN61'])
  })

  it('erasing does not deafen the pane to received decodes', () => {
    const h = new DecodeHistory()
    h.erase(1000)
    h.ingest([row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })], 1)
    expect(h.entries()).toHaveLength(1)
  })
})

// The snapshot re-offers the same decode list at 4 Hz for a whole period. A poll that changes
// nothing must SAY so (the pane skips its re-render and its filter/sort pass on it) and must leave
// every entry the same object (the pane's memoised rows compare by identity).
describe('an unchanged re-poll changes nothing', () => {
  const period = () => [
    row({ from: 'W9XYZ', message: 'CQ W9XYZ EN52', freqHz: 1200 }),
    row({ from: 'K2DEF', message: 'W1AW K2DEF -12', freqHz: 800 }),
  ]

  it('reports no change, keeps the version, and keeps each entry object', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    expect(h.ingest(period(), 10, 1_000)).toBe(true)
    const v = h.version
    const before = h.entries()
    // A fresh array of fresh, equal rows — exactly what the next poll delivers.
    expect(h.ingest(period(), 10, 1_300)).toBe(false)
    expect(h.version).toBe(v)
    const after = h.entries()
    expect(after).toHaveLength(2)
    after.forEach((e, i) => expect(e).toBe(before[i]))
  })

  it('a new row, or a flag moving under a row, is a change and a new entry for that row only', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest(period(), 10, 1_000)
    const [w9, k2] = h.entries()
    let v = h.version
    // The station gets worked: its row's `worked` flips under it (a row losing its chips).
    const worked = period()
    worked[1] = { ...worked[1], worked: true }
    expect(h.ingest(worked, 10, 1_300)).toBe(true)
    expect(h.version).toBeGreaterThan(v)
    expect(h.entries()[0]).toBe(w9)
    expect(h.entries()[1]).not.toBe(k2)
    expect(h.entries()[1].worked).toBe(true)
    // A new decode in the next period.
    v = h.version
    expect(h.ingest([row({ from: 'JA1XYZ', message: 'CQ JA1XYZ PM95', freqHz: 900 })], 11, 16_000)).toBe(true)
    expect(h.version).toBeGreaterThan(v)
    expect(h.entries()).toHaveLength(3)
  })

  it('a re-sent late period is no change either', () => {
    const late: LateDecodes = {
      band: '20m',
      dialMhz: 14.074,
      tier: 'FT8',
      periodStartMs: 8 * 15_000,
      slot: 9,
      rows: [row({ message: 'CQ W9XYZ EN52', freqHz: 1200 })],
    }
    const h = new DecodeHistory()
    h.setScope('40m', 'FT8')
    expect(h.ingestLate(late, 1_000)).toBe(true)
    const v = h.version
    const e = h.entries()[0]
    expect(h.ingestLate({ ...late, rows: late.rows.map((r) => ({ ...r })) }, 1_300)).toBe(false)
    expect(h.version).toBe(v)
    expect(h.entries()[0]).toBe(e)
  })

  it('every wipe moves the version: band change, erase, reset', () => {
    const h = new DecodeHistory()
    h.setScope('20m', 'FT8')
    h.ingest(period(), 10)
    let v = h.version
    h.setScope('20m', 'FT8') // same scope: nothing happens
    expect(h.version).toBe(v)
    h.setScope('40m', 'FT8')
    expect(h.version).toBeGreaterThan(v)
    v = h.version
    h.erase(5)
    expect(h.version).toBeGreaterThan(v)
    v = h.version
    h.reset()
    expect(h.version).toBeGreaterThan(v)
  })
})
