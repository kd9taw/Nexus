// @vitest-environment jsdom
//
// THE PICKER IS ON SCREEN. 1.7.1-test4 shipped a finished German translation that could not be
// selected: the language choices were captured in a module-level constant, which evaluates when
// the module is IMPORTED — through App → SettingsPanel, inside `import App from './App'` — and
// therefore long before startup installs the catalog. The array froze at ['en'], the picker's
// `length > 1` was never true, and the operator's report was "there is no language setting
// visible in appearances".
//
// Every existing test passed. They install a catalog and then read, which is the one order the
// real application never uses; and none of them RENDERED the control. So this one renders the
// real panel and looks for the real control, which is the only check that could have caught it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { installCatalog, setLocale } from '../i18n'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})
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

const RADIO = {
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
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
    />,
  )
}

afterEach(() => {
  setLocale('en')
  cleanup()
})
beforeEach(() => {
  localStorage.clear()
  // Reset IN PLACE — the mocked module captured these spy objects at import time.
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  // The panel renders NOTHING until its settings load resolves, and that chain calls a dozen
  // verbs whose results it walks. A verb left resolving `null` throws inside the chain, `form`
  // stays null, and the panel renders an empty div — which reads as "the control is missing"
  // when the truth is "the panel never mounted". This is the shipped tests' setup, not a
  // hand-guessed subset, for exactly that reason.
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.7.1'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings,
      ...RADIO,
      mycall: 'KD9TAW',
      mygrid: 'EN52',
      activeRadio: 0,
      radios: [RADIO],
    }),
  )
})

describe('the language picker', () => {
  it('appears once a second catalog is installed AFTER this module loaded', async () => {
    // The real startup order: the module graph is already imported by the time a catalog
    // arrives. This is the exact sequence that was broken.
    installCatalog('zz', { 'reveal.notNow': 'zz' })
    renderPanel()
    await screen.findByRole('tab', { name: /appearance/i })
    // The picker lives on the Appearance tab, and only the ACTIVE tab is in the DOM — so the
    // test has to walk there exactly as an operator does. (Asserting on a tab that never
    // rendered is how a control can be "found" while being unreachable.)
    fireEvent.click(screen.getByRole('tab', { name: /appearance/i }))
    const picker = screen.getByLabelText('Language') as HTMLSelectElement
    expect(picker).toBeTruthy()
    const values = Array.from(picker.options).map((o) => o.value)
    expect(values, 'English plus the newly installed language').toContain('zz')
    expect(values[0]).toBe('en')
  })
})
