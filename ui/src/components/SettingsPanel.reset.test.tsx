// @vitest-environment jsdom
//
// Reset to factory defaults, end to end through the confirmation.
//
// WHY THIS CONFIRMS THROUGH `confirmDialog` AND NOT `window.confirm`. The latter is INERT in
// the Tauri webview — wry implements no `runJavaScriptConfirmPanel`, so WKWebView shows nothing
// and returns false — which makes `if (!window.confirm(…)) return` cancel the action silently.
// A Reset written that way is a button that does nothing, with no dialog and no error, and it
// LOOKS correct in a jsdom test because jsdom's `confirm` can be spied into returning true.
// That is the trap this file is shaped to avoid: it drives the real dialog, so a regression to
// `window.confirm` fails here rather than shipping.
//
// `confirm.test.tsx` pins the dialog primitive. This pins the WIRING — that Reset reaches a
// real dialog, that the dialog states what SURVIVES, and that only an explicit yes reaches the
// backend verb.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { ConfirmHost } from '../confirm'
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

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}
// The radio the operator removes. It must NOT be the active one: the active card's Remove
// button renders disabled (with a teaching title), so the removable radio in a multi-radio
// roster is exactly the case that was reported broken.
const IC9700 = {
  ...FTDX10,
  id: 1,
  name: 'IC-9700',
  serialPort: 'COM7',
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
}

function twoRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700],
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

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


beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.17.12'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(twoRadioSettings()))
})
afterEach(cleanup)


/** Open the Radio tab and hand back the Reset field. Reset lives beside Backup, which is the
 *  point: the confirm tells the operator to back up first, and the means to do it is right
 *  there rather than somewhere they have to go and find. */
async function resetButton() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  return await screen.findByRole('button', { name: 'Reset all settings…' })
}

describe('Reset to factory defaults asks first, and the answer decides', () => {
  it('shows a real dialog — not window.confirm, which shows nothing in this webview', async () => {
    // Spied so the test can PROVE the inert primitive is not the one being used. Returning true
    // means a `window.confirm`-based Reset would sail straight through to the backend, so the
    // final assertion here is what distinguishes the two implementations.
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true)
    fireEvent.click(await resetButton())
    await waitFor(() =>
      expect(screen.getByText('Reset all settings to factory defaults?')).toBeTruthy(),
    )
    expect(confirmSpy).not.toHaveBeenCalled()
    // Nothing has happened yet: the dialog is a question, not a countdown.
    expect(api.get('resetSettings')).not.toHaveBeenCalled()
    confirmSpy.mockRestore()
  })

  it('renders a real ampersand in the hint, not the literal characters &amp;', async () => {
    // React escapes text children itself, so an HTML entity stored in the CATALOG comes out as
    // the five literal characters. `en.ts` documents this at the onboarding nudge; the hint here
    // names "Logging & Connectors" and would have shipped it as "Logging &amp; Connectors".
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const btn = await screen.findByRole('button', { name: 'Reset all settings…' })
    const hint = btn.closest('.settings-field')!.querySelector('.settings-hint')!.textContent ?? ''
    expect(hint).toContain('Logging & Connectors')
    expect(hint).not.toContain('&amp;')
  })

  it('states what SURVIVES, because "reset" is the word operators fear for their QSOs', async () => {
    fireEvent.click(await resetButton())
    await waitFor(() =>
      expect(screen.getByText('Reset all settings to factory defaults?')).toBeTruthy(),
    )
    const shown = document.body.textContent ?? ''
    expect(shown).toMatch(/contact log is not affected/i)
    expect(shown).toMatch(/keychain/i)
    expect(shown).toMatch(/cannot be undone/i)
  })

  it('resets through the backend verb when the operator confirms', async () => {
    fireEvent.click(await resetButton())
    await waitFor(() =>
      expect(screen.getByText('Reset all settings to factory defaults?')).toBeTruthy(),
    )
    screen.getByRole('button', { name: 'Reset' }).click()
    // Through the VERB, never by deleting settings.json: a running app holds the old
    // configuration in memory and writes it straight back, so a file-delete "reset" un-resets
    // itself on the next save.
    await waitFor(() => expect(api.get('resetSettings')).toHaveBeenCalled())
  })

  it('keeps everything when the operator dismisses', async () => {
    fireEvent.click(await resetButton())
    await waitFor(() =>
      expect(screen.getByText('Reset all settings to factory defaults?')).toBeTruthy(),
    )
    screen.getByRole('button', { name: 'Cancel' }).click()
    await waitFor(() =>
      expect(screen.queryByText('Reset all settings to factory defaults?')).toBeNull(),
    )
    expect(api.get('resetSettings')).not.toHaveBeenCalled()
  })

  it('reads the form back from the backend, not from the snapshot it asked for', async () => {
    // `reset_settings` runs ensure_radio_profiles / ensure_distinct_radio_ports /
    // ensure_routing_targets on top of Settings::default(), so what LANDED is not the bare
    // default. A form seeded from anything but a fresh read shows a roster the engine is not
    // driving — the same class of bug as a stale flat form overwriting a profile.
    // The verb resolves a SNAPSHOT on success. The default mock resolves null, which the panel
    // reads as "the reset did not happen" and correctly declines to reseed the form from — so
    // this path only exists to be tested once the call actually succeeds.
    api.get('resetSettings').mockImplementation(() => Promise.resolve({ settings: {} }))
    fireEvent.click(await resetButton())
    await waitFor(() =>
      expect(screen.getByText('Reset all settings to factory defaults?')).toBeTruthy(),
    )
    const before = api.get('getSettings').mock.calls.length
    screen.getByRole('button', { name: 'Reset' }).click()
    await waitFor(() => expect(api.get('resetSettings')).toHaveBeenCalled())
    await waitFor(() =>
      expect(api.get('getSettings').mock.calls.length).toBeGreaterThan(before),
    )
  })
})
