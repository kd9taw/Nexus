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
import { checkForUpdateManual } from './features/updateCheck'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import { t } from './i18n'
import type { UpdateInfo } from './types'

type Handle = {
  available: boolean
  version?: string
  download?: (cb: (ev: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void) => Promise<void>
  install?: () => Promise<void>
}

let nextCheck: () => Promise<Handle | null>
/** What `check_beta_update` returns when the beta channel is exercised (null = up to date; an
 *  Error = the check fails with it). */
let betaInfo: { version: string; notes: string | null } | null | Error = null
/** What `update_route` says of this install: a package the updater can replace (the NSIS setup,
 *  the AppImage, the macOS app) unless a test is a .deb. */
let selfUpdates = true
/** What the old notice's own feed says (`check_for_update`); null = unreachable. */
let noticeInfo: UpdateInfo | null = null
/** Whether `open_download_page` reached a browser. */
let pageOpens = true
const checkCalls: number[] = []
/** Every `invoke(cmd)` the hook made through the api bridge, in order. */
const invoked: string[] = []
let toasts: Toast[] = []
let unsubscribe: () => void = () => {}

const PAGE = 'https://github.com/kd9taw/nexus/releases/latest'
const INFO: UpdateInfo = { current: '1.15.0', latest: '9.9.9', updateAvailable: true, downloadUrl: PAGE }

beforeEach(() => {
  vi.useFakeTimers()
  checkCalls.length = 0
  invoked.length = 0
  betaInfo = null
  selfUpdates = true
  noticeInfo = null
  pageOpens = true
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
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
        if (cmd === 'update_install_block') return Promise.resolve(null)
        if (cmd === 'check_beta_update') {
          return betaInfo instanceof Error ? Promise.reject(betaInfo) : Promise.resolve(betaInfo)
        }
        if (cmd === 'update_route') return Promise.resolve({ selfUpdate: selfUpdates, downloadPage: PAGE })
        if (cmd === 'app_version') return Promise.resolve('1.15.0')
        if (cmd === 'check_for_update') {
          return noticeInfo ? Promise.resolve(noticeInfo) : Promise.reject(new Error('offline'))
        }
        if (cmd === 'open_download_page') {
          return pageOpens ? Promise.resolve() : Promise.reject(new Error('no launcher opened it'))
        }
        return Promise.resolve(undefined)
      },
    },
  }
})
afterEach(() => {
  vi.useRealTimers()
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
})

/** The old prompt: the "update available" toast, found by its own words. */
const notices = () =>
  toasts.filter((x) => x.message === t('update.available', { latest: '9.9.9', current: '1.15.0' }))
const messages = () => toasts.map((x) => x.message)

const HOUR = 60 * 60 * 1000

