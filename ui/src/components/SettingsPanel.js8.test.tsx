// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 — the section exists, sits where the registry says (the Digital
// tab, its own <fieldset id="settings-js8"> between PSK and SSTV), and its controls edit the
// fields interfaces.md §3.2 names. Defaults are asserted the JS8Call way (spec G3): auto-reply
// ON, relay ON, HB-ack OFF, all five speeds decoded (JS8Call 3.0's every-speed decode). The five
// speed switches write ONE number (js8RxSpeeds, a bitmask: slow 1 · normal 2 · fast 4 · turbo 8
// · ultra 16) so a hand-edited or stale settings.json degrades to "every speed" rather than to
// nothing. HB on/off is deliberately NOT
// a setting (session-only, spec G3) and this file asserts its absence.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { patchSettings } from '../settings/patch'
import type { Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "is one fieldset on the Digital tab, between PSK and…", takes
// 0.25 s and 0.27 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  js8RxSpeeds: 31,
  js8HbIntervalMin: 0,
  js8HbAck: false,
  js8Autoreply: true,
  js8Relay: true,
  js8IdleWatchdogMin: 60,
  js8CallsignAgingMin: 0,
  js8ActivityAgingMin: 2,
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

  it('reads the JS8Call defaults: auto-reply on, relay on, HB-ack off, five speeds decoded', async () => {
    const fs = await openJs8()
    const sw = (label: string) => control(fs, label)
    expect(sw('Auto-reply to queries').getAttribute('aria-checked')).toBe('true')
    expect(sw('Relay for other stations').getAttribute('aria-checked')).toBe('true')
    expect(sw('Answer heartbeats').getAttribute('aria-checked')).toBe('false')
    for (const s of ['Slow', 'Normal', 'Fast', 'JS8 40', 'JS8 60']) {
      expect(sw(s).getAttribute('aria-checked'), `${s} should decode by default`).toBe('true')
    }
    expect((control(fs, 'Transmit speed') as HTMLSelectElement).value).toBe('1')
    expect((control(fs, 'Idle watchdog (minutes)') as HTMLInputElement).value).toBe('60')
    expect((control(fs, 'Heartbeat interval (minutes)') as HTMLInputElement).value).toBe('0')
    expect((control(fs, 'Callsign aging (minutes)') as HTMLInputElement).value, 'CallsignAging: off').toBe('0')
    expect((control(fs, 'Band activity aging (minutes)') as HTMLInputElement).value, 'ActivityAging: 2').toBe('2')
  })

  // JS8Call's "Ask for confirmation before sending autoreply transmissions", on as it ships
  // (Configuration.ui:855, Configuration.cpp:1949).
  it('asks for confirmation before automatic replies by default, and a click turns it off', async () => {
    const fs = await openJs8()
    const sw = control(fs, 'Ask for confirmation before sending automatic replies')
    expect(sw.getAttribute('aria-checked'), 'on by default').toBe('true')
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('false')
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8AutoreplyConfirmation: false })),
    )
  })

  // JS8Call's "Only autoreply to these callsigns", "Never autoreply to these callsigns" and "Never
  // acknowledge heartbeats from these callsigns" (Configuration.ui:762-807): comma-separated,
  // empty by default, read upper-cased (`splitWords`, Configuration.cpp:2416-2428).
  it('the allow and deny lists are comma lists, empty by default, saved upper-cased', async () => {
    const fs = await openJs8()
    const allow = control(fs, 'Only auto-reply to these callsigns') as HTMLInputElement
    const deny = control(fs, 'Never auto-reply to these callsigns') as HTMLInputElement
    const hb = control(fs, 'Never acknowledge heartbeats from these callsigns') as HTMLInputElement
    for (const el of [allow, deny, hb]) expect(el.value, 'empty by default').toBe('')
    fireEvent.change(allow, { target: { value: 'w1aw, k1abc' } })
    fireEvent.change(deny, { target: { value: 'n0xyz' } })
    fireEvent.change(hb, { target: { value: ' kd2uwr ,' } })
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(
        expect.objectContaining({ js8AutoreplyAllow: ['W1AW', 'K1ABC'], js8AutoreplyDeny: ['N0XYZ'], js8HbAckDeny: ['KD2UWR'] }),
      ),
    )
  })

  it('the five speed switches edit one bitmask, independently', async () => {
    const fs = await openJs8()
    const sw = (label: string) => control(fs, label)
    fireEvent.click(sw('JS8 40'))
    expect(sw('JS8 40').getAttribute('aria-checked')).toBe('false')
    expect(sw('Slow').getAttribute('aria-checked')).toBe('true')
    expect(sw('Normal').getAttribute('aria-checked')).toBe('true')
    expect(sw('JS8 60').getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw('JS8 40'))
    expect(sw('JS8 40').getAttribute('aria-checked')).toBe('true')
  })

  // A mask saved before JS8 60 existed (15, the 2.3-era four) is shown as it is — JS8 60 off —
  // and its switch adds bit 16, so the save carries 31.
  it('a stored four-speed mask shows JS8 60 off, and its switch saves 31', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, ...js8Defaults, js8RxSpeeds: 15, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
    )
    const fs = await openJs8()
    const sw = (label: string) => control(fs, label)
    expect(sw('JS8 60').getAttribute('aria-checked')).toBe('false')
    expect(sw('JS8 40').getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw('JS8 60'))
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8RxSpeeds: 31 })))
  })

  it('offers JS8 60 as a transmit speed, index 4, with JS8 40 at 3', async () => {
    const fs = await openJs8()
    const sel = control(fs, 'Transmit speed') as HTMLSelectElement
    expect(Array.from(sel.options).map((o) => [o.value, o.textContent, o.title])).toEqual([
      ['0', 'Slow', ''], ['1', 'Normal', ''], ['2', 'Fast', ''], ['3', 'JS8 40', 'Turbo'], ['4', 'JS8 60', 'Ultra'],
    ])
    fireEvent.change(sel, { target: { value: '4' } })
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8Speed: 4 })))
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

  // JS8Call's "Remove callsigns from call activity after" runs 0 ("Disabled") to 1440 minutes,
  // one at a time (Configuration.ui:464-490).
  it('callsign aging takes whole minutes from 0 to 1440, and saves', async () => {
    const fs = await openJs8()
    const aging = control(fs, 'Callsign aging (minutes)') as HTMLInputElement
    fireEvent.change(aging, { target: { value: '5000' } })
    expect(aging.value, 'capped at 1440').toBe('1440')
    fireEvent.change(aging, { target: { value: '12.7' } })
    expect(aging.value, 'whole minutes').toBe('12')
    fireEvent.change(aging, { target: { value: 'abc' } })
    expect(aging.value, 'junk leaves it alone').toBe('12')
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8CallsignAgingMin: 12 })),
    )
  })

  // JS8Call's "Remove messages from band activity after" runs 0 ("Disabled") to 1440 minutes,
  // default 2 (Configuration.ui:506-531, Configuration.cpp:1854).
  it('band activity aging takes whole minutes from 0 to 1440, and saves', async () => {
    const fs = await openJs8()
    const aging = control(fs, 'Band activity aging (minutes)') as HTMLInputElement
    fireEvent.change(aging, { target: { value: '5000' } })
    expect(aging.value, 'capped at 1440').toBe('1440')
    fireEvent.change(aging, { target: { value: '0' } })
    expect(aging.value, 'off is a value').toBe('0')
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8ActivityAgingMin: 0 })),
    )
  })

  it('groups are a comma list, upper-cased, @-prefixed on the way in', async () => {
    const fs = await openJs8()
    const groups = control(fs, 'Groups') as HTMLInputElement
    fireEvent.change(groups, { target: { value: 'ares, @skcc ,,' } })
    // The box keeps the typed text until it is left (#370), then shows the list as it was read.
    fireEvent.blur(groups)
    expect(groups.value).toBe('@ARES, @SKCC')
  })

  it('HB on/off is NOT a setting here (session-only, spec G3)', async () => {
    const fs = await openJs8()
    const names = within(fs)
      .getAllByRole('switch')
      .map((s) => s.getAttribute('aria-label') ?? s.closest('label')?.textContent ?? '')
    expect(names.some((n) => /^heartbeat$|send heartbeats/i.test(n))).toBe(false)
  })

  // JS8Call will not let @APRSIS or @JS8NET be joined: "%1 is a group that cannot be joined"
  // (Configuration.cpp:1017 on adding one, :2451 on saving Settings; isGroupAllowed,
  // varicode.cpp:1314-1320).
  it('refuses to save @APRSIS or @JS8NET as a group, and says why', async () => {
    for (const group of ['@APRSIS', '@JS8NET']) {
      const fs = await openJs8()
      fireEvent.change(control(fs, 'Groups'), { target: { value: `@FUN, ${group.slice(1).toLowerCase()}` } })
      await clickSave()
      await waitFor(() =>
        expect(screen.queryByRole('alert')?.textContent ?? '', `${group}: the reason`).toContain(
          `${group} is a group that cannot be joined`,
        ),
      )
      expect(api.get('setSettings'), `${group}: nothing saved`).not.toHaveBeenCalled()
      cleanup()
    }
  })

  // JS8Call refuses OK while either is in its Groups field, whatever else changed
  // (Configuration.cpp:2449-2453, asked by accept() at :2595), reading the field upper-cased.
  it('a settings file that already holds one still loads, and no save goes through until it is taken out', async () => {
    for (const [stored, group] of [[['@APRSIS'], '@APRSIS'], [['@ARES', '@js8net'], '@JS8NET']] as const) {
      api.get('getSettings').mockImplementation(() =>
        Promise.resolve({ ...defaultSettings, ...js8Defaults, js8Groups: [...stored], mycall: 'KD9TAW', mygrid: 'EN52' } as never),
      )
      api.get('setSettings').mockClear()
      const fs = await openJs8()
      expect((control(fs, 'Groups') as HTMLInputElement).value, `${group}: it loads as it was`).toBe(stored.join(', '))
      fireEvent.change(control(fs, 'Idle watchdog (minutes)'), { target: { value: '30' } })
      await clickSave()
      await waitFor(() =>
        expect(screen.queryByRole('alert')?.textContent ?? '', `${group}: the reason`).toContain(
          `${group} is a group that cannot be joined`,
        ),
      )
      expect(api.get('setSettings'), `${group}: nothing saved`).not.toHaveBeenCalled()
      fireEvent.change(control(fs, 'Groups'), { target: { value: '@FUN' } })
      await clickSave()
      await waitFor(() => expect(api.get('setSettings'), `${group}: taken out, the Save goes through`).toHaveBeenCalled())
      cleanup()
    }
  })

  // Only the operator's Save is refused. A switch here that saves on the click and a cockpit's
  // own settings (through the patch seam) never pass through it, and the backend's own writers
  // (window places, band and rig state) never reach the panel at all.
  it('refuses only the Save: a switch that saves on the click and a cockpit write still save', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, ...js8Defaults, js8Groups: ['@APRSIS'], mycall: 'KD9TAW', mygrid: 'EN52' } as never),
    )
    await openJs8()
    await clickSave()
    await waitFor(() =>
      expect(screen.queryByRole('alert')?.textContent ?? '', 'control: the Save is refused').toContain(
        '@APRSIS is a group that cannot be joined',
      ),
    )
    fireEvent.click(await screen.findByRole('tab', { name: 'App' }))
    fireEvent.click(await screen.findByRole('switch', { name: 'Turn on beta updates' }))
    await waitFor(() => expect(api.get('setBetaUpdates'), 'the beta switch still saves').toHaveBeenCalledWith(true))
    // The CW cockpit's macro-set switch, as it writes it.
    await patchSettings((s) => ({ macros: { ...s.macros, activeCwProfile: 2 } }))
    const sent = api.get('setSettings').mock.lastCall?.[0] as Settings | undefined
    expect(sent?.macros.activeCwProfile, "a cockpit's own write still saves").toBe(2)
    expect(sent?.js8Groups, 'and keeps the group as it is').toEqual(['@APRSIS'])
  })

  // JS8Call's SPOT block: "Enable spotting @APRSIS messages to the APRS-IS network" (off in
  // Nexus, where JS8Call ships its `SpotToAPRS` on), 3.0's inbound relay (off, `APRSISRelay`: it
  // keys unattended, and its hint says so) and "Never spot these callsigns" (`SpotBlacklist`, read
  // upper-cased).
  it('the @APRSIS switch reads off, the APRS-IS relay off, the never-spot list empty, and each saves', async () => {
    const fs = await openJs8()
    const aprs = control(fs, 'Spot @APRSIS messages to APRS-IS')
    const relay = control(fs, 'Relay APRS-IS messages to heard stations')
    const never = control(fs, 'Never spot these callsigns') as HTMLInputElement
    expect(aprs.getAttribute('aria-checked'), 'off by default, an absent key included').toBe('false')
    expect(relay.getAttribute('aria-checked'), 'off by default').toBe('false')
    expect(never.value, 'empty by default').toBe('')
    expect(within(fs).getByText(/This transmits with nobody at the radio/), 'the relay says it keys unattended').toBeTruthy()
    fireEvent.click(aprs)
    fireEvent.click(relay)
    fireEvent.change(never, { target: { value: 'w1aw, k1abc/p ,' } })
    expect(aprs.getAttribute('aria-checked')).toBe('true')
    expect(relay.getAttribute('aria-checked')).toBe('true')
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(
        expect.objectContaining({ js8SpotAprs: true, js8AprsRelay: true, js8SpotNever: ['W1AW', 'K1ABC/P'] }),
      ),
    )
  })
})

