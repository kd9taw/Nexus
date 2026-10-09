// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING ▸ ENTER SENDS MESSAGE — the switches and the role picker, through the
// panel's own Save.
//
// ESM makes Enter a way to start a transmission, so the switches are OFF until the operator
// turns them on, one per cockpit, and nothing else turns them on. What this file proves is the
// half the Rust tests cannot see: that each switch and each mapping reaches the `set_settings`
// payload by value and comes back when the panel loads again, and that a macro set nobody
// mapped is sent back exactly as it was stored — byte for byte, with no `esmRoles` key added.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here mounts the panel twice; Settings panel files of
// this shape take about 0.3 s a case on one core, and a loaded full suite on this box has run
// cases up to 20 times slower than one core, past vitest's 5 s default. 15 s is the house
// budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock every export of `../api`, derived from the real module — a hand-kept list made the
// panel throw on mount when it fell behind, which reads as a behaviour regression.
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

function renderPanel() {
  return render(
    <SettingsPanel
      fieldDay={null as never}
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

/** An operator's CW profile saved before ESM existed, exactly as the file holds it. */
const OLD_PROFILE = {
  name: 'Old',
  macros: [
    { key: 'F1', label: 'CQ', text: 'CQ TEST {MYCALL}' },
    { key: 'F3', label: 'Exch', text: '! DE {MYCALL} {RST} {EXCH} {EXCH} K' },
  ],
}
/** The active profile, the operator's own, N1MM-style. */
const MINE = {
  name: 'Mine',
  macros: [
    { key: 'F1', label: 'CQ', text: 'CQ TEST {MYCALL}' },
    { key: 'F2', label: 'Exch', text: '5NN {EXCH}' },
    { key: 'F3', label: 'TU', text: 'TU {MYCALL} TEST' },
    { key: 'F4', label: 'Me', text: '{MYCALL}' },
    { key: 'F5', label: 'Him', text: '!' },
  ],
}

/** The settings the panel is told are on disk. */
let stored: Record<string, unknown> = {}

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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  stored = structuredClone({
    ...defaultSettings,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    macros: { ...defaultSettings.macros, cwProfiles: [OLD_PROFILE, MINE], activeCwProfile: 1 },
    voiceMessages: defaultSettings.voiceMessages.map((m) => (m.slot === 5 ? { ...m, file: 'agn.wav' } : m)),
  })
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(stored) as never))
  // A save persists, exactly as the backend does — which is what lets the SECOND mount below be
  // a real round trip rather than a re-read of the same fixture.
  api.get('setSettings').mockImplementation((s: unknown) => {
    stored = structuredClone(s as Record<string, unknown>)
    return Promise.resolve({} as never)
  })
})
afterEach(cleanup)

const openContesting = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))
const save = async () => {
  fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
}
const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown> & { macros: Record<string, unknown> }
}
const SWITCHES = {
  contestEsmCw: 'ESM in the CW cockpit',
  contestEsmRtty: 'ESM in the RTTY cockpit',
  contestEsmPhone: 'ESM in the Phone cockpit',
  contestEsmCallOnce: 'Call once (S&P)',
} as const
const checked = async () =>
  Object.fromEntries(
    await Promise.all(
      Object.entries(SWITCHES).map(async ([key, name]) => [
        key,
        (await screen.findByRole('switch', { name })).getAttribute('aria-checked'),
      ]),
    ),
  )
/** The key select of one step in one cockpit's picker. */
const stepSelect = (cockpit: string, role: string) =>
  document.querySelector(
    `#settings-contest-keys [data-esm-cockpit="${cockpit}"] [data-esm-role="${role}"] select`,
  ) as HTMLSelectElement

