// @vitest-environment jsdom
//
// Remove radio, end to end through the confirmation — the path that reported this bug.
//
// `window.confirm` is INERT in the Tauri webview (wry implements no `runJavaScriptConfirmPanel`,
// so WKWebView shows nothing and returns false), which made `if (!window.confirm(…)) return`
// cancel every destructive action silently. Remove radio was the operator's report: the button
// did nothing, with no dialog and no error.
//
// `confirm.test.tsx` pins the dialog primitive. This pins the WIRING — that the panel's Remove
// button reaches a real dialog, and that the answer decides whether the radio is actually
// removed. Neither half was covered before: no test touched any of the twelve converted paths.
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

async function clickRemove() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  // Every roster card renders a Remove button — the ACTIVE card's is disabled with a teaching
  // title — so scope to the enabled (removable) one rather than assuming a single match.
  const remove = (await screen.findAllByRole('button', { name: 'Remove' })).find(
    (b) => !(b as HTMLButtonElement).disabled && /roster/i.test((b as HTMLButtonElement).title),
  )!
  expect(remove).toBeTruthy()
  fireEvent.click(remove)
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

describe('Remove radio asks first, and the answer decides', () => {
  it('shows a real dialog naming the radio — not window.confirm, which shows nothing', async () => {
    await clickRemove()
    await waitFor(() => expect(screen.getByText('Remove IC-9700?')).toBeTruthy())
    // The operator is told what goes and what does not, before answering.
    expect(document.body.textContent).toContain('Your contact log is not affected')
    // And nothing has happened yet.
    expect(api.get('removeRadio')).not.toHaveBeenCalled()
  })

  it('removes the radio when the operator confirms', async () => {
    await clickRemove()
    await waitFor(() => expect(screen.getByText('Remove IC-9700?')).toBeTruthy())
    screen.getByRole('button', { name: 'Remove radio' }).click()

    // The whole point of the fix: the click reaches the backend, for the RIGHT radio.
    await waitFor(() => expect(api.get('removeRadio')).toHaveBeenCalledWith(1))
  })

  it('keeps the radio when the operator dismisses', async () => {
    await clickRemove()
    await waitFor(() => expect(screen.getByText('Remove IC-9700?')).toBeTruthy())
    screen.getByRole('button', { name: 'Cancel' }).click()

    await waitFor(() => expect(screen.queryByText('Remove IC-9700?')).toBeNull())
    expect(api.get('removeRadio')).not.toHaveBeenCalled()
  })
})