/** The footer Save (`type="submit"`); other sections carry their own "Save …" buttons. */
async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

// JS8Call's two send options, off as it ships them: "Hold PTT between frames while there are more to
// be transmitted" (HoldPTT, Configuration.cpp:2404) and "Allow Tx frequency changes while
// transmitting" (TxQSYAllowed, :2305), its TX-offset half. A settings file from before them carries
// neither key, and reads off.
describe('Settings ▸ Digital ▸ JS8 ▸ Sending', () => {
  it('Hold PTT between frames and Allow TX offset changes while sending read off, and save on', async () => {
    const fs = await openJs8()
    for (const label of ['Hold PTT between frames', 'Allow TX offset changes while sending']) {
      const sw = control(fs, label)
      expect(sw.getAttribute('aria-checked'), `${label}: off by default`).toBe('false')
      fireEvent.click(sw)
      expect(sw.getAttribute('aria-checked'), `${label}: on after a click`).toBe('true')
    }
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(
        expect.objectContaining({ js8HoldPtt: true, js8TxOffsetWhileSending: true }),
      ),
    )
  })

  // JS8Call's Tx delay (TxDelay, Configuration.cpp:2275): 0.2 s by default, 0.1 to 0.5 s on its
  // spin box (Configuration.ui sbTxDelay). Stored in ms; a file from before it reads 0.2 s.
  it('TX delay reads 0.2 s with no key in the file, offers 0.1 to 0.5 s, and saves the pick in ms', async () => {
    const fs = await openJs8()
    const sel = control(fs, 'TX delay') as HTMLSelectElement
    expect(sel.tagName).toBe('SELECT')
    expect(sel.value, "JS8Call's default").toBe('200')
    expect([...sel.options].map((o) => [o.value, o.textContent])).toEqual([
      ['100', '0.1 s'],
      ['200', '0.2 s'],
      ['300', '0.3 s'],
      ['400', '0.4 s'],
      ['500', '0.5 s'],
    ])
    fireEvent.change(sel, { target: { value: '400' } })
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ js8TxDelayMs: 400 })),
    )
  })
})

