// @vitest-environment jsdom
//
// SETTINGS ▸ SPOTS & ALERTS: THE THREE BAND MENUS SHOW THE CHOICE THE BOARDS FOLLOW, AND THE ALERT
// IS ITS OWN SWITCH.
//
// New DXCC, New grid and Rare grid 💎 each store one band choice (`alertDxccBands`,
// `alertGridBands`, `alertRareGridBands`). The Needed boards and the need icons follow those three
// (`needScopeOk`, features/needs.ts). The decode alert's on/off is a fourth key, `alertNew`, and
// it gates the sound and the pop-up only (decided at the station, `needed_alert.rs`).
//
// The panel used to fold the four together. Every menu read Off while `alertNew` was false, though
// the boards went on following the stored choices, so a station with the alert off saw Off, Off,
// Off and still had rare HF squares on its board. Picking any one menu from there wrote Off into
// the other two and turned `alertNew` back on, which took NEW ONE off every board.
//
// So, by value: the menus show what is stored whatever the switch says; picking a menu writes that
// menu and nothing else; the switch writes `alertNew` and nothing else; and both a settings file
// and a saved profile from 1.17.0, with the alert off, load with their real band choices, save
// the same keys with the same values and load again unchanged.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'

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

type Doc = Record<string, unknown>

/** The settings.json a 1.17.0 station has on disk, written by 1.17.0's own code (the file the
 *  rigctl tests in tempo-audio load, where its provenance is written down). */
const STORED_BY_1_17_0: Doc = JSON.parse(
  readFileSync(
    join(__dirname, '../../../crates/tempo-audio/tests/fixtures/settings-1.17.0-net-rigctl-clash.json'),
    'utf8',
  ),
)

/** The four keys, as the station that reported this holds them: the alert off, and the band
 *  choices its boards were following all along. */
const ALERT_OFF = {
  alertNew: false,
  alertDxccBands: 'all',
  alertGridBands: 'vhf',
  alertRareGridBands: 'all',
}
const KEYS = Object.keys(ALERT_OFF)
const fourOf = (doc: Doc) => Object.fromEntries(KEYS.map((k) => [k, doc[k]]))

/** The backend's copy: `getSettings` reads it, `setSettings` replaces it, as the real pair does. */
let onDisk: Doc
const copy = (doc: Doc): Doc => JSON.parse(JSON.stringify(doc))

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
      activeRadioId={onDisk.activeRadio as number}
      radio={undefined}
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

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  localStorage.clear()
  onDisk = { ...copy(STORED_BY_1_17_0), ...ALERT_OFF }
  api.get('getSettings').mockImplementation(() => Promise.resolve(copy(onDisk)))
  api.get('setSettings').mockImplementation((doc: Doc) => {
    onDisk = copy(doc)
    return Promise.resolve(null)
  })
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.17.0'))
  api.get('getDxccEntityNames').mockImplementation(() => Promise.resolve([]))
})
afterEach(cleanup)

async function openAlerts() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Spots & Alerts' }))
  await screen.findByText('Rare grid 💎')
}

/** A band menu, found by the words on screen beside it. */
const menu = (label: string) =>
  screen.getByText(label, { selector: '.settings-label' }).closest('label')!
    .querySelector('select') as HTMLSelectElement

/** What the section shows: the switch (null when there is none) and the three menus. */
const shown = () => ({
  alertNew: alertSwitch()?.getAttribute('aria-checked') ?? null,
  dxcc: menu('New DXCC').value,
  grid: menu('New grid').value,
  rareGrid: menu('Rare grid 💎').value,
})
const alertSwitch = () =>
  screen.queryByRole('switch', { name: 'New DXCC and grid alerts' }) as HTMLButtonElement | null

const pick = (label: string, value: string) => fireEvent.change(menu(label), { target: { value } })

/** Save, and hand back the document the backend was given. */
async function save(): Promise<Doc> {
  const before = api.get('setSettings').mock.calls.length
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )
  await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBe(before + 1))
  return api.get('setSettings').mock.calls[before][0] as Doc
}

