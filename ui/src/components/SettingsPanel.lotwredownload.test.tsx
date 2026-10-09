// @vitest-environment jsdom
//
// Download everything again (Settings ▸ Logging & Connectors ▸ Confirmations ▸ LoTW), end to end
// through its confirmation. An account whose LoTW sync cursor moved past confirmations it never
// received (a first download that asked for too little, #399) had no way to get them back: every
// download asks only for what LoTW matched after the cursor. The button empties the cursor and
// starts that download at once, the way Download confirmations does, so the operator watches it
// run. It asks first, because that download is large; it touches nothing else: no other setting,
// no contact, no confirmation. A download that cannot start, or fails, leaves the cursor empty,
// and the panel says the next one still asks for everything.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { ConfirmHost } from '../confirm'
import { pushToast } from '../toast'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "asks in a real dialog that says what happens, and what…", takes
// 0.43 s and 0.39 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock every export of `../api`, derived from the real module — a hand-kept list goes stale and
// makes the panel throw on mount, which reads as a regression rather than the mock it is.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => {
  const pushToast = vi.fn()
  return {
    pushToast,
    // The real one's contract: a failure becomes an error toast and a null, never a throw.
    withErrorToast: vi.fn(async (fn: () => Promise<unknown>, fallback: string) => {
      try {
        return await fn()
      } catch (err) {
        pushToast(`${fallback}: ${err instanceof Error ? err.message : String(err)}`, 'error')
        return null
      }
    }),
  }
})

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

const QUESTION = 'Download your whole LoTW history again?'
const OLD_CURSOR = '2026-08-04 21:12:44'
const STILL_ALL = 'Your next Download confirmations still asks for all of it'
const RESULT = {
  matched: 40,
  newlyConfirmed: 3,
  newlyConfirmedAny: 3,
  newlyCredited: 1,
  newlySubmitted: 0,
  promoted: 0,
  orphans: [],
}

/** The backend as far as the cursor goes: the reset empties it, a save writes what it carries,
 *  and a download starts from it. An empty cursor is a download of the whole history. */
const backend = { cursor: OLD_CURSOR, activeRadio: 0, downloadedFrom: [] as string[] }

/** The panel WITH a confirm host, which is how App renders it. Without a host `confirmDialog`
 *  fails closed, so a hostless render would "pass" a dismissal test for the wrong reason. */
function renderPanel() {
  return render(
    <>
      <SettingsPanel
        activeRadioId={0}
        scale={1 as never}
        scaleMode={'auto' as never}
        scaleCap={1 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density={'comfortable' as never}
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={features}
      />
      <ConfirmHost />
    </>,
  )
}

async function openLotw() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
  return screen.findByRole('button', { name: 'Download everything again' })
}

/** Click Download everything again and answer yes. */
async function redownload() {
  fireEvent.click(await openLotw())
  const dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Download everything again' }))
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  vi.mocked(pushToast).mockClear()
  backend.cursor = OLD_CURSOR
  backend.activeRadio = 0
  backend.downloadedFrom = []
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.17.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings,
      mycall: 'KD9TAW',
      lotwUsername: 'KD9TAW',
      lotwLastQsl: backend.cursor,
      activeRadio: backend.activeRadio,
    } as never),
  )
  api.get('setSettings').mockImplementation((s: { lotwLastQsl: string }) => {
    backend.cursor = s.lotwLastQsl
    return Promise.resolve(null)
  })
  api.get('resetLotwCursor').mockImplementation(() => {
    backend.cursor = ''
    return Promise.resolve()
  })
  api.get('downloadLotwReport').mockImplementation(() => {
    backend.downloadedFrom.push(backend.cursor)
    return Promise.resolve(RESULT)
  })
})
afterEach(cleanup)

describe('Download everything again asks first, and the answer decides', () => {
  it('asks in a real dialog that says what happens, and what does not', async () => {
    fireEvent.click(await openLotw())
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText(QUESTION)).toBeTruthy()
    expect(dialog.textContent).toContain('starts right away')
    expect(dialog.textContent).toContain('one large download')
    expect(dialog.textContent).toContain(
      'Your logged contacts and the confirmations on them are not changed',
    )
    expect(api.get('resetLotwCursor')).not.toHaveBeenCalled()
    expect(api.get('downloadLotwReport')).not.toHaveBeenCalled()
  })

  it('changes nothing on a no', async () => {
    fireEvent.click(await openLotw())
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByText(QUESTION)).toBeNull())
    expect(api.get('resetLotwCursor')).not.toHaveBeenCalled()
    expect(api.get('downloadLotwReport')).not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
  })
})

