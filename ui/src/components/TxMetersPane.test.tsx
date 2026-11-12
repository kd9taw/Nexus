// @vitest-environment jsdom
//
// THE LARGE TX METERS PANE. A READOUT that stands in the pane vocabulary of Operate, Phone, CW and SSTV and
// ships hidden: display only, with every control and the TX chrome outside it. Its size is computed over
// the REAL sheets (cssCascade: importance, specificity, source order), never read off them with a regex.
// ⚠️ jsdom does not lay out: the pixels are the real-Chrome sweep's.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt, type Mode } from '../cssCascade'
import { CockpitTxStrip } from './CockpitTxStrip'
import { paneRoleStyle } from './panes/CockpitPaneFrame'
import { TxMeters, TxMetersCell, TxMetersPane, TX_METERS_PANE_WEIGHT, alcBar, poBar, swrBar } from './TxMeters'
import { LARGE_FACE_FLOOR_EM, largeFaceColumns, needleScale } from './TxNeedles'
import {
  CW_PANELS,
  OPERATE_ARRANGE,
  OPERATE_PANELS,
  PHONE_PANELS,
  SSTV_PANELS,
  coercePanelLayout,
  panelStateIn,
  type PanelVocabulary,
} from '../features/panelState'
import type { RadioStatus } from '../types'

afterEach(() => {
  cleanup()
  localStorage.clear()
})

const ID = 'txMetersPane'
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const MODE: Mode = 'dark'
const win = (el: Element, prop: string) => winnerAt(RULES, MODE, chainOf(el), prop)?.value.trim() ?? null

const KEYED = {
  rigKeyed: true,
  swrScaleVerified: true,
  txAlc: 0.5,
  txPoW: 1500,
  ratedWatts: 1500,
  txSwr: 1.4,
  txCompDb: 6,
  paTempC: 47,
} as unknown as RadioStatus
// SSTV joined with the composer re-flow: the same pane, the same hidden start.
const VOCABS = [OPERATE_PANELS, PHONE_PANELS, CW_PANELS, SSTV_PANELS] as unknown as PanelVocabulary<string>[]

describe('the vocabulary', () => {
  it.each(VOCABS.map((v) => [v.view, v] as const))('%s lists it and ships it hidden', (_view, spec) => {
    expect(spec.panelIds).toContain(ID)
    expect(spec.defaultRemoved).toContain(ID)
    expect(panelStateIn(spec, coercePanelLayout(spec, null), ID)).toBe('removed')
  })

  it.each(VOCABS.map((v) => [v.view, v] as const))('%s gives no TX or safety control an id', (_view, spec) => {
    const words = /ptt|stop|abort|tune|send|macro|latch|txdock|txstrip|comp\b|proc\b|watchdog/i
    expect(spec.panelIds.filter((id) => words.test(id))).toEqual([])
  })

  it('FT: an added pane in both layouts, so ticking it in one leaves the other as it is', () => {
    expect(OPERATE_ARRANGE.classic.extra).toContain(ID)
    expect(OPERATE_ARRANGE.roster.extra).toContain(ID)
  })
})

describe('its record is clamped on load, like every pane’s', () => {
  it.each(VOCABS.map((v) => [v.view, v] as const))('%s: a stored share outside the writers’ range', (_view, spec) => {
    const load = (share: unknown) =>
      coercePanelLayout(spec, { v: 2, state: { [ID]: 'docked' }, share: { [ID]: share } }).share as Record<string, number>
    expect(load(1e-9)[ID]).toBe(0.15)
    expect(load(1e9)[ID]).toBe(1.85)
    expect(load(1)[ID]).toBe(1)
    for (const junk of [-1, 0, 'big', null, Number.NaN]) expect(load(junk)[ID]).toBeUndefined()
  })

  it.each(VOCABS.map((v) => [v.view, v] as const))('%s: a stored state that is not one', (_view, spec) => {
    const layout = coercePanelLayout(spec, { v: 2, state: { [ID]: 'gigantic' } })
    expect(panelStateIn(spec, layout, ID)).toBe('removed')
  })
})

