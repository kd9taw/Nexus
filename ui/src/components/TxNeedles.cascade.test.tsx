// @vitest-environment jsdom
//
// #343: HOW THE NEEDLE FACES SIT IN EACH HOST — computed over the REAL sheets, never read off them.
//
// The faces live in the three hosts the bars do: Phone and CW's dock (`pinned`, below md), Operate's
// telemetry cell (`inline`) and the TX strip's meters cell (`strip`, Phone and CW from md up). None of
// them is a pane, and neither strip may grow taller than its buttons, so a face's height is its two
// lines of type: the readout stands BESIDE the dial, which takes the whole height, and the faces share
// the width their host has, up to where the dial is as big as that height allows. The strip lays them
// in a first column across both of its rows; the dock gives them its first row. Nothing about a face
// changes with the TX cycle.
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
import { CockpitTxStrip } from './CockpitTxStrip'
import { OperateQsoStrip } from './OperateQsoStrip'
import { TxMeters } from './TxMeters'
import { TX_METER_STYLE_KEY } from '../txMeterStyle'
import type { QsoStatus, RadioStatus } from '../types'

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
function mount(host: Host, radio: RadioStatus, needle = true, viewport?: string) {
  if (needle) localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
  const ui = (radio: RadioStatus) => (
    <div data-viewport={viewport}>
      {host === 'dock' ? (
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
      )}
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
    // From the window (less the dashboard rail), never the cell: the cell's width changes with the TX
    // state caption, every over.
    expect(win(group, 'grid-template-columns')).toBe('clamp(20em, calc(var(--vw-eff, 100vw) - var(--dash-rail-taken, 0px) - 57em), 43em)')
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

  it.each(['cell', 'strip'] as const)(
    'that box is the dock’s alone: in %s the container keeps the spacing its host gives the bars',
    (host) => {
      const box = (needle: boolean) => {
        const { group } = mount(host, KEYED, needle)
        const out = { padding: win(group, 'padding'), gap: win(group, 'gap') }
        cleanup()
        localStorage.clear()
        return out
      }
      expect(box(true)).toEqual(box(false))
    },
  )

  it('and the larger dialog (#446) keeps its own spacing around the faces', () => {
    const box = (needle: boolean) => {
      if (needle) localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
      const { container } = render(
        <div className="txmeters-large">
          <TxMeters radio={KEYED} pinned />
        </div>,
      )
      const group = container.querySelector('.ph-txmeters')!
      const out = { padding: win(group, 'padding'), gap: win(group, 'gap') }
      cleanup()
      localStorage.clear()
      return out
    }
    expect(box(true)).toEqual(box(false))
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

describe('a face’s size is the room its host has, and the TX cycle does not touch it', () => {
  it.each(['strip', 'cell', 'dock'] as const)('%s: the faces share the width, in em, up to a ceiling', (host) => {
    const { face } = mount(host, KEYED)
    expect(win(face, 'flex')).toBe('1 1 0')
    // The strip's column is already held to its own floor (above); a host that wraps needs one here.
    expect(win(face, 'min-width')).toBe(host === 'strip' ? '0' : '7.7em')
    expect(win(face, 'max-width')).toBe('14em')
    // A fixed width is what kept them small whatever room there was.
    expect(win(face, 'width')).toBeNull()
  })

  it.each(['strip', 'cell', 'dock'] as const)('%s: the readout stands beside the dial instead of over it', (host) => {
    const { container } = mount(host, KEYED)
    expect(win(container.querySelector('.ph-txneedle-face')!, 'display')).toBe('flex')
    const band = container.querySelector('.ph-txneedle-band')!
    expect(win(band, 'position')).toBeNull()
    expect(win(band, 'flex')).toBe('none')
    expect(win(container.querySelector('.ph-txneedle-dial')!, 'flex')).toBe('1 1 0')
    // Larger than the 0.74rem it was. Operate's cell, with the least room, keeps its dial with a size less.
    expect(win(container.querySelector('.ph-txneedle-value')!, 'font-size')).toBe(
      host === 'cell' ? 'calc(13px * var(--text-scale))' : 'calc(14px * var(--text-scale))',
    )
    expect(win(band, 'width')).toBe(host === 'cell' ? '4.2em' : '4.4em')
  })

  it.each(['dock', 'cell', 'strip'] as const)('%s: the same size keyed and between overs', (host) => {
    const { face, next } = mount(host, KEYED)
    const keyed = [win(face, 'flex'), win(face, 'min-width'), win(face, 'max-width')]
    // Between overs, after one: the host's idle variant (the dock's bars go compact; the faces must not).
    const idle = next(IDLE)
    expect([win(idle, 'flex'), win(idle, 'min-width'), win(idle, 'max-width')]).toEqual(keyed)
  })

  it('the dock’s faces take the line before COMP does, and fold under it only on a narrow window', () => {
    const { needles } = mount('dock', KEYED)
    expect(win(needles, 'flex')).toBe('3 1 24em')
    expect(win(needles, 'min-width')).toBe('0')
  })
})

describe('the TX strip at md, where its column is shortest, takes the narrower readout', () => {
  it('md: the band and the value of Operate’s cell', () => {
    const { container } = mount('strip', KEYED, true, 'md')
    expect(win(container.querySelector('.ph-txneedle-band')!, 'width')).toBe('4.2em')
    expect(win(container.querySelector('.ph-txneedle-value')!, 'font-size')).toBe('calc(13px * var(--text-scale))')
  })

  it('the control: at lg the strip has the room for the full one', () => {
    const { container } = mount('strip', KEYED, true, 'lg')
    expect(win(container.querySelector('.ph-txneedle-band')!, 'width')).toBe('4.4em')
  })
})

describe('on a phone (xs) the dock draws a face whole, sized from the window', () => {
  it('the dock: the band over the dial, from the old 5em up, and the line never decides it', () => {
    const { container, face, needles } = mount('dock', KEYED, true, 'xs')
    expect(win(container.querySelector('.ph-txneedle-face')!, 'display')).toBe('block')
    expect(win(container.querySelector('.ph-txneedle-dial')!, 'aspect-ratio')).toBe('100 / 38')
    expect(win(container.querySelector('.ph-txneedle-band')!, 'position')).toBe('absolute')
    expect(win(face, 'width')).toBe('clamp(5em, calc((var(--vw-eff, 100vw) - 5em) / 3), 8em)')
    expect(win(face, 'flex')).toBe('none')
    expect(win(needles, 'flex')).toBe('none')
  })

  it('the control: Operate’s cell has a strip’s height at xs too, so its readout stays beside the dial', () => {
    const { container, face } = mount('cell', KEYED, true, 'xs')
    expect(win(container.querySelector('.ph-txneedle-face')!, 'display')).toBe('flex')
    expect(win(face, 'min-width')).toBe('6.5em')
  })
})

describe('the faces’ cells make room for them, with Needle only', () => {
  const qso = { state: 'idle', mycall: 'N0CALL' } as unknown as QsoStatus
  const noop = () => {}
  const operate = () =>
    render(
      <div className="cockpit-qso">
        <OperateQsoStrip
          qso={qso}
          radio={IDLE}
          onSetMode={noop}
          onCallCq={noop}
          onResend={noop}
          onFreetext={noop}
          onLog={noop}
          onSetTxEnabled={noop}
          onSetTune={noop}
          onHaltTx={noop}
          onSetHoldTxFreq={noop}
          telemetry={<span />}
        />
      </div>,
    ).container.querySelector('.cq-telemetry')!
  const txStrip = () =>
    render(<CockpitTxStrip radio={IDLE} onStopTx={noop} meters={<span />} />).container.querySelector('.cockpit-txstrip-meters')!

  it('Operate’s cell breaks the line where it always did, then takes the row’s surplus', () => {
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    const cell = operate()
    // Grows, never shrinks, from a fixed basis: nothing on the row changes width with the TX cycle,
    // so neither does the cell. It outbids the row's spacer until it reaches its ceiling.
    expect(win(cell, 'flex')).toBe('1000 0 24em')
    expect(win(cell, 'max-width')).toBe('min(43em, 100%)')
    // As tall as a face before the first over, so the first key-down does not raise the strip.
    expect(win(cell, 'min-height')).toBe('calc(26.4px * var(--text-scale) + 2px)')
  })

  it('the control: with bars Operate’s cell is the fixed width it was', () => {
    const cell = operate()
    expect(win(cell, 'flex')).toBe('none')
    expect(win(cell, 'width')).toBe('21em')
  })

  it('the TX strip’s cell keeps its zero basis and takes a higher ceiling', () => {
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    const cell = txStrip()
    expect(win(cell, 'flex')).toBe('1 1 0')
    expect(win(cell, 'min-width')).toBe('0')
    expect(win(cell, 'max-width')).toBe('65em')
  })

  it('the control: with bars the TX strip’s cell has the ceiling it had', () => {
    expect(win(txStrip(), 'max-width')).toBe('30em')
  })
})

describe('the larger TX meters dialog (#446) draws its faces larger', () => {
  it('in the dialog’s wrapper a face is wider than in any cockpit host, in em', () => {
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    const { container } = render(
      <div className="txmeters-large">
        <TxMeters radio={KEYED} pinned />
      </div>,
    )
    const face = container.querySelector('.ph-txneedle')!
    expect(win(face, 'width')).toBe('12em')
    expect(win(face, 'flex')).toBe('none')
    // The dialog has the height a strip has not: the dial above, the band over its lower half.
    expect(win(container.querySelector('.ph-txneedle-face')!, 'display')).toBe('block')
    expect(win(container.querySelector('.ph-txneedle-dial')!, 'aspect-ratio')).toBe('100 / 38')
    expect(win(container.querySelector('.ph-txneedle-band')!, 'position')).toBe('absolute')
    // Its name and value grow with it, as the dialog's bars' do.
    expect(win(container.querySelector('.ph-txneedle-value')!, 'font-size')).toBe('calc(16px * var(--text-scale))')
    // Beside faces that large, COMP takes a line of its own rather than squeezing past its type.
    expect(win(container.querySelector('.ph-txmeter')!, 'flex')).toBe('1 1 100%')
    // The control: the same meters outside the dialog keep the cockpit's.
    cleanup()
    expect(win(mount('dock', KEYED).face, 'width')).toBeNull()
  })
})

describe('the dial is drawn to its own shape', () => {
  it('the viewBox is the scale’s own shape, anchored at the top, and the SVG fills the dial out of flow', () => {
    const { container } = mount('dock', KEYED)
    const svg = container.querySelector('.ph-txneedle-svg')!
    const [, , w, h] = svg.getAttribute('viewBox')!.split(/\s+/).map(Number)
    expect(w / h).toBe(5)
    // In a dial narrower than that the scale stays at the top and the needle runs on down below it.
    expect(svg.getAttribute('preserveAspectRatio')).toBe('xMidYMin meet')
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
