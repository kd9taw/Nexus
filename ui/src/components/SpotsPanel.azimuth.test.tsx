// @vitest-environment jsdom
//
// The heading on a Spots row — the "all of the US is at ~309°" report (2026-09-01).
//
// The panel used to hand `azimuthTo` a hard-coded null peer grid, so EVERY row fell
// back to the entity centroid: from Alabama, each of a thousand US stations read
// "~309°" — the bearing to cty.dat's reference point in Kansas — while the row's own
// comment carried the station's real grid off the RBN wire. The contract now: a row
// that carries a grid gets an exact heading (no `~`), a grid-less row keeps the
// centroid heading marked `~`. Both states in ONE render, per the azimuth-test
// doctrine (a pane rendering no headings at all must not pass the negative case).
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import type { SpotRow } from '../types'

// cty.dat name verbatim — the centroid lookup is an exact-string join.
vi.mock('../api', () => ({
  openQrzPage: vi.fn(),
  getDxccEntityLocations: vi.fn(async () => [['United States', 39.8, -98.6]]),
}))

const spot = (over: Partial<SpotRow>): SpotRow => ({
  call: 'W1AW',
  entity: 'United States',
  zone: 5,
  band: '20m',
  freqMhz: 14.074,
  mode: 'Digital',
  spotter: 'K9IMM-#',
  corroborators: [],
  ageSecs: 5,
  comment: 'FT8 -15 dB CQ',
  licensed: true,
  ...over,
})

describe('SpotsPanel row headings', () => {
  afterEach(cleanup)

  it('uses the spot grid when carried, and the ~centroid only when not', async () => {
    render(
      <SpotsPanel
        spots={[
          // Heard grid carried: Los Angeles from northern Alabama — a real heading,
          // nowhere near the Kansas centroid, and NOT approximate.
          spot({ call: 'KA6VKP', grid: 'DM03' }),
          // No grid: the centroid fallback stays, marked `~`.
          spot({ call: 'N0XRG' }),
        ]}
        bandPlan={[]}
        selectedCall={null}
        onSelect={() => {}}
        onWork={() => {}}
        myGrid="EM64ue"
      />,
    )
    // Both rows render a heading; only the grid-less one is approximate.
    await waitFor(() => expect(document.querySelectorAll('.np-az').length).toBe(2))
    const rowAz = (call: string) =>
      screen.getByText(call).closest('[role="row"]')?.querySelector('.np-az')?.textContent ?? ''
    expect(rowAz('N0XRG')).toMatch(/^~\d+°$/)
    const exact = rowAz('KA6VKP')
    expect(exact).toMatch(/^\d+°$/) // no ~: this is the station's own square
    // And it must not be the centroid's number wearing exact clothing.
    expect(exact).not.toBe(rowAz('N0XRG').slice(1))
  })
})
