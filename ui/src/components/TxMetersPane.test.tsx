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
import { TxMeters, TxMetersCell, alcBar, poBar, swrBar } from './TxMeters'
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
  const DIAL = '.ph-txneedle-scale, .ph-txneedle-zone, .ph-txneedle-tick, .txmeters-pane-scale, .ph-txneedle-peak, .ph-txneedle-needle'

  it.each(['po', 'alc', 'swr'])('%s: the whole dial is drawn at half, standing on the band', (key) => {
    const face = render(<TxMeters radio={KEYED} pane />).container.querySelector(`[data-meter="${key}"]`)!
    const g = face.querySelector('g.txmeters-pane-dial')!
    expect(g).not.toBeNull()
    const m = /^translate\(([-\d.]+) ([-\d.]+)\) scale\(([\d.]+)\)$/.exec(g.getAttribute('transform') ?? '')!
    const [tx, ty, k] = [Number(m[1]), Number(m[2]), Number(m[3])]
    expect(k).toBe(0.5)
    // The middle of the band's top edge (50, 38) stays where it was: the dial stands on the band.
    expect(tx + 50 * k).toBeCloseTo(50, 6)
    expect(ty + 38 * k).toBeCloseTo(38, 6)
    const parts = [...face.querySelectorAll(DIAL)]
    expect(parts.length).toBeGreaterThan(10)
    for (const p of parts) expect(g.contains(p)).toBe(true)
  })

  it('the band, the name and the value are outside it, at 1.24.0’s size', () => {
    const face = render(<TxMeters radio={KEYED} pane />).container.querySelector('[data-meter="po"]')!
    const g = face.querySelector('g.txmeters-pane-dial')!
    for (const sel of ['.txmeters-pane-band', '.txmeters-pane-label', '.txmeters-pane-value']) {
      const el = face.querySelector(sel)!
      expect(el).not.toBeNull()
      expect(g.contains(el)).toBe(false)
    }
    expect(win(face.querySelector('.txmeters-pane-label')!, 'font-size')).toBe('6.5px')
    expect(win(face.querySelector('.txmeters-pane-value')!, 'font-size')).toBe('9px')
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
  const FLOOR = 98
  it.each([
    ['two rows would not fit: all three side by side, in view', 252, 140, 3],
    ['three rows would not fit: two rows, in view', 262, 290, 2],
    ['two rows exactly fit', 262, 200, 2],
    ['three rows exactly fit', 262, 302, 1],
    ['a pane too short for one row keeps one row', 600, 60, 3],
  ] as const)('%s', (_what, w, h, cols) => {
    expect(largeFaceColumns(w, h, 4, FLOOR)).toBe(cols)
  })

  it('the control: with no floor the first two would choose more rows', () => {
    expect(largeFaceColumns(252, 140)).toBe(2)
    expect(largeFaceColumns(262, 290)).toBe(1)
  })
})

describe('the columns that draw the faces biggest', () => {
  it.each([
    ['a short wide pane keeps all three in view', 600, 110, 3],
    ['a wide pane', 1200, 300, 3],
    ['a tall narrow pane stacks them', 300, 700, 1],
    ['a pane near square takes two and one', 450, 300, 2],
    ['a phone’s width with the floor’s height', 340, 98, 3],
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
