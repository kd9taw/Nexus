// @vitest-environment jsdom
//
// Is the FT-710 scope opt-in REACHABLE?
//
// The backend gates the whole feature on `RadioProfile::yaesu_rf_scope`, so if this toggle does
// not render there is no way to turn the scope on at all — the feature would ship dead with every
// unit test still green, because every one of them exercises the layer below this.
//
// It lives inside Rig & CAT's ADVANCED group, which is `defaultOpen={false}`, beside the Icom
// native-CI-V and Flex panadapter toggles that are gated the same way. That is deliberate (it
// needs a manual library install and an EX-menu change), but it means the control is invisible
// until two disclosures are opened — so the test opens them, and a change that buries it deeper
// fails here.
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
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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
  rigModel: 1049,
  rigModelName: 'Yaesu FT-710',
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


describe('the FT-710 scope opt-in is reachable', () => {
  /** Open Radio ▸ Rig & CAT ▸ Advanced and hand back every field label now on screen. */
  function labelsWithAdvancedOpen() {
    const grp = document.querySelector(
      '#settings-rig-advanced .settings-group-toggle',
    ) as HTMLButtonElement | null
    expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
    // Collapsed by default — the labels must be re-read AFTER this click, not before.
    expect(grp!.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(grp!)
    return [...document.querySelectorAll('.settings-label')].map((e) => e.textContent?.trim() ?? '')
  }

  it('offers the toggle when the edited radio is an FT-710', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const labels = labelsWithAdvancedOpen()
    expect(labels.some((t) => /FT-710 RF scope/i.test(t))).toBe(true)
  })

  it('does NOT offer it for a radio whose bridge this cannot speak', async () => {
    // Model-gated on 1049. Offering it elsewhere would be an invitation to a setting that cannot
    // work — and this half is what stops the gate being widened to "any Yaesu" by accident.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...(twoRadioSettings() as unknown as Record<string, unknown>),
        rigModel: 1042,
        rigModelName: 'Yaesu FTDX10',
      } as never),
    )
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const labels = labelsWithAdvancedOpen()
    expect(labels.some((t) => /FT-710 RF scope/i.test(t))).toBe(false)
  })
})
