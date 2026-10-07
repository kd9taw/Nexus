// @vitest-environment jsdom
//
// Download everything again (Settings ▸ Logging & Connectors ▸ Confirmations ▸ LoTW), end to end
// through its confirmation. An account whose LoTW sync cursor moved past confirmations it never
// received (a first download that asked for too little, #399) had no way to get them back: every
// download asks only for what LoTW matched after the cursor. The button empties the cursor, so
// the next download is the whole history, once. It asks first, because that download is large;
// it says so after; and it touches nothing else: no other setting, no contact, no confirmation.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { ConfirmHost } from '../confirm'
import { pushToast } from '../toast'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

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
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

const QUESTION = 'Download your whole LoTW history again?'

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

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  vi.mocked(pushToast).mockClear()
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
      lotwLastQsl: '2026-08-04 21:12:44',
    } as never),
  )
})
afterEach(cleanup)

describe('Download everything again asks first, and the answer decides', () => {
  it('asks in a real dialog that says what happens, and what does not', async () => {
    fireEvent.click(await openLotw())
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText(QUESTION)).toBeTruthy()
    expect(dialog.textContent).toContain('one large download')
    expect(dialog.textContent).toContain(
      'Your logged contacts and the confirmations on them are not changed',
    )
    expect(api.get('resetLotwCursor')).not.toHaveBeenCalled()
  })

  it('empties the cursor on a yes, says so, and changes nothing else', async () => {
    fireEvent.click(await openLotw())
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Download everything again' }))
    await waitFor(() => expect(api.get('resetLotwCursor')).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(
        expect.stringContaining('Your next LoTW download brings your whole confirmation history'),
        'success',
      ),
    )
    // Only the cursor: no settings save carrying anything else, and no download of its own.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
    expect(api.get('downloadLotwReport')).not.toHaveBeenCalled()
  })

  it('changes nothing on a no', async () => {
    fireEvent.click(await openLotw())
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByText(QUESTION)).toBeNull())
    expect(api.get('resetLotwCursor')).not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
  })
})
