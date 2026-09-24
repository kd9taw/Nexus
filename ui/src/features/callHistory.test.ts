import { describe, expect, it } from 'vitest'
import { bandKey, callHistory, callsSummary, entitySlots, historySummary, isNewEntity, modeKey } from './callHistory'
import type { LoggedQso } from '../types'

function qso(call: string, band: string, mode: string, whenUnix: number, confirmed = false): LoggedQso {
  return {
    call,
    grid: null,
    band,
    freqMhz: 14.2,
    mode,
    rstSent: '59',
    rstRcvd: '59',
    whenUnix,
    confirmed,
    awardConfirmed: false,
  }
}

const LOG: LoggedQso[] = [
  qso('W1AW', '40m', 'CW', 1000, true),
  qso('K9XYZ', '20m', 'SSB', 1500),
  qso('w1aw', '20m', 'FT8', 2000), // same call, lowercase, newer
  qso('W1AW', '20m', 'SSB', 1800, true),
]

describe('callHistory', () => {
  it('empty call or no prior QSOs → not worked before', () => {
    expect(callHistory(LOG, '', '20m').workedBefore).toBe(false)
    expect(callHistory(LOG, 'DX0NEW', '20m')).toMatchObject({ workedBefore: false, count: 0 })
  })

  it('matches a call case-insensitively and counts all prior QSOs', () => {
    const h = callHistory(LOG, 'w1aw', '15m')
    expect(h.workedBefore).toBe(true)
    expect(h.count).toBe(3) // W1AW + w1aw + W1AW
  })

  it('lastUnix is the most recent contact, not log order', () => {
    expect(callHistory(LOG, 'W1AW', '15m').lastUnix).toBe(2000)
  })

  it('dupeThisBand is true only when worked on the current band', () => {
    expect(callHistory(LOG, 'W1AW', '20m').dupeThisBand).toBe(true) // worked 20m
    expect(callHistory(LOG, 'W1AW', '40m').dupeThisBand).toBe(true) // worked 40m
    expect(callHistory(LOG, 'W1AW', '15m').dupeThisBand).toBe(false) // never on 15m
    expect(callHistory(LOG, 'W1AW', '').dupeThisBand).toBe(false) // no band → skip
  })

  it('the mode-scoped dupe check goes through modeKey, so a sideband still meets SSB', () => {
    // ⭐ The Dupe/B4 badge with Settings ▸ b4MatchMode on. The compare here was the one
    // mode compare in this module that did NOT go through `modeKey` — `entitySlots` above it
    // does, and its own comment says why ("USB against a log full of LSB read as a mode never
    // worked"). Once the Phone cockpit logs the SIDEBAND a contact was worked on, the live
    // mode is "USB"/"LSB" while a log full of history holds "SSB", and a raw compare takes the
    // badge dark for every phone station the operator has ever worked.
    for (const live of ['USB', 'LSB', 'SSB', 'ssb']) {
      expect(
        callHistory(LOG, 'W1AW', '20m', live, true).dupeThisBand,
        `${live} did not meet the stored SSB contact`,
      ).toBe(true)
    }
    // …and a stored sideband meets a live SSB, which is the same fold read the other way —
    // every phone contact already in an operator's log stays matchable.
    const stored = [qso('W1AW', '20m', 'LSB', 1800)]
    expect(callHistory(stored, 'W1AW', '20m', 'SSB', true).dupeThisBand).toBe(true)
    expect(callHistory(stored, 'W1AW', '20m', 'USB', true).dupeThisBand).toBe(true)
    // THE CONTROL: the fold is the sidebands only. A genuinely different mode on the same
    // band is not a dupe — without this the assertions above would pass on a compare that had
    // simply stopped discriminating at all.
    expect(callHistory(LOG, 'W1AW', '20m', 'CW', true).dupeThisBand).toBe(false)
    expect(callHistory(LOG, 'W1AW', '20m', 'FM', true).dupeThisBand).toBe(false)
    expect(callHistory(stored, 'W1AW', '20m', 'AM', true).dupeThisBand).toBe(false)
  })

  it('counts confirmed QSOs and collects distinct bands + modes', () => {
    const h = callHistory(LOG, 'W1AW', '20m')
    expect(h.confirmedCount).toBe(2) // the 40m CW + 20m SSB are confirmed
    expect(h.bands).toEqual(['40m', '20m'])
    expect(h.modes).toEqual(['CW', 'FT8', 'SSB'])
  })
})