function offering(
  version: string,
  download: () => Promise<void> = () => Promise.resolve(), // instant "download" — phase goes straight to ready
): () => Promise<Handle | null> {
  return () =>
    Promise.resolve({
      available: true,
      version,
      download,
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
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('9.9.9')
  })

  it('a dismissed version STAYS dismissed across the hourly re-check', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate(false))
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
    const { result } = renderHook(() => useSelfUpdate(false))
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
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(result.current.phase).toBe('ready')

    act(() => result.current.install())
    await settle()
    expect(installed, 'control: the plugin install itself ran').toBe(true)
    expect(invoked).toContain('restart_app')
    expect(result.current.phase, 'still installing while the restart is in flight').toBe('installing')
  })

  // Windows loses everything a quit would have flushed unless we flush it OURSELVES, first.
  // `tauri-plugin-updater` 2.10.1 ends its Windows arm with `ShellExecuteW(installer)` then
  // `std::process::exit(0)` (updater.rs:865) — the return value is not even read, so once that
  // line is reached the process is gone. `exit` delivers no `RunEvent`, and `quit_cleanup`
  // hangs off `ExitRequested`/`Exit`, so on a Windows self-update the conversation history,
  // the Field Day log, the open propagation episodes, the window geometry and the tail of the
  // diagnostic log all die unwritten. The plugin's own `on_before_exit` hook is not reachable
  // from the JS command path (the plugin `Builder` does not expose it), so the flush has to be
  // asked for from here, BEFORE install() — after it, there is no "after".
  it('install() flushes the journals BEFORE handing off to the installer', async () => {
    nextCheck = () =>
      Promise.resolve({
        available: true,
        version: '9.9.9',
        download: () => Promise.resolve(),
        install: () => {
          invoked.push('plugin:install')
          return Promise.resolve()
        },
      })
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()

    act(() => result.current.install())
    await settle()
    const flush = invoked.indexOf('prepare_update_install')
    const handoff = invoked.indexOf('plugin:install')
    expect(handoff, 'control: the plugin install itself ran').toBeGreaterThanOrEqual(0)
    expect(flush, 'the backend was never asked to flush').toBeGreaterThanOrEqual(0)
    expect(flush, 'the flush must precede the handoff — the process may not survive it').toBeLessThan(handoff)
  })

  it('a failing plugin install surfaces the error and never restarts', async () => {
    nextCheck = () =>
      Promise.resolve({
        available: true,
        version: '9.9.9',
        download: () => Promise.resolve(),
        install: () => Promise.reject(new Error('Read-only file system (os error 30)')),
      })
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()

    act(() => result.current.install())
    await settle()
    expect(result.current.phase).toBe('error')
    expect(result.current.error).toContain('Read-only file system')
    expect(invoked, 'a failed swap must not restart into the old bundle').not.toContain('restart_app')
  })
})

describe('opt-in beta channel', () => {
  it('checks the beta feed (not the stable plugin) and reaches ready', async () => {
    betaInfo = { version: '1.10.3-beta.2', notes: 'notes' }
    // If the stable plugin check were used, this rejection would surface — it must NOT run.
    nextCheck = () => Promise.reject(new Error('stable check must not run in beta mode'))
    const { result } = renderHook(() => useSelfUpdate(true))
    await settle()
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('1.10.3-beta.2')
    expect(invoked, 'the beta check ran').toContain('check_beta_update')
    expect(checkCalls, 'the stable plugin check never ran on the beta channel').toHaveLength(0)
  })

  it('installs through the beta path, flushing before the handoff, then restarts', async () => {
    betaInfo = { version: '1.10.3-beta.2', notes: null }
    nextCheck = () => Promise.reject(new Error('stable check must not run in beta mode'))
    const { result } = renderHook(() => useSelfUpdate(true))
    await settle()

    act(() => result.current.install())
    await settle()
    const flush = invoked.indexOf('prepare_update_install')
    const handoff = invoked.indexOf('install_beta_update')
    expect(handoff, 'the beta installer ran').toBeGreaterThanOrEqual(0)
    expect(flush, 'the backend was asked to flush').toBeGreaterThanOrEqual(0)
    expect(flush, 'flush must precede the beta install — the process may not survive it').toBeLessThan(handoff)
    expect(invoked, 'macOS/Linux restart goes through quit cleanup').toContain('restart_app')
    // The stable plugin install must never be reached on the beta channel.
    expect(invoked).not.toContain('plugin:install')
  })

  it('stays quiet when the beta feed reports nothing newer', async () => {
    betaInfo = null
    nextCheck = () => Promise.reject(new Error('stable check must not run in beta mode'))
    const { result } = renderHook(() => useSelfUpdate(true))
    await settle()
    expect(result.current.phase).toBe('idle')
    expect(result.current.version).toBeNull()
  })
})

