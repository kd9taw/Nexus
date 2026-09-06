// @vitest-environment jsdom
//
// The band outlook must keep refreshing WHILE A STATION IS SELECTED.
//
// Operator report (2026-09-06): "the Chase section doesn't appear to be refreshing the data,
// it stays stuck on old information, the other sections are auto updating ok" — seen in the
// detached Connect window, but NOT pop-out-specific: the effect lives in ConnectView and
// behaves identically in both windows.
//
// The cause was a guard written for one consumer and inherited by three others. The
// map/outlook strip shows `pathPred` instead of `bandOutlook` whenever a station is selected
// (ConnectView.tsx:419/:441), so the fetch early-returned on `selectedCall`. But Chase
// (ChasePane.tsx:44), the Chase feed (ChaseFeedPane.tsx:25) and the band-outlook heatmap
// (connect/panes.tsx:274/:569) read `bandOutlook` unconditionally — so a selection froze
// their openness / "best window" column at whatever it last held, forever, while every
// sibling pane carried on updating off its own poll.
//
// The second half was the `prop?.asOf` dep: `asOf` is stamped only on a real SWPC fetch and
// served from a 300 s cache (PROP_TTL_SECS, src-tauri/src/lib.rs:1239), so even with nothing
// selected this refreshed at most every five minutes rather than on any poll cadence.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'

// The map is irrelevant here and needs a real layout engine — the house stub (see
// AprsCockpit.board.test.tsx:15) keeps this test about the data cadence, not about canvas.
vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

// Mock derived from the real module, never a hand-kept literal — a partial mock leaves every
// other export undefined and the component throws on mount (the trap AmpStrip.test.tsx names).
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathPrediction: vi.fn(async () => null),
}))
import { getBandOutlook } from '../api'
import { ConnectView } from './ConnectView'

const props = (selectedCall: string | null) => ({
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall,
  onSelectCall: () => {},
  needByCall: new Map(),
  onWorkSpot: undefined,
  needAlerts: [],
  amp: null,
})

describe('the band outlook is kept warm for the panes that read it unconditionally', () => {
  beforeEach(() => {
    // jsdom has no ResizeObserver; the house stub (stop-line.test.tsx:299).
    globalThis.ResizeObserver = class {
      observe() {}
      disconnect() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver
    vi.useFakeTimers()
    vi.mocked(getBandOutlook).mockClear()
  })
  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  it('keeps refreshing while a station is selected — the Chase staleness report', () => {
    render(<ConnectView {...props('W1AW')} />)
    const onMount = vi.mocked(getBandOutlook).mock.calls.length
    expect(onMount).toBeGreaterThan(0) // fetched at all, even with a selection

    vi.advanceTimersByTime(5 * 60_000)
    expect(vi.mocked(getBandOutlook).mock.calls.length).toBeGreaterThan(onMount)
    // Before the fix this stayed at 0: the effect early-returned on `selectedCall`, so
    // Chase's openness column froze at whatever it held when the station was picked.
  })

  it('also refreshes with nothing selected, on its own cadence rather than prop.asOf', () => {
    render(<ConnectView {...props(null)} />)
    const onMount = vi.mocked(getBandOutlook).mock.calls.length
    vi.advanceTimersByTime(5 * 60_000)
    // The control for the case above: unselected was never the broken path, and it must
    // still refresh — on the interval, NOT gated behind a `prop.asOf` that only moves once
    // per 300 s server cache window (and never at all when `prop` is null, as here).
    expect(vi.mocked(getBandOutlook).mock.calls.length).toBeGreaterThan(onMount)
  })
})
