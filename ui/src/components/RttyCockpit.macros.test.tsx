// @vitest-environment jsdom
//
// THE RTTY DOCK'S F1–F8: two built-in sets, editable in place, sent by click or by F-key.
//
// The token rules and the set model are `features/rttyMacros.test.ts`. This file is the wiring:
//   · an F-key reaches the SAME `send()` → `rtty_send` a click does — there is no second path
//     to the transmitter to keep honest;
//   · the editor saves through `setRttyMacros` and never through the settings form;
//   · Esc with the editor open is the stop-line question, and it is swept in
//     `stop-line.test.tsx` against the real header, not here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import { StationControlContext } from '../stationAccess'
import * as api from '../api'
import * as toast from '../toast'
import { frameForAir } from '../features/rttyMacros'
import type { AppSnapshot, RttyState, Settings } from '../types'

const IDLE = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  sending: false,
  latched: false,
  backend: 'afsk',
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  haltTx: vi.fn(),
  setRttyMacros: vi.fn(),
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
const rttySend = mocked(api.rttySend)
const setRttyMacros = mocked(api.setRttyMacros)
const setSettings = mocked(api.setSettings)
const pushToast = mocked(toast.pushToast)

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

/** The same station inside a running contest. `sentExchange` is what the SESSION composes,
 *  without the signal report — `{RST}` is its own token (engine:
 *  `exch_keys_the_running_contests_sent_exchange_and_field_day_is_unchanged`). */
const contest = (sentExchange: string) =>
  ({
    ...snap,
    fieldDay: { running: true, state: 'run', qsoCount: 0, sections: 0, points: 0, log: [], sentExchange },
  }) as unknown as AppSnapshot

type Macros = Settings['macros']
const macrosWith = (over: Partial<Macros> = {}): Macros => ({ chat: [], qso: [], band: [], ...over })

let state: RttyState = IDLE
beforeEach(() => {
  state = IDLE
  mocked(api.getRttyState).mockReset().mockImplementation(async () => state)
  mocked(api.rttyAutoArm).mockReset().mockImplementation(async () => state)
  rttySend.mockReset().mockImplementation(async () => ({ ...state, sending: true }))
  // The engine answers with the macros it SAVED — echo the request, as the real command does.
  setRttyMacros
    .mockReset()
    .mockImplementation(async (profiles: unknown, active: unknown) => macrosWith({ rttyProfiles: profiles as Macros['rttyProfiles'], activeRttyProfile: active as string }))
  setSettings.mockReset()
  pushToast.mockReset()
})
afterEach(cleanup)

async function renderCockpit(props: Partial<Parameters<typeof RttyCockpit>[0]> = {}, control = true) {
  const r = render(
    <StationControlContext.Provider value={control}>
      <RttyCockpit snap={snap} active {...props} />
    </StationControlContext.Provider>,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

const key = (k: string) => document.querySelector(`[data-rtty-macro="${k}"] .cw-macro`) as HTMLButtonElement
const labelOf = (k: string) => key(k).querySelector('.cw-macro-label')!.textContent
const editOf = (k: string) => document.querySelector(`[data-rtty-macro="${k}"] .rtty-macro-edit`) as HTMLButtonElement | null
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
        activeRttyProfile: 'contest',
        rttyProfiles: [{ name: 'contest', macros: [{ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05' }] }],
      }),
    })
    expect(['F1', 'F2', 'F3', 'F7', 'F8'].map(labelOf)).toEqual(['CQ', 'Run exch', 'TU', 'AGN', 'B4'])
    expect(screen.getByRole('button', { name: 'Contest' }).getAttribute('aria-pressed')).toBe('true')
  })
})

