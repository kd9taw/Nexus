// @vitest-environment jsdom
//
// LONG-PATH BEAM (#338). Two testers asked for the pair — a one-click turn to the worked
// station, and the reciprocal for the long way round. The short-path half shipped; this is the
// other one, and it is a REAL render of `RotorStrip` rather than a stub, because the defect
// #338 reported in the first place was a control that rendered as nothing while its props
// looked fine from the host's side.
//
// ⚠️ WHY TWO BUTTONS AND NOT A TOGGLE. Every bearing Nexus displays is short path and says so
// out loud — `grid.ts::azimuthTitle`'s own note is that "a chaser who assumes long path on a
// low band points the beam 180° wrong". A toggle carries a state the operator can misread at a
// glance; two buttons cannot be. The reciprocal arithmetic lives in Rust (`(short + 180)
// rem_euclid 360`), so what is pinned here is that the UI ASKS for the right one.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorStrip } from './RotorStrip'

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(212)),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '127.0.0.1' } as never)),
  getSatTrackStatus: vi.fn((): Promise<null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<null> => Promise.resolve(null)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

afterEach(cleanup)
beforeEach(() => vi.clearAllMocks())

const beam = () => screen.getByRole('button', { name: '→ DL1ABC' })
const lp = () => screen.getByRole('button', { name: 'LP' })

describe('the long-path beam button', () => {
  it('asks for the reciprocal, and the short-path button still asks for short', async () => {
    const onPointAt = vi.fn()
    render(<RotorStrip targetCall="DL1ABC" onPointAt={onPointAt} />)
    // Precondition: a rotator answers, so the strip renders at all. Without this the
    // assertions below would pass vacuously against a strip that drew nothing.
    await waitFor(() => expect(screen.queryByRole('button', { name: 'LP' })).not.toBeNull())

    fireEvent.click(lp())
    expect(onPointAt).toHaveBeenCalledWith('DL1ABC', true)

    // ⭐ THE CONTROL, and it is the half that matters: adding the reciprocal must not have
    // turned the ordinary beam into a long-path slew. A test that only clicked LP would pass
    // just as well if BOTH buttons now sent `true`.
    onPointAt.mockClear()
    fireEvent.click(beam())
    expect(onPointAt).toHaveBeenCalledWith('DL1ABC', false)
  })

  it('offers neither button when the cockpit has no station in play', async () => {
    const onPointAt = vi.fn()
    const { rerender } = render(<RotorStrip targetCall="DL1ABC" onPointAt={onPointAt} />)
    await waitFor(() => expect(screen.queryByRole('button', { name: 'LP' })).not.toBeNull())
    // Same guard the short-path button has always had: no target, no slew. LP must not
    // outlive it and offer a turn toward nobody.
    rerender(<RotorStrip targetCall={null} onPointAt={onPointAt} />)
    expect(screen.queryByRole('button', { name: 'LP' })).toBeNull()
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()
  })

  it('offers neither button when the host wires no slew at all', async () => {
    render(<RotorStrip targetCall="DL1ABC" />)
    // The rotator still answers, so the strip itself is present — this is about the pair.
    await waitFor(() => expect(api.readRotator).toHaveBeenCalled())
    expect(screen.queryByRole('button', { name: 'LP' })).toBeNull()
  })
})
