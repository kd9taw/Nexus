// @vitest-environment jsdom
//
// FULL-SCREEN MAP — one button in, the SAME button out, and Escape as the second exit.
//
// Operator request: the map is run popped out on a second monitor and they want the whole
// window to be map, with the Layers panel out of the way, on one press. The two ways that
// request goes wrong are both trap-shaped, so both are pinned here:
//
//   1. A way in that is not the way out — a mode you enter with a button and leave with
//      something else, or worse, leave with nothing visible at all. The toolbar is therefore
//      the one piece of chrome full screen KEEPS, and the assertions below hold the exit
//      button to being the same DOM node that turned the mode on.
//   2. Layers locked to whatever was on when you pressed it. The panel is hidden, not gone:
//      the toolbar grows a Layers button while full, and it brings the panel back.
//
// Plus the storage trap this codebase has already paid for once (the POTA map opening with
// Parks off, MapView.dedicatedIntent.test.tsx): `surfaceGet` INHERITS the primary surface's
// value on a surface's first open. For layers that carry-over is the feature; for a
// window-shape flag it would open a brand-new pop-out with its chrome already hidden. The
// last two tests are the inheritance guard and its positive control.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
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

// jsdom has no ResizeObserver; a no-op keeps `size` at {0,0}, which makes the heavy canvas
// draw effect bail before `getContext` (the house stub — MapView.persist.test.tsx:29).
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const FULL_KEY = 'nexus.connect.mapfull'

function props(extra: Record<string, unknown> = {}) {
  return {
    myGrid: 'EN52',
    theme: 'dark' as never,
    stations: [],
    prop: null,
    selectedCall: null,
    onSelectCall: () => {},
    needByCall: new Map(),
    ...extra,
  }
}

async function mount(extra: Record<string, unknown> = {}) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<MapView {...props(extra)} />)
  })
  return r
}

/** The Layers panel — present in flow normally, hidden by full screen, peeked back by the
 *  toolbar's Layers button (as an overlay, which is CSS; jsdom lays nothing out). */
const layersPanel = (c: HTMLElement) => c.querySelector('.map-layers')
const fullBtn = () => screen.getByRole('button', { name: /full screen/i })

describe('the map goes full screen on one button', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('hides the Layers panel, and the SAME button brings it back', async () => {
    const { container } = await mount()
    expect(layersPanel(container), 'normally the panel is in flow').not.toBeNull()

    const btn = fullBtn()
    fireEvent.click(btn)
    expect(layersPanel(container)).toBeNull()
    expect(container.querySelector('.map-view')?.className).toContain('map-full')

    // THE WAY OUT IS THE WAY IN — the same node, still on screen, now saying so.
    const back = screen.getByRole('button', { name: /exit full screen/i })
    expect(back, 'the exit must be the button that turned it on').toBe(btn)
    expect(back.getAttribute('aria-pressed')).toBe('true')

    fireEvent.click(back)
    expect(layersPanel(container)).not.toBeNull()
    expect(container.querySelector('.map-view')?.className).not.toContain('map-full')
  })

  it('keeps the map controls reachable while full — zoom, projection, and a way to Layers', async () => {
    const { container } = await mount()
    fireEvent.click(fullBtn())

    // Every control the operator steers the map with is still rendered: the toolbar is
    // deliberately the chrome full screen keeps.
    expect(screen.getByRole('button', { name: 'Zoom in' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Zoom out' })).toBeTruthy()
    expect(container.querySelectorAll('.map-proj').length).toBeGreaterThan(0)

    // And the layer picks are not frozen at whatever was on when they pressed it.
    const layersBtn = screen.getByRole('button', { name: /layers/i })
    expect(layersPanel(container)).toBeNull()
    fireEvent.click(layersBtn)
    expect(layersPanel(container), 'the Layers button peeks the panel back').not.toBeNull()
    // Still full screen — the peek is an overlay, not an exit.
    expect(container.querySelector('.map-view')?.className).toContain('map-full')
    fireEvent.click(layersBtn)
    expect(layersPanel(container)).toBeNull()
  })

  it('Escape leaves full screen — the second-monitor exit that is not the mouse', async () => {
    const { container } = await mount()
    fireEvent.click(fullBtn())
    expect(container.querySelector('.map-view')?.className).toContain('map-full')

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(container.querySelector('.map-view')?.className).not.toContain('map-full')
    expect(layersPanel(container)).not.toBeNull()
  })

  it('CONTROL — Escape does nothing when the map is not full screen', async () => {
    // The listener must not be a standing window-level Escape handler: it exists only while
    // the mode it leaves is on. Without this, "Escape exits" could be an unconditional
    // handler that happens to look right.
    const { container } = await mount()
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(container.querySelector('.map-view')?.className).not.toContain('map-full')
    expect(layersPanel(container)).not.toBeNull()
  })

  it('yields Escape to whatever already answered it (a dialog closes first)', async () => {
    const { container } = await mount()
    fireEvent.click(fullBtn())
    // An overlay that handled the key marks the event handled; tearing the window's chrome
    // down on the same press would be two things happening on one keystroke.
    const e = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true })
    e.preventDefault()
    window.dispatchEvent(e)
    expect(container.querySelector('.map-view')?.className).toContain('map-full')
  })

  it('reports the mode to its host, on mount and on every change', async () => {
    // The seam Connect hides its header, rails and strip on. Reported on mount so a restored
    // full-screen surface opens with the frame already gone instead of flashing it away.
    const onFullChange = vi.fn()
    await mount({ onFullChange })
    expect(onFullChange).toHaveBeenLastCalledWith(false)
    fireEvent.click(fullBtn())
    expect(onFullChange).toHaveBeenLastCalledWith(true)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onFullChange).toHaveBeenLastCalledWith(false)
  })
})

describe('full screen is remembered per surface, and never inherited', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => {
    cleanup()
    window.history.replaceState({}, '', '/')
  })

  it('persists the choice and restores it on a true remount', async () => {
    const first = await mount()
    fireEvent.click(fullBtn())
    expect(localStorage.getItem(FULL_KEY)).toBe('1')
    await act(async () => first.unmount())

    const { container } = await mount()
    expect(container.querySelector('.map-view')?.className).toContain('map-full')
    expect(layersPanel(container)).toBeNull()
  })

  it('a NEW surface does NOT inherit the primary surface’s full screen', async () => {
    // The bug shape: the main window is full screen, the operator opens the POTA map pop-out,
    // and it comes up with no header, no rails and no Layers panel — chrome they never hid,
    // on a window they just opened. `surfaceGet`'s carry-over is right for a layer pick and
    // wrong for a window shape.
    localStorage.setItem(FULL_KEY, '1') // the primary (main) surface's key
    window.history.replaceState({}, '', '/?panel=operatemap')
    const { container } = await mount()
    expect(container.querySelector('.map-view')?.className).not.toContain('map-full')
    expect(layersPanel(container)).not.toBeNull()
  })

  it('POSITIVE CONTROL — this surface’s OWN record still restores it', async () => {
    // Without this the test above would pass on a build where the flag never restores at all.
    localStorage.setItem(`${FULL_KEY}.operatemap`, '1')
    window.history.replaceState({}, '', '/?panel=operatemap')
    const { container } = await mount()
    expect(container.querySelector('.map-view')?.className).toContain('map-full')
  })
})