describe('the pane’s body is a readout', () => {
  const pane = () => render(<TxMeters radio={KEYED} pane />).container

  it('no control of any kind, and neither COMP nor the temperatures', () => {
    const c = pane()
    expect(c.querySelector('button, input, select, textarea, a, [role="slider"], [tabindex]')).toBeNull()
    expect(c.querySelector('.ph-txmeter, .ph-txmeter-track')).toBeNull()
    expect([...c.querySelectorAll('[data-meter]')].map((m) => m.getAttribute('data-meter'))).toEqual(['po', 'alc', 'swr'])
  })

  it('needles whichever way the strip draws its meters (Bars is the stored default here)', () => {
    expect(pane().querySelectorAll('.ph-txneedle-needle')).toHaveLength(3)
  })

  it('the same reading as the strip, not a second source, and in words for a screen reader', () => {
    const c = pane()
    const po = c.querySelector('[data-meter="po"] svg')!
    expect(po.getAttribute('role')).toBe('img')
    expect(po.getAttribute('aria-label')).toBe('PO 1500 W')
    expect(c.querySelector('[data-meter="po"] .txmeters-pane-value')?.textContent).toBe('1500 W')
    expect(c.querySelector('[data-meter="alc"] .txmeters-pane-value')?.textContent).toBe('50%')
    expect(c.querySelector('[data-meter="po"] .ph-txneedle-needle')?.getAttribute('data-frac')).toBe('1.000')
  })

  it('every face is scaled in the drawing: its value in the SVG, so nothing can be ellipsized', () => {
    const c = pane()
    const value = c.querySelector('.txmeters-pane-value')!
    expect(value.closest('svg')).not.toBeNull()
    expect(win(value, 'text-overflow')).toBeNull()
    expect(win(value, 'overflow')).toBeNull()
  })
})

// Operator, 1.24.0: "the dials on the new tx meters are way too large, reduce them by 50%". Half as wide and half as
// high in the same pane, with the name and value no smaller; the pixels on the real screens are
// ui/probe/txmeterspane-probe.mjs's.
describe('the dial is half 1.24.0’s, the readout is not', () => {
  const DIAL = '.ph-txneedle-scale, .ph-txneedle-zone, .ph-txneedle-tick, .ph-txneedle-peak, .ph-txneedle-needle'
  const dialOf = (face: Element) => {
    const g = face.querySelector('g.txmeters-pane-dial')!
    const m = /^translate\(([-\d.]+) ([-\d.]+)\) scale\(([\d.]+)\)$/.exec(g.getAttribute('transform') ?? '')!
    return { g, tx: Number(m[1]), ty: Number(m[2]), k: Number(m[3]) }
  }

  it.each(['po', 'alc', 'swr'])('%s: the whole dial is drawn at half, standing just above the band', (key) => {
    const face = render(<TxMeters radio={KEYED} pane />).container.querySelector(`[data-meter="${key}"]`)!
    const { g, tx, ty, k } = dialOf(face)
    expect(k).toBe(0.5)
    // The middle of the band's top edge (50, 38) goes to (50, 41): the dial is centred over the band and sits 3 units
    // into it, so the needle's foot runs in under the band and the numbers have their room above.
    expect(tx + 50 * k).toBeCloseTo(50, 6)
    expect(ty + 38 * k).toBeCloseTo(41, 6)
    const parts = [...face.querySelectorAll(DIAL)]
    expect(parts.length).toBeGreaterThan(10)
    for (const p of parts) expect(g.contains(p)).toBe(true)
  })

  it.each(['po', 'alc', 'swr'])('%s: the scale’s numbers are NOT halved: outside the dial’s scale, at 1.24.0’s size, where the dial puts them', (key) => {
    const face = render(<TxMeters radio={KEYED} pane />).container.querySelector(`[data-meter="${key}"]`)!
    const { g, tx, ty, k } = dialOf(face)
    const nums = [...face.querySelectorAll('.txmeters-pane-scale')]
    expect(nums).toHaveLength(5)
    for (const n of nums) {
      expect(g.contains(n)).toBe(false)
      // 5 % over 1.24.0's 4.6, like the name and value: at 0.96 of 1.24.0's scale 4.6 rounded a pixel under it.
      const fs = parseFloat(win(n, 'font-size') ?? '')
      expect(fs).toBeGreaterThanOrEqual(4.6)
      expect(fs).toBeLessThanOrEqual(4.6 * 1.05)
      // On the scaled dial's own frame: the point (x, y) of the dial's units lands at (tx + kx, ty + ky), so mapped
      // back each number stands on the dial's centre line, its baseline outside the needle's tip (R + 2 = 62).
      const x = (Number(n.getAttribute('x')) - tx) / k
      const y = (Number(n.getAttribute('y')) - ty) / k
      expect(Math.hypot(x - 50, y - 63)).toBeGreaterThan(62 + 4)
    }
  })

  it('the band, the name and the value are outside it, no smaller than 1.24.0’s 6.5 and 9', () => {
    const face = render(<TxMeters radio={KEYED} pane />).container.querySelector('[data-meter="po"]')!
    const g = face.querySelector('g.txmeters-pane-dial')!
    for (const sel of ['.txmeters-pane-band', '.txmeters-pane-label', '.txmeters-pane-value']) {
      const el = face.querySelector(sel)!
      expect(el).not.toBeNull()
      expect(g.contains(el)).toBe(false)
    }
    const px = (sel: string) => parseFloat(win(face.querySelector(sel)!, 'font-size') ?? '')
    expect(px('.txmeters-pane-label')).toBeGreaterThanOrEqual(6.5)
    expect(px('.txmeters-pane-value')).toBeGreaterThanOrEqual(9)
    // …and only a hair over: the drawing stands at about 1.24.0's scale, so these are about 1.24.0's pixels.
    expect(px('.txmeters-pane-label')).toBeLessThanOrEqual(6.5 * 1.05)
    expect(px('.txmeters-pane-value')).toBeLessThanOrEqual(9 * 1.05)
  })

  it('the drawing ends at the top of the half-size dial: nothing above it', () => {
    const svg = render(<TxMeters radio={KEYED} pane />).container.querySelector('[data-meter="po"] svg')!
    // 1.24.0's was 0 −9 100 59: the dial from −9 down to the band at 38, then the band to 50. Half the dial
    // stands on the band from 14.5, so the drawing is 35.5 high.
    expect(svg.getAttribute('viewBox')).toBe('0 14.5 100 35.5')
  })
})

