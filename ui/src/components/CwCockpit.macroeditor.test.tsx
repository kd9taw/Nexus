// @vitest-environment jsdom
//
// THE CW DOCK'S MACRO EDITOR — RTTY's and PSK's editor (`MacroEditor.tsx`, through
// `CwMacroEditor.tsx`), reached the same three ways: the ✎, a right-click on a key, and Ctrl+Shift
// with its F-key. None of them keys anything; Save and Send once are the editor's two exits.
//
// Every assertion is on a seam's recorded calls, never on a caption:
//   · THE TRANSMIT SEAM, `send_cw`: a right-click and the shortcut key nothing, and Send once
//     hands it the EDITED message through the key's own `send()`, refused exactly where the key
//     is — CW's tokens go out unexpanded to the engine's expander, as the key's own do;
//   · THE STORE, `set_settings` through the settings patch seam: Save writes the ACTIVE PROFILE'S
//     key, which is what Settings ▸ CW ▸ Macros edits (round-tripped through the real panel), and
//     Send once writes nothing;
//   · THE STOPS, `stop_cw` and `halt_tx`: Esc with the editor open and nothing live closes it and
//     turns nothing off; anything live, it is the abort it always was.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { SettingsPanel } from './SettingsPanel'
import { CONTEST_LAYOUT_ROLES } from '../features/esmRoles'
import type { AppSnapshot, FieldDayStatus } from '../types'
import type { FeaturesApi } from '../useFeatures'
import * as toast from '../toast'
import { installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET: the house's 15 s (a loaded full suite runs cases many times slower than one core);
// a test that hangs still fails.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(async () => ({}))
    return spies[name]
  }
  return { spies, get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  return mod
})
vi.mock('../toast', () => {
  const pushToast = vi.fn()
  return {
    pushToast,
    // Like the real one: run the action; on a failure, toast and answer null.
    withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
      try {
        return await action()
      } catch {
        pushToast('failed', 'error')
        return null
      }
    }),
  }
})
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const sendCw = () => api.get('sendCw')
const setSettings = () => api.get('setSettings')
const pushToast = toast.pushToast as unknown as ReturnType<typeof vi.fn>

/** The operator's own profile, all eight keys, as Settings ▸ CW ▸ Macros holds one. */
const MINE = {
  name: 'Mine',
  macros: [
    { key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} K' },
    { key: 'F2', label: 'Call', text: '! DE {MYCALL} K' },
    { key: 'F3', label: 'Rpt', text: '! UR {RST} {RST} BK' },
    { key: 'F4', label: '73', text: '! TU 73 SK' },
    { key: 'F5', label: 'Me', text: '{MYCALL}' },
    { key: 'F6', label: 'Him', text: '! ' },
    { key: 'F7', label: 'AGN', text: 'AGN AGN' },
    { key: 'F8', label: '?', text: '? ' },
  ],
}

/** The settings the backend holds: what `get_settings` answers, and what `set_settings` replaces. */
let stored: Record<string, unknown> & { macros: Record<string, unknown> }
const withProfiles = (cwProfiles: unknown[], activeCwProfile: number) => {
  stored = structuredClone({ ...defaultSettings, mycall: 'KD9TAW', macros: { ...defaultSettings.macros, cwProfiles, activeCwProfile } })
}
const lastSaved = () => {
  const calls = setSettings().mock.calls
  return calls[calls.length - 1][0] as typeof stored
}

function snap(radio: Record<string, unknown> = {}, fieldDay?: Partial<FieldDayStatus>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      tuning: false, txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500,
      splitTxMhz: null, smeterDb: null, ...radio,
    },
    ...(fieldDay ? { fieldDay } : {}),
  } as unknown as AppSnapshot
}
/** A running contest whose exchange carries a report: the dock's built-in set is the contest layout. */
const QSO_PARTY = {
  running: true, state: 'run', event: 'ilqp', qsoCount: 0, sections: 0, points: 0, log: [], sentExchange: 'COOK',
  receives: [{ key: 'RST', kind: 'rst', required: true }, { key: 'QTH', kind: 'oneOf', required: true }],
} as unknown as FieldDayStatus

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

async function renderCockpit(s: AppSnapshot = snap()) {
  const r = render(
    <CwCockpit snap={s} theme="dark" onWorkSpot={() => {}} spots={[]} fieldDay={s.fieldDay ?? undefined} />,
  )
  await flush()
  return r
}

