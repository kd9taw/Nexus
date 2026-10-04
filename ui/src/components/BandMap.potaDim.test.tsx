// @vitest-environment jsdom
// EVERY POTA ACTIVATOR'S TICK WEARS A DIM POTA COLOUR, in Band activity and its pop-out band map
// (operator, 2026-10-03, "Dim POTA color for all": "Every activator gets a dim POTA color. A park you
// still need keeps the full new-park color, and a real need such as a new country still takes its
// color first. SOTA (S) and DXpeditions (✈) stay as they are.").
//
// The precedence, one colour per tick:
//   1. a real need's own colour (a new country, band, mode and the rest);
//   2. a park still to be worked: the full new-park colour (NewPark, `need-pota`);
//   3. any other POTA activator: the dim POTA colour (`pota-dim`);
//   4. plain.
// Each case below gives the candidates DIFFERENT colours and asserts the winner by its class set,
// so a rule that fired in the wrong order cannot pass. The maps are built the way App and the
// pop-out build them (`visibleNeeds` → `alertsByCall` → `topNeedByCall` / `activityTypeByCall`).
//
// THE DIM COLOUR READS THE P's OWN SOURCE: `typeByCall`, from the same gated alerts. Whatever takes
// the P away takes the colour with it, and the filters that act on needs (the CW/Phone mode gate,
// the band scopes) leave both standing together, because an activation is not a need.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import type { ReactElement } from 'react'
import { BandMap } from './BandMap'
import { BandStrip } from './BandStrip'
import { activityTypeByCall, alertsByCall, topNeedByCall, visibleNeeds, type NeedBandScopes } from '../features/needs'
import type { NeedAlert, NeedTag, SpotRow } from '../types'

afterEach(cleanup)

function spot(call: string, freqMhz: number, over: Partial<SpotRow> = {}): SpotRow {
  return {
    call,
    entity: '',
    zone: 0,
    band: '20m',
    freqMhz,
    mode: 'Phone',
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 10,
    comment: '',
    licensed: true,
    ...over,
  }
}

function alert(call: string, tags: NeedTag[], over: Partial<NeedAlert> = {}): NeedAlert {
  return { call, entity: '', band: '20m', zone: 0, tags, priority: 0, headline: '', mode: 'Phone', freqMhz: null, ...over }
}

/** The two maps exactly as the hosts derive them from the backend's alerts. */
function hostMaps(alerts: NeedAlert[], enabled = { cw: true, phone: true }, scopes?: NeedBandScopes) {
  const gated = visibleNeeds(alerts, enabled, scopes)
  return { needByCall: topNeedByCall(alertsByCall(gated)), typeByCall: activityTypeByCall(gated) }
}

type Maps = ReturnType<typeof hostMaps>

/** A colour class on a tick: a need's (`is-need`, `need-*`) or the dim POTA one. */
const colours = (el: Element | null | undefined) =>
  [...(el?.classList ?? [])].filter((c) => c === 'is-need' || c === 'pota-dim' || c.startsWith('need-')).sort()

interface Surface {
  name: string
  mode: 'Phone' | 'CW'
  render: (spots: SpotRow[], maps: Maps, mode: 'Phone' | 'CW') => ReactElement
  /** The station's spot (its button), its tick, and the marks the tick's colour may land on. */
  marks: (root: Element, call: string) => { spot: Element; tick: Element; painted: Element[] }
  /** The class set a real need's colour puts on a tick here (`is-need` widens the strip's bar). */
  need: (cls: string) => string[]
}

const SURFACES: Surface[] = [
  {
    name: 'Band activity (BandStrip)',
    mode: 'Phone',
    render: (spots, maps, mode) => (
      <BandStrip
        band="20m"
        dialMhz={mode === 'CW' ? 14.025 : 14.2}
        txAllowed
        spots={spots}
        spotMode={mode}
        needByCall={maps.needByCall}
        typeByCall={maps.typeByCall}
        onWorkSpot={() => {}}
      />
    ),
    marks: (root, call) => {
      const spot = [...root.querySelectorAll('.bandstrip-spot')].find((b) => b.querySelector('.bandstrip-spot-call')?.textContent === call)
      if (!spot) throw new Error(`no strip spot for ${call}`)
      const tick = spot.querySelector('.bandstrip-tick')!
      return { spot, tick, painted: [tick] }
    },
    need: (cls) => ['is-need', `need-${cls}`],
  },
  {
    name: 'the pop-out band map (BandMap)',
    mode: 'Phone',
    render: (spots, maps, mode) => (
      <BandMap
        band="20m"
        dialMhz={mode === 'CW' ? 14.025 : 14.2}
        txAllowed
        spots={spots}
        spotMode={mode}
        needByCall={maps.needByCall}
        typeByCall={maps.typeByCall}
        onWorkSpot={() => {}}
      />
    ),
    // The map draws the tick at the true frequency and the label's left edge beside it; the
    // colour is on both, so both are held.
    marks: (root, call) => {
      const spot = [...root.querySelectorAll('.bandmap-spot')].find((b) => b.querySelector('.bandmap-call')?.textContent === call)
      if (!spot) throw new Error(`no map spot for ${call}`)
      const tick = spot.previousElementSibling!
      expect(tick.classList.contains('bandmap-tick'), `${call}: the tick sits beside its label`).toBe(true)
      return { spot, tick, painted: [tick, spot] }
    },
    need: (cls) => [`need-${cls}`],
  },
]

const badgeOf = (spot: Element) => spot.querySelector('.spot-type-badge:not(.type-beacon)')?.textContent ?? null

