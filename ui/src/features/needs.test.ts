import { describe, expect, it } from 'vitest'
import { NEED_TIER, chaseRank, modeClassOf, strongestNeed, visibleNeeds, workTarget } from './needs'
import type { BandChannel, NeedAlert, NeedTag } from '../types'

function alert(call: string, mode: string, band = '20m', freqMhz: number | null = null): NeedAlert {
  return {
    call,
    entity: 'Test',
    band,
    zone: 14,
    tags: ['NewEntity'],
    priority: 100,
    headline: 'New one',
    mode,
    freqMhz,
  }
}

const BAND_PLAN: BandChannel[] = [
  { band: '20m', group: 'HF', dialMhz: 14.074, mode: 'USB', label: '20m', note: '' },
  { band: '40m', group: 'HF', dialMhz: 7.074, mode: 'USB', label: '40m', note: '' },
]

describe('visibleNeeds', () => {
  const all = [alert('A', 'Digital'), alert('B', 'CW'), alert('C', 'Phone')]

  it('digital op (modes off) sees only digital needs — board unchanged', () => {
    const v = visibleNeeds(all, { cw: false, phone: false })
    expect(v.map((a) => a.call)).toEqual(['A'])
  })

  it('CW enabled surfaces CW needs; Phone still hidden', () => {
    const v = visibleNeeds(all, { cw: true, phone: false })
    expect(v.map((a) => a.call)).toEqual(['A', 'B'])
  })

  it('both modes on shows everything', () => {
    expect(visibleNeeds(all, { cw: true, phone: true }).length).toBe(3)
  })

  it('an unknown mode class defaults to visible (fail-open, never hide a need)', () => {
    const v = visibleNeeds([alert('Z', 'SSTV')], { cw: false, phone: false })
    expect(v.length).toBe(1)
  })
})

describe('workTarget', () => {
  it('CW need with an exact spot freq → QSY there, cw view', () => {
    const t = workTarget(alert('3Y0J', 'CW', '20m', 14.025), BAND_PLAN)
    expect(t).toEqual({ call: '3Y0J', view: 'cw', freqMhz: 14.025, band: '20m' })
  })

  it('Phone need opens the phone cockpit at the spot frequency', () => {
    const t = workTarget(alert('EA5DX', 'Phone', '20m', 14.25), BAND_PLAN)
    expect(t).toEqual({ call: 'EA5DX', view: 'phone', freqMhz: 14.25, band: '20m' })
  })

  it('CW need with no exact freq → the band CW activity freq, NOT the FT8 dial', () => {
    // Regression: a freq-less CW/Phone need used to fall back to the tier dial (14.074 = FT8),
    // which sent CW/phone click-to-work to an FT8 frequency. Now it lands in the CW window.
    const t = workTarget(alert('JA1XYZ', 'CW', '20m', null), BAND_PLAN)
    expect(t?.freqMhz).toBe(14.03)
    const p = workTarget(alert('EA5', 'Phone', '20m', null), BAND_PLAN)
    expect(p?.freqMhz).toBe(14.25)
  })

  it('a Digital need QSYs to the exact spot freq and opens the digital cockpit (N1MM-style)', () => {
    const t = workTarget(alert('A', 'Digital', '20m', 14.074), BAND_PLAN)
    expect(t).toEqual({ call: 'A', view: 'operate', freqMhz: 14.074, band: '20m' })
  })

  it('a Digital need with no spot freq falls back to the band default channel', () => {
    const t = workTarget(alert('A', 'Digital', '40m', null), BAND_PLAN)
    expect(t).toEqual({ call: 'A', view: 'operate', freqMhz: 7.074, band: '40m' })
  })

  it('no frequency resolvable (unknown band, no spot freq) → null', () => {
    expect(workTarget(alert('A', 'CW', '60m', null), BAND_PLAN)).toBeNull()
  })

  it('an RTTY need routes to the rtty cockpit at the exact spot freq', () => {
    const t = workTarget(alert('DL1RT', 'RTTY', '20m', 14.085), BAND_PLAN)
    expect(t).toEqual({ call: 'DL1RT', view: 'rtty', freqMhz: 14.085, band: '20m' })
  })
})

