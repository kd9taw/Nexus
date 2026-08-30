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
  RX_TOL_HZ,
  TIER_PERIOD_SECS,
} from './decodeHistory'
import type { DecodeRow } from './types'

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
