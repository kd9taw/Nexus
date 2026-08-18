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
  install?: () => Promise<void>
}

let nextCheck: () => Promise<Handle | null>
const checkCalls: number[] = []
/** Every `invoke(cmd)` the hook made through the api bridge, in order. */
const invoked: string[] = []

beforeEach(() => {
  vi.useFakeTimers()
  checkCalls.length = 0
  invoked.length = 0
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    updater: {
      check: () => {
        checkCalls.push(Date.now())
        return nextCheck()
      },
    },
    // The api.ts bridge: update_install_block answers "nothing blocks" so install()
    // proceeds; restart_app just has to be SEEN — the assertion is that it is called.
    core: {
      invoke: (cmd: string) => {
        invoked.push(cmd)
        return Promise.resolve(cmd === 'update_install_block' ? null : undefined)
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

describe('install and restart', () => {
  // The mac QA audit (2026-08-17): the updater plugin's install() swaps the bundle on disk
  // and resolves with the OLD build still running on macOS/Linux — nothing restarts unless
  // WE do it. The hook must therefore invoke restart_app AFTER the plugin install resolves,
  // never before (a restart mid-swap would relaunch a half-written bundle).
  it('install() asks the backend to restart once the plugin install resolves', async () => {
    let installed = false
    nextCheck = () =>
      Promise.resolve({
        available: true,
        version: '9.9.9',
        download: () => Promise.resolve(),
        install: () => {
          installed = true
          return Promise.resolve()
        },
      })
    const { result } = renderHook(() => useSelfUpdate())
    await settle()
    expect(result.current.phase).toBe('ready')

    act(() => result.current.install())
    await settle()
    expect(installed, 'control: the plugin install itself ran').toBe(true)
    expect(invoked).toContain('restart_app')
    expect(result.current.phase, 'still installing while the restart is in flight').toBe('installing')
  })

  it('a failing plugin install surfaces the error and never restarts', async () => {
    nextCheck = () =>
      Promise.resolve({
        available: true,
        version: '9.9.9',
        download: () => Promise.resolve(),
        install: () => Promise.reject(new Error('Read-only file system (os error 30)')),
      })
    const { result } = renderHook(() => useSelfUpdate())
    await settle()

    act(() => result.current.install())
    await settle()
    expect(result.current.phase).toBe('error')
    expect(result.current.error).toContain('Read-only file system')
    expect(invoked, 'a failed swap must not restart into the old bundle').not.toContain('restart_app')
  })
})
