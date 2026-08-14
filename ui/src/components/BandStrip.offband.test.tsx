// @vitest-environment jsdom
//
// Listening off the ham bands (WWV, shortwave, CB, marine, the gaps between band edges) is a
// first-class supported use case — operator ruling 2026-08-13 — and off the bands the backend
// sends `radio.band` as the EMPTY STRING. BandStrip answered that with `return null`: a pane
// frame still titled "Band activity" with NOTHING inside it and no word about why. Its sibling
// BandMap already handled the identical case honestly, so this pins the SAME two sentences on
// the strip rather than inventing a second vocabulary for one condition.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandStrip } from './BandStrip'
import { BandMap } from './BandMap'
import type { SpotRow } from '../types'

function spot(over: Partial<SpotRow> = {}): SpotRow {
  return {
    call: 'DL1ABC',
    entity: 'Germany',
    zone: 14,
    band: '20m',
    freqMhz: 14.25,
    mode: 'Phone',
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 30,
    comment: '',
    licensed: true,
    ...over,
  }
}

afterEach(cleanup)

describe('BandStrip off the band plan', () => {
  it('says what it cannot show instead of rendering nothing (band = "")', () => {
    const { container } = render(
      <BandStrip band="" dialMhz={5.0} txAllowed={false} spots={[]} onWorkSpot={() => {}} />,
    )
    expect(container.querySelector('.bandstrip')).toBeTruthy()
    expect(container.textContent).toContain('off the band plan')
    expect(container.textContent).toContain('no band-plan data for this frequency')
  })

  it('names the band when the backend has one we have no range for (11m, CB)', () => {
    const { container } = render(
      <BandStrip band="11m" dialMhz={27.185} txAllowed={false} spots={[]} onWorkSpot={() => {}} />,
    )
    expect(container.textContent).toContain('11m — off the band plan')
    expect(container.textContent).toContain('no band-plan data for 11m')
  })

  it('uses its sibling BandMap’s wording verbatim — one condition, one vocabulary', () => {
    const strip = render(
      <BandStrip band="" dialMhz={5.0} txAllowed={false} spots={[]} onWorkSpot={() => {}} />,
    ).container
    const map = render(
      <BandMap band="" dialMhz={5.0} txAllowed={false} spots={[]} onWorkSpot={() => {}} />,
    ).container
    const text = (root: ParentNode) => root.querySelector('.bandstrip-count')?.textContent
    expect(text(strip)).toBe(text(map))
    expect(strip.querySelector('.bandstrip-empty')?.textContent).toBe(
      map.querySelector('.bandmap-empty')?.textContent,
    )
  })

  it('a normal band still renders the whole strip — track, spots and axis (the control)', () => {
    const { container } = render(
      <BandStrip
        band="20m"
        dialMhz={14.25}
        txAllowed
        spots={[spot()]}
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelector('.bandstrip-track')).toBeTruthy()
    expect(container.querySelector('.bandstrip-spot')).toBeTruthy()
    expect(container.querySelector('.bandstrip-dial')).toBeTruthy()
    expect(container.querySelector('.bandstrip-axis')).toBeTruthy()
    expect(container.textContent).toContain('1 SSB spot · 20m')
    expect(container.textContent).not.toContain('off the band plan')
  })
})
