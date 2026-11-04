// @vitest-environment jsdom
//
// #343: HOW THE NEEDLE FACES SIT IN EACH HOST — computed over the REAL sheets, never read off them.
//
// The faces live in the three hosts the bars do: Phone and CW's dock (`pinned`, below md), Operate's
// fixed-width telemetry cell (`inline`) and the TX strip's meters cell (`strip`, Phone and CW from md
// up). The strip must never grow taller than its buttons and the cell is fixed width, so the faces
// there take the compact size and the strip lays them in a first column across both of its rows; the
// dock gives them its first row. Nothing about a face changes with the TX cycle.
//
// Every assertion resolves the cascade WINNER for the rendered element (cssCascade: importance →
// specificity → source order) — a regex over the sheet would pass on a dead selector.
// ⚠️ jsdom does not lay out: nothing here is a pixel, only what the cascade declares. The pixels are
// the real-Chrome pass's (tasks/scratch/n6-needle/).
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt, type Mode } from '../cssCascade'
import { TxMeters } from './TxMeters'
import { TX_METER_STYLE_KEY } from '../txMeterStyle'
import type { RadioStatus } from '../types'

afterEach(() => {
  cleanup()
  localStorage.clear()
})

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const MODE: Mode = 'dark'
const win = (el: Element, prop: string) => winnerAt(RULES, MODE, chainOf(el), prop)?.value.trim() ?? null

const KEYED = { rigKeyed: true, swrScaleVerified: true, txAlc: 0.5, txPoW: 40, txSwr: 1.4, txCompDb: 6 } as unknown as RadioStatus
const IDLE = { rigKeyed: false } as unknown as RadioStatus

type Host = 'dock' | 'cell' | 'strip'
/** The meters inside their real host, so a rule scoped by where they sit competes as on screen. */
function mount(host: Host, radio: RadioStatus, needle = true) {
  if (needle) localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
  const ui = (radio: RadioStatus) =>
    host === 'dock' ? (
      <div className="cockpit-txdock">
        <TxMeters radio={radio} pinned />
      </div>
    ) : host === 'cell' ? (
      <div className="cq-telemetry">
        <TxMeters radio={radio} inline />
      </div>
    ) : (
      <div className="cockpit-txstrip">
        <div className="cockpit-txstrip-meters">
          <TxMeters radio={radio} strip />
        </div>
      </div>
    )
  const { container, rerender } = render(ui(radio))
  const q = (s: string) => container.querySelector(s)!
  return {
    group: q('.ph-txmeters'),
    needles: q('.ph-txneedles'),
    face: q('.ph-txneedle'),
    container,
    /** The same panel on the next poll. */
    next: (r: RadioStatus) => {
      rerender(ui(r))
      return q('.ph-txneedle')
    },
  }
}

describe('the TX strip: the faces take a first column across both rows, and never wrap', () => {
  it('the strip’s grid gives the faces a column of their own width', () => {
    const { group, needles } = mount('strip', KEYED)
    expect(win(group, 'grid-template-columns')).toBe('max-content')
    // The strip's own two rows and column flow still hold: COMP and the temperatures follow on.
    expect(win(group, 'grid-template-rows')).toBe('repeat(2, auto)')
    expect(win(group, 'grid-auto-flow')).toBe('column')
    expect(win(needles, 'grid-row')).toBe('1 / -1')
    expect(win(needles, 'flex-wrap')).toBe('nowrap')
  })

  it('the control: with bars the strip is as it was (no column of its own)', () => {
    const { group } = mount('strip', KEYED, false)
    expect(win(group, 'grid-template-columns')).toBe('none')
  })
})

