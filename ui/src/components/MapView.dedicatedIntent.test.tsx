// @vitest-environment jsdom
//
// A DEDICATED-INTENT surface must not inherit another surface's layer picks (POTA-map defect).
//
// The `operatemap` pop-out mounts a bare MapView with intent='pota', whose whole purpose is the
// Parks (`ota`) activator layer. But `surfaceGet` deliberately inherits the PRIMARY surface's
// stored layers on a surface's first open (the #199 carry-over), so `hadStoredLayers` was TRUE
// on the POTA map's first mount — seeded from whatever the Connect map had — and the intent
// preset (the only thing that sets `ota:true`) was suppressed. The map then opened showing
// Connect's layers with Parks OFF.
//
// The fix is the opt-in `dedicatedIntent` prop: on a surface dedicated to one intent, an
// inherited value is another surface's pick for a different purpose, so `hadStoredLayers` must
// consult THIS surface's OWN key only. The force is self-limiting — the persist effect writes
// this surface's own key on mount, so it applies exactly once (the true first open); a later
// operator choice to turn Parks off is then respected.
//
// These drive the REAL component on a NON-MAIN surface (`?panel=operatemap`), the coverage the
// stubbed DetachedPanel.operatemap.test.tsx cannot give: it asserts the prop is passed, never
// that Parks renders on.
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

// jsdom has no ResizeObserver; a no-op keeps `size` at {0,0}, which makes the heavy canvas draw
// effect bail before `getContext` (same trick as MapView.persist.test.tsx).
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const BARE_KEY = 'nexus.connect.layers' // the primary (main) surface's key — what Connect writes
const OWN_KEY = 'nexus.connect.layers.operatemap' // the operatemap surface's OWN key

function props(intent: MapIntent, dedicatedIntent?: boolean) {
  return {
    myGrid: 'EN52',
    theme: 'dark' as never,
    stations: [],
    prop: null,
    selectedCall: null,
    onSelectCall: () => {},
    needByCall: new Map(),
    intent,
    dedicatedIntent,
  }
}

// A stored layer table with Parks OFF; the 'pota' preset would turn Parks ON, so `ota` is the
// discriminator between "an inherited/own pick honoured" (false) and "the preset applied" (true).
const OTA_OFF = JSON.stringify({ ...DEFAULT_LAYERS, ota: { visible: false, opacity: 1 } })

/** The layer table the persist effect wrote to THIS surface's own key (the next open restores it). */
function storedOwn() {
  const v = localStorage.getItem(OWN_KEY)
  return v ? JSON.parse(v) : null
}

describe('MapView dedicated-intent surface (POTA map)', () => {
  beforeEach(() => {
    localStorage.clear()
    // A torn-off, single-purpose surface: `surfaceGet`/`surfaceSet` key off `?panel=`.
    window.history.replaceState({}, '', '/?panel=operatemap')
  })
  afterEach(() => {
    cleanup()
    window.history.replaceState({}, '', '/')
  })

  it('THE BUG: opens with Parks ON despite the inherited (Connect) layer picks', async () => {
    // Only the bare/primary key is stored, Parks off — exactly what the Connect map leaves behind.
    localStorage.setItem(BARE_KEY, OTA_OFF)
    await act(async () => {
      render(<MapView {...props('pota', true)} />)
    })
    expect(storedOwn().ota.visible).toBe(true)
  })

  it('CONTROL: the operator OWN choice on this surface is respected (force is first-open only)', async () => {
    // This surface has ALREADY written its own key with Parks off (a later open, operator's pick).
    localStorage.setItem(OWN_KEY, OTA_OFF)
    await act(async () => {
      render(<MapView {...props('pota', true)} />)
    })
    expect(storedOwn().ota.visible).toBe(false)
  })

  it('REGRESSION CONTROL: WITHOUT the prop a surface still inherits + suppresses the preset (#199)', async () => {
    localStorage.setItem(BARE_KEY, OTA_OFF)
    await act(async () => {
      render(<MapView {...props('pota')} />) // no dedicatedIntent -> inherits the bare key
    })
    // Would FAIL if the default flipped to on: the preset would then turn Parks on.
    expect(storedOwn().ota.visible).toBe(false)
  })
})
