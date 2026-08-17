// @vitest-environment jsdom
//
// "Not now" must actually mean not now (operator-adjacent report, 2026-08-16).
//
// `dismissed` was a boolean that was WRITTEN by dismiss() and read by nothing, so closing the
// update banner lasted exactly until the hourly re-check re-ran the whole flow, silently
// re-downloaded, and put the banner straight back. Dismissal is per-VERSION on purpose:
// refusing 1.4.1 today must not also swallow 1.5.0 next month — an update the operator has
// never been asked about is not something a stale "no" should answer.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useSelfUpdate } from './useSelfUpdate'

type Handle = {
  available: boolean
  version?: string
  download?: (cb: (ev: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void) => Promise<void>
}

let nextCheck: () => Promise<Handle | null>
const checkCalls: number[] = []

beforeEach(() => {
  vi.useFakeTimers()
  checkCalls.length = 0
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    updater: {
      check: () => {
        checkCalls.push(Date.now())
        return nextCheck()
      },
    },
  }
})
afterEach(() => {
  vi.useRealTimers()
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
})

const HOUR = 60 * 60 * 1000

function offering(version: string): () => Promise<Handle | null> {
  return () =>
    Promise.resolve({
      available: true,
      version,
      download: () => Promise.resolve(), // instant "download" — phase goes straight to ready
    })
}

/** Let the hook's async check/download chain settle under fake timers. */
async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0)
  })
}

describe('self-update dismissal', () => {
  it('downloads an offered update and reaches ready (control)', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate())
    await settle()
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('9.9.9')
  })

  it('a dismissed version STAYS dismissed across the hourly re-check', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate())
    await settle()
    expect(result.current.phase).toBe('ready')

    act(() => result.current.dismiss())
    expect(result.current.phase).toBe('idle')

    // The hourly tick re-checks — and must NOT resurrect the banner for the same version.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOUR + 1000)
    })
    expect(checkCalls.length, 'control: the re-check itself still runs').toBeGreaterThanOrEqual(2)
    expect(result.current.phase, 'the dismissed version must not come back').toBe('idle')
  })

  it('a NEWER version than the dismissed one still lands', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate())
    await settle()
    act(() => result.current.dismiss())

    nextCheck = offering('10.0.0')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOUR + 1000)
    })
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('10.0.0')
  })
})