describe('FT’s Band Activity beside it keeps its floor (review 2026-11-12, F1)', () => {
  // Band Activity (weight 1.6) shares a divider with the meters pane in FT's arranged rail, and the pair's split is
  // the mean of the two weights (OperateCockpit `fillPairs`): 1.3 beside 1.24.0's weight 1, 1.115 beside 0.63. So its
  // inline style is NOT byte for byte 1.24.0's: `var(--pane-share, 1.3) / 1.3` became `var(--pane-share, 1.115) /
  // 1.115`. What the style computes is: its floor is the stock floor times the operator's share, the same number.
  const floorFactor = (st: ReturnType<typeof paneRoleStyle>) => {
    const m = /var\(--pane-share, ([\d.]+)\) \/ ([\d.]+)\)/.exec(String(st.minHeight))!
    const share = (st as Record<string, unknown>)['--pane-share']
    return (share != null ? Number(share) : Number(m[1])) / Number(m[2])
  }
  const grow = (st: ReturnType<typeof paneRoleStyle>) => {
    const share = (st as Record<string, unknown>)['--pane-share']
    return share != null ? Number(share) : Number(/var\(--pane-share, ([\d.]+)\) 1 0/.exec(String(st.flex))![1])
  }
  const old = (1.6 + 1) / 2
  const now = (1.6 + TX_METERS_PANE_WEIGHT.operate) / 2
  it.each([
    ['no share stored', undefined],
    ['a 1.24.0 drag stored, Band Activity smaller', 0.7],
    ['a 1.24.0 drag stored, Band Activity larger', 1.4],
  ] as const)('%s: the same floor as 1.24.0', (_what, share) => {
    const before = paneRoleStyle({ weight: 1.6, split: old, share: share != null ? share * old : undefined })
    const after = paneRoleStyle({ weight: 1.6, split: now, share: share != null ? share * now : undefined })
    expect(after.minHeight).not.toBe(before.minHeight) // the string moved…
    expect(floorFactor(after)).toBeCloseTo(floorFactor(before), 9) // …the floor did not
  })

  it('a stored drag keeps its proportion: both panes of the pair scale by the same split', () => {
    const pair = (split: number, a: number, b: number) => [
      grow(paneRoleStyle({ weight: 1.6, split, share: a * split })),
      grow(paneRoleStyle({ weight: TX_METERS_PANE_WEIGHT.operate, split, share: b * split, lowFloor: '--txmeters-pane-floor' })),
    ]
    const [b1, m1] = pair(old, 1.3, 0.7)
    const [b2, m2] = pair(now, 1.3, 0.7)
    expect(b2 / m2).toBeCloseTo(b1 / m1, 9)
  })
})

