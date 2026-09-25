// @vitest-environment jsdom
//
// LAYER PERSISTENCE SURVIVES A MOUNT/REMOUNT ROUND-TRIP (#211 follow-up).
//
// #199 shipped per-surface layer persistence; the existing MapView.layers.test.ts covers only
// the pure `layersFromStored` parser, not the component's mount-time interplay between the
// restored layer state and the intent preset. This file closes that gap: it drives the real
// component so the rule that yields the preset to a restored pick is exercised on first mount AND
// on a true unmount/remount, and the positive control confirms an *actual* intent switch to an
// intent never used before still applies that intent's preset.
//
// The picks now live per intent (`features/intentMapSettings`, `nexus.connect.intents`). These
// tests seed the OLD shared key on purpose: that is what an upgrading operator has on disk, and
// the migration into the active intent is what keeps their pick.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { MapView, DEFAULT_LAYERS, type MapIntent } from './MapView'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
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

const LEGACY_LAYERS_KEY = 'nexus.connect.layers'

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

/** The layer table the persist effect wrote back for `intent` — what the next launch restores. */
function stored(intent: MapIntent) {
  const v = localStorage.getItem('nexus.connect.intents')
  return v ? JSON.parse(v)[intent]?.layers : null
}

// A restored pick with OTA off; the 'pota' preset would turn OTA on, so OTA is the discriminator
// between "restored pick honoured" (false) and "preset clobbered it" (true).
const RESTORED = JSON.stringify({ ...DEFAULT_LAYERS, ota: { visible: false, opacity: 1 } })

describe('MapView layer persistence round-trip', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('attaches to a canvas that appears after station location arrives, including recovery', () => {
    const observe=vi.spyOn(RO.prototype,'observe'),disconnect=vi.spyOn(RO.prototype,'disconnect')
    try {
      const base=props('casual')
      const view=render(<MapView {...base} myGrid="" />)
      expect(view.container.querySelector('canvas')).toBeNull()
      for(let attempt=0;attempt<2;attempt++){
        view.rerender(<MapView {...base} myGrid="FN31" />)
        const wrap=view.container.querySelector('.map-canvas-wrap')
        expect(wrap).not.toBeNull()
        expect(observe).toHaveBeenLastCalledWith(wrap)
        expect(observe).toHaveBeenCalledTimes(attempt+1)
        view.rerender(<MapView {...base} myGrid="" />)
        expect(view.container.querySelector('canvas')).toBeNull()
        expect(disconnect).toHaveBeenCalledTimes(attempt+1)
      }
    } finally { observe.mockRestore();disconnect.mockRestore() }
  })

  it('first mount keeps the restored pick against the intent preset', async () => {
    localStorage.setItem(LEGACY_LAYERS_KEY, RESTORED)
    await act(async () => {
      render(<MapView {...props('pota')} />)
    })
    expect(stored('pota').ota.visible).toBe(false)
  })

  it('a true unmount/remount still keeps the restored pick', async () => {
    localStorage.setItem(LEGACY_LAYERS_KEY, RESTORED)
    let first: ReturnType<typeof render>
    await act(async () => {
      first = render(<MapView {...props('pota')} />)
    })
    await act(async () => first.unmount())
    await act(async () => {
      render(<MapView {...props('pota')} />)
    })
    expect(stored('pota').ota.visible).toBe(false)
  })

  it('POSITIVE CONTROL — switching to an intent never used before DOES apply its preset', async () => {
    localStorage.setItem(LEGACY_LAYERS_KEY, RESTORED)
    let v: ReturnType<typeof render>
    await act(async () => {
      v = render(<MapView {...props('casual')} />) // casual leaves OTA alone
    })
    expect(stored('casual').ota.visible).toBe(false)
    await act(async () => {
      v.rerender(<MapView {...props('pota')} />) // first use of pota turns OTA on
    })
    expect(stored('pota').ota.visible).toBe(true)
    // …and Ragchew's own record was not touched by the switch.
    expect(stored('casual').ota.visible).toBe(false)
  })
})
