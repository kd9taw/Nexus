import { describe, expect, it } from 'vitest'
import {
  NEED_TIER,
  alertsForSurface,
  chaseRank,
  modeClassOf,
  strongestNeed,
  tagsForSurface,
  topNeedByCall,
  alertsByCall,
  visibleNeeds,
  workTarget,
} from './needs'
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

// ---------------------------------------------------------------------------
// Field report 2026-07-29: the Needed system claimed a "new mode on 30m" for
// Asiatic Russia on an operator who has six 30m FT8 contacts with that entity.
// The backend need was real (they have never worked it on CW) but the roster
// chips are keyed by CALL alone, so a CW alert painted an unqualified MODE chip
// onto their 30m FT8 surface. tagsForSurface is the gate those maps were missing.
// ---------------------------------------------------------------------------
describe('tagsForSurface (the false "new mode" gate)', () => {
  const ar = (tags: NeedTag[], mode: string, band = '30m'): NeedAlert => ({
    call: 'RF9C',
    entity: 'Asiatic Russia',
    band,
    zone: 17,
    tags,
    priority: 30,
    headline: 'New mode — CW Asiatic Russia',
    mode,
    freqMhz: null,
  })

  it('drops a CW new-mode need from a 30m FT8 surface (the operator report)', () => {
    expect(tagsForSurface(ar(['NewMode'], 'CW'), '30m', 'FT8')).toEqual([])
  })

  it('keeps a digital new-mode need on a digital surface', () => {
    expect(tagsForSurface(ar(['NewMode'], 'FT8'), '30m', 'FT8')).toEqual(['NewMode'])
  })

  it('matches the backend submode vocabulary against a class label', () => {
    // The backend sends mode: 'FT8'/'FT4'/'RTTY' verbatim for digital rows, but the
    // decode feed describes itself as the class 'Digital'. Raw === never matched.
    for (const m of ['FT8', 'FT4', 'RTTY', 'Digital']) {
      expect(tagsForSurface(ar(['NewMode'], m), '30m', 'Digital')).toEqual(['NewMode'])
    }
  })

  it('folds band-label case (a real log carries both 30M and 30m)', () => {
    expect(tagsForSurface(ar(['NewBand'], 'FT8', '30m'), '30M', 'FT8')).toEqual(['NewBand'])
    expect(tagsForSurface(ar(['NewBand'], 'FT8', ' 30M '), '30m', 'FT8')).toEqual(['NewBand'])
  })

  it('keeps an all-time-new entity on any band and any mode', () => {
    expect(tagsForSurface(ar(['NewEntity'], 'CW', '20m'), '30m', 'FT8')).toEqual(['NewEntity'])
  })

  it('drops a cross-band new-band claim', () => {
    expect(tagsForSurface(ar(['NewBand'], 'FT8', '20m'), '30m', 'FT8')).toEqual([])
  })

  it('keeps a same-class new-mode need scored on another band (the predicate is entity-wide)', () => {
    // worked_mode is keyed (entity, mode-class) with NO band, so a digital mode need
    // is closable on any band — including this one.
    expect(tagsForSurface(ar(['NewMode'], 'FT8', '20m'), '30m', 'FT8')).toEqual(['NewMode'])
  })

  it('drops a cross-band confirmation (confirmed_band IS per band)', () => {
    expect(tagsForSurface(ar(['Confirm'], 'FT8', '20m'), '30m', 'FT8')).toEqual([])
  })

  it('keeps program flags regardless of band or mode', () => {
    expect(tagsForSurface(ar(['Pota', 'Sota', 'Dxped', 'Wanted'], 'CW', '20m'), '30m', 'FT8')).toEqual(
      ['Pota', 'Sota', 'Dxped', 'Wanted'],
    )
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

describe('alertsByCall → topNeedByCall (the shared host chain)', () => {
  // The exact pre-fix failure, end to end through the SHARED helpers every host
  // must use: the backend hands alerts out priority-DESCENDING, so a last-wins
  // grouping coloured a NewEntity-on-20m + Confirm-on-40m call as the dim
  // Confirm — but only on whichever pop-out re-rolled the map by hand.
  it('NewEntity on 20m + Confirm on 40m resolves to NewEntity', () => {
    const alerts = [ranked(['NewEntity'], 100, '20m'), ranked(['Confirm'], 10, '40m')]
    const top = topNeedByCall(alertsByCall(alerts))
    expect(top.get(alerts[0].call.toUpperCase())).toBe('NewEntity')
  })
})

describe('topNeedByCall', () => {
  // The map every surface colours rows from — roster, band strip and map. It used to be
  // built by writing each alert into the same slot with NO guard, so the last one won; the
  // backend orders alerts priority-descending, making that reliably the WEAKEST.
  it('colours a multi-band call from its STRONGEST alert', () => {
    const byCall = new Map([
      ['DL1ABC', [ranked(['NewEntity'], 100, '20m'), ranked(['Confirm'], 10, '40m')]],
    ])
    expect(topNeedByCall(byCall).get('DL1ABC')).toBe('NewEntity')
  })

  it('does not depend on the order the alerts arrive in', () => {
    const strong = ranked(['NewEntity'], 100, '20m')
    const weak = ranked(['Confirm'], 10, '40m')
    expect(topNeedByCall(new Map([['W1AW', [strong, weak]]])).get('W1AW')).toBe('NewEntity')
    expect(topNeedByCall(new Map([['W1AW', [weak, strong]]])).get('W1AW')).toBe('NewEntity')
  })

  it('takes the lead tag of that alert, not the highest tag across all of them', () => {
    // tags[0] within one alert is already its own strongest reason (the backend sorts it),
    // so the answer is "the best alert's lead tag" — not a union re-ranked here.
    const byCall = new Map([['K9LC', [ranked(['NewBand', 'Pota'], 50)]]])
    expect(topNeedByCall(byCall).get('K9LC')).toBe('NewBand')
  })

  it('omits a call whose every alert is tagless, so has() still means "needed"', () => {
    const byCall = new Map([['NOBODY', [ranked([], 99)]]])
    expect(topNeedByCall(byCall).has('NOBODY')).toBe(false)
  })

  it('keeps the calls apart and passes the keys through untouched', () => {
    const byCall = new Map([
      ['A', [ranked(['NewGrid'], 55)]],
      ['B', [ranked(['NewMode'], 30)]],
    ])
    const m = topNeedByCall(byCall)
    expect([...m]).toEqual([
      ['A', 'NewGrid'],
      ['B', 'NewMode'],
    ])
    expect(topNeedByCall(new Map()).size).toBe(0)
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

// ── The composition point ────────────────────────────────────────────────────────────
// Two independently-correct systems meet here: the surface gate decides what a band +
// mode class can legitimately claim, and strongestNeed/chaseRank rank what survives.
// Feeding ungated alerts into the ranking would let a need the operator CANNOT close
// pick a row's colour and its place in "sort by need" — which is the field report.
describe('alertsForSurface (gate, then rank)', () => {
  const a = (tags: NeedTag[], mode: string, band: string, priority: number): NeedAlert => ({
    call: 'RF9C',
    entity: 'Asiatic Russia',
    band,
    zone: 17,
    tags,
    priority,
    headline: '',
    mode,
    freqMhz: null,
  })

  it('drops an alert with nothing left to claim here', () => {
    // A CW new-mode need on a 30m FT8 roster: real, but not closable on this surface.
    expect(alertsForSurface([a(['NewMode'], 'CW', '30m', 30)], '30m', 'FT8')).toEqual([])
  })

  it('keeps an alert rewritten to only its applicable tags', () => {
    const got = alertsForSurface([a(['NewEntity', 'Confirm'], 'FT8', '20m', 100)], '30m', 'FT8')
    expect(got).toHaveLength(1)
    // NewEntity is band-agnostic and survives; Confirm is per-band and does not.
    expect(got[0].tags).toEqual(['NewEntity'])
    expect(got[0].priority).toBe(100) // lead tag survived — keep the backend's number
  })

  it('repairs priority when the gate removes the alert LEAD tag', () => {
    // 30m CW alert tagged [NewMode, NewZone] seen on the 30m FT8 roster. The zone need is
    // genuine and stays, but the backend priority of 30 was computed from NewMode — left
    // alone it would rank a needed CQ zone below a bare new band.
    const got = alertsForSurface([a(['NewMode', 'NewZone'], 'CW', '30m', 30)], '30m', 'FT8')
    expect(got).toHaveLength(1)
    expect(got[0].tags).toEqual(['NewZone'])
    expect(got[0].priority).toBe(NEED_TIER.NewZone) // 70, not 30
  })

  it('leaves the ungated alert objects untouched (no mutation)', () => {
    const src = a(['NewMode', 'NewZone'], 'CW', '30m', 30)
    alertsForSurface([src], '30m', 'FT8')
    expect(src.tags).toEqual(['NewMode', 'NewZone'])
    expect(src.priority).toBe(30)
  })

  it('handles a missing alerts map', () => {
    expect(alertsForSurface(undefined, '30m', 'FT8')).toEqual([])
    expect(alertsForSurface(null, '30m', 'FT8')).toEqual([])
  })

  it('ranks only what survives the gate — the field report end to end', () => {
    // The operator on 30m FT8. Two alerts for one call: a CW new-mode (priority 30, NOT
    // closable here) and a digital confirm on this band (priority 10, closable).
    const alerts = [a(['NewMode'], 'CW', '30m', 30), a(['Confirm'], 'FT8', '30m', 10)]
    // Ungated, the CW need is "strongest" and would colour the row MODE — the false claim.
    expect(strongestNeed(alerts)?.tags[0]).toBe('NewMode')
    // Gated first, the only real claim on this surface wins.
    const gated = alertsForSurface(alerts, '30m', 'FT8')
    expect(strongestNeed(gated)?.tags[0]).toBe('Confirm')
    expect(chaseRank(gated, 'Confirm')).toBe(10)
  })

  it('still lets the strongest CLOSABLE need win over a weaker one', () => {
    const alerts = [a(['Confirm'], 'FT8', '30m', 10), a(['NewEntity'], 'FT8', '30m', 100)]
    const gated = alertsForSurface(alerts, '30m', 'FT8')
    expect(strongestNeed(gated)?.tags[0]).toBe('NewEntity')
    expect(chaseRank(gated, 'NewEntity')).toBe(100)
  })
})

describe('the mode label the Needed board shows never changes where a click lands', () => {
  // Reported on 1.0.3: some rows read "Digital" and some "FT8" for the same kind of station.
  // The backend now sends the specific mode when the dial identifies one. That is a DISPLAY
  // change only, and this is the guard that keeps it one: routing matches 'CW'/'Phone'/'RTTY'
  // exactly and defaults everything else to Operate, so both labels must take the same arm.
  const alert = (mode: string, freqMhz: number | null = 14.074) =>
    ({
      call: 'W1AW',
      band: '20m',
      mode,
      freqMhz,
      entity: 'United States',
      zone: 5,
      tags: [],
      headline: '',
      priority: 1,
    }) as unknown as Parameters<typeof workTarget>[0]

  it('FT8 and Digital pick the same cockpit and the same dial', () => {
    const a = workTarget(alert('Digital'), [])
    const b = workTarget(alert('FT8'), [])
    expect(a).not.toBeNull()
    expect(b).toEqual(a)
    expect(b?.view).toBe('operate')
  })

  it('...and so does FT4, the other label the spot path can now produce', () => {
    expect(workTarget(alert('FT4'), [])?.view).toBe('operate')
    expect(workTarget(alert('MSK144'), [])?.view).toBe('operate')
  })

  it('CW and Phone are still routed by name — the change must not disturb them', () => {
    expect(workTarget(alert('CW', null), [])?.view).toBe('cw')
    expect(workTarget(alert('Phone', null), [])?.view).toBe('phone')
    expect(workTarget(alert('RTTY'), [])?.view).toBe('rtty')
  })
})
