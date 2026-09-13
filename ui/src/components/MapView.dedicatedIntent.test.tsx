// @vitest-environment jsdom
//
// A DEDICATED-INTENT surface must not inherit another surface's layer picks (POTA-map defect).
//
// The `operatemap` pop-out mounts a bare MapView with intent='pota', whose whole purpose is the
// Parks (`ota`) activator layer. But `surfaceGet` deliberately inherits the PRIMARY surface's
// stored values on a surface's first open (the #199 carry-over), so the POTA map once opened with
// whatever the Connect map had — Parks OFF — because an inherited pick suppressed the intent preset
// (the only thing that sets `ota:true`).
//
// The fix is the opt-in `dedicatedIntent` prop: on a surface dedicated to one intent, an inherited
// value is another surface's pick, so only what was written ON THIS surface counts — both the
// per-intent store (`nexus.connect.intents`) and, for an operator upgrading, the old shared layer
// key. The force is self-limiting — the persist effect writes this surface's own record on mount,
// so it applies exactly once (the true first open); a later choice to turn Parks off is respected.
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

// The primary (main) surface's keys — what Connect writes — and this surface's OWN.
const BARE_STORE = 'nexus.connect.intents'
const OWN_STORE = 'nexus.connect.intents.operatemap'
const BARE_LEGACY = 'nexus.connect.layers'
const OWN_LEGACY = 'nexus.connect.layers.operatemap'

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
const OTA_OFF_LAYERS = { ...DEFAULT_LAYERS, ota: { visible: false, opacity: 1 } }
const OTA_OFF = JSON.stringify(OTA_OFF_LAYERS)
const POTA_OTA_OFF_STORE = JSON.stringify({ pota: { layers: OTA_OFF_LAYERS } })

/** POTA's Parks visibility as persisted to THIS surface's own record (what the next open restores). */
function ownParks(): boolean | null {
  const v = localStorage.getItem(OWN_STORE)
  return v ? JSON.parse(v).pota.layers.ota.visible : null
}

async function mount(intent: MapIntent, dedicated?: boolean) {
  await act(async () => {
    render(<MapView {...props(intent, dedicated)} />)
  })
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

  it('THE BUG: opens with Parks ON despite the inherited (Connect) legacy layer picks', async () => {
    // Only the bare/primary legacy key is stored, Parks off — what an older Connect map left behind.
    localStorage.setItem(BARE_LEGACY, OTA_OFF)
    await mount('pota', true)
    expect(ownParks()).toBe(true)
  })

  it('opens with Parks ON despite the primary surface’s own POTA record', async () => {
    localStorage.setItem(BARE_STORE, POTA_OTA_OFF_STORE)
    await mount('pota', true)
    expect(ownParks()).toBe(true)
  })

  it('CONTROL: the operator OWN choice on this surface is respected (force is first-open only)', async () => {
    // This surface has ALREADY written its own record with Parks off (a later open, operator's pick).
    localStorage.setItem(OWN_STORE, POTA_OTA_OFF_STORE)
    await mount('pota', true)
    expect(ownParks()).toBe(false)
  })

  it('CONTROL: an upgrading operator’s own pre-upgrade pick on this surface migrates and is respected', async () => {
    localStorage.setItem(OWN_LEGACY, OTA_OFF)
    await mount('pota', true)
    expect(ownParks()).toBe(false)
  })

  it('REGRESSION CONTROL: WITHOUT the prop a surface still inherits + suppresses the preset (#199)', async () => {
    localStorage.setItem(BARE_STORE, POTA_OTA_OFF_STORE)
    await mount('pota') // no dedicatedIntent -> inherits the primary record
    // Would FAIL if the default flipped to on: the preset would then turn Parks on.
    expect(ownParks()).toBe(false)
  })
})