describe.each(SURFACES)('$name', (s) => {
  // One station per rule, each a candidate for a DIFFERENT colour.
  const STATIONS = [
    spot('K1WRK', 14.205), // activating a park already worked today
    spot('K1NEW', 14.22), // activating a park still to be worked
    spot('VP8NEW', 14.24), // a new country, activating a park still to be worked
    spot('K1PLN', 14.26), // not activating, nothing needed
    spot('K1BND', 14.28), // not activating, a new band
    spot('W7SOT', 14.3), // on a summit, nothing needed
    spot('3Y0DX', 14.32), // a DXpedition, nothing needed
  ]
  const ALERTS = [
    alert('K1WRK', ['Pota']),
    alert('K1NEW', ['NewPark', 'Pota']),
    alert('VP8NEW', ['NewEntity', 'NewPark', 'Pota']),
    alert('K1BND', ['NewBand']),
    alert('W7SOT', ['Sota']),
    alert('3Y0DX', ['Dxped']),
  ]
  const shown = (spots = STATIONS, maps = hostMaps(ALERTS)) => render(s.render(spots, maps, s.mode)).container

  it('a worked-park activator wears the dim POTA colour, beside its P', () => {
    const k = s.marks(shown(), 'K1WRK')
    for (const el of k.painted) expect(colours(el)).toEqual(['pota-dim'])
    expect(badgeOf(k.spot)).toBe('P')
  })

  it('a park still to be worked keeps the full new-park colour, never the dim one', () => {
    const k = s.marks(shown(), 'K1NEW')
    for (const el of k.painted) expect(colours(el)).toEqual(s.need('pota'))
    expect(badgeOf(k.spot)).toBe('P')
  })

  it('a new country that is activating takes the country colour, over the park and the dim one', () => {
    const k = s.marks(shown(), 'VP8NEW')
    for (const el of k.painted) expect(colours(el)).toEqual(s.need('entity'))
    expect(badgeOf(k.spot)).toBe('P')
  })

  it('a station that is not activating stays plain, and a need of its own keeps its colour', () => {
    const root = shown()
    for (const el of s.marks(root, 'K1PLN').painted) expect(colours(el)).toEqual([])
    for (const el of s.marks(root, 'K1BND').painted) expect(colours(el)).toEqual(s.need('band'))
    expect(badgeOf(s.marks(root, 'K1PLN').spot)).toBeNull()
  })

  it('SOTA and DXpeditions are unchanged: their badge, and no colour', () => {
    const root = shown()
    for (const [call, badge] of [['W7SOT', 'S'], ['3Y0DX', '✈']] as const) {
      const k = s.marks(root, call)
      for (const el of k.painted) expect(colours(el), call).toEqual([])
      expect(badgeOf(k.spot), call).toBe(badge)
    }
  })

  it('a beacon row stays plain even when its call is activating (a beacon is never worth working)', () => {
    const root = shown([spot('K1WRK', 14.1, { beacon: 'ncdxf' })])
    for (const el of s.marks(root, 'K1WRK').painted) expect(colours(el)).toEqual([])
  })

  it('wherever a tick is dim the P is there, and every P with nothing to colour it is dim', () => {
    const root = shown()
    for (const st of STATIONS) {
      const k = s.marks(root, st.call)
      const dim = colours(k.tick).includes('pota-dim')
      const needs = colours(k.tick).some((c) => c.startsWith('need-'))
      expect(dim, `${st.call}: dim only with a P`).toBe(badgeOf(k.spot) === 'P' && !needs)
    }
  })

  describe('the dim colour follows the P through the filters', () => {
    it('an activator the alerts no longer carry has neither the P nor the dim colour', () => {
      // Nothing in the alert list for K1GON: the backend drops a live activator whose spot has
      // aged out of the activation window or is off the band plan. The P goes, so does the colour,
      // even though the spotter's comment still names a park: the comment is not the source.
      const root = shown([spot('K1GON', 14.21, { comment: 'POTA US-1234' })], hostMaps(ALERTS))
      const k = s.marks(root, 'K1GON')
      expect(badgeOf(k.spot)).toBeNull()
      for (const el of k.painted) expect(colours(el)).toEqual([])
    })

    it('a band scope that withholds the new-country colour leaves the activator dim, with its P', () => {
      const atno = [alert('VP8ATN', ['NewEntity', 'Pota'])]
      const stations = [spot('VP8ATN', 14.25)]
      // New ones scoped to VHF and up: on 20 m the country colour is withheld, the activation is not.
      const scoped = s.marks(shown(stations, hostMaps(atno, undefined, { dxcc: 'vhf' })), 'VP8ATN')
      for (const el of scoped.painted) expect(colours(el)).toEqual(['pota-dim'])
      expect(badgeOf(scoped.spot)).toBe('P')
      cleanup()
      // The control: the same alert in scope takes the country colour.
      const open = s.marks(shown(stations, hostMaps(atno)), 'VP8ATN')
      for (const el of open.painted) expect(colours(el)).toEqual(s.need('entity'))
    })

    it('the CW mode gate drops a CW award colour and keeps the P with the dim colour', () => {
      const cw = [alert('VP8CW', ['NewEntity', 'Pota'], { mode: 'CW' })]
      const stations = [spot('VP8CW', 14.03, { mode: 'CW' })]
      const off = s.marks(render(s.render(stations, hostMaps(cw, { cw: false, phone: true }), 'CW')).container, 'VP8CW')
      for (const el of off.painted) expect(colours(el)).toEqual(['pota-dim'])
      expect(badgeOf(off.spot)).toBe('P')
      cleanup()
      const on = s.marks(render(s.render(stations, hostMaps(cw), 'CW')).container, 'VP8CW')
      for (const el of on.painted) expect(colours(el)).toEqual(s.need('entity'))
    })
  })
})