const key = (k: string) => document.querySelector(`[data-cw-macro="${k}"] .cw-macro`) as HTMLButtonElement
const labelOf = (k: string) => key(k).querySelector('.cw-macro-label')!.textContent
const editor = () => document.querySelector('.rtty-macro-editor') as HTMLElement | null
const field = (name: RegExp) => screen.getByLabelText(name) as HTMLInputElement
const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement
/** A keydown on the window (or `target`); true when nothing cancelled its default. */
const press = (k: string, init: KeyboardEventInit = {}, target: Element | Window = window) =>
  fireEvent.keyDown(target, { key: k, ...init })

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(async () => ({}))
  }
  withProfiles([{ name: 'Default', macros: [] }], 0)
  api.get('getSettings').mockImplementation(async () => structuredClone(stored))
  api.get('setSettings').mockImplementation(async (s: unknown) => {
    stored = structuredClone(s as typeof stored)
    return {}
  })
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '',
    prompt: '', recommended: null, workedCall: null, rst: null, name: null,
  }))
  api.get('selectPeer').mockImplementation(async () => null)
  api.get('previewCw').mockImplementation(async (text: string) => text)
  api.get('setCwKeyer').mockImplementation(async () => null)
  pushToast.mockReset()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('opening the editor keys nothing', () => {
  it('a right-click opens THAT key’s editor with the caret in Message, and keeps the browser’s menu shut', async () => {
    await renderCockpit()
    expect(fireEvent.contextMenu(key('F3')), 'the browser’s menu opened over a transmit key').toBe(false)
    expect(editor()?.getAttribute('aria-label')).toBe('Edit F3')
    expect(document.activeElement, 'the caret is in the message').toBe(field(/^message$/i))
    expect(field(/^message$/i).value).toBe('! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN')
    expect(field(/^title$/i).value).toBe('Reply')
    // The dock is lifted over the pane region's dividers while its editor stands on it
    // (cockpit-panes.css `.cockpit-txdock-editing`; the stacking itself is measured in Chrome).
    const dock = document.querySelector('.cockpit-txdock') as HTMLElement
    expect(dock.classList.contains('cockpit-txdock-editing')).toBe(true)
    await flush()
    expect(sendCw(), 'a right-click keyed the rig').not.toHaveBeenCalled()
    // POSITIVE CONTROL: a left click on a key still sends it.
    fireEvent.click(button('Cancel'))
    expect(dock.classList.contains('cockpit-txdock-editing'), 'closed: the dock is back at its level').toBe(false)
    fireEvent.click(key('F1'))
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('CQ CQ DE {MYCALL} {MYCALL} K'))
  })

  it('Ctrl+Shift+F3 opens F3’s editor and keys nothing; a bare F-key keys nothing while it is open', async () => {
    await renderCockpit()
    expect(press('F3', { ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+F3 reached the browser').toBe(false)
    expect(editor()?.getAttribute('aria-label')).toBe('Edit F3')
    expect(document.activeElement, 'the caret is in the message').toBe(field(/^message$/i))
    expect(press('F1', {}, field(/^message$/i)), 'F1 reached the browser').toBe(false)
    await flush()
    expect(sendCw(), 'a key fired while its editor was open').not.toHaveBeenCalled()
    // With Alt or Cmd as well it is not the chord, and it stays the system's.
    fireEvent.click(button('Cancel'))
    expect(press('F3', { ctrlKey: true, shiftKey: true, altKey: true })).toBe(true)
    expect(press('F3', { ctrlKey: true, shiftKey: true, metaKey: true })).toBe(true)
    expect(editor()).toBeNull()
    // POSITIVE CONTROL: with the editor closed, the bare key sends.
    press('F1')
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('CQ CQ DE {MYCALL} {MYCALL} K'))
  })

  it('the ✎ names the shortcut, for the keyboard and the screen reader', async () => {
    await renderCockpit()
    const edit = document.querySelector('[data-cw-macro="F4"] .rtty-macro-edit') as HTMLButtonElement
    expect(edit.getAttribute('aria-keyshortcuts')).toBe('Control+Shift+F4')
    expect(edit.getAttribute('aria-label')).toBe('Edit the F4 macro')
    expect(edit.title).toBe('Edit the F4 macro. You can also right-click the key, or press Ctrl+Shift+F4.')
  })
})

describe('Send once', () => {
  it('keys the EDITED message through the key’s own send — and the key keeps its own', async () => {
    await renderCockpit()
    fireEvent.contextMenu(key('F2'))
    fireEvent.change(field(/^message$/i), { target: { value: '! DE {MYCALL} QRZ? K' } })
    fireEvent.click(button('Send once'))
    await waitFor(() => expect(sendCw()).toHaveBeenCalledTimes(1))
    // The template, as the key hands over its own: the engine's expander fills `!` and the rest.
    expect(sendCw()).toHaveBeenCalledWith('! DE {MYCALL} QRZ? K')
    expect(editor(), 'sent: the editor closes').toBeNull()
    expect(setSettings(), 'Send once saved the key').not.toHaveBeenCalled()
    expect(labelOf('F2')).toBe('Call')
    press('F2')
    await waitFor(() => expect(sendCw()).toHaveBeenCalledTimes(2))
    expect(sendCw().mock.calls[1][0]).toBe('! DE {MYCALL} {MYCALL} K')
  })

  it('commits a typed His Call before it keys, as the key does, so `!` sends that call', async () => {
    // The engine answers the commit late: nothing may key until it has the call `!` expands to.
    let answer: (value: unknown) => void = () => {}
    api.get('selectPeer').mockImplementation(() => new Promise((resolve) => (answer = resolve)))
    await renderCockpit()
    fireEvent.change(screen.getByLabelText('His call'), { target: { value: 'k1abc' } })
    fireEvent.contextMenu(key('F2'))
    fireEvent.change(field(/^message$/i), { target: { value: '! DE {MYCALL} AGN?' } })
    fireEvent.click(button('Send once'))
    await flush()
    expect(api.get('selectPeer')).toHaveBeenCalledWith('K1ABC')
    expect(sendCw(), 'keyed before the engine had the call `!` sends').not.toHaveBeenCalled()
    await act(async () => answer(null))
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('! DE {MYCALL} AGN?'))
  })

  it('is refused exactly where the key is — outside the licence nothing reaches send_cw, and the draft stays', async () => {
    await renderCockpit(snap({ txAllowed: false, txRefusal: 'Outside your privileges here' }))
    press('F1')
    await flush()
    expect(sendCw(), 'fixture: the key itself is refused').not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenLastCalledWith('Outside your privileges here', 'info', 3500)
    pushToast.mockClear()
    fireEvent.contextMenu(key('F1'))
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ TEST {MYCALL} K' } })
    fireEvent.click(button('Send once'))
    await flush()
    expect(sendCw(), 'Send once went round the licence gate').not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith('Outside your privileges here', 'info', 3500)
    expect(field(/^message$/i).value, 'a refused Send once keeps the draft').toBe('CQ TEST {MYCALL} K')
  })

  it('with TX off it goes where the key goes: to send_cw, whose engine side is the CW arm', async () => {
    // A CW F-key IS the operator's transmit act (`Engine::send_cw` arms TX; ESM's Enter, which
    // may not, goes to `send_cw_armed` instead). Send once is that key's path, so it goes the same
    // way; nothing here reaches `send_cw_armed`.
    await renderCockpit(snap({ txEnabled: false }))
    press('F1')
    await waitFor(() => expect(sendCw()).toHaveBeenCalledTimes(1))
    fireEvent.contextMenu(key('F1'))
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ CQ {MYCALL} K' } })
    fireEvent.click(button('Send once'))
    await waitFor(() => expect(sendCw()).toHaveBeenCalledTimes(2))
    expect(sendCw().mock.calls.map((c) => c[0])).toEqual(['CQ CQ DE {MYCALL} {MYCALL} K', 'CQ CQ {MYCALL} K'])
    expect(api.get('sendCwArmed')).not.toHaveBeenCalled()
  })

  it('will not send a token the expander cannot fill — CW’s tokens are matched with case', async () => {
    await renderCockpit()
    fireEvent.contextMenu(key('F1'))
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ DE {mycall} K' } })
    expect(screen.getByRole('alert').textContent).toContain('{mycall}')
    expect(button('Send once').disabled).toBe(true)
    expect(button('Save').disabled).toBe(true)
    // POSITIVE CONTROL: the same token as the expander writes it sends.
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ DE {MYCALL} {HISNAME} K' } })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(button('Send once').disabled).toBe(false)
  })
})

