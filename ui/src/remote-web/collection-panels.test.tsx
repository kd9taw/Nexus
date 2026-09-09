// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StationControlContext } from '../stationAccess'
import { SpotsPanel } from '../components/SpotsPanel'
import { NeededPanel } from '../components/NeededPanel'
import type { NeedAlert, SpotRow } from '../types'

afterEach(() => { cleanup(); sessionStorage.clear(); localStorage.clear() })
const spot: SpotRow = { call: 'W1AW', entity: 'United States', zone: 5, band: '20m', freqMhz: 14.025,
  mode: 'CW', spotter: 'K3LR', corroborators: [], ageSecs: 2, comment: '', licensed: true, spotterLocal: true }
it('the existing Spots panel selects and filters without working the station in an observer session', () => {
  const work = vi.fn(), select = vi.fn()
  render(<StationControlContext.Provider value={false}><SpotsPanel spots={[spot]} bandPlan={[]} selectedCall={null} onSelect={select} onWork={work} /></StationControlContext.Provider>)
  fireEvent.click(screen.getByText('W1AW').closest('.np-row')!)
  expect(select).toHaveBeenCalledWith('W1AW')
  expect(work).not.toHaveBeenCalled()
  fireEvent.change(screen.getByLabelText('Search spots'), { target: { value: 'JA1' } })
  expect(screen.queryByText('W1AW')).toBeNull()
})
it('Needed observation cannot QSY or point an antenna from click or keyboard paths', () => {
  const work = vi.fn(), qsy = vi.fn(), point = vi.fn(), select = vi.fn()
  const alert = { call: 'W1AW', band: '20m', freqMhz: 14.025, mode: 'CW', entity: 'United States', zone: 5,
    priority: 100, tags: ['NewEntity'], headline: 'New entity', evidence: 'Local receiver', admittedAt: Math.floor(Date.now() / 1000) } as unknown as NeedAlert
  render(<StationControlContext.Provider value={false}><NeededPanel alerts={[alert]} bandPlan={[]} selectedCall={null} onSelect={select} onWork={work} onQsy={qsy} onPoint={point} /></StationControlContext.Provider>)
  fireEvent.click(screen.getByText('W1AW').closest('.np-row')!)
  expect(select).toHaveBeenCalledWith('W1AW')
  select.mockClear()
  fireEvent.keyDown(screen.getByText('W1AW').closest('.np-row')!, { key: 'Enter' })
  expect(select).toHaveBeenCalledWith('W1AW')
  expect(work).not.toHaveBeenCalled(); expect(qsy).not.toHaveBeenCalled(); expect(point).not.toHaveBeenCalled()
  expect(screen.queryByLabelText('Azimuth')).toBeNull()
})
