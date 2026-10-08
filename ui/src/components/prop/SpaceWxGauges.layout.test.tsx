// @vitest-environment jsdom
//
// THE SPACE WX GAUGES SIT TWO TO A ROW IN A CONNECT BOX. `.swx-strip` asks for a grid, and
// `.connect .pane-body .swx-strip` for two columns of it, but the strip is also a `.panel`, and
// `.panel { display: flex; flex-direction: column }` comes later in the sheet at the same
// specificity, so it won: the gauges stood one per row, six rows, and the 30-day lines under them
// started below the fold of the default bottom slot at every window size (measured in Chrome).
// The grid now wins inside a Connect box, and the solar-wind speed's unit may drop under its value
// when a column is too narrow for both (the 200 px rail), where it would otherwise run past the
// column's edge; so may a gauge's value under its name (Spanish "VIENTO 487" in a 77 px column).
//
// Computed from the sheet with the app's own resolver (cssCascade.ts), on the real box as the grid
// renders it: jsdom lays nothing out, so where the gauges land is measured in Chrome, not here.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt } from '../../cssCascade'

vi.mock('../../api', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  getSolarIndices: vi.fn(() => new Promise(() => {})),
}))

import { PaneBody, PaneFrame } from '../connect/PaneFrame'
import type { PaneContext } from '../connect/paneContext'

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

afterEach(cleanup)

const LIVE = {
  myGrid: 'EN52',
  prop: {
    source: 'live',
    asOf: Math.floor(Date.now() / 1000),
    spaceWx: { sfi: 142, kp: 2.33, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: { bzNt: -3.4, btNt: 6.1, speedKms: 487, density: 5.2 } },
  },
  scales: null,
  alerts: [],
} as unknown as PaneContext

/** The Space Wx box as a slot of `host` draws it, and the elements the cascade is asked about. */
function box(host: string) {
  const { container } = render(
    <div className={host}>
      <PaneFrame slotId="bottom2" paneId="spacewx" ctx={LIVE} onAssign={() => {}} />
    </div>,
  )
  const strip = container.querySelector('.swx-strip')!
  const windUnit = container.querySelector('.swx-vu')!
  expect(strip, 'control: the gauges are drawn').not.toBeNull()
  expect(windUnit, 'control: the solar-wind speed is drawn with its unit').not.toBeNull()
  return { strip, windUnit }
}

const won = (el: Element, ...props: string[]) => winnerAt(RULES, 'dark', chainOf(el), ...props)

describe('the Space Wx gauges in a Connect box', () => {
  it('are a grid of two columns, not the panel’s single column', () => {
    const { strip } = box('app connect')
    expect(won(strip, 'display')?.value, `display is won by "${won(strip, 'display')?.rule.selector}"`).toBe('grid')
    expect(won(strip, 'grid-template-columns')?.value).toBe('repeat(2, 1fr)')
  })

  it('let the solar-wind speed’s unit go under its value where the column is too narrow for both', () => {
    const { windUnit } = box('app connect')
    expect(won(windUnit, 'flex-wrap')?.value).toBe('wrap')
  })

  it('let a gauge’s value go under its name where the two do not fit side by side (Spanish “VIENTO 487”)', () => {
    const { strip } = box('app connect')
    expect(won(strip.querySelector('.swx-head')!, 'flex-wrap')?.value).toBe('wrap')
  })

  it('control: outside a Connect box the rule does not reach (the cockpit pane grids share .pane-body)', () => {
    const { strip } = box('app')
    expect(won(strip, 'display')?.value).toBe('flex')
    expect(won(strip.querySelector('.swx-head')!, 'flex-wrap'), 'a gauge head outside Connect wraps').toBeNull()
  })
})

describe('the Space Wx gauges in a box inside a cockpit (any pane in any area, 2026-10-07)', () => {
  /** The box as a cockpit draws it: the cockpit pane frame, its body, and the box's own wrapper. */
  function cockpitBox(wrapped: boolean) {
    const { container } = render(
      <div className="app">
        <section className="pane-frame">
          <div className="pane-body">
            {wrapped ? (
              <div className="box-body">
                <PaneBody pane="spacewx" ctx={LIVE} />
              </div>
            ) : (
              <PaneBody pane="spacewx" ctx={LIVE} />
            )}
          </div>
        </section>
      </div>,
    )
    const strip = container.querySelector('.swx-strip')!
    expect(strip, 'control: the gauges are drawn').not.toBeNull()
    return strip
  }

  it('read as they do on Conditions: two to a row, the unit and the value free to go under', () => {
    const strip = cockpitBox(true)
    expect(won(strip, 'display')?.value, `display is won by "${won(strip, 'display')?.rule.selector}"`).toBe('grid')
    expect(won(strip, 'grid-template-columns')?.value).toBe('repeat(2, 1fr)')
    expect(won(strip.querySelector('.swx-vu')!, 'flex-wrap')?.value).toBe('wrap')
    expect(won(strip.querySelector('.swx-head')!, 'flex-wrap')?.value).toBe('wrap')
    // …and the strip is a flattened panel there, as in a Connect box: no card inside the box's frame.
    expect(won(strip, 'border-top-width', 'border')?.value ?? '').toMatch(/^0/)
  })

  it('control: a cockpit pane that is no box is still outside the rule', () => {
    expect(won(cockpitBox(false), 'display')?.value).toBe('flex')
  })
})