// ONE UPDATE PROMPT (operator, 2026-09-29). The self-updater is the prompt wherever it can
// replace this install; the old notice (a toast whose Download button opens the release page)
// speaks only where it cannot. App.updatePrompt.test.tsx holds the launch half in the real App.
describe('where the updater cannot replace this install (the .deb packages)', () => {
  // The plugin is in every desktop build, and the manifest's bare `linux-x86_64` key (the
  // AppImage) matches a .deb on a PC, so the check "succeeded", the AppImage downloaded, and
  // Install failed every time: the plugin installs a .deb only from a .deb.
  it('never checks, downloads or offers, on either channel, at launch or on the hour', async () => {
    selfUpdates = false
    nextCheck = offering('9.9.9')
    betaInfo = { version: '1.10.3-beta.2', notes: null }
    for (const beta of [false, true]) {
      const { result, unmount } = renderHook(() => useSelfUpdate(beta))
      await settle()
      await act(async () => {
        await vi.advanceTimersByTimeAsync(HOUR + 1000)
      })
      expect(invoked, 'control: the route was asked').toContain('update_route')
      expect(checkCalls, 'no stable check, so no AppImage download').toHaveLength(0)
      expect(invoked, 'no beta check').not.toContain('check_beta_update')
      expect(result.current.phase).toBe('idle')
      unmount()
    }
  })
})

describe('the old notice', () => {
  it('gives way when the install prompt arrives later', async () => {
    // Launch: the self-update check fails, so the notice speaks…
    noticeInfo = INFO
    nextCheck = () => Promise.reject(new Error('Could not fetch a valid release JSON from the remote'))
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(notices(), 'premise: the notice is up').toHaveLength(1)
    // …and on the hour the self-updater gets through. Both on one screen is the bug.
    nextCheck = offering('9.9.9')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOUR + 1000)
    })
    expect(result.current.phase).toBe('ready')
    expect(notices(), 'one prompt: the notice gave way').toHaveLength(0)
  })

  it('gives way on the beta channel too', async () => {
    noticeInfo = INFO
    betaInfo = new Error('no pre-release manifest yet')
    const { result } = renderHook(() => useSelfUpdate(true))
    await settle()
    expect(notices(), 'premise: the notice is up').toHaveLength(1)
    betaInfo = { version: '1.10.3-beta.2', notes: null }
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOUR + 1000)
    })
    expect(result.current.phase).toBe('ready')
    expect(notices(), 'one prompt: the notice gave way').toHaveLength(0)
  })
})

