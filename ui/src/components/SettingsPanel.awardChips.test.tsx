// @vitest-environment jsdom
//
// SETTINGS ▸ SPOTS & ALERTS: "Show WPX prefix needs" and "Show WAE country needs" sit with the
// other Needed-board switches in Alerts, read off for a settings file from before them, and each
// writes its own key and nothing else (operator ruling, 2026-10-25: both opt-in).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

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

/** A settings file from before the two switches: the one a 1.17.0 station wrote (the file the
 *  rigctl tests in tempo-audio load, where its provenance is written down). */
const STORED_BY_1_17_0: Doc = JSON.parse(
  readFileSync(
    join(__dirname, '../../../crates/tempo-audio/tests/fixtures/settings-1.17.0-net-rigctl-clash.json'),
    'utf8',
  ),
)
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
  onDisk = copy(STORED_BY_1_17_0)
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

const wpx = () => screen.getByRole('switch', { name: 'Show WPX prefix needs' })
const wae = () => screen.getByRole('switch', { name: 'Show WAE country needs' })

async function save(): Promise<Doc> {
  const before = api.get('setSettings').mock.calls.length
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )
  await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBe(before + 1))
  return api.get('setSettings').mock.calls[before][0] as Doc
}

describe('Settings ▸ Spots & Alerts: the WPX and WAE need switches', () => {
  it('sit in the Alerts section beside the confirmation switch, both off for an older file', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Spots & Alerts' }))
    await screen.findByRole('switch', { name: 'Show WPX prefix needs' })
    expect([wpx().getAttribute('aria-checked'), wae().getAttribute('aria-checked')]).toEqual(['false', 'false'])
    const confirm = screen.getByRole('switch', { name: 'Confirmation opportunities' })
    expect(wpx().closest('fieldset')).toBe(confirm.closest('fieldset'))
    expect(wae().closest('fieldset')).toBe(confirm.closest('fieldset'))
  })

  it('each writes its own key and leaves the other and the alert switches alone', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Spots & Alerts' }))
    await screen.findByRole('switch', { name: 'Show WPX prefix needs' })
    const keep = ['alertNew', 'alertConfirmTier', 'alertDxccBands', 'alertGridBands', 'alertRareGridBands']
    const kept = (d: Doc) => Object.fromEntries(keep.map((k) => [k, d[k]]))
    const original = kept(onDisk)

    fireEvent.click(wpx())
    let saved = await save()
    expect([saved.showWpxNeeds, saved.showWaeNeeds ?? false]).toEqual([true, false])
    expect(kept(saved)).toEqual(original)

    fireEvent.click(wae())
    fireEvent.click(wpx())
    saved = await save()
    expect([saved.showWpxNeeds, saved.showWaeNeeds]).toEqual([false, true])
    expect(kept(saved)).toEqual(original)
  })
})