describe('isNewEntity', () => {
  const log = [{ country: 'Japan' }, { country: null }, {}]

  it('country absent from the log → new entity', () => {
    expect(isNewEntity(log, 'Fiji')).toBe(true)
    expect(isNewEntity([], 'Fiji')).toBe(true)
  })

  it('already-logged country matches case-insensitively → not new', () => {
    expect(isNewEntity(log, 'Japan')).toBe(false)
    expect(isNewEntity(log, 'JAPAN')).toBe(false)
    expect(isNewEntity(log, ' japan ')).toBe(false)
  })

  it('empty/null/whitespace country → never claims new', () => {
    expect(isNewEntity(log, '')).toBe(false)
    expect(isNewEntity(log, '   ')).toBe(false)
    expect(isNewEntity(log, null)).toBe(false)
    expect(isNewEntity(log, undefined)).toBe(false)
  })
})

describe('entitySlots', () => {
  // One entity (Japan) across several calls/bands/modes, plus another entity and a
  // blank-country row that must never bleed into Japan's slots.
  const log = [
    { call: 'JA1A', country: 'Japan', band: '20m', mode: 'SSB' },
    { call: 'JA7B', country: ' japan ', band: '40m', mode: 'CW' }, // same entity, diff call, case/space
    { call: 'W1AW', country: 'United States', band: '20m', mode: 'FT8' },
    { call: 'JR3C', country: 'JAPAN', band: '20m', mode: 'CW' }, // dupe 20m band for Japan
    { call: 'X', country: null, band: '15m', mode: 'SSB' }, // blank country — not any entity
  ]

  it('unworked or blank country → not worked, empty slots', () => {
    const none = { workedEver: false, bandsWorked: [], modesWorked: [], bandUnknown: false }
    expect(entitySlots(log, 'Fiji')).toEqual(none)
    expect(entitySlots(log, '')).toEqual(none)
    expect(entitySlots(log, null)).toEqual(none)
    expect(entitySlots([], 'Japan')).toEqual(none)
  })

  it('collects distinct entity bands/modes across calls, case/whitespace-insensitive on country', () => {
    const s = entitySlots(log, 'japan')
    expect(s.workedEver).toBe(true)
    expect(s.bandsWorked).toEqual(['20M', '40M']) // distinct, normalized, first-seen order
    expect(s.modesWorked).toEqual(['SSB', 'CW'])
  })

  it('normalizes bands/modes so membership tests are case/whitespace-tolerant', () => {
    const s = entitySlots(log, 'JAPAN')
    expect(s.bandsWorked.includes('20m'.trim().toUpperCase())).toBe(true)
    expect(s.bandsWorked.includes('15M')).toBe(false) // 15m was the blank-country row, not Japan
    expect(s.modesWorked.includes('ft8'.toUpperCase())).toBe(false) // FT8 was the USA row
  })
})

describe('historySummary', () => {
  it('first-contact cue when never worked', () => {
    expect(historySummary(callHistory(LOG, 'DX0NEW', ''))).toBe('First contact — new station!')
  })

  it('count + most-recent band/mode/date, in UTC (not log order)', () => {
    // W1AW: 3 QSOs; most recent is whenUnix=2000 → 20m FT8; 2000s = 1 Jan 1970 UTC.
    expect(historySummary(callHistory(LOG, 'W1AW', ''))).toBe('3 QSOs — last on 20m FT8, 1 Jan 1970')
  })

  it('singular QSO with a real date', () => {
    const log = [qso('N0P', '15m', 'SSB', Math.floor(Date.UTC(2026, 2, 14) / 1000))]
    expect(historySummary(callHistory(log, 'N0P', ''))).toBe('1 QSO — last on 15m SSB, 14 Mar 2026')
  })

  it('omits band/mode when absent', () => {
    const log = [qso('N0B', '', '', 1000)]
    expect(historySummary(callHistory(log, 'N0B', ''))).toBe('1 QSO — last 1 Jan 1970')
  })
})