describe('Settings ▸ Spots & Alerts: the band menus and the alert switch', () => {
  it('shows the band choices the boards follow while the alert is off', async () => {
    renderPanel()
    await openAlerts()
    expect(shown(), 'the menus must show what is stored, whatever the alert says').toEqual({
      alertNew: 'false',
      dxcc: 'all',
      grid: 'vhf',
      rareGrid: 'all',
    })
  })

  it('has the alert on/off as its own switch, in the same section as the menus', async () => {
    renderPanel()
    await openAlerts()
    const sw = alertSwitch()
    expect(sw?.getAttribute('aria-checked') ?? null, 'no alert switch, or it does not read the stored false').toBe(
      'false',
    )
    expect(sw!.closest('fieldset')).toBe(menu('New DXCC').closest('fieldset'))
  })

  it('picking one menu changes that one and nothing else, the alert switch included', async () => {
    renderPanel()
    await openAlerts()
    pick('Rare grid 💎', 'vhf')
    expect(fourOf(await save())).toEqual({ ...ALERT_OFF, alertRareGridBands: 'vhf' })
    pick('New grid', 'all')
    expect(fourOf(await save())).toEqual({ ...ALERT_OFF, alertRareGridBands: 'vhf', alertGridBands: 'all' })
    pick('New DXCC', 'hf')
    expect(fourOf(await save())).toEqual({
      alertNew: false,
      alertDxccBands: 'hf',
      alertGridBands: 'all',
      alertRareGridBands: 'vhf',
    })
  })

  it('with the alert on, setting every menu to Off leaves the alert switch on', async () => {
    onDisk = { ...onDisk, alertNew: true }
    renderPanel()
    await openAlerts()
    pick('New DXCC', 'off')
    pick('New grid', 'off')
    pick('Rare grid 💎', 'off')
    expect(fourOf(await save())).toEqual({
      alertNew: true,
      alertDxccBands: 'off',
      alertGridBands: 'off',
      alertRareGridBands: 'off',
    })
  })

  it('turning the switch on or off changes no menu', async () => {
    renderPanel()
    await openAlerts()
    expect(alertSwitch(), 'no alert switch').not.toBeNull()
    fireEvent.click(alertSwitch()!)
    expect(shown()).toEqual({ alertNew: 'true', dxcc: 'all', grid: 'vhf', rareGrid: 'all' })
    expect(fourOf(await save())).toEqual({ ...ALERT_OFF, alertNew: true })
    fireEvent.click(alertSwitch()!)
    expect(shown()).toEqual({ alertNew: 'false', dxcc: 'all', grid: 'vhf', rareGrid: 'all' })
    expect(fourOf(await save())).toEqual(ALERT_OFF)
  })
})

describe('a 1.17.0 settings file or profile with the alert off round-trips unchanged', () => {
  it('a settings file: loads with its real band choices, saves every key it had, loads the same again', async () => {
    const stored = copy(onDisk)
    renderPanel()
    await openAlerts()
    const saved = await save()
    // No stored key renamed or dropped, and the four come back with the values they went in with.
    expect(Object.keys(stored).filter((k) => !(k in saved)), 'keys the save dropped or renamed').toEqual([])
    expect(fourOf(saved)).toEqual(ALERT_OFF)
    expect(shown(), 'as loaded').toEqual({ alertNew: 'false', dxcc: 'all', grid: 'vhf', rareGrid: 'all' })
    cleanup()
    renderPanel()
    await openAlerts()
    expect(shown(), 'after the round trip').toEqual({ alertNew: 'false', dxcc: 'all', grid: 'vhf', rareGrid: 'all' })
  })

  it('a saved profile: loads over different settings with its own choices and its alert off', async () => {
    // The live settings disagree with the profile on all four keys, so whichever wins is read
    // off the values.
    onDisk = { ...onDisk, alertNew: true, alertDxccBands: 'all', alertGridBands: 'vhf', alertRareGridBands: 'vhf' }
    const profile = { ...copy(STORED_BY_1_17_0), alertNew: false, alertDxccBands: 'hf', alertGridBands: 'all', alertRareGridBands: 'all' }
    localStorage.setItem('nexus.profiles', JSON.stringify([{ name: 'Portable', schema: 2, settings: profile }]))
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const list = (await screen.findByText('Saved profiles', { selector: '.settings-label' })).closest('label')!
    fireEvent.change(list.querySelector('select')!, { target: { value: 'Portable' } })
    fireEvent.click(list.querySelector('button')!)
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalledTimes(1))
    expect(fourOf(api.get('setSettings').mock.calls[0][0] as Doc)).toEqual({
      alertNew: false,
      alertDxccBands: 'hf',
      alertGridBands: 'all',
      alertRareGridBands: 'all',
    })
    await openAlerts()
    expect(shown()).toEqual({ alertNew: 'false', dxcc: 'hf', grid: 'all', rareGrid: 'all' })
  })
})
