// @vitest-environment jsdom
//
// Settings ▸ Logbook (operator, 2026-10-25). The logbook's own settings moved onto one tab from
// Logging & Connectors, Appearance and Config. A move is presentational only: every control must
// still show the value the station holds and write the SAME setting it wrote before, so nobody's
// stored choice changes by being moved. Renders the real panel and walks to the tab the way an
// operator does — a control on a tab that is not rendered must not count as found.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { CALL_CARD_HOVER_KEY } from '../features/callCardHover'

// The house budget: a loaded full suite has run cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

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
  id: 0, name: 'FTDX10', enabled: true, serialPort: 'COM3', baud: 38400, rigModel: 1042,
  rigModelName: 'Yaesu FTDX10', rigConn: 'serial', rigAddr: '', rigctldPort: 4532,
  rotctldPort: 4533, icomNativeCat: false, audioIn: 'in-0', audioOut: 'out-0', txLevel: 1,
  rxGain: 1, pttMethod: 'cat', rotatorModel: 0, rotatorPort: '', rotatorBaud: 9600,
  rotatorHost: '', nativeScope: 'auto', bands: [],
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** Values that differ from every default, so a control that reads the wrong key shows the wrong one. */
const STORED = {
  qrzUsername: 'KD9TAWQ',
  hamqthUsername: 'KD9TAWH',
  callookLookup: false,
  callbookCacheDays: 45,
  historySnapshotDays: 21,
  b4MatchMode: true,
  lotwMaxAgeDays: 120,
  logbookColumns: { shown: ['iota'], hidden: [], widths: { call: 90 } },
}

beforeEach(() => {
  localStorage.clear()
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.13.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings, ...RADIO, ...STORED,
      mycall: 'KD9TAW', mygrid: 'EN52', activeRadio: 0, radios: [RADIO],
    }),
  )
})
afterEach(cleanup)

