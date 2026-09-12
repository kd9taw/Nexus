// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import type { View } from '../features/registry'
import { StationDataContext } from '../stationAccess'
import { Menu } from '../components/ui/Menu'
import { QuickNavigation, RemotePresentationContext } from './presentation'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })
function setup(enabled: View[] = ['operate', 'cw', 'phone', 'needed', 'logbook']) {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  const select = vi.fn(), change = vi.fn()
  function Harness() {
    const [view, setView] = useState<View>('cw')
    return <StationDataContext.Provider value={false}>
      <RemotePresentationContext.Provider value={{ presentation: 'quick', change, radioDetails: false, setRadioDetails: () => {} }}>
        <div className="app"><QuickNavigation view={view} available={v => enabled.includes(v)}
          onSelect={v => { select(v); setView(v) }} /></div>
      </RemotePresentationContext.Provider>
    </StationDataContext.Provider>
  }
  render(<Harness />)
  return { select, change }
}

it('opens only available Nexus modes with the keyboard even while station data is unavailable', async () => {
  const h = setup()
  const trigger = screen.getByRole('button', { name: 'Choose operating view' })
  expect(trigger.textContent).toBe('CW')
  fireEvent.keyDown(trigger, { key: 'ArrowDown' })
  const phone = await screen.findByRole('menuitem', { name: 'Phone' })
  expect(screen.getAllByRole('menuitem').map(e => e.textContent)).toEqual(['FT', 'Phone', 'CW'])
  fireEvent.click(phone)
  expect(h.select).toHaveBeenCalledExactlyOnceWith('phone')
  expect(screen.queryByRole('menu')).toBeNull()
  expect(screen.getByRole('button', { name: 'Choose operating view' }).textContent).toBe('Phone')
  expect(h.change).not.toHaveBeenCalled()
})

it('returns directly from Hunt and Log to the last operating view without opening its picker', () => {
  const h = setup()
  for (const destination of ['Hunt', 'Log']) {
    fireEvent.click(screen.getByRole('button', { name: destination }))
    fireEvent.click(screen.getByRole('button', { name: 'Operate' }))
    expect(h.select).toHaveBeenLastCalledWith('cw')
    expect(screen.queryByRole('menu')).toBeNull()
  }
  fireEvent.click(screen.getByRole('button', { name: 'Full Nexus' }))
  expect(h.change).toHaveBeenCalledExactlyOnceWith('full')
})

it('keeps the existing station-dependent menu closed when data is unavailable', () => {
  render(<StationDataContext.Provider value={false}><Menu trigger={<button>Station action</button>}
    items={[{ label: 'Apply', onSelect: vi.fn() }]} /></StationDataContext.Provider>)
  fireEvent.keyDown(screen.getByRole('button', { name: 'Station action' }), { key: 'ArrowDown' })
  expect(screen.queryByRole('menu')).toBeNull()
})