describe('Save writes the store Settings ▸ CW ▸ Macros edits', () => {
  const settingsFeatures = { enabled: () => true, setEnabled: vi.fn(), all: () => [], profile: 'full', setProfile: vi.fn() } as unknown as FeaturesApi
  /** The real Settings panel on its CW tab, reading what the backend now holds. */
  async function openSettingsCw() {
    for (const name of ['getRigModels', 'getAllRigModels', 'getSerialPortsDetailed', 'getBandPlan', 'getCredentialsStatus', 'getConnectionLog', 'detectRigs'])
      api.get(name).mockImplementation(async () => [])
    api.get('getAudioDevices').mockImplementation(async () => ({ input: [], output: [] }))
    api.get('appVersion').mockImplementation(async () => '0.0.0')
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
        features={settingsFeatures}
      />,
    )
    fireEvent.click(await screen.findByRole('tab', { name: 'CW' }))
  }

  it('on a profile of the operator’s own: that ONE key, over the live settings — and Settings shows it', async () => {
    withProfiles([{ name: 'Default', macros: [] }, MINE], 1)
    await renderCockpit()
    // The backend moved on since the cockpit read it (the dial, say): the save must not undo that.
    stored = { ...stored, dialMhz: 7.025 }
    fireEvent.contextMenu(key('F3'))
    fireEvent.change(field(/^message$/i), { target: { value: '! UR {RST} {RST} GL BK' } })
    await act(async () => {
      fireEvent.click(button('Save'))
    })
    expect(setSettings()).toHaveBeenCalledTimes(1)
    expect(lastSaved().macros.cwProfiles).toEqual([
      { name: 'Default', macros: [] },
      { name: 'Mine', macros: MINE.macros.map((m) => (m.key === 'F3' ? { ...m, text: '! UR {RST} {RST} GL BK' } : m)) },
    ])
    expect(lastSaved().macros.activeCwProfile).toBe(1)
    expect(lastSaved().dialMhz, 'the save wrote a stale copy back').toBe(7.025)
    expect(editor(), 'saved: the editor closes').toBeNull()
    expect(sendCw(), 'Save keyed the rig').not.toHaveBeenCalled()
    // The dock now sends the saved text.
    press('F3')
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('! UR {RST} {RST} GL BK'))
    // …and Settings ▸ CW ▸ Macros shows it, on the same profile.
    cleanup()
    await openSettingsCw()
    expect((await screen.findByLabelText('F3 text') as HTMLInputElement).value).toBe('! UR {RST} {RST} GL BK')
    expect((screen.getByLabelText('F1 text') as HTMLInputElement).value).toBe('CQ CQ DE {MYCALL} K')
  })

  it('on the built-ins: the profile becomes the eight keys the dock showed, as Customize in Settings writes them, with this one changed', async () => {
    await renderCockpit()
    fireEvent.contextMenu(key('F3'))
    fireEvent.change(field(/^message$/i), { target: { value: '! UR {RST} {RST} BK' } })
    await act(async () => {
      fireEvent.click(button('Save'))
    })
    const mine = lastSaved().macros.cwProfiles as { name: string; macros: { key: string; label: string; text: string }[]; esmRoles?: unknown }[]
    expect(mine).toHaveLength(1)
    expect(mine[0].esmRoles, 'the everyday set has no Enter Sends Message steps').toBeUndefined()
    // CONTROL, through the real panel: Customize on the same empty profile writes the same eight
    // keys, labels and all, but for the one changed here.
    cleanup()
    const ours = mine[0].macros
    withProfiles([{ name: 'Default', macros: [] }], 0)
    setSettings().mockClear()
    await openSettingsCw()
    fireEvent.click(await screen.findByRole('button', { name: 'Customize' }))
    fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))
    await waitFor(() => expect(setSettings()).toHaveBeenCalled())
    const customized = (lastSaved().macros.cwProfiles as { macros: { key: string; label: string; text: string }[] }[])[0].macros
    expect(ours).toEqual(customized.map((m) => (m.key === 'F3' ? { ...m, text: '! UR {RST} {RST} BK' } : m)))
  })

  it('in a contest, on the built-ins: the contest set the dock showed, and the steps Enter Sends Message was sending', async () => {
    await renderCockpit(snap({}, QSO_PARTY))
    expect(labelOf('F1'), 'fixture: the dock shows the contest set').toBe('CQ TEST')
    fireEvent.contextMenu(key('F3'))
    expect(field(/^message$/i).value).toBe('TU {MYCALL}')
    fireEvent.change(field(/^message$/i), { target: { value: 'TU {MYCALL} GL' } })
    await act(async () => {
      fireEvent.click(button('Save'))
    })
    expect(lastSaved().macros.cwProfiles).toEqual([
      {
        name: 'Default',
        macros: [
          { key: 'F1', label: 'CQ TEST', text: 'CQ TEST DE {MYCALL} {MYCALL} K' },
          { key: 'F2', label: 'Exch', text: '! {RST} {EXCH}' },
          { key: 'F3', label: 'TU', text: 'TU {MYCALL} GL' },
          { key: 'F4', label: 'My Call', text: '{MYCALL}' },
          { key: 'F5', label: 'His Call', text: '! ' },
          { key: 'F6', label: 'S&P exch', text: 'TU {RST} {EXCH}' },
          { key: 'F7', label: 'B4', text: '! QSO B4' },
          { key: 'F8', label: 'AGN', text: 'AGN AGN' },
        ],
        // The set is no longer the layout word for word, so its steps are written with it:
        // Enter still sends F3 for TU, now with the new text.
        esmRoles: CONTEST_LAYOUT_ROLES,
      },
    ])
  })

  it('a built-in caption left as it was is stored as Settings stores it, not in the language on screen', async () => {
    installCatalog('de', DE)
    setLocale('de')
    try {
      await renderCockpit()
      fireEvent.contextMenu(key('F2'))
      expect(field(/^titel$/i).value, 'fixture: the editor shows the German caption').toBe('Rufen')
      fireEvent.change(field(/^nachricht$/i), { target: { value: '! DE {MYCALL} K' } })
      await act(async () => {
        fireEvent.click(button('Speichern'))
      })
      const keys = (lastSaved().macros.cwProfiles as { macros: { key: string; label: string }[] }[])[0].macros
      expect(keys.find((m) => m.key === 'F2')?.label).toBe('Call')
      expect(keys.find((m) => m.key === 'F3')?.label).toBe('Reply')
    } finally {
      setLocale('en')
    }
  })

  it('Reset this button puts the built-in back on a key of the operator’s own, and is off on a built-in key', async () => {
    withProfiles([{ name: 'Mine', macros: MINE.macros }], 0)
    await renderCockpit()
    fireEvent.contextMenu(key('F3'))
    await act(async () => {
      fireEvent.click(button('Reset this button'))
    })
    const f3 = (lastSaved().macros.cwProfiles as { macros: { key: string }[] }[])[0].macros.find((m) => m.key === 'F3')
    expect(f3).toEqual({ key: 'F3', label: 'Reply', text: '! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN' })
    // POSITIVE CONTROL: on the built-ins there is nothing to put back.
    cleanup()
    withProfiles([{ name: 'Default', macros: [] }], 0)
    await renderCockpit()
    fireEvent.contextMenu(key('F3'))
    expect(button('Reset this button').disabled).toBe(true)
  })

  it('a failed save keeps the editor open with the draft', async () => {
    setSettings().mockImplementation(async () => {
      throw new Error('settings were not saved')
    })
    await renderCockpit()
    fireEvent.contextMenu(key('F2'))
    fireEvent.change(field(/^message$/i), { target: { value: '! DE {MYCALL} TEST' } })
    await act(async () => {
      fireEvent.click(button('Save'))
    })
    expect(editor()).not.toBeNull()
    expect(field(/^message$/i).value).toBe('! DE {MYCALL} TEST')
    expect(pushToast).toHaveBeenCalledWith('failed', 'error')
  })
})