describe('modeKey — spellings of one mode fold, different modes never do', () => {
  it('USB and LSB are SSB: sideband is not a mode you can newly work', () => {
    // The false badge this kills: an operator with a log full of LSB contacts works one on USB
    // and is told it is a mode they have never worked. ADIF models both as SUBMODEs of SSB.
    expect(modeKey('USB')).toBe('SSB')
    expect(modeKey('LSB')).toBe('SSB')
    expect(modeKey('ssb')).toBe('SSB')
    expect(modeKey(' usb ')).toBe('SSB')
  })

  it('the same waveform under two spellings folds', () => {
    expect(modeKey('BPSK31')).toBe('PSK31')
    expect(modeKey('PSK31')).toBe('PSK31')
    expect(modeKey('BPSK63')).toBe('PSK63')
  })

  it('⭐ does NOT fold different modes together — this is not the DXCC class', () => {
    // Folding to CW/Phone/Digital would say an FT8 contact covers FT4, and an FM contact covers
    // SSB. Correct for what DXCC awards, wrong for a board that shows specific modes.
    expect(modeKey('FM')).toBe('FM')
    expect(modeKey('AM')).toBe('AM')
    expect(modeKey('FT4')).toBe('FT4')
    expect(modeKey('FT8')).toBe('FT8')
    expect(modeKey('RTTY')).toBe('RTTY')
    for (const m of ['FM', 'AM', 'FT4', 'FT8', 'RTTY', 'CW']) {
      expect(modeKey(m)).not.toBe('SSB')
    }
  })

  it('leaves ambiguous tokens alone rather than guessing', () => {
    // A bare MFSK row may be FT4, JS8 or something else — current imports promote ADIF SUBMODE
    // so they store FT4 directly, and this is older residue. PH is N1MM/N3FJP's generic phone
    // token and may have been FM. Mapping either invents a contact that was never made.
    expect(modeKey('MFSK')).toBe('MFSK')
    expect(modeKey('PH')).toBe('PH')
    expect(modeKey('')).toBe('')
    expect(modeKey(null)).toBe('')
  })
})

describe('bandKey — resolve the band, or admit it cannot be known', () => {
  it('a token that names a band wins', () => {
    expect(bandKey({ band: '20m' })).toBe('20M')
    expect(bandKey({ band: ' 40M ' })).toBe('40M')
  })

  it('falls back to the frequency when the token names nothing', () => {
    // The issue: `from_band_token` and `from_label` disagree on suffixes like `-fm`, so some
    // stored tokens parse in one path and not the other, and the frequency was never tried.
    expect(bandKey({ band: '20m-fm', freqMhz: 14.074 })).toBe('20M')
    expect(bandKey({ band: '', freqMhz: 7.074 })).toBe('40M')
    expect(bandKey({ band: null, freqMhz: 14.074 })).toBe('20M')
  })

  it('⭐ null when neither is usable — the case that made a permanent false badge', () => {
    // Every imported row in the 11k log this came from carries FREQ=0.000000, so an unparseable
    // BAND has nothing to fall back on. That is not "not worked" — it is not knowable.
    expect(bandKey({ band: 'nonsense', freqMhz: 0 })).toBeNull()
    expect(bandKey({ band: 'nonsense' })).toBeNull()
    expect(bandKey({ band: '', freqMhz: 0 })).toBeNull()
    expect(bandKey({})).toBeNull()
    expect(bandKey({ band: '20m-fm', freqMhz: 0 })).toBeNull() // token unusable, no frequency
  })
})

