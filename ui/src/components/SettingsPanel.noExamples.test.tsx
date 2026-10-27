// @vitest-environment jsdom
//
// SETTINGS — NO MADE-UP SAMPLE VALUES IN AN EMPTY BOX.
//
// An empty box used to show a sample in grey: a callsign, a grid, a state, a Field Day class, an
// IP address, a COM port, a club name. A sample like that reads as a value already filled in. The
// rule: a box shows nothing, a word that says what goes in it, a format, or the real default the
// program will use. This test renders the panel with the boxes empty, walks every tab, and checks
// each box's placeholder BY VALUE.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

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
})
afterEach(cleanup)

/** The caption of the field a box sits in, or its own aria-label. */
const captionOf = (el: HTMLElement): string => {
  const field = el.closest('.settings-field, label')
  const caption = field?.querySelector('.settings-label')?.textContent?.trim()
  return caption || el.getAttribute('aria-label') || '(no caption)'
}

/** Every box's placeholder on every tab, keyed by caption, for one set of settings. */
async function placeholdersWith(overrides: Record<string, unknown>): Promise<Map<string, string[]>> {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, ...overrides } as never),
  )
  render(
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
  await waitFor(() => expect(screen.getAllByRole('tab').length).toBeGreaterThan(5))
  // The form arrives after the settings load; the Station tab's callsign box is the sign.
  await screen.findByText(t('settings.station.callsign.label'), { selector: '.settings-label' })
  const found = new Map<string, string[]>()
  for (const tab of screen.getAllByRole('tab')) {
    fireEvent.click(tab)
    // A collapsed group renders no boxes; open every one on the tab.
    for (const toggle of document.querySelectorAll<HTMLButtonElement>('button[aria-expanded="false"]')) {
      fireEvent.click(toggle)
    }
    for (const el of document.querySelectorAll<HTMLInputElement>('input, textarea')) {
      if (el.type === 'checkbox' || el.type === 'radio') continue
      const key = captionOf(el)
      const list = found.get(key) ?? []
      list.push(el.getAttribute('placeholder') ?? '')
      found.set(key, list)
    }
  }
  cleanup()
  return found
}

// Every conditional box is shown: empty values, and the choice that reveals each box. Some
// choices exclude each other (one rig connection, one keyer, one amplifier link), so there are
// three renders.
const EMPTY = {
  mycall: '',
  mygrid: '',
  opName: '',
  opState: '',
  fdEvent: 'arrlfd',
  fdClass: '',
  fdSection: '',
  fdJoinAddr: '',
  fdHostEnable: false,
  fdEventName: '',
  fdPosition: '',
  contestQthCounty: '',
  contestQthState: '',
  contestCheck: '',
  contestCqZone: 0,
  contestItuZone: 0,
  pttMethod: 'rts',
  pttSerialPort: '',
  serialPort: '',
  rigModel: 2036,
  flexRadioIp: '',
  rotatorModel: 603,
  rotatorPort: '',
  rotatorHost: '',
  stationPowerW: null,
  blockedCalls: [],
  rttyBackend: 'fsk',
  rttyFskPort: '',
  aprsIsEnabled: true,
  aprsIsWatchCalls: [],
  clusterNodesAuto: false,
  clusterHosts: [''],
  clusterSsid: '',
  dxkeeperHost: '',
  n3fjpHost: '',
  n1mmAddr: '',
  eqslQthNickname: '',
  cloudlogUrl: '',
  cloudlogStationId: '',
}
const SCENARIOS = [
  { ...EMPTY, rigConn: 'serial', cwKeyer: 'winkeyer', winkeyerPort: '', ampModel: 'spe', ampPort: '' },
  { ...EMPTY, rigConn: 'network', rigAddr: '', cwKeyer: 'serial', cwKeyPort: '', ampModel: 'kpa', ampPort: ':1500' },
  { ...EMPTY, rigConn: 'icomlan', icomLanHost: '' },
]

let seen: Map<string, string[]>
/** The placeholder of the box captioned `caption` (the first, when several share it). */
const box = (caption: string): string | undefined => seen.get(caption)?.[0]