describe('Esc with the editor open', () => {
  it('with nothing live it closes the editor and turns nothing off — the CW keyer’s own stop is still sent', async () => {
    await renderCockpit()
    fireEvent.contextMenu(key('F1'))
    fireEvent.keyDown(field(/^message$/i), { key: 'Escape' })
    expect(editor(), 'Esc did not close the editor').toBeNull()
    expect(api.get('haltTx'), 'Esc-to-cancel turned TX off').not.toHaveBeenCalled()
    // The last word of an over has left the queue while the keyer still sends it, and nothing in
    // the snapshot shows it: the keyer's stop goes anyway. On an idle keyer it has nothing to cut.
    expect(api.get('stopCw')).toHaveBeenCalledTimes(1)
    // CONTROL: with the editor closed, the same idle Esc is the abort, as it always was.
    press('Escape')
    expect(api.get('stopCw')).toHaveBeenCalledTimes(2)
    expect(api.get('haltTx')).toHaveBeenCalledTimes(1)
  })

  it('with an over on the air it is the abort, editor or no editor, and the editor stays', async () => {
    await renderCockpit(snap({ txBusyReason: 'CW is sending' }))
    press('F3', { ctrlKey: true, shiftKey: true })
    expect(editor(), 'fixture: the shortcut opened the editor').not.toBeNull()
    fireEvent.keyDown(field(/^message$/i), { key: 'Escape' })
    expect(api.get('stopCw')).toHaveBeenCalledTimes(1)
    expect(api.get('haltTx')).toHaveBeenCalledTimes(1)
    expect(editor(), 'a stop is not a cancel').not.toBeNull()
  })

  it('a tune carrier is live too', async () => {
    await renderCockpit(snap({ tuning: true }))
    fireEvent.contextMenu(key('F1'))
    fireEvent.keyDown(field(/^message$/i), { key: 'Escape' })
    expect(api.get('haltTx')).toHaveBeenCalledTimes(1)
  })
})