describe('F-keys go through the same send() a click does', () => {
  it('F1 and a click on F1 hand rtty_send the identical text', async () => {
    await renderCockpit()
    fireEvent.click(key('F1'))
    await waitFor(() => expect(rttySend).toHaveBeenCalledTimes(1))
    press('F1')
    await waitFor(() => expect(rttySend).toHaveBeenCalledTimes(2))
    expect(rttySend.mock.calls[1]).toEqual(rttySend.mock.calls[0])
    expect(rttySend.mock.calls[0][0]).toBe(frameForAir('CQ CQ CQ DE W9XYZ W9XYZ K'))
  })

  it('takes the browser’s F-key away, and works with the caret in the compose bar or the Call box', async () => {
    await renderCockpit()
    const compose = screen.getByLabelText('RTTY compose')
    compose.focus()
    expect(press('F1', {}, compose), 'F1 reached the browser — WebView2 help / F5 reload').toBe(false)
    await waitFor(() => expect(rttySend).toHaveBeenCalledTimes(1))
    const call = document.querySelector('.rtty-hiscall') as HTMLInputElement
    fireEvent.change(call, { target: { value: 'k1abc' } })
    call.focus()
    press('F2', {}, call)
    await waitFor(() => expect(rttySend).toHaveBeenLastCalledWith(frameForAir('K1ABC DE W9XYZ W9XYZ K')))
  })

  it('is bound only while RTTY is the visible view', async () => {
    const { rerender } = await renderCockpit({ active: false })
    press('F1')
    expect(rttySend).not.toHaveBeenCalled()
    await act(async () => {
      rerender(
        <StationControlContext.Provider value>
          <RttyCockpit snap={snap} active />
        </StationControlContext.Provider>,
      )
    })
    press('F1')
    await waitFor(() => expect(rttySend).toHaveBeenCalledTimes(1))
  })

  it('a held key’s repeats send nothing, and a modified key is left to the system', async () => {
    await renderCockpit()
    press('F1', { repeat: true })
    expect(press('F4', { altKey: true }), 'Alt+F4 belongs to the window manager').toBe(true)
    press('F1', { ctrlKey: true })
    await act(async () => {})
    expect(rttySend).not.toHaveBeenCalled()
  })

  it('an empty key sends nothing', async () => {
    await renderCockpit()
    press('F5')
    await act(async () => {})
    expect(rttySend).not.toHaveBeenCalled()
  })

  it('a Remote observer gets no F-keys and no editing', async () => {
    await renderCockpit({}, false)
    press('F1')
    await act(async () => {})
    expect(rttySend).not.toHaveBeenCalled()
    expect(document.querySelectorAll('.rtty-macro-edit')).toHaveLength(0)
    fireEvent.click(key('F5'))
    expect(editor()).toBeNull()
  })

  it('refuses {EXCH} with nothing to fill it — a toast, and nothing on the air', async () => {
    await renderCockpit({ macros: macrosWith({ activeRttyProfile: 'contest' }) })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2') // {CALL} 599 {EXCH} {EXCH}
    await act(async () => {})
    expect(rttySend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledTimes(1)
    // POSITIVE CONTROL: F5 is {CALL} alone, and goes.
    press('F5')
    await waitFor(() => expect(rttySend).toHaveBeenCalledWith(frameForAir('K1ABC')))
  })

  it('keys {EXCH} from the RUNNING contest — the session’s own sent exchange', async () => {
    await renderCockpit({ macros: macrosWith({ activeRttyProfile: 'contest' }), snap: contest('04 WI') })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2') // {CALL} 599 {EXCH} {EXCH}
    await waitFor(() => expect(rttySend).toHaveBeenCalledWith(frameForAir('K1ABC 599 04 WI 04 WI')))
    expect(pushToast, 'nothing to complain about').not.toHaveBeenCalled()
  })

  it('an EMPTY sent exchange is nothing to send — the same refusal as no contest at all', async () => {
    // The field is present on every snapshot and empty outside a contest, so "" must read as
    // "there is none", never as a message with a hole where the exchange goes.
    await renderCockpit({ macros: macrosWith({ activeRttyProfile: 'contest' }), snap: contest('') })
    fireEvent.change(document.querySelector('.rtty-hiscall')!, { target: { value: 'K1ABC' } })
    press('F2')
    await act(async () => {})
    expect(rttySend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledTimes(1)
  })
})

describe('the macro editor', () => {
  it('✎ opens it; Save writes that ONE key through setRttyMacros — never the settings form', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F2')!)
    expect(editor()).not.toBeNull()
    fireEvent.change(field(/^title$/i), { target: { value: 'Reply' } })
    fireEvent.change(field(/^message$/i), { target: { value: '{CALL} DE {MYCALL} TNX' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(setRttyMacros).toHaveBeenCalledWith(
      [{ name: 'everyday', macros: [{ key: 'F2', label: 'Reply', text: '{CALL} DE {MYCALL} TNX' }] }],
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
    fireEvent.change(field(/^message$/i), { target: { value: 'CQ CQ DE {MYCALL} K' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(onMacrosSaved).toHaveBeenCalledWith(
      expect.objectContaining({ rttyProfiles: [{ name: 'everyday', macros: [{ key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} K' }] }] }),
    )
  })

  it('Cancel closes it and saves nothing', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F1')!)
    fireEvent.change(field(/^title$/i), { target: { value: 'Something else' } })
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(editor()).toBeNull()
    expect(setRttyMacros).not.toHaveBeenCalled()
    expect(labelOf('F1')).toBe('CQ')
  })

  it('“Reset this button” removes the saved entry and the built-in comes back', async () => {
    await renderCockpit({
      macros: macrosWith({
        rttyProfiles: [{ name: 'everyday', macros: [{ key: 'F1', label: 'Run', text: 'CQ W9XYZ' }, { key: 'F5', label: 'Rig', text: 'RIG 100W' }] }],
      }),
    })
    expect(labelOf('F1')).toBe('Run')
    fireEvent.click(editOf('F1')!)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reset this button' }))
    })
    expect(setRttyMacros).toHaveBeenCalledWith([{ name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'RIG 100W' }] }], '')
    expect(labelOf('F1')).toBe('CQ')
    // POSITIVE CONTROL: on a built-in key there is nothing to reset.
    fireEvent.click(editOf('F2')!)
    expect((screen.getByRole('button', { name: 'Reset this button' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('clicking an EMPTY key opens its editor instead of sending', async () => {
    await renderCockpit()
    fireEvent.click(key('F6'))
    expect(rttySend).not.toHaveBeenCalled()
    expect(editor()?.getAttribute('aria-label')).toMatch(/F6/)
  })

  it('flags a token it does not know and will not save it', async () => {
    await renderCockpit()
    fireEvent.click(key('F5'))
    const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement
    fireEvent.change(field(/^message$/i), { target: { value: 'TNX {NAME}' } })
    expect(screen.getByRole('alert').textContent).toContain('{NAME}')
    expect(save.disabled).toBe(true)
    // POSITIVE CONTROL: a known token saves.
    fireEvent.change(field(/^message$/i), { target: { value: 'TNX {CALL}' } })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(save.disabled).toBe(false)
  })

  it('will not caption a transmit key Stop or Esc', async () => {
    await renderCockpit()
    fireEvent.click(editOf('F4')!)
    const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement
    for (const caption of ['Stop', 'esc']) {
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
    setRttyMacros.mockRejectedValueOnce(new Error('InvalidAction'))
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
    expect(rttySend).not.toHaveBeenCalled()
  })
})

describe('Everyday / Contest', () => {
  it('switching saves the choice and shows the other set', async () => {
    await renderCockpit()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Contest' }))
    })
    expect(setRttyMacros).toHaveBeenCalledWith([], 'contest')
    expect(labelOf('F3')).toBe('TU')
    expect(screen.getByRole('button', { name: 'Contest' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('“Reset set” asks inline, then writes the empty list for that set only', async () => {
    const everyday = { name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'RIG 100W' }] }
    await renderCockpit({
      macros: macrosWith({
        activeRttyProfile: 'contest',
        rttyProfiles: [{ name: 'contest', macros: [{ key: 'F1', label: 'Run', text: 'CQ W9XYZ' }] }, everyday],
      }),
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reset set' }))
    expect(setRttyMacros).not.toHaveBeenCalled()
    expect(screen.getByRole('status').textContent).toMatch(/Contest/)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reset' }))
    })
    expect(setRttyMacros).toHaveBeenCalledWith([{ name: 'contest', macros: [] }, everyday], 'contest')
    expect(labelOf('F1')).toBe('CQ')
  })

  it('offers no reset for a set that is all built-ins', async () => {
    await renderCockpit()
    expect((screen.getByRole('button', { name: 'Reset set' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