describe('Settings: an empty box shows no made-up sample value', () => {
  beforeEach(async () => {
    if (seen) return
    seen = new Map()
    for (const s of SCENARIOS) {
      for (const [k, v] of await placeholdersWith(s)) {
        seen.set(k, [...(seen.get(k) ?? []), ...v])
      }
    }
  }, 60_000)

  it.each([
    // Station
    [t('settings.station.callsign.label'), ''],
    [t('settings.station.grid.label'), ''],
    [t('settings.station.opName.label'), ''],
    [t('settings.station.opState.label'), ''],
    // Radio
    [t('settings.profiles.save.label'), ''],
    [t('settings.rigControl.serialPort.label'), 'Select or type'],
    [t('settings.rigControl.pttPort.label'), 'blank = use the CAT port'],
    [t('settings.rigControl.netAddr.label'), ''],
    [t('settings.rigControl.icomLan.host.label'), ''],
    [t('settings.rigControl.flexIp.label'), ''],
    [t('settings.rotator.port.label'), ''],
    [t('settings.rotator.external.label'), 'host:port'],
    [t('settings.amplifier.port.label'), ''],
    [t('settings.amplifier.host.label'), ''],
    // Digital
    ['Blocked callsigns', 'none'],
    [t('settings.digital.stationPower.label'), ''],
    // CW
    [t('settings.cw.winkeyerPort.label'), 'K1EL WinKeyer serial port'],
    [t('settings.cw.keyPort.label'), 'the keying interface (separate from CAT)'],
    [t('settings.rtty.fskPort.label'), "e.g. the FTDX10's USB Enhanced COM"],
    // Spots & Alerts
    [t('settings.aprs.is.watchCalls.label'), ''],
    [t('settings.alerts.geo.entities.label'), 'type to search'],
    [t('watchlist.add.value.aria'), ''],
    // Logging & Connectors
    [t('settings.integrations.clusterSsid.label'), ''],
    [t('settings.dxkeeper.host.label'), 'empty = off'],
    [t('settings.n3fjp.host.label'), 'empty = off'],
    [t('settings.n1mm.addr.label'), 'empty = off'],
    [t('settings.confirmations.eqsl.qthNickname.label'), ''],
    [t('settings.confirmations.cloudlog.url.label'), ''],
    [t('settings.confirmations.cloudlog.stationId.label'), ''],
    // Contesting
    [t('settings.contestStation.county.label'), ''],
    [t('settings.contestStation.state.label'), ''],
    [t('settings.contestStation.check.label'), ''],
    [t('settings.contestStation.cqZone.label'), ''],
    [t('settings.contestStation.ituZone.label'), ''],
    [t('settings.fieldDay.class.label'), ''],
    [t('settings.fieldDay.section.label'), ''],
    [t('settings.fdWho.call.label'), ''],
    [t('settings.fdWho.position.label'), ''],
    [t('settings.fdClub.eventName.label'), ''],
    [t('settings.fdClub.join.label'), ''],
  ])('%s: %j', (caption, want) => {
    expect(seen.has(caption), `no box captioned "${caption}" was rendered`).toBe(true)
    // Every box under the caption, in every render: some captions are shared (a state box on two
    // tabs) and a pair (port and baud) sits under one.
    expect([...new Set(seen.get(caption))]).toEqual([want])
  })

  it('an empty cluster-node row shows no sample node', () => {
    const rows = [...seen.entries()].filter(([, v]) => v.includes('ve7cc.net:23'))
    expect(rows).toEqual([])
  })

  it('keeps the words and the real defaults (a control: the sweep reached these boxes)', () => {
    expect(box(t('settings.station.fdOperator.label'))).toBe(t('settings.station.fdOperator.placeholder'))
    expect(box(t('settings.rigControl.rigctldPort.label'))).toBe('4534')
    expect(box(t('settings.aprs.is.host.label'))).toBe('rotate.aprs2.net')
    expect(box('DXLab Base Port')).toBe('52000')
    expect(box(t('settings.n3fjp.port.label'))).toBe('1100')
  })
})
