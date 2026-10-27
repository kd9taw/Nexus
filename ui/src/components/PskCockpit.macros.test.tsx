// @vitest-environment jsdom
//
// THE PSK DOCK'S F1–F8 (#316): two built-in sets, editable in place, sent by click or by F-key.
//
// The token rules and the set model are `features/pskMacros.test.ts`. This file is the wiring:
//   · an F-key reaches the SAME `send()` → `psk_send` a click does — there is no second path
//     to the transmitter to keep honest, and until this landed the F-keys reached NOTHING: the
//     dock drew "F1".."F4" on its buttons and no handler ever read them;
//   · the editor saves through `setPskMacros` and never through the settings form;
//   · a saved set SURVIVES A RELOAD — the round trip through the settings mirror, which is what
//     a serde default on the Rust side turns into a silent reset;
//   · Esc with the editor open is the stop-line question, and it is swept in
//     `stop-line.test.tsx` against the real header, not here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { PskCockpit } from './PskCockpit'
import * as api from '../api'
import * as toast from '../toast'
import type { AppSnapshot, PskState, Settings } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "Esc closes an idle editor, and stops a live one instead…", takes
// 0.52 s and 0.52 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const IDLE = {
  armed: true,
  afcHz: 0,
  signal: false,
  centerHz: 1000,
  text: '',
  charConf: [],
  sending: false,
  latched: false,
  keyerError: null,
} as unknown as PskState

vi.mock('../api', () => ({
  getPskState: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  pskArm: vi.fn(),
  pskAutoArm: vi.fn(),
  pskClear: vi.fn(),
  pskAfcReset: vi.fn(),
  pskNet: vi.fn(),
  pskSend: vi.fn(),
  pskSetLatched: vi.fn(),
  pskSetMode: vi.fn(),
  pskType: vi.fn(),
  pskStop: vi.fn(),
  haltTx: vi.fn(),
  atuTune: vi.fn(),
  setRfPower: vi.fn(),
  setTune: vi.fn(),
  setPskMacros: vi.fn(),
  setSettings: vi.fn(),
}))
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
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const mocked = <T,>(f: T) => f as unknown as ReturnType<typeof vi.fn>
const pskSend = mocked(api.pskSend)
const setPskMacros = mocked(api.setPskMacros)
const setSettings = mocked(api.setSettings)
const pushToast = mocked(toast.pushToast)

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.07, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

/** The same station inside a running contest. `sentExchange` is what the SESSION composes,
 *  without the signal report — `{RST}` is its own token. */
const contest = (sentExchange: string) =>
  ({
    ...snap,
    fieldDay: { running: true, state: 'run', qsoCount: 0, sections: 0, points: 0, log: [], sentExchange },
  }) as unknown as AppSnapshot

type Macros = Settings['macros']
const macrosWith = (over: Partial<Macros> = {}): Macros => ({ chat: [], qso: [], band: [], ...over })

let state: PskState = IDLE
beforeEach(() => {
  state = IDLE
  mocked(api.getPskState).mockReset().mockImplementation(async () => state)
  mocked(api.pskAutoArm).mockReset().mockImplementation(async () => state)
  pskSend.mockReset().mockImplementation(async () => ({ ...state, sending: true }))
  // The engine answers with the macros it SAVED — echo the request, as the real command does.
  setPskMacros
    .mockReset()
    .mockImplementation(async (profiles: unknown, active: unknown) =>
      macrosWith({ pskProfiles: profiles as Macros['pskProfiles'], activePskProfile: active as string }),
    )
  setSettings.mockReset()
  pushToast.mockReset()
})
afterEach(cleanup)

