// @vitest-environment jsdom
//
// A HEADER CLUSTER WITH NOTHING OF ITS OWN COSTS NO ROOM. CockpitHeader wraps its `children` in
// `.ch-mode-extras` whenever a child is PASSED, and a passed child can render nothing: the rotor
// strip on a station with no rotator, which is the RTTY, PSK and JS8 headers' only child. An empty
// flex item still costs the header's line one more `gap`, and a header that fitted can wrap. The
// region carries the header's actions (⊞ Panels, the amplifier, power, CAT) at the end of its own
// row, so that they share its last line rather than take one of their own; with nothing else in it,
// it is exactly those actions, the one header item they were before, and no empty item is left.
//
// Computed, not grepped: the winning `display` for the rendered element over the real sheets.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { css, loadSheets } from '../cssCascade.testkit'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const snap = {
  radio: {
    dialMhz: 14.08, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true,
    tuning: false, txAllowed: true,
  },
} as unknown as AppSnapshot

/** A child that renders nothing, as the rotor strip does on a station with no rotator. */
const Nothing = () => null

const mount = (child: React.ReactNode) =>
  render(
    <CockpitHeader snap={snap} modeIndicator={<span>RTTY</span>} bandControl={<span>20m</span>}>
      {child}
    </CockpitHeader>,
  )
const region = () => document.querySelector('.ch-mode-extras')

beforeAll(() => loadSheets())
afterEach(cleanup)

describe('the header’s control cluster costs nothing when nothing of its own renders', () => {
  it('a cluster whose child renders nothing holds just the actions, and the header has no empty item', () => {
    mount(<Nothing />)
    expect(region(), 'fixture: a passed child still makes the region').not.toBeNull()
    expect([...region()!.children].map((e) => e.className), 'only the actions are in it').toEqual(['ch-actions'])
    const header = document.querySelector('.cockpit-header')!
    const empty = [...header.children].filter((e) => e.childNodes.length === 0 && css(e as HTMLElement, 'display') !== 'none')
    expect(empty.map((e) => e.className), 'an empty item still takes a gap on the header').toEqual([])
  })

  it('the actions come after the mode controls, inside their row, and stay a flex row', () => {
    mount(<span>WPM</span>)
    expect(css(region()!, 'display')).toBe('flex')
    expect([...region()!.children].map((e) => e.tagName === 'SPAN' ? e.textContent : e.className)).toEqual(['WPM', 'ch-actions'])
  })

  // THE CONTROL: with no mode controls passed at all, the actions are the header item they always were.
  it('with no cluster passed, the actions are a header item of their own', () => {
    render(<CockpitHeader snap={snap} modeIndicator={<span>RTTY</span>} bandControl={<span>20m</span>} />)
    expect(region()).toBeNull()
    expect(document.querySelector('.cockpit-header > .ch-actions')).not.toBeNull()
  })
})
