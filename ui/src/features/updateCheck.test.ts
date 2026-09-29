// R3 — the update toast's Download button did nothing on Linux, and silenced itself forever.
//
// TWO DEFECTS ON ONE CLICK, both shipped:
//   (a) Toasts.tsx ran `toast.action?.()` and then `dismissToast()` unconditionally and
//       synchronously, so the toast vanished whether or not the action worked.
//   (b) this module recorded `nexus.update.dismissedVersion` on that unverified "success", and
//       `maybeCheckForUpdate` then suppressed the prompt for that version on EVERY future
//       launch. One click on a button that did nothing permanently retired update notices.
//
// The root cause was one layer down (the Linux opener returned Ok as soon as `xdg-open` was
// SPAWNED, never reading its exit status). That half is fixed in Rust; this file pins OUR half:
// nothing is recorded and nothing is retired on a result we cannot trust, and a failure hands
// the operator the URL instead of a dead end.
//
// There was no test for this flow at all — `features/` had ten test files and none for the
// updater.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { UpdateInfo } from '../types'

// Node test env: in-memory localStorage (the connectConfig.test.ts shim).
class MemoryStorage {
  private m = new Map<string, string>()
  get length() { return this.m.size }
  clear() { this.m.clear() }
  getItem(k: string) { return this.m.has(k) ? (this.m.get(k) as string) : null }
  key(i: number) { return [...this.m.keys()][i] ?? null }
  removeItem(k: string) { this.m.delete(k) }
  setItem(k: string, v: string) { this.m.set(k, String(v)) }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore
vi.stubGlobal('window', { localStorage: memStore, setTimeout } as unknown as Window & typeof globalThis)

vi.mock('../toast', () => ({ pushToast: vi.fn(), dismissToast: vi.fn() }))
vi.mock('../api', () => ({ checkForUpdate: vi.fn(), openDownloadPage: vi.fn(), appVersion: vi.fn() }))

import { dismissToast, pushToast } from '../toast'
import { appVersion, checkForUpdate, openDownloadPage } from '../api'
import { t } from '../i18n'
import {
  checkForUpdateManual,
  installSelfUpdate,
  maybeCheckForUpdate,
  retireDownloadPrompt,
  type SelfUpdateAnswer,
} from './updateCheck'

const toasts = vi.mocked(pushToast)
const dismiss = vi.mocked(dismissToast)
const check = vi.mocked(checkForUpdate)
const open = vi.mocked(openDownloadPage)
const version = vi.mocked(appVersion)

const LS_DISMISSED = 'nexus.update.dismissedVersion'
const URL = 'https://github.com/kd9taw/Nexus/releases'

const INFO: UpdateInfo = {
  current: '1.6.1',
  latest: '1.7.0',
  updateAvailable: true,
  downloadUrl: URL,
}

/** The Download button the operator actually clicks, as Toasts.tsx would invoke it. */
function downloadAction(): unknown {
  const call = toasts.mock.calls.find((c) => (c[3] as { action?: unknown } | undefined)?.action)
  expect(call, 'no toast with an action was pushed').toBeTruthy()
  return (call![3] as { action: () => unknown }).action()
}

beforeEach(() => {
  memStore.clear()
  toasts.mockClear()
  dismiss.mockClear()
  check.mockReset().mockResolvedValue(INFO)
  open.mockReset().mockResolvedValue(undefined)
  version.mockReset().mockResolvedValue('1.6.1')
})

describe('the update prompt', () => {
  it('offers Download when a newer build exists', async () => {
    await maybeCheckForUpdate()
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('1.7.0')
    expect(toasts.mock.calls[0][2]).toBe(0) // non-expiring — it must not time out unseen
  })
})

describe('Download succeeded — the browser really opened', () => {
  it('records the dismissal, so the prompt does not nag again for that version', async () => {
    await maybeCheckForUpdate()
    await downloadAction()
    expect(localStorage.getItem(LS_DISMISSED)).toBe('1.7.0')
  })

  it('resolves, which is what tells the toast it may close', async () => {
    await maybeCheckForUpdate()
    await expect(Promise.resolve(downloadAction())).resolves.toBeUndefined()
  })
})

describe('Download FAILED — the opener could not reach a browser', () => {
  // ⚠️ BLOCK BODY, DELIBERATELY. `open.mockRejectedValue(...)` RETURNS the mock, and vitest
  // treats a function returned from `beforeEach` as a teardown callback — it then calls the
  // mock after every test, producing an unhandled rejection that fails all four of these.
  beforeEach(() => {
    open.mockRejectedValue('no xdg-open on PATH')
  })

  it('records NOTHING — a failed open must never retire the notice', async () => {
    await maybeCheckForUpdate()
    await expect(Promise.resolve(downloadAction())).rejects.toBeTruthy()
    expect(localStorage.getItem(LS_DISMISSED)).toBeNull()
  })

  it('REJECTS, which is what keeps the update toast on screen', async () => {
    await maybeCheckForUpdate()
    await expect(Promise.resolve(downloadAction())).rejects.toBeTruthy()
  })

  it('hands the operator the download URL instead of a dead end', async () => {
    await maybeCheckForUpdate()
    await Promise.resolve(downloadAction()).catch(() => {})
    const err = toasts.mock.calls.find((c) => c[1] === 'error')
    expect(err, 'no error toast was raised').toBeTruthy()
    expect(err![0]).toContain(URL)
    expect(err![2]).toBe(0) // non-expiring: a URL you have to read must not vanish
    // …and in a form they can take with them.
    expect((err![3] as { action?: unknown } | undefined)?.action).toBeTypeOf('function')
  })

  it('still prompts on the NEXT launch — the self-silencing bug, end to end', async () => {
    await maybeCheckForUpdate()
    await Promise.resolve(downloadAction()).catch(() => {})
    toasts.mockClear()
    // Relaunch.
    await maybeCheckForUpdate()
    expect(toasts.mock.calls.some((c) => String(c[0]).includes('1.7.0'))).toBe(true)
  })
})

// ONE UPDATE PROMPT (operator, 2026-09-29): where the signed self-updater can replace this
// install it is the prompt, and this notice speaks only when it could not answer.
describe('the notice, beside the self-updater', () => {
  it("stays silent when this launch's self-update check answered", async () => {
    await maybeCheckForUpdate(Promise.resolve(true))
    expect(check, 'its feed is still read: that is the launch line in the diagnostic log').toHaveBeenCalled()
    expect(toasts).not.toHaveBeenCalled()
  })

  it('speaks, exactly as before, when it could not', async () => {
    await maybeCheckForUpdate(Promise.resolve(false))
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('1.7.0')
    expect(toasts.mock.calls[0][2]).toBe(0)
  })

  it('gives way to the install prompt when that arrives later', async () => {
    toasts.mockReturnValueOnce(41)
    await maybeCheckForUpdate(Promise.resolve(false))
    retireDownloadPrompt()
    expect(dismiss).toHaveBeenCalledWith(41)
  })
})

describe('Settings ▸ Check for updates', () => {
  let dispose: () => void = () => {}
  const answering = (answer: SelfUpdateAnswer) => {
    dispose = installSelfUpdate(() => Promise.resolve(answer))
  }
  afterEach(() => dispose())

  it('says an update is downloading, and does not ask the notice', async () => {
    answering({ kind: 'downloading', version: '1.7.0' })
    await checkForUpdateManual()
    expect(toasts.mock.calls.map((c) => c[0])).toEqual([t('update.downloading', { version: '1.7.0' })])
    expect(check).not.toHaveBeenCalled()
  })

  it('adds nothing while the install prompt is already up', async () => {
    answering({ kind: 'ready', version: '1.7.0' })
    await checkForUpdateManual()
    expect(toasts).not.toHaveBeenCalled()
    expect(check).not.toHaveBeenCalled()
  })

  it('says "up to date" with the running version', async () => {
    answering({ kind: 'upToDate' })
    await checkForUpdateManual()
    expect(toasts.mock.calls.map((c) => c[0])).toEqual([t('update.upToDate', { current: '1.6.1' })])
    expect(check).not.toHaveBeenCalled()
  })

  it.each(['failed', 'unsupported'] as const)(
    'hands a %s self-update check to the notice, which answers exactly as before',
    async (kind) => {
      answering({ kind })
      await checkForUpdateManual()
      expect(check).toHaveBeenCalledTimes(1)
      expect(toasts.mock.calls[0][0]).toBe(t('update.available', { latest: '1.7.0', current: '1.6.1' }))
    },
  )

  it('is the notice alone with no self-updater installed, and after it goes', async () => {
    answering({ kind: 'upToDate' })
    dispose()
    await checkForUpdateManual()
    expect(check).toHaveBeenCalledTimes(1)
  })
})