describe('modeClassOf (map-spot → cockpit routing)', () => {
  it('CW routes to the CW cockpit', () => {
    expect(modeClassOf('CW')).toBe('CW')
    expect(modeClassOf('cw')).toBe('CW')
  })
  it('voice modes AND the "Phone" class label route to Phone', () => {
    // Both ADIF tokens and our own class label — a need alert's mode is the LABEL "Phone",
    // which previously fell through to Digital and routed a phone need to the wrong cockpit.
    for (const m of ['SSB', 'USB', 'LSB', 'FM', 'AM', 'ssb', 'Phone', 'PHONE']) {
      expect(modeClassOf(m)).toBe('Phone')
    }
  })
  it('digital + unknown + missing route to Digital (fail-safe)', () => {
    for (const m of ['FT8', 'FT4', 'RTTY', 'PSK31', 'JS8', 'weird', '', null, undefined]) {
      expect(modeClassOf(m)).toBe('Digital')
    }
  })
})

// ── Chase ranking ────────────────────────────────────────────────────────────────────
// "Sort by need has no discernible order — a new band-mode sits three quarters of the way
// down the list" (operator report). Two separate defects were behind it, and both are
// pinned below: the roster ranked a station by ONE tag chosen arbitrarily among its alerts,
// and it collapsed the backend's priority (rarity boost, activation bumps) into a bare
// ordinal. The ranking itself is not invented here — it mirrors NeedTag::tier().

/** A need alert with the tags + priority under test. */
function ranked(tags: NeedTag[], priority: number, band = '20m'): NeedAlert {
  return {
    call: 'W1AW',
    entity: 'Test',
    band,
    zone: 14,
    tags,
    priority,
    headline: 'Need',
    mode: 'Digital',
    freqMhz: null,
  }
}

describe('NEED_TIER mirrors the backend gradient', () => {
  it('ranks the award needs in the documented order, strictly descending', () => {
    // Pinned as an ORDER, not as a set of magic numbers: this is the claim the roster and
    // the Needed board both depend on. Values themselves are NeedTag::tier() verbatim.
    const ladder: NeedTag[] = [
      'Wanted', 'NewEntity', 'NewZone', 'NewState', 'NewGrid', 'NewBand', 'NewMode', 'Confirm',
    ]
    const weights = ladder.map((t) => NEED_TIER[t])
    expect(weights).toEqual([120, 100, 70, 60, 55, 50, 30, 10])
    for (let i = 1; i < weights.length; i++) {
      expect(weights[i], `${ladder[i]} must rank below ${ladder[i - 1]}`).toBeLessThan(weights[i - 1])
    }
  })

  it('gives the appended program tags no primary weight', () => {
    // The backend appends these onto an existing award need and expresses their pull as a
    // priority bump; weighting them here would double-count it.
    for (const t of ['Dxped', 'Pota', 'Sota'] as NeedTag[]) expect(NEED_TIER[t]).toBe(0)
  })
})

describe('strongestNeed', () => {
  it('picks the highest-priority alert whatever order they arrive in', () => {
    const weak = ranked(['Confirm'], 10, '40m')
    const strong = ranked(['NewEntity'], 100, '20m')
    expect(strongestNeed([weak, strong])?.band).toBe('20m')
    expect(strongestNeed([strong, weak])?.band).toBe('20m')
  })

  it('keeps the earlier alert on a tie (the backend orders them descending)', () => {
    const first = ranked(['NewBand'], 50, '20m')
    const second = ranked(['NewBand'], 50, '40m')
    expect(strongestNeed([first, second])?.band).toBe('20m')
  })

  it('skips alerts carrying no tags, and reads nothing as nothing', () => {
    expect(strongestNeed([ranked([], 999)])).toBeNull()
    expect(strongestNeed([])).toBeNull()
    expect(strongestNeed(undefined)).toBeNull()
    expect(strongestNeed(null)).toBeNull()
  })
})

