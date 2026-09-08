// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 — the section exists, sits where the registry says (the Digital
// tab, its own <fieldset id="settings-js8"> between PSK and SSTV), and its controls edit the
// fields interfaces.md §3.2 names. Defaults are asserted the JS8Call way (spec G3): auto-reply
// ON, relay ON, HB-ack OFF, all four speeds decoded. The four speed switches write ONE number
// (js8RxSpeeds, a bitmask: slow 1 · normal 2 · fast 4 · turbo 8) so a hand-edited or stale
// settings.json degrades to "all four" rather than to nothing. HB on/off is deliberately NOT
// a setting (session-only, spec G3) and this file asserts its absence.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
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

// Every export of `../api`, derived from the real module (the SettingsPanel.aprsplacement
// pattern) so a verb missing from a hand-kept list cannot throw on mount.
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

/** The JS8Call defaults (G3), as the Rust side serialises them. */
const js8Defaults = {
  js8Speed: 1,
  js8RxSpeeds: 15,
  js8HbIntervalMin: 0,
  js8HbAck: false,
  js8Autoreply: true,
  js8Relay: true,
  js8IdleWatchdogMin: 60,
  js8Info: '',
  js8Status: '',
  js8Groups: [] as string[],
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, ...js8Defaults, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

async function openJs8(): Promise<HTMLFieldSetElement> {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  // The panel renders once the settings promise lands; wait on a JS8 control, not the legend
  // (the word "JS8" also appears in hint text).
  await screen.findByText('Transmit speed')
  const fs = document.getElementById('settings-js8')
  expect(fs, 'no <fieldset id="settings-js8">').not.toBeNull()
  expect(fs!.tagName).toBe('FIELDSET')
  return fs as HTMLFieldSetElement
}

/** The control a label names: the toggle button, select or input inside the same `<label>` /
 *  `.settings-field` box as the label SPAN. Not an accessible-name query: the panel wraps
 *  label + control + hint in one <label>, so the accessible name carries the hint text too;
 *  and the speed names (Slow…) also appear as <option>s, so only `.settings-label` spans count. */
function control(fs: HTMLElement, label: string): HTMLElement {
  const span = within(fs)
    .getAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  expect(span, `no settings-label span "${label}"`).toBeDefined()
  const box = span!.closest('label, .settings-field')
  expect(box, `no field box around "${label}"`).not.toBeNull()
  const el = box!.querySelector('button[role="switch"], select, input')
  expect(el, `no control in the "${label}" field`).not.toBeNull()
  return el as HTMLElement
}

describe('Settings ▸ Digital ▸ JS8', () => {
  it('is one fieldset on the Digital tab, between PSK and SSTV, with the registry label as legend', async () => {
    const fs = await openJs8()
    expect(fs.querySelector('legend')!.textContent).toBe('JS8')
    const ids = Array.from(document.querySelectorAll('fieldset.settings-section')).map((f) => f.id)
    expect(ids.indexOf('settings-js8')).toBe(ids.indexOf('settings-psk') + 1)
    expect(ids.indexOf('settings-sstv')).toBe(ids.indexOf('settings-js8') + 1)
  })

  it('reads the JS8Call defaults: auto-reply on, relay on, HB-ack off, four speeds decoded', async () => {
    const fs = await openJs8()
    const sw = (label: string) => control(fs, label)
    expect(sw('Auto-reply to queries').getAttribute('aria-checked')).toBe('true')
    expect(sw('Relay for other stations').getAttribute('aria-checked')).toBe('true')
    expect(sw('Answer heartbeats').getAttribute('aria-checked')).toBe('false')
    for (const s of ['Slow', 'Normal', 'Fast', 'Turbo']) {
      expect(sw(s).getAttribute('aria-checked'), `${s} should decode by default`).toBe('true')
    }
    expect((control(fs, 'Transmit speed') as HTMLSelectElement).value).toBe('1')
    expect((control(fs, 'Idle watchdog (minutes)') as HTMLInputElement).value).toBe('60')
    expect((control(fs, 'Heartbeat interval (minutes)') as HTMLInputElement).value).toBe('0')
  })

  it('the four speed switches edit one bitmask, independently', async () => {
    const fs = await openJs8()
    const sw = (label: string) => control(fs, label)
    fireEvent.click(sw('Turbo'))
    expect(sw('Turbo').getAttribute('aria-checked')).toBe('false')
    expect(sw('Slow').getAttribute('aria-checked')).toBe('true')
    expect(sw('Normal').getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw('Turbo'))
    expect(sw('Turbo').getAttribute('aria-checked')).toBe('true')
  })

  it('the minute fields take whole non-negative numbers and ignore junk', async () => {
    const fs = await openJs8()
    const idle = control(fs, 'Idle watchdog (minutes)') as HTMLInputElement
    fireEvent.change(idle, { target: { value: 'abc' } })
    expect(idle.value).toBe('60')
    fireEvent.change(idle, { target: { value: '30' } })
    expect(idle.value).toBe('30')
    fireEvent.change(idle, { target: { value: '-4' } })
    expect(idle.value).toBe('0')
  })

  it('groups are a comma list, upper-cased, @-prefixed on the way in', async () => {
    const fs = await openJs8()
    const groups = control(fs, 'Groups') as HTMLInputElement
    fireEvent.change(groups, { target: { value: 'ares, @skcc ,,' } })
    expect(groups.value).toBe('@ARES, @SKCC')
  })

  it('HB on/off is NOT a setting here (session-only, spec G3)', async () => {
    const fs = await openJs8()
    const names = within(fs)
      .getAllByRole('switch')
      .map((s) => s.getAttribute('aria-label') ?? s.closest('label')?.textContent ?? '')
    expect(names.some((n) => /^heartbeat$|send heartbeats/i.test(n))).toBe(false)
  })
})
