// @vitest-environment jsdom
//
// THE LAYERS PANEL COLLAPSES LIKE CONDITIONS.
//
// Connect already has one collapsible overlay: the Conditions rail on the map's right edge folds to
// a pill and remembers that per window (`nexus.connect.insights.collapsed`). The Layers panel — a
// long list of toggles and opacity sliders on a map that is only ~460 px wide at the 1024×768 floor —
// had no way out of the way. It now folds the same way: a chevron to collapse, a pill to bring it
// back, remembered per window, with the expanded state exposed to assistive tech. The 3-D globe's
// panel shares the component and the record (Globe3D.render.test.tsx).
//
// Drives the REAL MapView; only the api is stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { MapView } from './MapView'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const KEY = 'nexus.connect.layersPanel.collapsed'

const props = {
  myGrid: 'EN52',
  theme: 'dark' as never,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  intent: 'dx' as const,
}

async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<MapView {...props} />)
  })
  return r
}

const panel = () => screen.queryByRole('complementary', { name: 'Layers' })
const pill = () => screen.queryByRole('button', { name: 'Layers' })
const collapseBtn = () => screen.getByRole('button', { name: 'Collapse Layers' })

describe('the 2-D Layers panel collapses to a pill, like Conditions', () => {
  beforeEach(() => {
    localStorage.clear()
    window.history.replaceState({}, '', '/')
  })
  afterEach(() => {
    cleanup()
    window.history.replaceState({}, '', '/')
  })

  it('collapses to a pill and expands back, with the state exposed to assistive tech', async () => {
    await mount()
    expect(panel(), 'CONTROL: open by default').not.toBeNull()
    expect(screen.getByLabelText('Band heat (openings)')).toBeTruthy()
    expect(pill(), 'no pill while open').toBeNull()

    const btn = collapseBtn()
    expect(btn.getAttribute('aria-expanded')).toBe('true')
    fireEvent.click(btn)
    expect(panel()).toBeNull()
    expect(screen.queryByLabelText('Band heat (openings)')).toBeNull()

    const p = pill()
    expect(p).not.toBeNull()
    expect(p!.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(p!)
    expect(panel()).not.toBeNull()
    expect(screen.getByLabelText('Band heat (openings)')).toBeTruthy()
  })

  it('is remembered for this window across a remount', async () => {
    const first = await mount()
    fireEvent.click(collapseBtn())
    expect(localStorage.getItem(KEY)).toBe('1')
    await act(async () => first.unmount())

    await mount()
    expect(panel()).toBeNull()
    expect(pill()).not.toBeNull()
  })

  it('is per window: collapsing it in a pop-out leaves the main window’s record alone', async () => {
    window.history.replaceState({}, '', '/?panel=connect')
    await mount()
    fireEvent.click(collapseBtn())
    expect(localStorage.getItem(`${KEY}.connect`)).toBe('1')
    expect(localStorage.getItem(KEY)).toBeNull()
  })
})