describe('the pane gives the room back (operator, 1.24.0: shrink the pane too)', () => {
  it('its frame names the low floor, which only it reads, and the sheet sets it lower than a feed’s 10em', () => {
    const c = render(<TxMetersPane radio={KEYED} weight={TX_METERS_PANE_WEIGHT.operate} />).container
    const frame = c.querySelector<HTMLElement>('[data-pane="txMetersPane"]')!
    expect(frame.style.minHeight).toBe('min(var(--cockpit-fill-min, 0px), var(--txmeters-pane-floor, var(--cockpit-fill-min, 0px)))')
    const app = render(<div className="app" />).container.querySelector('.app')!
    const floor = win(app, '--txmeters-pane-floor')
    expect(floor).toMatch(/^min\(([\d.]+)em, 100%\)$/)
    expect(Number(/([\d.]+)em/.exec(floor!)![1])).toBeLessThan(10)
    // No other rule in either sheet names it: no other pane's floor can move with it.
    expect(RULES.filter((r) => r.decls.some((d) => `${d.prop}:${d.value}`.includes('--txmeters-pane-floor'))).map((r) => r.selector)).toEqual(['.app'])
  })

  it('takes less than a feed’s share of its column in every cockpit, and its own weight as its grow', () => {
    for (const [cockpit, w] of Object.entries(TX_METERS_PANE_WEIGHT)) {
      expect(w, cockpit).toBeGreaterThan(0.4)
      expect(w, cockpit).toBeLessThan(1)
    }
    const c = render(<TxMetersPane radio={KEYED} weight={TX_METERS_PANE_WEIGHT.sstv} />).container
    expect(c.querySelector('[data-pane="txMetersPane"]')!.getAttribute('style')).toContain(`flex: var(--cockpit-pane-flex, ${TX_METERS_PANE_WEIGHT.sstv} 1 0)`)
  })
})

describe('the scale reads what the bars read', () => {
  it('power: full scale is the radio’s rated output', () => {
    for (const rated of [5, 100, 1500]) {
      const scale = needleScale('po', { ratedWatts: rated, swrVerified: true })
      expect(scale.map((s) => s.frac)).toEqual([0, 0.25, 0.5, 0.75, 1])
      for (const s of scale) expect(poBar(Number(s.text), rated).frac).toBeCloseTo(s.frac, 1)
      expect(scale[4].text).toBe(String(rated))
    }
  })

  it('ALC in percent, SWR from 1:1 to 3:1', () => {
    for (const s of needleScale('alc', { ratedWatts: 100, swrVerified: true })) {
      expect(alcBar(Number(s.text) / 100).frac).toBeCloseTo(s.frac, 5)
    }
    const swr = needleScale('swr', { ratedWatts: 100, swrVerified: true })
    expect(swr.map((s) => s.text)).toEqual(['1', '1.5', '2', '2.5', '3'])
    for (const s of swr) expect(swrBar(Number(s.text)).frac).toBeCloseTo(s.frac, 5)
  })

  it('no numbers on an SWR scale Nexus cannot stand behind (the reading stays, marked SWR?)', () => {
    expect(needleScale('swr', { ratedWatts: 100, swrVerified: false })).toEqual([])
  })

  it('the labels are drawn, and grow with the face because they are in its viewBox', () => {
    const c = render(<TxMeters radio={KEYED} pane />).container
    expect([...c.querySelectorAll('[data-meter="swr"] .txmeters-pane-scale')].map((e) => e.textContent)).toEqual(['1', '1.5', '2', '2.5', '3'])
    // A major tick at each label, and finer ones between them.
    expect(c.querySelectorAll('[data-meter="swr"] .ph-txneedle-tick').length).toBeGreaterThan(5)
  })
})

describe('its size, computed', () => {
  it('fills its pane body in the measured columns, with a floor a pane body can scroll', () => {
    const c = render(<div className="pane-body"><TxMeters radio={KEYED} pane /></div>).container
    const root = c.querySelector<HTMLElement>('.txmeters-pane')!
    expect(win(root, 'display')).toBe('grid')
    expect(win(root, 'grid-template-columns')).toBe('repeat(var(--txmeters-pane-cols, 3), minmax(0, 1fr))')
    // The floor is the track's: a floor on the face alone leaves the track short, and the next row
    // stands over it. The pane body scrolls what no longer fits.
    expect(win(root, 'grid-auto-rows')).toBe(`minmax(${LARGE_FACE_FLOOR_EM}em, 1fr)`)
    expect(win(root, 'height')).toBe('100%')
    // Not measured (jsdom lays nothing out): all three in a row.
    expect(root.style.getPropertyValue('--txmeters-pane-cols')).toBe('3')
    const meter = c.querySelector('.txmeters-pane-meter')!
    expect(win(meter, 'min-width')).toBe('0')
    expect(win(meter, 'min-height')).toBeNull()
  })

  it('no strip or dock rule reaches it: it is not `.ph-txneedle`', () => {
    const c = render(<TxMeters radio={KEYED} pane />).container
    expect(c.querySelector('.ph-txneedle, .ph-txmeters')).toBeNull()
  })

  it('structure stays in the fenced sheet: styles.css has no rule for its box', () => {
    const styles = parseRules(sheet('styles.css'))
    const hits = styles.map((r) => r.selector).filter((s) => /\.txmeters-pane(-meter)?(?![a-z0-9-])/.test(s))
    expect(hits.filter((s) => /\.txmeters-pane(-meter)?$/.test(s.trim()))).toEqual([])
  })
})