describe('Settings ▸ Contesting ▸ Enter Sends Message', () => {
  it('ships every switch OFF: nothing turns ESM on for the operator', async () => {
    renderPanel()
    await openContesting()
    const off = { contestEsmCw: 'false', contestEsmRtty: 'false', contestEsmPhone: 'false', contestEsmCallOnce: 'false' }
    expect(await checked()).toEqual(off)
    // …and settings that carry no switch at all read as off too: an absent switch is never on.
    cleanup()
    for (const key of Object.keys(SWITCHES)) delete stored[key]
    renderPanel()
    await openContesting()
    expect(await checked()).toEqual(off)
  })

  it('saves one switch on, by value, and the panel reads it back when it loads again', async () => {
    renderPanel()
    await openContesting()
    fireEvent.click(await screen.findByRole('switch', { name: SWITCHES.contestEsmRtty }))
    await save()
    expect(
      Object.fromEntries(Object.keys(SWITCHES).map((key) => [key, lastSaved()[key]])),
    ).toEqual({ contestEsmCw: false, contestEsmRtty: true, contestEsmPhone: false, contestEsmCallOnce: false })

    cleanup()
    renderPanel()
    await openContesting()
    expect(await checked()).toEqual({
      contestEsmCw: 'false',
      contestEsmRtty: 'true',
      contestEsmPhone: 'false',
      contestEsmCallOnce: 'false',
    })
  })

  it('maps the active CW profile\'s keys onto it, and leaves an old profile byte for byte as it was', async () => {
    renderPanel()
    await openContesting()
    fireEvent.change(stepSelect('cw', 'tu'), { target: { value: 'F3' } })
    fireEvent.change(stepSelect('cw', 'callExch'), { target: { value: 'F5' } })
    const then = document.querySelectorAll(
      '#settings-contest-keys [data-esm-cockpit="cw"] [data-esm-role="callExch"] select',
    )[1] as HTMLSelectElement
    fireEvent.change(then, { target: { value: 'F2' } })
    await save()
    const profiles = lastSaved().macros.cwProfiles as Record<string, unknown>[]
    expect(profiles[1]).toEqual({ ...MINE, esmRoles: { tu: ['F3'], callExch: ['F5', 'F2'] } })
    expect(JSON.stringify(profiles[0])).toBe(JSON.stringify(OLD_PROFILE))

    // …and comes back mapped: the picker shows what Enter would send at each step.
    cleanup()
    renderPanel()
    await openContesting()
    const sends = (role: string) =>
      document.querySelector(`#settings-contest-keys [data-esm-cockpit="cw"] [data-esm-role="${role}"] .esm-role-sends`)
        ?.textContent
    expect(sends('callExch')).toBe('! 5NN {EXCH}')
    expect(sends('tu')).toBe('TU {MYCALL} TEST')
  })

  it('a mapping cleared again leaves no trace: the profile is saved exactly as it was stored', async () => {
    renderPanel()
    await openContesting()
    fireEvent.change(stepSelect('cw', 'tu'), { target: { value: 'F3' } })
    fireEvent.change(stepSelect('cw', 'tu'), { target: { value: '' } })
    fireEvent.click(await screen.findByRole('switch', { name: SWITCHES.contestEsmCw })) // something to save
    await save()
    expect(JSON.stringify(lastSaved().macros.cwProfiles)).toBe(JSON.stringify([OLD_PROFILE, MINE]))
  })

  it('maps an RTTY set by its id, and the voice keyer once, by value', async () => {
    renderPanel()
    await openContesting()
    fireEvent.change(screen.getByRole('combobox', { name: 'Macro set' }), { target: { value: 'everyday' } })
    fireEvent.change(stepSelect('rtty', 'tu'), { target: { value: 'F4' } })
    fireEvent.change(stepSelect('phone', 'again'), { target: { value: 'F6' } })
    await save()
    expect(lastSaved().macros.rttyEsmRoles).toEqual({ everyday: { tu: ['F4'] } })
    expect(lastSaved().voiceEsmRoles).toEqual({ again: ['F6'] })
  })

  it('saves nothing new for a station that maps nothing: no ESM map is written', async () => {
    renderPanel()
    await openContesting()
    fireEvent.click(await screen.findByRole('switch', { name: SWITCHES.contestEsmCw }))
    await save()
    expect(Object.keys(lastSaved()).filter((k) => /esmroles/i.test(k))).toEqual([])
    expect(Object.keys(lastSaved().macros).filter((k) => /esmroles/i.test(k))).toEqual([])
  })

  it('shows a profile on the built-in sets read-only: no control, and that ESM steps aside there today', async () => {
    stored = { ...stored, macros: { ...(stored.macros as object), cwProfiles: [{ name: 'Default', macros: [] }], activeCwProfile: 0 } }
    renderPanel()
    await openContesting()
    const cw = document.querySelector('#settings-contest-keys .cw-macro-editor') as HTMLElement
    expect(cw.querySelectorAll('select')).toHaveLength(0)
    expect(cw.querySelector('.esm-roles-none')?.textContent).toBe(
      'This set has no step mapped, so ESM steps aside: Enter logs as it does with ESM off.',
    )
  })
})
