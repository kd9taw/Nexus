// @vitest-environment jsdom
//
// THE AMPLIFIER BOX'S READINGS ARE BIG: power out and SWR drawn as the Clock box draws its digits (SVG text in a
// viewBox, sized by its cell), the other readings a step larger than the body text. jsdom lays nothing out, so
// these are the drawing's own numbers and the cascade winners (cssCascade.testkit); the sizes on screen were
// measured in Chrome over the dashboard rail, from its 200 px floor to its 720 px widest.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { AmpPane } from './AmpPane'
import type { AmpStatus } from '../../types'
import { atToken, css, loadSheets, pxOf } from '../../cssCascade.testkit'

beforeAll(() => loadSheets())
afterEach(cleanup)

const live = (over: Partial<AmpStatus> = {}): AmpStatus => ({
  family: 'spe',
  model: '15K',
  linked: true,
  reason: '',
  operate: true,
  transmitting: true,
  outputWatts: 1500,
  swr: 1.4,
  swrAtu: 1.1,
  volts: 45.9,
  amps: 32.5,
  temp: 77,
  tempCelsius: false,
  alarm: 'none',
  alarmRaised: false,
  warning: 'none',
  warningRaised: false,
  kpaFault: null,
  ...over,
})

/** The big readings as drawn: the text, its viewBox and the length it is held to. */
const big = (c: HTMLElement) =>
  [...c.querySelectorAll<SVGSVGElement>('.amp-big svg')].map((svg) => ({
    text: svg.textContent,
    viewBox: svg.getAttribute('viewBox'),
    length: svg.querySelector('text')?.getAttribute('textLength'),
  }))

describe('the Amplifier box draws power out and SWR as its big readings', () => {
  it('as SVG text sized by its cell, in a viewBox that holds six characters and half of one more', () => {
    const { container } = render(<AmpPane amp={live()} radioOnAir={false} />)
    // Each character is held to 60 of the viewBox's units (0.6 of the 100-unit font): "1500 W" fills six.
    expect(big(container)).toEqual([
      { text: '1500 W', viewBox: '0 0 390 80', length: '360' },
      { text: '1.4:1', viewBox: '0 0 390 80', length: '300' },
    ])
    // The labels stay words, above them.
    expect([...container.querySelectorAll('.amp-big-row .amp-k')].map((k) => k.textContent)).toEqual(['Power out', 'SWR'])
  })

  it('draws a dead link’s two big readings as the em dash, the same size as a reading', () => {
    const { container } = render(<AmpPane amp={live({ linked: false, reason: 'noAnswer' })} radioOnAir={false} />)
    expect(big(container)).toEqual([
      { text: '—', viewBox: '0 0 390 80', length: '60' },
      { text: '—', viewBox: '0 0 390 80', length: '60' },
    ])
  })

  it('widens the viewBox of a reading longer than six characters rather than run it past its cell', () => {
    const { container } = render(<AmpPane amp={live({ outputWatts: 12000 })} radioOnAir={false} />)
    expect(big(container)[0]).toEqual({ text: '12000 W', viewBox: '0 0 450 80', length: '420' })
  })

  it('sizes the drawing by the cell’s width, capped at 40 px times Text size as the Clock’s local time is', () => {
    const { container } = render(<AmpPane amp={live()} radioOnAir={false} />)
    const svg = container.querySelector<SVGSVGElement>('.amp-big svg')
    expect(svg, 'power out is not drawn as a big reading').not.toBeNull()
    if (!svg) return
    expect([css(svg, 'width'), css(svg, 'height')]).toEqual(['100%', 'auto'])
    expect(pxOf(svg, 'max-height')).toBe(40)
    expect(atToken('--text-scale', '1.25', () => pxOf(svg, 'max-height')), 'at Larger').toBe(50)
    // Two to a line where each keeps 8em (the stock 300 px rail, its scrollbar showing), one to a line below
    // that (the rail's 200 px floor).
    expect(css(container.querySelector('.amp-big-row')!, 'grid-template-columns')).toBe('repeat(auto-fit, minmax(min(8em, 100%), 1fr))')
  })
})

describe('the other readings are a step larger than the body text', () => {
  it('pre-ATU SWR, PA temp, Vdc and current at the title size, 18 px at Normal (14 before)', () => {
    const { container } = render(<AmpPane amp={live()} radioOnAir={false} />)
    const small = [...container.querySelectorAll<HTMLElement>('.amp-grid .amp-v')]
    for (const v of small.filter((s) => !s.querySelector('svg'))) expect(pxOf(v, 'font-size'), v.textContent ?? '').toBe(18)
    expect(small.map((v) => v.textContent), 'the small readings').toEqual(['1.1:1', '77°', '45.9 V', '32.5 A'])
  })
})