async function openTab(name: string) {
  render(
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
  await screen.findAllByDisplayValue('KD9TAW')
  fireEvent.click(screen.getByRole('tab', { name }))
}

const section = (id: string) => {
  const el = document.getElementById(`settings-${id}`)
  if (!el) throw new Error(`section ${id} is not on screen`)
  return within(el)
}

/** The footer Save, and what it sent. */
async function saved(): Promise<Record<string, unknown>> {
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

const MOVED = ['logbook-display', 'logbook-history', 'callbook', 'connections-b4', 'lotw-users', 'callsign-state', 'country-file']

describe('Settings ▸ Logbook', () => {
  it('is a tab of its own and holds every moved section', async () => {
    await openTab('Logbook')
    for (const id of MOVED) expect(document.getElementById(`settings-${id}`), id).not.toBeNull()
  })

  it('Logging & Connectors keeps the sign-ins and uploads, and none of the moved sections', async () => {
    await openTab('Logging & Connectors')
    for (const id of MOVED) expect(document.getElementById(`settings-${id}`), id).toBeNull()
    const conf = section('confirmations')
    expect(conf.getByRole('textbox', { name: /^LoTW username/ })).toBeTruthy()
    expect(conf.getByRole('switch', { name: /Auto-upload QSOs to QRZ/ })).toBeTruthy()
    expect(conf.getByPlaceholderText('from your QRZ logbook settings page')).toBeTruthy()
    // The callbook lookup sign-ins and the cache left with the lookups.
    expect(conf.queryByRole('textbox', { name: /^QRZ username/ })).toBeNull()
    expect(conf.queryByRole('textbox', { name: /^HamQTH username/ })).toBeNull()
    expect(conf.queryByRole('spinbutton', { name: /^Keep answers for/ })).toBeNull()
    expect(document.getElementById('settings-integrations-feeds')).not.toBeNull()
  })

  it('the controls it left behind are gone from Appearance and Config', async () => {
    await openTab('Appearance')
    expect(screen.queryByRole('checkbox', { name: 'Open the station card when the pointer rests on a call' })).toBeNull()
    expect(section('map-globe').queryByRole('button', { name: 'Reset' })).toBeNull()
    // Positive control: the globe switch stayed with the map.
    expect(section('map-globe').getByRole('checkbox', { name: 'Show the 3-D globe above the Logbook' })).toBeTruthy()
    fireEvent.click(screen.getByRole('tab', { name: 'Config' }))
    expect(section('data-folder').queryByRole('spinbutton', { name: /^Keep deleted contacts/ })).toBeNull()
  })
})

describe('each moved control reads and writes the same setting', () => {
  it('callbook lookups: QRZ and HamQTH usernames, callook.info and the cache days', async () => {
    await openTab('Logbook')
    const cb = section('callbook')
    const qrz = cb.getByRole('textbox', { name: /^QRZ username/ }) as HTMLInputElement
    const hamqth = cb.getByRole('textbox', { name: /^HamQTH username/ }) as HTMLInputElement
    const callook = cb.getByRole('checkbox', { name: /^Also ask callook\.info/ }) as HTMLInputElement
    const days = cb.getByRole('spinbutton', { name: /^Keep answers for/ }) as HTMLInputElement
    expect([qrz.value, hamqth.value, callook.checked, days.value]).toEqual(['KD9TAWQ', 'KD9TAWH', false, '45'])
    fireEvent.change(qrz, { target: { value: 'N0QRZ' } })
    fireEvent.change(hamqth, { target: { value: 'N0HQ' } })
    fireEvent.click(callook)
    fireEvent.change(days, { target: { value: '12' } })
    const s = await saved()
    expect([s.qrzUsername, s.hamqthUsername, s.callookLookup, s.callbookCacheDays]).toEqual(['N0QRZ', 'N0HQ', true, 12])
  })

  it('callbook lookups credit HamQTH where the operator sees it', async () => {
    await openTab('Logbook')
    section('callbook').getByText('Data provided by HamQTH.com.')
  })

  it('callbook cache: Refresh and Clear call the same verbs', async () => {
    await openTab('Logbook')
    const cb = section('callbook')
    fireEvent.change(cb.getByRole('textbox', { name: 'Refresh this call' }), { target: { value: 'W1AW' } })
    fireEvent.click(cb.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(api.get('callbookRefresh')).toHaveBeenCalledWith('W1AW'))
    fireEvent.click(cb.getByRole('button', { name: 'Clear' }))
    await waitFor(() => expect(api.get('callbookCacheClear')).toHaveBeenCalled())
  })

  it('history: keep deleted contacts for (days)', async () => {
    await openTab('Logbook')
    const days = section('logbook-history').getByRole('spinbutton', { name: /^Keep deleted contacts/ }) as HTMLInputElement
    expect(days.value).toBe('21')
    fireEvent.change(days, { target: { value: '7' } })
    expect((await saved()).historySnapshotDays).toBe(7)
  })

  it('display: Logbook columns Reset and the station card on hover', async () => {
    await openTab('Logbook')
    const disp = section('logbook-display')
    fireEvent.click(disp.getByRole('button', { name: 'Reset' }))
    expect(api.get('setLogbookColumns')).toHaveBeenCalledWith(null)
    const hover = disp.getByRole('checkbox', { name: 'Open the station card when the pointer rests on a call' }) as HTMLInputElement
    expect(hover.checked).toBe(false)
    fireEvent.click(hover)
    expect(localStorage.getItem(CALL_CARD_HOVER_KEY)).toBe('on')
  })

  it('worked-before and the LoTW users list', async () => {
    await openTab('Logbook')
    const b4 = section('connections-b4').getByRole('switch', { name: /^Match mode too/ })
    expect(b4.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(b4)
    const age = section('lotw-users').getByRole('spinbutton', { name: /^Count as a LoTW user/ }) as HTMLInputElement
    expect(age.value).toBe('120')
    fireEvent.change(age, { target: { value: '200' } })
    const s = await saved()
    expect([s.b4MatchMode, s.lotwMaxAgeDays]).toEqual([false, 200])
  })
})
