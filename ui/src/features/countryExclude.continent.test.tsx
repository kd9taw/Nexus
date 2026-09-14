// @vitest-environment jsdom
//
// #229 (pa0kgb): "show only certain continents in FTx band activity — hide-by-country does not
// scale for a continent-wide filter." A ticked continent hides every entity on it, through the
// same entity-name set `isHiddenByCountry` already matches on, so every protection (the QSO
// partner, a call to us, a needed entity) applies unchanged. The UI has no cty.dat: the
// entity → continent table comes from the backend.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'

vi.mock('../api', () => ({
  getDxccEntityContinents: vi.fn(() =>
    Promise.resolve([
      ['Fed. Rep. of Germany', 'EU'],
      ['France', 'EU'],
      ['Japan', 'AS'],
      ['United States', 'NA'],
    ]),
  ),
}))

import { COUNTRY_EXCLUDE_CONTINENTS_KEY, useCountryExclude } from './countryExclude'

function Probe() {
  const c = useCountryExclude()
  return (
    <div>
      <span data-testid="hidden">{[...c.hidden].sort().join('|')}</span>
      <span data-testid="continents">{[...c.continents].join('|')}</span>
      <button onClick={() => c.toggleContinent('EU')}>EU</button>
      <button onClick={() => c.setPaused(!c.paused)}>pause</button>
      <button onClick={c.clear}>clear</button>
    </div>
  )
}

const hidden = () => screen.getByTestId('hidden').textContent

beforeEach(() => localStorage.clear())
afterEach(() => cleanup())

describe('hide by continent (#229)', () => {
  it('a ticked continent hides every entity on it, and persists under its own key', async () => {
    render(<Probe />)
    expect(hidden(), 'nothing ticked, nothing hidden').toBe('')
    fireEvent.click(screen.getByText('EU'))
    await waitFor(() => expect(hidden()).toBe('Fed. Rep. of Germany|France'))
    expect(localStorage.getItem(COUNTRY_EXCLUDE_CONTINENTS_KEY)).toBe('["EU"]')
  })

  it('pause shows everything but keeps the tick; clear drops it', async () => {
    render(<Probe />)
    fireEvent.click(screen.getByText('EU'))
    await waitFor(() => expect(hidden()).not.toBe(''))
    fireEvent.click(screen.getByText('pause'))
    await waitFor(() => expect(hidden()).toBe(''))
    expect(screen.getByTestId('continents').textContent, 'the tick survives a pause').toBe('EU')
    fireEvent.click(screen.getByText('pause'))
    await waitFor(() => expect(hidden()).toBe('Fed. Rep. of Germany|France'))
    fireEvent.click(screen.getByText('clear'))
    await waitFor(() => expect(hidden()).toBe(''))
    expect(screen.getByTestId('continents').textContent).toBe('')
  })

  it('ignores a stored code that is not a continent — a hide filter must never guess', async () => {
    localStorage.setItem(COUNTRY_EXCLUDE_CONTINENTS_KEY, '["EU","XX",7]')
    render(<Probe />)
    expect(screen.getByTestId('continents').textContent).toBe('EU')
    await waitFor(() => expect(hidden()).toBe('Fed. Rep. of Germany|France'))
  })
})