describe('entitySlots resolves through those keys', () => {
  it('a sideband spelling does not read as a new mode', () => {
    const log = [{ call: 'G0A', country: 'England', band: '20m', mode: 'LSB' }]
    const s = entitySlots(log, 'England')
    expect(s.modesWorked).toEqual(['SSB'])
    expect(s.modesWorked.includes(modeKey('USB'))).toBe(true)
  })

  it('an unparseable band is recovered from the frequency', () => {
    const log = [{ call: 'G0A', country: 'England', band: '20m-fm', mode: 'SSB', freqMhz: 14.2 }]
    const s = entitySlots(log, 'England')
    expect(s.bandsWorked).toEqual(['20M'])
    expect(s.bandUnknown).toBe(false)
  })

  it('⭐ an unrecoverable band is flagged, not silently counted as some other band', () => {
    const log = [{ call: 'G0A', country: 'England', band: 'nonsense', mode: 'SSB', freqMhz: 0 }]
    const s = entitySlots(log, 'England')
    expect(s.workedEver).toBe(true)
    expect(s.bandsWorked).toEqual([]) // never a phantom token that matches nothing forever
    expect(s.bandUnknown).toBe(true) // the caller must not claim "new band" on this
  })

  it('a row with no band at all is silent, not unknown', () => {
    // Distinct from the case above: nothing was recorded, so there is nothing unresolvable.
    const log = [{ call: 'G0A', country: 'England', mode: 'SSB' }]
    expect(entitySlots(log, 'England').bandUnknown).toBe(false)
  })
})

// The JS8 roster's log join, moved out of the cockpit so a `LogSource` can answer it (SPEC-2 v3
// C17b). ORACLE is the cockpit's own loop as it stood (Js8Cockpit.tsx at d5be14ea, :417-435),
// pasted unchanged; `callsSummary` must build the same entries for every call.
describe('callsSummary is the JS8 roster join, moved', () => {
  function ORACLE(log: LoggedQso[], stationCalls: string) {
    const out = new Map<string, { count: number; lastUnix: number | null; grid: string; name: string; comment: string }>()
    for (const call of stationCalls.split(' ').filter(Boolean)) {
      const hist = callHistory(log, call, '')
      if (!hist.workedBefore) continue
      const last = hist.qsos.reduce((a, b) => (b.whenUnix > a.whenUnix ? b : a))
      out.set(call, {
        count: hist.count,
        lastUnix: hist.lastUnix,
        grid: (last.grid ?? '').trim(),
        name: (last.name ?? '').trim(),
        comment: (last.comment ?? '').trim(),
      })
    }
    return out
  }
  const row = (call: string, whenUnix: number, grid: string | null, name: string | null, comment: string | null) =>
    ({ ...qso(call, '20m', 'FT8', whenUnix), grid, name, comment }) as LoggedQso

  it('agrees with the cockpit\'s loop, including the first-of-equal-times "latest" rule', () => {
    const log = [
      row('W1AW', 100, 'FN31 ', ' Hiram', 'first'),
      row('w1aw', 300, null, 'Hiram P', null),
      row('W1AW', 300, 'FN42', 'later twin', 'not chosen'), // equal time, later in the log: not "latest"
      row('K1ABC', 50, 'EN52', null, ' park '),
      row(' k1abc ', 60, '', 'Kay', ''),
    ]
    const stationCalls = ['K1ABC', 'NONE', 'W1AW', 'w1aw'].sort().join(' ')
    const want = ORACLE(log, stationCalls)
    const got = callsSummary(log, stationCalls.split(' ').filter(Boolean))
    expect(new Map(Object.entries(got))).toEqual(want)
    expect(want.get('W1AW')?.name, 'the fixture must reach the tie rule').toBe('Hiram P')
    expect(want.has('NONE')).toBe(false)
  })
})
