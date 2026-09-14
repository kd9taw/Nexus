// @vitest-environment jsdom
//
// #174 (yannick): "filter the spots by origin, country or continent — spotted from Europe only,
// or from France only". The panel could only say "heard on MY continent". The backend now sends
// where each voice for a spot is (the spotter and every corroborator, cty.dat-resolved, since the
// UI has no cty.dat), and the panel filters on it: a spot stays when ANY voice matches.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import type { SpotRow } from '../types'

const spot = (
  call: string,
  entity: string,
  voices: { spotter: string; corroborators?: string[] },
  spotterConts: string[],
  spotterEntities: string[],
): SpotRow =>
  ({
    call,
    entity,
    zone: 0,
    state: null,
    band: '20m',
    freqMhz: 14.074,
    mode: 'Digital',
    submode: 'FT8',
    spotter: voices.spotter,
    corroborators: voices.corroborators ?? [],
    ageSecs: 60,
    comment: '',
    licensed: true,
    // Everyone is "local" here, so the default-on continent filter hides nothing and the only
    // thing narrowing the feed is the filter under test.
    spotterLocal: true,
    spotterConts,
    spotterEntities,
  }) as unknown as SpotRow

const SPOTS = [
  spot('JA1ABC', 'Japan', { spotter: 'DL8LAS' }, ['EU'], ['Fed. Rep. of Germany']),
  spot('K2DEF', 'United States', { spotter: 'W3LPL-#' }, ['NA'], ['United States']),
  // Two voices on two continents: the spotter in Japan, a corroborator in France.
  spot('VK2ABC', 'Australia', { spotter: 'JA1XYZ', corroborators: ['F5ABC'] }, ['AS', 'EU'], ['Japan', 'France']),
]

function renderPanel() {
  render(<SpotsPanel spots={SPOTS} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />)
}

afterEach(() => {
  cleanup()
  sessionStorage.clear()
  localStorage.clear()
})

describe('spotted-from filter (#174)', () => {
  it('keeps only spots some voice on the chosen continent reported', () => {
    renderPanel()
    // Positive control: all three rows are on screen before choosing anything.
    for (const c of ['JA1ABC', 'K2DEF', 'VK2ABC']) expect(screen.queryByText(c), c).toBeTruthy()

    const from = screen.getByRole('group', { name: 'Spotted from' })
    fireEvent.click(within(from).getByRole('button', { name: 'EU' }))

    expect(screen.queryByText('JA1ABC'), 'spotted from Germany').toBeTruthy()
    expect(screen.queryByText('VK2ABC'), 'a corroborator in France counts').toBeTruthy()
    expect(screen.queryByText('K2DEF'), 'spotted only from North America').toBeNull()
  })

  it('keeps only spots some voice in the chosen country reported', () => {
    renderPanel()
    const countries = screen.getByRole('group', { name: 'Spotter countries' })
    fireEvent.click(within(countries).getByRole('button', { name: 'France' }))

    expect(screen.queryByText('VK2ABC')).toBeTruthy()
    expect(screen.queryByText('JA1ABC'), 'Germany is not France').toBeNull()
    expect(screen.queryByText('K2DEF')).toBeNull()
  })

  it('Clear brings every spot back', () => {
    renderPanel()
    fireEvent.click(within(screen.getByRole('group', { name: 'Spotted from' })).getByRole('button', { name: 'NA' }))
    expect(screen.queryByText('JA1ABC')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    for (const c of ['JA1ABC', 'K2DEF', 'VK2ABC']) expect(screen.queryByText(c), c).toBeTruthy()
  })
})
