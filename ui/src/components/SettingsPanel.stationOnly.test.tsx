// @vitest-environment jsdom
//
// SET AT THE STATION. A computer driving the station reads, beside the settings, every setting a
// save from there keeps as the station has it (`stationOnlySettings`): the transmit guards and
// switches, the TQSL path, the addresses a stored key or password is sent to, and the stored keys.
// There each renders read-only, with the station's value and "Set at the station", instead of
// taking an edit the save would quietly put back. At the station itself nothing changes.
//
// The list is the station's own: `stationOnlySettings.json` is held to the Rust list the read
// sends (`the_ui_fixture_carries_the_station_only_list`), so a setting added there turns this red
// until its control here is read-only from afar.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { t } from '../i18n'
import { SETTINGS_SECTIONS, SETTINGS_TABS } from '../settings/registry'
import defaultSettings from './__fixtures__/defaultSettings.json'
import STATION_ONLY from './__fixtures__/stationOnlySettings.json'

vi.setConfig({ testTimeout: 60_000 })

// The station-only settings no form here edits: each is kept by the save from afar all the same.
// The source sweep below turns red the day one of them gets a control.
const NOT_ON_THE_FORM = [
  'directedMaxCalls',
  'maxPowerAm',
  'catRtsState',
  'catDtrState',
  'cloudlogKey',
]

// The station-only settings whose control is drawn at the station alone: from afar the row says
// "Set at the station" and there is no control at all, not even a read-only one.
const DRAWN_AT_THE_STATION_ONLY = ['tciServerTx', 'js8SoundFiles']

const SOURCES = import.meta.glob(['./*.tsx', '!./*.test.tsx'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

// The capture switch is offered on Windows only.
vi.mock('../platform', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  IS_WINDOWS: true,
}))

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (see SettingsPanel.lotwupload.test.tsx
// for why a hand-kept list is the trap).
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

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** Settings that show each conditional control: the CAT broker's PTT, the TCI server's own
 *  controls, the Icom network address or the serial PTT port and lines, the Flex client's Beta, the
 *  SWR threshold, the CW keyline's port and line or the WinKeyer's port, and true FSK's port and
 *  line. Three, as one radio connects one way and CW keys one way. */
const SHOWING = [
  { rigConn: 'icomlan', catBroker: true, tciServer: true, wsjtxUdp: true, swrStopEnabled: true, pttMethod: 'rts',
    cwKeyer: 'serial', rttyBackend: 'fsk' },
  { rigConn: 'serial', catBroker: true, tciServer: true, wsjtxUdp: true, swrStopEnabled: true, pttMethod: 'rts',
    cwKeyer: 'winkeyer', rttyBackend: 'fsk' },
  { rigConn: 'network', rigModel: 2036, flexNativeCat: true, flexRadioIp: '192.168.1.20', catBroker: true, tciServer: true,
    wsjtxUdp: true, swrStopEnabled: true, pttMethod: 'rts' },
]

/** Each station-only control on every tab, by setting, with the settings `read` answers; and each
 *  setting a "Set at the station" line names. */
async function controls(read: Record<string, unknown>) {
  const found = new Map<string, HTMLElement[]>()
  const said = new Set<string>()
  for (const showing of SHOWING) {
    for (const tab of SETTINGS_TABS) {
      const target = SETTINGS_SECTIONS.find((s) => s.tab === tab.id)?.id
      if (!target) continue
      api.get('getSettings').mockImplementation(() =>
        Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', ...showing, ...read } as never),
      )
      const { container, findAllByRole } = render(
        <SettingsPanel
          target={target}
          activeRadioId={0}
          radio={{ swrScaleVerified: true } as never}
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
      await findAllByRole('tab')
      await new Promise((r) => setTimeout(r, 0))
      // Open every closed group, those inside one too.
      for (let i = 0; i < 5; i++) {
        const closed = [...container.querySelectorAll<HTMLElement>('[aria-expanded="false"]')]
        if (closed.length === 0) break
        closed.forEach((b) => fireEvent.click(b))
      }
      for (const el of container.querySelectorAll<HTMLElement>('[data-setting]')) {
        const name = el.dataset.setting!
        found.set(name, [...(found.get(name) ?? []), el])
      }
      for (const el of container.querySelectorAll<HTMLElement>('[data-set-at-station]')) {
        if (el.textContent === t('settings.stationOnly.hint')) said.add(el.dataset.setAtStation!)
      }
      cleanup()
    }
  }
  return { found, said }
}

const hintFor = (el: HTMLElement) =>
  el.closest('.settings-field')?.querySelector<HTMLElement>('[data-set-at-station]') ?? null

describe('settings set at the station', () => {
  it('from afar, each is read-only and says "Set at the station"', async () => {
    const { found, said } = await controls({
      stationOnlySettings: STATION_ONLY,
      secretsSavedAtStation: { clublogApiKey: true, cloudlogKey: false },
    })
    const onTheForm = STATION_ONLY.filter(
      (name) => !NOT_ON_THE_FORM.includes(name) && !DRAWN_AT_THE_STATION_ONLY.includes(name),
    )
    expect([...found.keys()].sort(), 'every station-only control is found').toEqual(
      [...onTheForm].sort(),
    )
    // Drawn at the station alone: no control from afar, and its row says where it is set.
    for (const name of DRAWN_AT_THE_STATION_ONLY) {
      expect(found.has(name), `${name}: a control from afar`).toBe(false)
      expect(said.has(name), `${name}: "Set at the station"`).toBe(true)
    }
    const wrong: string[] = []
    for (const [name, els] of found) {
      for (const el of els) {
        if (!(el as HTMLInputElement).disabled) wrong.push(`${name}: editable`)
        if (hintFor(el)?.textContent !== t('settings.stationOnly.hint')) wrong.push(`${name}: no hint`)
      }
    }
    expect(wrong).toEqual([])
  })

  it('at the station, each is editable and says nothing', async () => {
    const { found, said } = await controls({})
    expect(found.size, 'CONTROL: the controls are found').toBeGreaterThan(30)
    expect(said.size, 'nothing says "Set at the station" at the station').toBe(0)
    // What is drawn at the station alone is drawn there, and checked below like every other.
    for (const name of DRAWN_AT_THE_STATION_ONLY) {
      expect(found.has(name), `${name}: no control at the station`).toBe(true)
    }
    const wrong: string[] = []
    for (const [name, els] of found) {
      for (const el of els) {
        if ((el as HTMLInputElement).disabled) wrong.push(`${name}: read-only`)
        if (hintFor(el) !== null) wrong.push(`${name}: hint`)
      }
    }
    expect(wrong).toEqual([])
  })

  it('the settings no form here edits really have no control', () => {
    const edited = NOT_ON_THE_FORM.filter((name) =>
      Object.values(SOURCES).some((src) => src.includes(`'${name}'`) || src.includes(`form.${name}`)),
    )
    expect(edited).toEqual([])
    expect(
      Object.values(SOURCES).some((src) => src.includes("'txWatchdogMin'")),
      'CONTROL: the sweep finds an edited one',
    ).toBe(true)
  })
})
