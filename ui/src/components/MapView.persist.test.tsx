// @vitest-environment jsdom
//
// LAYER PERSISTENCE SURVIVES A MOUNT/REMOUNT ROUND-TRIP (#211 follow-up).
//
// #199 shipped per-surface layer persistence; the existing MapView.layers.test.ts covers only
// the pure `layersFromStored` parser, not the component's mount-time interplay between the
// restored layer state and the intent-preset effect. This file closes that gap: it drives the
// real component so the guard that yields the preset to a restored pick (the `intentFirstRun` +
// `hadStoredLayers` refs) is exercised on first mount AND on a true unmount/remount, and the
// positive control confirms an *actual* intent switch still re-applies the preset.
//
// NOTE: #211 reported layers "not persisting" on 1.10.2. The hypothesised seam was the intent
// effect clobbering restored state on (re)mount; these tests prove that seam is correct — the
// clobber does not reproduce here. The remaining coverage this file adds is the regression guard.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { MapView, DEFAULT_LAYERS, type MapIntent } from './MapView'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

// jsdom has no ResizeObserver; MapView installs one on its wrap ref. A no-op keeps `size` at
// {0,0}, which is exactly what makes the heavy canvas draw effect bail before `getContext`.
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const LAYERS_KEY = 'nexus.connect.layers'

function props(intent: MapIntent) {
  return {
    myGrid: 'EN52',
    theme: 'dark' as never,
    stations: [],
    prop: null,
    selectedCall: null,
    onSelectCall: () => {},
    needByCall: new Map(),
    intent,
  }
}

/** The layer table the persist effect wrote back — the value the next launch would restore. */
function stored() {
  const v = localStorage.getItem(LAYERS_KEY)
  return v ? JSON.parse(v) : null
}

// A restored pick with OTA off; the 'pota' preset would turn OTA on, so OTA is the discriminator
// between "restored pick honoured" (false) and "preset clobbered it" (true).
const RESTORED = JSON.stringify({ ...DEFAULT_LAYERS, ota: { visible: false, opacity: 1 } })

describe('MapView layer persistence round-trip', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('first mount keeps the restored pick against the intent preset', async () => {
    localStorage.setItem(LAYERS_KEY, RESTORED)
    await act(async () => {
      render(<MapView {...props('pota')} />)
    })
    expect(stored().ota.visible).toBe(false)
  })

  it('a true unmount/remount still keeps the restored pick', async () => {
    localStorage.setItem(LAYERS_KEY, RESTORED)
    let first: ReturnType<typeof render>
    await act(async () => {
      first = render(<MapView {...props('pota')} />)
    })
    await act(async () => first.unmount())
    await act(async () => {
      render(<MapView {...props('pota')} />)
    })
    expect(stored().ota.visible).toBe(false)
  })

  it('POSITIVE CONTROL — an actual intent switch DOES re-apply the preset', async () => {
    localStorage.setItem(LAYERS_KEY, RESTORED)
    let v: ReturnType<typeof render>
    await act(async () => {
      v = render(<MapView {...props('casual')} />) // casual leaves OTA alone
    })
    expect(stored().ota.visible).toBe(false)
    await act(async () => {
      v.rerender(<MapView {...props('pota')} />) // switching TO pota turns OTA on
    })
    expect(stored().ota.visible).toBe(true)
  })
})