describe('Settings ▸ Check for updates', () => {
  /** Mount the hook up to date, so a press has something to find. */
  async function upToDateAtLaunch() {
    nextCheck = () => Promise.resolve(null)
    const hook = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(hook.result.current.phase, 'premise: up to date at launch').toBe('idle')
    invoked.length = 0
    return hook
  }
  const press = async () => {
    await act(async () => {
      await checkForUpdateManual()
    })
    await settle()
  }

  it('finds an update the launch did not, says it is downloading, then offers the install', async () => {
    const { result } = await upToDateAtLaunch()
    noticeInfo = INFO // the notice's feed would say "available": it must not be the one asked
    nextCheck = offering('9.9.9')
    await press()
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('9.9.9')
    expect(messages()).toContain(t('update.downloading', { version: '9.9.9' }))
    expect(notices(), 'no old notice beside it').toHaveLength(0)
    expect(invoked, "the notice's feed was not asked").not.toContain('check_for_update')
    // Nexus never installs on its own schedule: finding an update is not a press of Install.
    for (const cmd of ['prepare_update_install', 'install_beta_update', 'restart_app']) {
      expect(invoked, cmd).not.toContain(cmd)
    }
  })

  it('offers again a version put off with "Not now": the operator asked', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    act(() => result.current.dismiss())
    expect(result.current.phase).toBe('idle')
    await press()
    expect(result.current.phase).toBe('ready')
    expect(result.current.version).toBe('9.9.9')
  })

  it('while an update is ready, starts nothing, adds nothing and installs nothing', async () => {
    nextCheck = offering('9.9.9')
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(result.current.phase).toBe('ready')
    const checks = checkCalls.length
    invoked.length = 0
    await press()
    expect(result.current.phase, 'the banner stays as it is').toBe('ready')
    expect(checkCalls, 'no second check or download').toHaveLength(checks)
    expect(toasts, 'the banner on screen is the answer').toHaveLength(0)
    expect(invoked).not.toContain('restart_app')
  })

  it('says "up to date" with the running version', async () => {
    await upToDateAtLaunch()
    noticeInfo = INFO
    await press()
    expect(messages()).toEqual([t('update.upToDate', { current: '1.15.0' })])
    expect(invoked, "the notice's feed was not asked").not.toContain('check_for_update')
  })

  it("falls back to the notice's own check when the self-update check fails", async () => {
    await upToDateAtLaunch()
    noticeInfo = INFO
    nextCheck = () => Promise.reject(new Error('Could not fetch a valid release JSON from the remote'))
    await press()
    expect(invoked).toContain('check_for_update')
    expect(notices(), 'the notice, with its Download button').toHaveLength(1)
  })

  it("is the notice's check on a .deb, exactly as before", async () => {
    selfUpdates = false
    nextCheck = offering('9.9.9')
    renderHook(() => useSelfUpdate(false))
    await settle()
    noticeInfo = INFO
    await press()
    expect(notices()).toHaveLength(1)
    expect(checkCalls, 'the updater stayed out of it').toHaveLength(0)
  })

  it('while the launch download is still on its way, says so, and a failure then is shown', async () => {
    let fail: (e: Error) => void = () => {}
    nextCheck = offering('9.9.9', () => new Promise<void>((_resolve, reject) => (fail = reject)))
    const { result } = renderHook(() => useSelfUpdate(false))
    await settle()
    expect(result.current.phase, 'premise: downloading').toBe('downloading')
    const checks = checkCalls.length
    await press()
    expect(messages()).toContain(t('update.downloading', { version: '9.9.9' }))
    expect(checkCalls, 'no second check or download').toHaveLength(checks)
    await act(async () => {
      fail(new Error('connection reset'))
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(result.current.phase, 'it became a download the operator asked for').toBe('error')
  })

  it('shows a download that fails after the press, with the way out', async () => {
    const { result } = await upToDateAtLaunch()
    nextCheck = offering('9.9.9', () => Promise.reject(new Error('connection reset')))
    await press()
    expect(result.current.phase, 'the operator asked, so the failure is theirs to see').toBe('error')
    expect(result.current.error).toContain('connection reset')
    expect(notices(), 'the banner carries the download page; no second prompt').toHaveLength(0)
  })
})

describe('the way out of a failed update', () => {
  async function failedInstall() {
    nextCheck = () =>
      Promise.resolve({
        available: true,
        version: '9.9.9',
        download: () => Promise.resolve(),
        install: () => Promise.reject(new Error('Read-only file system (os error 30)')),
      })
    const hook = renderHook(() => useSelfUpdate(false))
    await settle()
    act(() => hook.result.current.install())
    await settle()
    expect(hook.result.current.phase, 'premise: the install failed').toBe('error')
    return hook
  }

  it('opens the download page, and puts this version away for the session', async () => {
    const { result } = await failedInstall()
    act(() => result.current.downloadInstead())
    await settle()
    expect(invoked).toContain('open_download_page')
    expect(result.current.phase).toBe('idle')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOUR + 1000)
    })
    expect(result.current.phase, 'the hourly check does not bring the same version back').toBe('idle')
  })

  it('hands over the address when no browser opens, and keeps the failure on screen', async () => {
    pageOpens = false
    const { result } = await failedInstall()
    act(() => result.current.downloadInstead())
    await settle()
    const handover = toasts.find((x) => x.message === t('update.downloadFailed', { url: PAGE }))
    expect(handover, 'the address, to copy').toBeTruthy()
    expect(handover!.actionLabel).toBe(t('update.copyLink'))
    expect(result.current.phase).toBe('error')
  })
})