async function renderCockpit(props: Partial<Parameters<typeof PskCockpit>[0]> = {}) {
  const r = render(
    <PskCockpit snap={snap} active {...props} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

const key = (k: string) => document.querySelector(`[data-psk-macro="${k}"] .cw-macro`) as HTMLButtonElement
const labelOf = (k: string) => key(k).querySelector('.cw-macro-label')!.textContent
const editOf = (k: string) => document.querySelector(`[data-psk-macro="${k}"] .rtty-macro-edit`) as HTMLButtonElement | null
const editor = () => document.querySelector('.rtty-macro-editor') as HTMLElement | null
const field = (name: RegExp) => screen.getByLabelText(name) as HTMLInputElement
const press = (k: string, init: KeyboardEventInit = {}, target: Element | Window = window) =>
  fireEvent.keyDown(target, { key: k, ...init })

describe('the F1–F8 row', () => {
  it('renders the Everyday built-ins — the shipped four, then four empty keys', async () => {
    await renderCockpit()
    expect(['F1', 'F2', 'F3', 'F4'].map(labelOf)).toEqual(['CQ', 'Answer', 'Exchange', '73'])
    for (const k of ['F5', 'F6', 'F7', 'F8']) {
      expect(key(k).classList.contains('rtty-macro-empty'), `${k} should be an empty key`).toBe(true)
      expect(editOf(k), 'an empty key has nothing to edit with ✎ — clicking it IS the edit').toBeNull()
    }
  })

  it('shows the saved set: the operator’s own caption as written, the rest built in', async () => {
    await renderCockpit({
      macros: macrosWith({
        activePskProfile: 'contest',
        pskProfiles: [{ name: 'contest', macros: [{ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05' }] }],
      }),
    })
    expect(['F1', 'F2', 'F3', 'F7', 'F8'].map(labelOf)).toEqual(['CQ', 'Run exch', 'TU', 'B4', 'AGN'])
    expect(screen.getByRole('button', { name: 'Contest' }).getAttribute('aria-pressed')).toBe('true')
  })

  // ⚠️ PSK'S SETS ARE NOT RTTY'S. The docks share their implementation, so the settings field a
  // cockpit reads is the one thing that says which mode's keys it is showing.
  it('reads pskProfiles — an RTTY set in the same settings object reaches this dock never', async () => {
    await renderCockpit({
      macros: macrosWith({
        activeRttyProfile: 'contest',
        rttyProfiles: [{ name: 'contest', macros: [{ key: 'F1', label: 'RTTY RUN', text: 'CQ TEST W9XYZ' }] }],
      }),
    })
    expect(labelOf('F1'), 'the PSK dock rendered the RTTY dock’s saved key').toBe('CQ')
    expect(
      screen.getByRole('button', { name: 'Everyday' }).getAttribute('aria-pressed'),
      'and RTTY’s active set did not switch this dock',
    ).toBe('true')
    // POSITIVE CONTROL: the same shape under PSK's own key DOES reach it.
    cleanup()
    await renderCockpit({
      macros: macrosWith({
        activePskProfile: 'contest',
        pskProfiles: [{ name: 'contest', macros: [{ key: 'F1', label: 'PSK RUN', text: 'cq test w9xyz' }] }],
      }),
    })
    expect(labelOf('F1')).toBe('PSK RUN')
  })
})

describe('F-keys go through the same send() a click does', () => {
  it('F1 and a click on F1 hand psk_send the identical text', async () => {
    await renderCockpit()
    fireEvent.click(key('F1'))
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
    press('F1')
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(2))
    expect(pskSend.mock.calls[1]).toEqual(pskSend.mock.calls[0])
    // What goes on the air is the message and nothing else — PSK adds no framing.
    expect(pskSend.mock.calls[0][0]).toBe('CQ CQ CQ de W9XYZ W9XYZ pse k')
  })

  it('takes the browser’s F-key away, and works with the caret in the compose bar or the Call box', async () => {
    await renderCockpit()
    const compose = screen.getByLabelText('PSK compose')
    compose.focus()
    expect(press('F1', {}, compose), 'F1 reached the browser — WebView2 help / F5 reload').toBe(false)
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
    const call = document.querySelector('.rtty-hiscall') as HTMLInputElement
    fireEvent.change(call, { target: { value: 'k1abc' } })
    call.focus()
    press('F2', {}, call)
    await waitFor(() => expect(pskSend).toHaveBeenLastCalledWith('K1ABC de W9XYZ W9XYZ k'))
  })

  it('is bound only while PSK is the visible view', async () => {
    const { rerender } = await renderCockpit({ active: false })
    press('F1')
    expect(pskSend).not.toHaveBeenCalled()
    await act(async () => {
      rerender(
        <PskCockpit snap={snap} active />,
      )
    })
    press('F1')
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
  })

  it('a held key’s repeats send nothing, and a modified key is left to the system', async () => {
    await renderCockpit()
    press('F1', { repeat: true })
    expect(press('F4', { altKey: true }), 'Alt+F4 belongs to the window manager').toBe(true)
    press('F1', { ctrlKey: true })
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
  })

  // ⚠️ AN OUTCOME ASSERTION, AND IT CANNOT NAME WHICH GUARD HELD — said plainly rather than
  // dressed up. TWO hold it: `sendMacro` refuses an empty slot, and `send()` refuses a blank
  // line, so deleting either one leaves this green (measured — removing the `isEmptyPskSlot`
  // check reddened nothing). What it is still worth is the outcome itself, an empty key never
  // keying the transmitter, and the control below is what stops it passing for the wrong
  // reason: F-keys generally being dead, which is exactly how this dock shipped.
  it('an empty key keys nothing — while a filled one on the same row does', async () => {
    await renderCockpit()
    press('F5')
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
    expect(pushToast, 'an empty key is inert, not an error').not.toHaveBeenCalled()
    // POSITIVE CONTROL: the keyboard is live — F1 on the same row goes.
    press('F1')
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
  })

  it('refuses {EXCH} with nothing to fill it — a toast, and nothing on the air', async () => {
    await renderCockpit({ macros: macrosWith({ activePskProfile: 'contest' }) })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2') // {CALL} 599 {EXCH} {EXCH}
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledTimes(1)
    // POSITIVE CONTROL: F5 is {CALL} alone, and goes.
    press('F5')
    await waitFor(() => expect(pskSend).toHaveBeenCalledWith('K1ABC'))
  })

  it('keys {EXCH} from the RUNNING contest — the session’s own sent exchange', async () => {
    await renderCockpit({ macros: macrosWith({ activePskProfile: 'contest' }), snap: contest('04 WI') })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2')
    await waitFor(() => expect(pskSend).toHaveBeenCalledWith('K1ABC 599 04 WI 04 WI'))
    expect(pushToast, 'nothing to complain about').not.toHaveBeenCalled()
  })

  it('an EMPTY sent exchange is nothing to send — the same refusal as no contest at all', async () => {
    await renderCockpit({ macros: macrosWith({ activePskProfile: 'contest' }), snap: contest('') })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2')
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledTimes(1)
  })

  // ⚠️ THE COMPOSE BAR IS NOT TOKEN-CHECKED, AND THAT IS DELIBERATE. PSK31's varicode carries a
  // brace perfectly, so typed text has always gone out exactly as typed — including braces —
  // and still does. Only a MACRO's braces were meant as tokens, so only a macro is refused.
  it('leaves typed braces alone while refusing an unknown token in a macro', async () => {
    await renderCockpit()
    const compose = screen.getByLabelText('PSK compose') as HTMLInputElement
    fireEvent.change(compose, { target: { value: 'the rig is a {homebrew} rx' } })
    fireEvent.keyDown(compose, { key: 'Enter' })
    await waitFor(() => expect(pskSend).toHaveBeenCalledWith('the rig is a {homebrew} rx'))
    // The same text in a macro is refused before it can reach the air.
    fireEvent.click(key('F6'))
    fireEvent.change(field(/^message$/i), { target: { value: 'the rig is a {homebrew} rx' } })
    expect(screen.getByRole('alert').textContent).toContain('{homebrew}')
    expect((screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement).disabled).toBe(true)
  })
})

describe('the macro editor', () => {
  it('✎ opens it; Save writes that ONE key through setPskMacros — never the settings form', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F2')!)
    expect(editor()).not.toBeNull()
    fireEvent.change(field(/^title$/i), { target: { value: 'Reply' } })
    fireEvent.change(field(/^message$/i), { target: { value: '{CALL} de {MYCALL} tnx' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(setPskMacros).toHaveBeenCalledWith(
      [{ name: 'everyday', macros: [{ key: 'F2', label: 'Reply', text: '{CALL} de {MYCALL} tnx' }] }],
      '',
    )
    expect(setSettings).not.toHaveBeenCalled()
    expect(editor()).toBeNull()
    expect(labelOf('F2')).toBe('Reply')
  })

  it('hands the saved macros up, so the App mirror is what the engine saved', async () => {
    const onMacrosSaved = vi.fn()
    await renderCockpit({ onMacrosSaved })
    fireEvent.click(editOf('F1')!)
    fireEvent.change(field(/^message$/i), { target: { value: 'cq cq de {MYCALL} k' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(onMacrosSaved).toHaveBeenCalledWith(
      expect.objectContaining({
        pskProfiles: [{ name: 'everyday', macros: [{ key: 'F1', label: 'CQ', text: 'cq cq de {MYCALL} k' }] }],
      }),
    )
  })

  // ⛔ THE ROUND TRIP, and the reason it is its own test. Everything above proves the editor
  // ASKED for a save. This proves the answer survives the trip back: the mirror App holds is
  // fed to a fresh mount, and the dock must come up holding the operator's message rather than
  // the built-in. A serde default that silently drops the field on the Rust side, or a cockpit
  // that reads the wrong key out of the mirror, is invisible in every other test here — the
  // operator finds it by restarting Nexus and losing their macros.
  it('a saved set survives a reload — the mirror the engine returned re-renders as the dock', async () => {
    const onMacrosSaved = vi.fn()
    await renderCockpit({ onMacrosSaved })
    fireEvent.click(editOf('F4')!)
    fireEvent.change(field(/^title$/i), { target: { value: 'Sign off' } })
    fireEvent.change(field(/^message$/i), { target: { value: '{CALL} de {MYCALL} 73 sk' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    const persisted = onMacrosSaved.mock.calls[0][0] as Macros
    cleanup()

    // A FRESH MOUNT — the app restarted and read the settings back.
    await renderCockpit({ macros: persisted })
    expect(labelOf('F4'), 'the saved caption did not survive the round trip').toBe('Sign off')
    // The MESSAGE survived too, not just the caption — read off what F4 actually transmits,
    // which is the only assertion here that a wrong-but-present caption cannot pass.
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F4')
    await waitFor(() => expect(pskSend).toHaveBeenLastCalledWith('K1ABC de W9XYZ 73 sk'))
  })

  it('Cancel closes it and saves nothing', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F1')!)
    fireEvent.change(field(/^title$/i), { target: { value: 'Something else' } })
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(editor()).toBeNull()
    expect(setPskMacros).not.toHaveBeenCalled()
    expect(labelOf('F1')).toBe('CQ')
  })

  it('“Reset this button” removes the saved entry and the built-in comes back', async () => {
    await renderCockpit({
      macros: macrosWith({
        pskProfiles: [{ name: 'everyday', macros: [{ key: 'F1', label: 'Run', text: 'cq w9xyz' }, { key: 'F5', label: 'Rig', text: 'rig 100w' }] }],
      }),
    })
    expect(labelOf('F1')).toBe('Run')
    fireEvent.click(editOf('F1')!)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reset this button' }))
    })
    expect(setPskMacros).toHaveBeenCalledWith([{ name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'rig 100w' }] }], '')
    expect(labelOf('F1')).toBe('CQ')
    // POSITIVE CONTROL: on a built-in key there is nothing to reset.
    fireEvent.click(editOf('F2')!)
    expect((screen.getByRole('button', { name: 'Reset this button' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('clicking an EMPTY key opens its editor instead of sending', async () => {
    await renderCockpit()
    fireEvent.click(key('F6'))
    expect(pskSend).not.toHaveBeenCalled()
    expect(editor()?.getAttribute('aria-label')).toMatch(/F6/)
  })

  it('will not caption a transmit key as a stop, in any shipped language', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F4')!)
    const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement
    for (const caption of ['Stop', 'esc', 'Stopp', 'Parar', 'Arrêter', '停止']) {
      fireEvent.change(field(/^title$/i), { target: { value: caption } })
      expect(screen.getByRole('alert'), caption).toBeTruthy()
      expect(save.disabled, caption).toBe(true)
    }
    fireEvent.change(field(/^title$/i), { target: { value: 'Stopwatch' } })
    expect(save.disabled).toBe(false)
    // …and the real Esc/Stop is untouched by any of it, still exactly one control by that name.
    expect(screen.getAllByRole('button', { name: /^esc\s*stop$/i })).toHaveLength(1)
  })

  it('a failed save keeps the editor open with the operator’s draft', async () => {
    setPskMacros.mockRejectedValueOnce(new Error('InvalidAction'))
    await renderCockpit()
    fireEvent.click(editOf('F3')!)
    fireEvent.change(field(/^title$/i), { target: { value: 'Rpt' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(editor()).not.toBeNull()
    expect(field(/^title$/i).value).toBe('Rpt')
    expect(pushToast).toHaveBeenCalled()
  })

  it('F-keys do nothing while it is open — and still never reach the browser', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F1')!)
    expect(press('F1', {}, field(/^title$/i))).toBe(false)
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
  })

  // The stop-line question, from the editor's side. The SWEEP is stop-line.test.tsx's PSK case
  // against the real header; what is pinned here is that an idle editor swallows Esc and a live
  // transmitter does not.
  it('Esc closes an idle editor, and stops a live one instead of closing it', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F1')!)
    press('Escape')
    await act(async () => {})
    expect(editor(), 'Esc on an idle station closes the editor').toBeNull()
    expect(mocked(api.pskStop), 'and stops nothing').not.toHaveBeenCalled()

    state = { ...IDLE, sending: true }
    mocked(api.pskStop).mockResolvedValue(state)
    mocked(api.haltTx).mockResolvedValue({})
    await act(async () => {
      await Promise.resolve()
    })
    await waitFor(() => expect(document.querySelector('.psk-stop')).not.toHaveProperty('disabled', true))
    fireEvent.click(editOf('F1')!)
    press('Escape')
    await act(async () => {})
    expect(mocked(api.pskStop), 'an over on the air: Esc is a STOP, editor or no editor').toHaveBeenCalled()
  })
})

// RIGHT-CLICK, CTRL+SHIFT+F-KEY AND SEND ONCE — RTTY's, on PSK's own seam (`psk_send`) and
// store (`set_psk_macros`). Every assertion is on those recorded calls, never on a caption.
describe('right-click, Ctrl+Shift+F-key and Send once', () => {
  const sendOnce = () => screen.getByRole('button', { name: 'Send once' }) as HTMLButtonElement
  const locked = { ...snap, radio: { ...snap.radio, txAllowed: false, txRefusal: 'Outside your privileges here' } } as unknown as AppSnapshot
  beforeEach(() => {
    mocked(api.pskStop).mockClear()
    mocked(api.haltTx).mockClear()
  })

  it('a right-click opens THAT key’s editor with the caret in Message, keeps the browser’s menu shut, and sends nothing', async () => {
    await renderCockpit()
    expect(fireEvent.contextMenu(key('F3')), 'the browser’s menu opened over a transmit key').toBe(false)
    expect(editor()?.getAttribute('aria-label')).toBe('Edit F3')
    expect(document.activeElement, 'the caret is in the message').toBe(field(/^message$/i))
    expect(field(/^message$/i).value).toBe('{CALL} de {MYCALL} ur 599 599 btu k')
    fireEvent.contextMenu(key('F7'))
    expect(editor()?.getAttribute('aria-label')).toBe('Edit F7')
    await act(async () => {})
    expect(pskSend, 'a right-click keyed the rig').not.toHaveBeenCalled()
    // POSITIVE CONTROL: a left click on a key still sends it.
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    fireEvent.click(key('F1'))
    await waitFor(() => expect(pskSend).toHaveBeenCalledWith('CQ CQ CQ de W9XYZ W9XYZ pse k'))
  })

  it('Ctrl+Shift+F3 opens F3’s editor from the compose bar, keeps the key from the browser, and sends nothing', async () => {
    await renderCockpit()
    const compose = screen.getByLabelText('PSK compose')
    compose.focus()
    expect(press('F3', { ctrlKey: true, shiftKey: true }, compose), 'Ctrl+Shift+F3 reached the browser').toBe(false)
    expect(editor()?.getAttribute('aria-label')).toBe('Edit F3')
    expect(document.activeElement, 'the caret is in the message').toBe(field(/^message$/i))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(press('F3', { ctrlKey: true, shiftKey: true, altKey: true }), 'Ctrl+Shift+Alt+F3 is the system’s').toBe(true)
    expect(editor()).toBeNull()
    await act(async () => {})
    expect(pskSend, 'the shortcut keyed the rig').not.toHaveBeenCalled()
    // POSITIVE CONTROL: a bare key still sends (F1, which needs no call in the box).
    press('F1')
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
  })

  it('the ✎ names the shortcut, for the keyboard and the screen reader', async () => {
    await renderCockpit()
    const edit = editOf('F4')!
    expect(edit.getAttribute('aria-keyshortcuts')).toBe('Control+Shift+F4')
    expect(edit.title).toBe('Edit the F4 macro. You can also right-click the key, or press Ctrl+Shift+F4.')
  })

  it('Send once keys the EDITED message through the key’s own path — and the key keeps its own', async () => {
    await renderCockpit()
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    fireEvent.contextMenu(key('F2'))
    fireEvent.change(field(/^message$/i), { target: { value: '{CALL} de {MYCALL} qrz?' } })
    fireEvent.click(sendOnce())
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(1))
    expect(pskSend).toHaveBeenCalledWith('K1ABC de W9XYZ qrz?')
    expect(editor(), 'sent: the editor closes').toBeNull()
    expect(setPskMacros, 'Send once saved the key').not.toHaveBeenCalled()
    expect(setSettings).not.toHaveBeenCalled()
    expect(labelOf('F2')).toBe('Answer')
    press('F2')
    await waitFor(() => expect(pskSend).toHaveBeenCalledTimes(2))
    expect(pskSend.mock.calls[1][0]).toBe('K1ABC de W9XYZ W9XYZ k')
  })

  it('Send once is refused exactly where the key is — outside the licence, nothing reaches psk_send, and the draft stays', async () => {
    await renderCockpit({ snap: locked })
    press('F1')
    await act(async () => {})
    expect(pskSend, 'fixture: the key itself is refused').not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenLastCalledWith('Outside your privileges here', 'info', 3500)
    pushToast.mockClear()
    fireEvent.contextMenu(key('F1'))
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ de {MYCALL} k' } })
    fireEvent.click(sendOnce())
    await act(async () => {})
    expect(pskSend, 'Send once went round the licence gate').not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith('Outside your privileges here', 'info', 3500)
    expect(field(/^message$/i).value, 'a refused Send once keeps the draft').toBe('CQ de {MYCALL} k')
    // …and a {CALL} with no call in the box is refused as the key's is.
    cleanup()
    pushToast.mockClear()
    await renderCockpit()
    fireEvent.contextMenu(key('F2'))
    fireEvent.click(sendOnce())
    await act(async () => {})
    expect(pskSend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith('Enter their call first (the {CALL} field)', 'info', 3000)
  })

  it('Esc with a right-click’s editor open: idle, it only closes; with an over on the air, it stops', async () => {
    await renderCockpit()
    fireEvent.contextMenu(key('F1'))
    fireEvent.keyDown(field(/^message$/i), { key: 'Escape' })
    expect(editor(), 'Esc on an idle station closes the editor').toBeNull()
    expect(mocked(api.pskStop)).not.toHaveBeenCalled()
    expect(mocked(api.haltTx), 'Esc-to-cancel turned TX off').not.toHaveBeenCalled()
    cleanup()
    // CONTROL: an over on the air — Esc is a stop, editor or no editor.
    state = { ...IDLE, sending: true }
    mocked(api.pskStop).mockResolvedValue(state)
    mocked(api.haltTx).mockResolvedValue({})
    await renderCockpit()
    await waitFor(() => expect((document.querySelector('.psk-stop') as HTMLButtonElement).disabled).toBe(false))
    press('F1', { ctrlKey: true, shiftKey: true })
    expect(editor(), 'fixture: the shortcut opened the editor').not.toBeNull()
    fireEvent.keyDown(field(/^message$/i), { key: 'Escape' })
    expect(mocked(api.pskStop)).toHaveBeenCalled()
    expect(mocked(api.haltTx)).toHaveBeenCalled()
  })
})

describe('Everyday / Contest', () => {
  it('switching saves the choice and shows the other set', async () => {
    await renderCockpit()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Contest' }))
    })
    expect(setPskMacros).toHaveBeenCalledWith([], 'contest')
    expect(labelOf('F3')).toBe('TU')
    expect(screen.getByRole('button', { name: 'Contest' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('“Reset set” asks inline, then writes the empty list for that set only', async () => {
    const everyday = { name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'rig 100w' }] }
    await renderCockpit({
      macros: macrosWith({
        activePskProfile: 'contest',
        pskProfiles: [{ name: 'contest', macros: [{ key: 'F1', label: 'Run', text: 'cq w9xyz' }] }, everyday],
      }),
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reset set' }))
    expect(setPskMacros).not.toHaveBeenCalled()
    expect(screen.getByRole('status').textContent).toMatch(/Contest/)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reset' }))
    })
    expect(setPskMacros).toHaveBeenCalledWith([{ name: 'contest', macros: [] }, everyday], 'contest')
    expect(labelOf('F1')).toBe('CQ')
  })

  it('offers no reset for a set that is all built-ins', async () => {
    await renderCockpit()
    expect((screen.getByRole('button', { name: 'Reset set' }) as HTMLButtonElement).disabled).toBe(true)
  })
})

// ⭐ THE SCORE CHIP rides in this cockpit's Log pane HEAD, beside the one-word title — the strip below
// sits in a 24–44em column whose own header row has no room left — and the strip carries no second copy.
describe('the contest score chip', () => {
  it('reads the running contest in the Log pane’s head', async () => {
    await renderCockpit({ snap: contest('IL') })
    const head = document.querySelector('section.pane-frame[data-pane="log"] > .pane-head')
    expect(head?.querySelector('.contest-score-chip')?.textContent ?? null).toBe('Field Day · 0 Q · 0 sections · —/hr')
  })
})