describe('the dock: the faces and the bars left share a line, wrapping only when the window is narrow', () => {
  it('the dock lays the faces and COMP out in one wrapping row, not in the bars’ grid', () => {
    const { group, needles, container } = mount('dock', KEYED)
    expect(win(group, 'display')).toBe('flex')
    expect(win(group, 'flex-wrap')).toBe('wrap')
    expect(win(needles, 'flex-wrap')).toBe('wrap')
    // COMP takes the share a dock bar takes in the bars' grid, beside the faces.
    expect(win(container.querySelector('.ph-txmeter')!, 'flex')).toBe('1 1 150px')
  })

  it('the dock’s box is the same keyed and between overs, so a bottom-anchored dock never moves Stop TX', () => {
    // The bars' container switches its compact/full variant with the TX cycle (`.pinned` comes and
    // goes); with needles the faces are one size, and the container's own padding and gap must not
    // follow that switch either.
    const { group, next } = mount('dock', KEYED)
    const keyed = { padding: win(group, 'padding'), gap: win(group, 'gap') }
    next(IDLE)
    expect(group.className).toContain('idle')
    expect({ padding: win(group, 'padding'), gap: win(group, 'gap') }).toEqual(keyed)
  })

  it('before the first reading, the dock’s hint shares the faces’ line instead of a line of its own', () => {
    const { container } = mount('dock', IDLE)
    const hint = container.querySelector('.ph-txmeters-hint')!
    expect(hint).not.toBeNull()
    expect(win(hint, 'flex')).toBe('1 1 150px')
    expect(win(hint, 'min-width')).toBe('0')
  })

  it('the control: with bars the dock is the bars’ grid', () => {
    const { group } = mount('dock', KEYED, false)
    expect(win(group, 'display')).toBe('grid')
  })
})

describe('Operate’s cell: the faces take the first row of its grid', () => {
  it('a full-width row that wraps on a narrow window', () => {
    const { group, needles } = mount('cell', KEYED)
    expect(win(group, 'display')).toBe('grid')
    expect(win(needles, 'grid-column')).toBe('1 / -1')
    expect(win(needles, 'flex-wrap')).toBe('wrap')
  })
})

describe('a face’s size is its host’s, and the TX cycle does not touch it', () => {
  it('one compact size in every host: no taller than the strip’s buttons, and the dock grows least', () => {
    const widths = (['strip', 'cell', 'dock'] as const).map((h) => {
      const w = win(mount(h, KEYED).face, 'width')
      cleanup()
      return w
    })
    expect(widths).toEqual(['5em', '5em', '5em'])
  })

  it.each(['dock', 'cell', 'strip'] as const)('%s: the same width keyed and between overs', (host) => {
    const { face, next } = mount(host, KEYED)
    const keyed = win(face, 'width')
    expect(keyed).not.toBeNull()
    // Between overs, after one: the host's idle variant (the dock's bars go compact; the faces must not).
    expect(win(next(IDLE), 'width')).toBe(keyed)
  })

  it('every width is in em, so the faces follow the zoom and the text size', () => {
    for (const h of ['strip', 'cell', 'dock'] as const) {
      expect(win(mount(h, KEYED).face, 'width')).toMatch(/^[\d.]+em$/)
      cleanup()
    }
  })
})

describe('the dial is drawn to its own shape', () => {
  it('the dial box has the viewBox’s aspect, and the SVG fills it out of flow', () => {
    const { container } = mount('dock', KEYED)
    const svg = container.querySelector('.ph-txneedle-svg')!
    const [, , w, h] = svg.getAttribute('viewBox')!.split(/\s+/).map(Number)
    expect(win(container.querySelector('.ph-txneedle-dial')!, 'aspect-ratio')).toBe(`${w} / ${h}`)
    expect(win(svg, 'position')).toBe('absolute')
    expect(win(svg, 'inset')).toBe('0')
  })

  it('the needle turns about the pivot it is drawn from, by the SVG attribute every webview reads alike', () => {
    const { container } = mount('dock', KEYED)
    const line = container.querySelector('.ph-txneedle-needle')!
    const m = /^rotate\((-?[\d.]+) (-?[\d.]+) (-?[\d.]+)\)$/.exec(line.getAttribute('transform') ?? '')
    expect(m, line.getAttribute('transform') ?? 'no transform attribute').not.toBeNull()
    expect([m![2], m![3]]).toEqual([line.getAttribute('x1'), line.getAttribute('y1')])
    expect((line as SVGElement).style.transform).toBe('')
    // No CSS transform competes with the attribute (a CSS one would win over it, engine by engine).
    expect(win(line, 'transform')).toBeNull()
    expect(win(line, 'transform-origin')).toBeNull()
  })
})
