// @vitest-environment jsdom
// PROGRAMS' SPOTS ON THE COCKPITS' BAND STRIP (Share by TCI).
//
// The strip draws a program's spot as it draws a cluster spot, its tick dotted, its source ("TCI 1")
// beside the call in the hover label, a tooltip of its own ("from TCI connection 1", and "click to
// tune" where a cluster spot's says "click to work"), the program's text as text. ⛔ A click on
// it goes to `onTciSpot`, which only tunes, never to `onWorkSpot`; with no `onTciSpot` it does
// nothing. The cluster spot beside it is the control throughout.
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { BandStrip } from './BandStrip'
import type { SpotRow } from '../types'

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({
    call: 'K1ABC',
    entity: '',
    zone: 0,
    band: '20m',
    mode: 'CW',
    freqMhz: 14.025,
    spotter: '',
    corroborators: [],
    ageSecs: 30,
    comment: '',
    licensed: true,
    ...over,
  }) as SpotRow

const CLUSTER = spot({ call: 'W1AW', freqMhz: 14.03, spotter: 'K3LR' })
const XSS = '<script>alert(1)</script><img src=x onerror=alert(1)>'
const PROGRAM = spot({ call: 'K1ABC', freqMhz: 14.025, comment: XSS, tci: 1 })

const base = {
  band: '20m',
  dialMhz: 14.03,
  txAllowed: true,
  spots: [CLUSTER],
  spotMode: 'CW' as const,
  onWorkSpot: vi.fn(),
  onTciSpot: vi.fn(),
}

beforeEach(() => {
  cleanup()
  base.onWorkSpot.mockClear()
  base.onTciSpot.mockClear()
})

/** The flag for `call`. */
const flag = (root: HTMLElement, call: string) =>
  [...root.querySelectorAll<HTMLButtonElement>('.bandstrip-spot')].find((b) =>
    b.querySelector('.bandstrip-spot-call')?.textContent?.startsWith(call),
  )

describe("a program's spot on the band strip", () => {
  it('is drawn dotted with its source beside the call and in the tooltip; CONTROL: the cluster spot is not', () => {
    const { container } = render(<BandStrip {...base} tciSpots={[PROGRAM]} />)
    const ours = flag(container, 'K1ABC')!
    expect(ours.className).toContain('from-tci')
    expect(ours.querySelector('.bandstrip-spot-call .spot-type-badge.type-tci')?.textContent).toBe('TCI 1')
    // Its tooltip names the source in one phrase and says the click only tunes.
    expect(ours.title).toContain('from TCI connection 1')
    expect(ours.title).toMatch(/ — click to tune$/)
    const cluster = flag(container, 'W1AW')!
    expect(cluster.className).not.toContain('from-tci')
    expect(cluster.querySelector('.type-tci')).toBeNull()
    expect(cluster.title).not.toContain('TCI')
    expect(cluster.title).toMatch(/ — click to work$/)
  })

  it("shows the program's text as text: no element is made of it", () => {
    const { container } = render(<BandStrip {...base} tciSpots={[PROGRAM]} />)
    expect(flag(container, 'K1ABC')!.title).toContain(XSS)
    expect(container.querySelector('script, img')).toBeNull()
  })

  it('⛔ a click on it goes to onTciSpot and never to onWorkSpot; CONTROL: the cluster spot is worked', () => {
    const { container } = render(<BandStrip {...base} tciSpots={[PROGRAM]} />)
    fireEvent.click(flag(container, 'K1ABC')!)
    expect(base.onTciSpot).toHaveBeenCalledWith(PROGRAM)
    expect(base.onWorkSpot).not.toHaveBeenCalled()
    fireEvent.click(flag(container, 'W1AW')!)
    expect(base.onWorkSpot).toHaveBeenCalledWith(CLUSTER)
    expect(base.onTciSpot).toHaveBeenCalledTimes(1)
  })

  it('⛔ with no onTciSpot a click on it does nothing, never a Work', () => {
    const { container } = render(<BandStrip {...base} onTciSpot={undefined} tciSpots={[PROGRAM]} />)
    fireEvent.click(flag(container, 'K1ABC')!)
    expect(base.onWorkSpot).not.toHaveBeenCalled()
  })
})
