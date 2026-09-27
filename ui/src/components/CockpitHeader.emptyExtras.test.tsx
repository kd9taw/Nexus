// @vitest-environment jsdom
//
// A HEADER CLUSTER WITH NOTHING IN IT TAKES NO ROOM. CockpitHeader wraps its `children` in
// `.ch-mode-extras` whenever a child is PASSED, and a passed child can render nothing: the rotor
// strip on a station with no rotator, which is the RTTY, PSK and JS8 headers' only child. An empty
// region is still a flex item, so it costs the header's line one more `gap`, and a header that
// fitted can wrap. On the Remote page's Quick header, where the region spans a grid row of its
// own, it costs an empty row plus its gap. `.ch-mode-extras:empty` takes it out of layout, so a
// station with no rotator sees those three headers exactly as before.
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

describe('the header’s control cluster takes no room when nothing in it renders', () => {
  it('an empty cluster is out of layout', () => {
    mount(<Nothing />)
    expect(region(), 'fixture: a passed child still makes the region').not.toBeNull()
    expect(region()!.childNodes.length, 'fixture: the child rendered nothing').toBe(0)
    expect(css(region()!, 'display'), 'an empty cluster still takes a gap on the header').toBe('none')
  })

  // THE CONTROL: a cluster with something in it is the flex row it always was.
  it('a cluster with something in it is still the flex row it was', () => {
    mount(<span>WPM</span>)
    expect(css(region()!, 'display')).toBe('flex')
  })
})