describe('the columns, with the floor a row of faces cannot go under', () => {
  // 4.3em at 14 px; the face is 100 wide by 35.5 high (the half-size dial on its band).
  const FLOOR = 60
  it.each([
    ['two rows would not fit: all three side by side, in view', 252, 84, 3],
    ['three rows would not fit: two rows, in view', 262, 175, 2],
    ['two rows exactly fit', 262, 124, 2],
    ['three rows exactly fit', 262, 188, 1],
    ['a pane too short for one row keeps one row', 600, 36, 3],
  ] as const)('%s', (_what, w, h, cols) => {
    expect(largeFaceColumns(w, h, 4, FLOOR)).toBe(cols)
  })

  it('the control: with no floor the first two would choose more rows', () => {
    expect(largeFaceColumns(252, 84)).toBe(2)
    expect(largeFaceColumns(262, 175)).toBe(1)
  })
})

describe('the columns that draw the faces biggest', () => {
  it.each([
    ['a short wide pane keeps all three in view', 600, 66, 3],
    ['a wide pane', 1200, 180, 3],
    ['a tall narrow pane stacks them', 300, 421, 1],
    ['a pane near square takes two and one', 450, 180, 2],
    ['a phone’s width with the floor’s height', 340, 59, 3],
    ['not measured', 0, 0, 3],
    ['hidden by a keep-alive host', 0, 400, 3],
  ] as const)('%s', (_what, w, h, cols) => {
    expect(largeFaceColumns(w, h)).toBe(cols)
  })
})

describe('one way to it from the meters', () => {
  it('the TX strip: a toggle beside the meters, which is not there without a record to write', () => {
    const onToggle = vi.fn()
    const radio = { rigKeyed: false } as unknown as RadioStatus
    render(<CockpitTxStrip radio={radio} onStopTx={() => {}} meters={<span />} metersPane={{ shown: false, onToggle }} />)
    const button = screen.getByRole('button', { name: 'Show the large TX meters pane' })
    expect(button.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(button)
    expect(onToggle).toHaveBeenCalledTimes(1)
    cleanup()
    render(<CockpitTxStrip radio={radio} onStopTx={() => {}} meters={<span />} />)
    expect(screen.queryByRole('button', { name: /large TX meters/ })).toBeNull()
  })

  it('the toggle stands after the cell and takes no part in the strip’s line', () => {
    const radio = { rigKeyed: false } as unknown as RadioStatus
    const { container } = render(
      <CockpitTxStrip radio={radio} onStopTx={() => {}} meters={<span />} metersPane={{ shown: true, onToggle: () => {} }} />,
    )
    const toggle = container.querySelector('.txmeters-pane-toggle')!
    expect(toggle.previousElementSibling?.className).toContain('cockpit-txstrip-meters')
    expect(win(toggle, 'flex')).toBe('none')
    expect(toggle.getAttribute('aria-label')).toBe('Hide the large TX meters pane')
  })

  it('Operate’s larger dialog offers it, closes, and keeps Close', () => {
    const onToggle = vi.fn()
    const { container } = render(<TxMetersCell radio={KEYED} pane={{ shown: false, onToggle }} />)
    fireEvent.click(container.querySelector('.txmeters-open')!)
    fireEvent.click(screen.getByRole('button', { name: 'Show as a pane' }))
    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(document.querySelector('.txmeters-large')).toBeNull()
  })

  it('…and does not, once the pane is on screen', () => {
    const { container } = render(<TxMetersCell radio={KEYED} pane={{ shown: true, onToggle: () => {} }} />)
    fireEvent.click(container.querySelector('.txmeters-open')!)
    expect(screen.queryByRole('button', { name: 'Show as a pane' })).toBeNull()
  })
})