// The 55-minute rule puts the stations it stops on "Never acknowledge heartbeats from these
// callsigns" from the station itself, as JS8Call's addToHbBlacklist does (Configuration.cpp:
// 1226-1231), while this form may be open. The form's Save carries only what the operator changed,
// over the settings the backend holds at the Save, and its edit of that list is made against the
// list as it is then: a station the rule added after Settings opened is never written away.
describe('Settings ▸ Digital ▸ JS8: the never-acknowledge list the 55-minute rule adds to', () => {
  it('keeps a station the rule added while Settings was open, the list edited or not', async () => {
    const loaded = { ...defaultSettings, ...js8Defaults, mycall: 'KD9TAW', mygrid: 'EN52', js8HbAckDeny: ['N0XYZ', 'K9ZZZ'] }
    api.get('getSettings').mockImplementation(() => Promise.resolve(loaded as never))
    const fs = await openJs8()
    const list = control(fs, 'Never acknowledge heartbeats from these callsigns') as HTMLInputElement
    expect(list.value, 'the form shows the list it loaded').toBe('N0XYZ, K9ZZZ')
    // The rule adds W1AW in the backend; the open form still holds what it loaded.
    const live = { ...loaded, js8HbAckDeny: ['N0XYZ', 'K9ZZZ', 'W1AW'] }
    api.get('getSettings').mockImplementation(() => Promise.resolve(live as never))
    const sent = () => (api.get('setSettings').mock.lastCall?.[0] as Settings | undefined)?.js8HbAckDeny
    // Another field saved: the list is the backend's, W1AW in it.
    fireEvent.change(control(fs, 'Idle watchdog (minutes)'), { target: { value: '30' } })
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalledTimes(1))
    expect(sent(), 'another field saved').toEqual(['N0XYZ', 'K9ZZZ', 'W1AW'])
    // The list itself edited, K9ZZZ out and K1ABC in: the edit goes over the list as it is now.
    fireEvent.change(list, { target: { value: 'N0XYZ, K1ABC' } })
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalledTimes(2))
    expect(sent(), 'the list edited').toEqual(['N0XYZ', 'W1AW', 'K1ABC'])
  })
})