describe('a yes starts the whole-history download at once', () => {
  it('empties the cursor, then runs the download where the operator can see it', async () => {
    let finish!: (r: typeof RESULT) => void
    api.get('downloadLotwReport').mockImplementation(() => {
      backend.downloadedFrom.push(backend.cursor)
      return new Promise((resolve) => {
        finish = resolve
      })
    })
    await redownload()
    await waitFor(() => expect(api.get('downloadLotwReport')).toHaveBeenCalledTimes(1))
    expect(api.get('resetLotwCursor')).toHaveBeenCalledTimes(1)
    const [reset] = api.get('resetLotwCursor').mock.invocationCallOrder
    const [download] = api.get('downloadLotwReport').mock.invocationCallOrder
    expect(reset).toBeLessThan(download)
    // The download Download confirmations runs, from an empty cursor: the whole history.
    expect(backend.downloadedFrom).toEqual([''])
    // While it runs, the panel shows it running, and neither button can start a second one.
    const busy = await screen.findByRole('button', { name: 'Downloading…' })
    expect((busy as HTMLButtonElement).disabled).toBe(true)
    const again = screen.getByRole('button', { name: 'Download everything again' })
    expect((again as HTMLButtonElement).disabled).toBe(true)
    finish(RESULT)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith('LoTW: 3 newly confirmed, 1 credited', 'success'),
    )
    expect(pushToast).not.toHaveBeenCalledWith(expect.stringContaining(STILL_ALL), 'info')
  })

  // The save the download makes first sends the whole form when the active radio changed
  // elsewhere since the form loaded, and the form still holds the cursor it loaded. That save
  // must not put the old cursor back between the reset and the download.
  it('the save before the download cannot put the old cursor back', async () => {
    const button = await openLotw()
    backend.activeRadio = 1
    fireEvent.click(button)
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Download everything again' }))
    await waitFor(() => expect(api.get('downloadLotwReport')).toHaveBeenCalledTimes(1))
    expect(api.get('setSettings')).toHaveBeenCalledTimes(1)
    expect(backend.downloadedFrom).toEqual([''])
  })

  it.each([
    [
      'the save before it is refused',
      () =>
        api.get('setSettings').mockImplementation(() =>
          Promise.reject(new Error('The logbook is busy. Try again in a moment.')),
        ),
    ],
    [
      'LoTW refuses it',
      () =>
        api.get('downloadLotwReport').mockImplementation(() =>
          Promise.reject(new Error('No LoTW password stored — set it in Settings.')),
        ),
    ],
    [
      'it fails part way',
      () =>
        api.get('downloadLotwReport').mockImplementation(() =>
          Promise.reject(new Error('LoTW did not answer in time')),
        ),
    ],
  ])('when %s, the cursor stays empty and the panel says so', async (_how, arrange) => {
    arrange()
    await redownload()
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(expect.stringContaining(STILL_ALL), 'info'),
    )
    expect(api.get('resetLotwCursor')).toHaveBeenCalledTimes(1)
    expect(backend.cursor).toBe('')
    expect(pushToast).toHaveBeenCalledWith(expect.stringContaining('LoTW sync failed: '), 'error')
    expect(pushToast).not.toHaveBeenCalledWith(expect.any(String), 'success')
  })

  it('a reset that fails starts no download, which would only fetch what is new', async () => {
    api.get('resetLotwCursor').mockImplementation(() => Promise.reject(new Error('disk full')))
    await redownload()
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(
        'Could not reset the LoTW download: disk full',
        'error',
      ),
    )
    expect(api.get('downloadLotwReport')).not.toHaveBeenCalled()
    expect(backend.cursor).toBe(OLD_CURSOR)
  })

  it('control: Download confirmations alone still carries on from the cursor', async () => {
    await openLotw()
    fireEvent.click(screen.getByRole('button', { name: 'Download confirmations' }))
    await waitFor(() => expect(api.get('downloadLotwReport')).toHaveBeenCalledTimes(1))
    expect(api.get('resetLotwCursor')).not.toHaveBeenCalled()
    expect(backend.downloadedFrom).toEqual([OLD_CURSOR])
  })
})
