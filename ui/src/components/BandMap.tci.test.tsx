// @vitest-environment jsdom
// PROGRAMS' SPOTS ON THE BAND MAP (Share by TCI).
//
// A program on the station's TCI server (a logger, a skimmer) sends spots; the band map draws each
// as it draws a cluster spot, dotted, with its source ("TCI 2", the program's connection number)
// before the call, and a tooltip of its own: "from TCI connection 2", and "click to tune" where a
// cluster spot's says "click to work". What a program sends is data: its text reaches the tooltip
// as the characters it is, never as markup. ⛔ A click on a program's spot goes to `onTciSpot`,
// which only tunes, and never to `onWorkSpot`, whose Work can change the section and arm; with no
// `onTciSpot` it does nothing at all. Every case has its control: the cluster spot beside it.
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { BandMap } from './BandMap'
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

const CLUSTER = spot({ call: 'W1AW', freqMhz: 14.03, spotter: 'K3LR', comment: 'QST' })
const XSS = '<img src=x onerror=alert(1)> <b>bold</b> &amp;'
const PROGRAM = spot({ call: 'K1ABC', freqMhz: 14.025, comment: XSS, tci: 2 })

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
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  base.onWorkSpot.mockClear()
  base.onTciSpot.mockClear()
})

/** The label button for `call`. */
const label = (root: HTMLElement, call: string) =>
  [...root.querySelectorAll<HTMLButtonElement>('.bandmap-spot')].find(
    (b) => b.querySelector('.bandmap-call')?.textContent === call,
  )

describe("a program's spot on the band map", () => {
  it('is drawn with its source before the call and in the tooltip, dotted; CONTROL: the cluster spot has none', () => {
    const { container } = render(<BandMap {...base} tciSpots={[PROGRAM]} />)
    const ours = label(container, 'K1ABC')
    expect(ours, 'the program spot is drawn').toBeTruthy()
    expect(ours!.querySelector('.spot-type-badge.type-tci')?.textContent).toBe('TCI 2')
    expect(ours!.className).toContain('from-tci')
    // Its tooltip names the source in one phrase and says the click only tunes.
    expect(ours!.title).toContain('from TCI connection 2')
    expect(ours!.title).toMatch(/ — click to tune$/)
    const cluster = label(container, 'W1AW')
    expect(cluster, 'control: the cluster spot is drawn').toBeTruthy()
    expect(cluster!.querySelector('.type-tci')).toBeNull()
    expect(cluster!.className).not.toContain('from-tci')
    expect(cluster!.title).not.toContain('TCI')
    expect(cluster!.title).toMatch(/ — click to work$/)
    expect(container.querySelectorAll('.bandmap-tick.from-tci')).toHaveLength(1)
  })

  it("shows the program's text as text: no element is made of it", () => {
    const { container } = render(<BandMap {...base} tciSpots={[PROGRAM]} />)
    const ours = label(container, 'K1ABC')!
    expect(ours.title).toContain(XSS)
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('.bandmap-track b')).toBeNull()
    // CONTROL: the same text as a cluster spot's comment is shown the same way.
    cleanup()
    const control = render(<BandMap {...base} spots={[spot({ ...CLUSTER, comment: XSS })]} />)
    expect(label(control.container, 'W1AW')!.title).toContain(XSS)
    expect(control.container.querySelector('img')).toBeNull()
  })

  it('⛔ a click on it goes to onTciSpot and never to onWorkSpot; CONTROL: the cluster spot is worked', () => {
    const { container } = render(<BandMap {...base} tciSpots={[PROGRAM]} />)
    fireEvent.click(label(container, 'K1ABC')!)
    expect(base.onTciSpot).toHaveBeenCalledTimes(1)
    expect(base.onTciSpot).toHaveBeenCalledWith(PROGRAM)
    expect(base.onWorkSpot).not.toHaveBeenCalled()
    fireEvent.click(label(container, 'W1AW')!)
    expect(base.onWorkSpot).toHaveBeenCalledWith(CLUSTER)
    expect(base.onTciSpot).toHaveBeenCalledTimes(1)
  })

  it('⛔ with no onTciSpot a click on it does nothing, never a Work', () => {
    const { container } = render(<BandMap {...base} onTciSpot={undefined} tciSpots={[PROGRAM]} />)
    fireEvent.click(label(container, 'K1ABC')!)
    expect(base.onWorkSpot).not.toHaveBeenCalled()
  })

  it('a program spot that also arrived in the cluster list is still a program spot', () => {
    const { container } = render(<BandMap {...base} spots={[CLUSTER, PROGRAM]} />)
    fireEvent.click(label(container, 'K1ABC')!)
    expect(base.onTciSpot).toHaveBeenCalledWith(PROGRAM)
    expect(base.onWorkSpot).not.toHaveBeenCalled()
  })

  it("draws only this band's spots of the map's mode; CONTROL: its own are drawn", () => {
    const elsewhere = [
      spot({ call: 'DL1ABC', band: '40m', freqMhz: 7.025, tci: 1 }),
      spot({ call: 'G4ABC', mode: 'Phone', freqMhz: 14.25, tci: 1 }),
    ]
    const { container } = render(<BandMap {...base} tciSpots={[...elsewhere, PROGRAM]} />)
    expect(label(container, 'DL1ABC')).toBeUndefined()
    expect(label(container, 'G4ABC')).toBeUndefined()
    expect(label(container, 'K1ABC')).toBeTruthy()
  })
})