describe('chaseRank', () => {
  it('ranks a multi-band call by its STRONGEST need, not its last', () => {
    // THE ROOT CAUSE. needByCall (App.tsx) writes one tag per call with no guard, so for a
    // station heard on two bands the LAST alert wins — and the backend hands them out
    // priority-descending, making that the weakest. Ranking off the alert set fixes it.
    const alerts = [ranked(['NewEntity'], 100, '20m'), ranked(['Confirm'], 10, '40m')]
    expect(chaseRank(alerts, 'Confirm')).toBe(100)
    expect(chaseRank(alerts, 'Confirm')).toBeGreaterThan(chaseRank([ranked(['NewBand'], 50)], 'NewBand'))
  })

  it('keeps the backend rarity boost, which a bare tag ladder would throw away', () => {
    // A rare grid is 55 + 20 = 75 and an ultra-rare 55 + 30 = 85, so either outranks a new
    // zone (70) even though the bare NewGrid tier (55) sits below it.
    const rareGrid = ranked(['NewGrid'], 75)
    const newZone = ranked(['NewZone'], 70)
    expect(chaseRank([rareGrid], 'NewGrid')).toBeGreaterThan(chaseRank([newZone], 'NewZone'))
    expect(NEED_TIER.NewGrid).toBeLessThan(NEED_TIER.NewZone) // …the opposite way round on tags alone
  })

  it('floats a park activation above a bare confirm but below the award tiers', () => {
    // OTA_ACTIVATION_PRIORITY = 20: above Confirm (10), below NewMode (30).
    const pota = ranked(['Pota'], 20)
    expect(chaseRank([pota], 'Pota')).toBeGreaterThan(chaseRank([ranked(['Confirm'], 10)], 'Confirm'))
    expect(chaseRank([pota], 'Pota')).toBeLessThan(chaseRank([ranked(['NewMode'], 30)], 'NewMode'))
  })

  it('falls back to the tag tier for a host that passes no alert set', () => {
    expect(chaseRank(undefined, 'NewEntity')).toBe(NEED_TIER.NewEntity)
    expect(chaseRank([], 'NewBand')).toBe(NEED_TIER.NewBand)
    expect(chaseRank(undefined, 'NewEntity')).toBeGreaterThan(chaseRank(undefined, 'NewMode'))
  })

  it('ranks a station with nothing needed below every real need', () => {
    expect(chaseRank(undefined, null)).toBe(0)
    expect(chaseRank([], undefined)).toBe(0)
    // Including the weakest real one — a workable-but-unneeded station must never outrank a
    // confirm, and a park activation must beat it too.
    expect(chaseRank(undefined, null)).toBeLessThan(chaseRank([ranked(['Confirm'], 10)], 'Confirm'))
    expect(chaseRank(undefined, null)).toBeLessThan(chaseRank([ranked(['Pota'], 20)], 'Pota'))
  })

  it("pins the operator's complaint: a new-mode row outranks anything merely workable", () => {
    // The report said "new band-mode". There is no such rank: LogNeeds::need short-circuits,
    // so a slot needed on band AND mode reports NewBand alone and the two are mutually
    // exclusive. The nearest real rank is NewMode — band worked, this mode class not — so
    // that is what this pins: above a workable row, below new band and new entity.
    const newMode = chaseRank([ranked(['NewMode'], 30)], 'NewMode')
    const workable = chaseRank(undefined, null)
    const newBand = chaseRank([ranked(['NewBand'], 50)], 'NewBand')
    const newEntity = chaseRank([ranked(['NewEntity'], 100)], 'NewEntity')
    expect(newMode).toBeGreaterThan(workable)
    expect(newMode).toBeLessThan(newBand)
    expect(newBand).toBeLessThan(newEntity)
  })

  it('sorts a mixed roster into chase order end to end', () => {
    const rows = [
      { call: 'PLAIN', rank: chaseRank(undefined, null) },
      { call: 'MODE', rank: chaseRank([ranked(['NewMode'], 30)], 'NewMode') },
      { call: 'ENTITY', rank: chaseRank([ranked(['NewEntity'], 100)], 'NewEntity') },
      { call: 'CONFIRM', rank: chaseRank([ranked(['Confirm'], 10)], 'Confirm') },
      { call: 'BAND', rank: chaseRank([ranked(['NewBand'], 50)], 'NewBand') },
      { call: 'WANTED', rank: chaseRank([ranked(['Wanted', 'NewEntity'], 120)], 'Wanted') },
    ]
    rows.sort((a, b) => b.rank - a.rank)
    expect(rows.map((r) => r.call)).toEqual(['WANTED', 'ENTITY', 'BAND', 'MODE', 'CONFIRM', 'PLAIN'])
  })
})
