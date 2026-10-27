// @vitest-environment jsdom
//
// ⭐ THE CONTEST STRIP'S ALT+W AND CTRL+O, by value:
//
//   - Alt+W clears the whole log line, the call and every exchange box, and releases the serial
//     number in flight; a second Alt+W right after puts back what it cleared: the call, the boxes
//     and their call-history marks. Any other key between the two, or a line typed since, ends the
//     chance, as any other key lets a Ctrl+D press lapse.
//   - Ctrl+O asks for the operator box, which the window answers (`features/contestKeys.ts`); the
//     strip only asks.
//
// Both are window keys while the strip's host is on screen, as Ctrl+D is, and neither is ever a
// stop key's: Esc and Space pass by untouched. Never on the hosted Remote page.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { OPERATOR_BOX_ASK } from '../features/contestKeys'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  // Asked only by a session with the aids on (`aided`).
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 100, checkedAt: 100, nextCheckAt: 0, count: 3 })),
  getScpCalls: vi.fn(() => Promise.resolve(['K9AAA', 'W9XYZ', 'K9ABC'])),
  getCallHistory: vi.fn(() =>
    Promise.resolve({ contest: 'ilqp', fileName: 'il-2026.txt', entries: { K9AAA: { Loc1: 'WILL' } } }),
  ),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** The Illinois QSO Party as the engine serialises it: RST and county. */
const party = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    ...over,
  }) as unknown as FieldDayStatus

const SNAP = { radio: { band: '20m', dialMhz: 14.025, txEnabled: true, txAllowed: true }, hunt: null } as unknown as AppSnapshot

function Strip({ fieldDay = party(), active = true, remote = false }: { fieldDay?: FieldDayStatus; active?: boolean; remote?: boolean }) {
  return (
    <LogEntry
      snap={SNAP}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode="CW"
      active={active}
      remote={
        remote
          ? { submit: vi.fn(), canSubmit: true, busy: false, pending: false, resetKey: 0, recall: () => null }
          : undefined
      }
    />
  )
}

const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const boxes = () => [...document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')]
const rstBox = () => boxes()[0]
const qthBox = () => boxes()[1]
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
/** The line as the strip shows it: the call, then every box. */
const lineNow = () => [callBox().value, ...boxes().map((b) => b.value)]
/** A key on the window, as the browser delivers it; true when nothing held it. */
const press = async (init: KeyboardEventInit) => {
  let free = true
  await act(async () => {
    free = fireEvent.keyDown(window, init)
  })
  return free
}
const ALT_W = { key: 'w', code: 'KeyW', altKey: true }
const CTRL_O = { key: 'o', code: 'KeyO', ctrlKey: true }

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('Alt+W — wipe the log line, and put it back', () => {
  it('clears the call and every box, a report back to its default, and a second Alt+W right after puts it all back', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    type(rstBox(), '579')
    type(qthBox(), 'COOK')
    expect(lineNow()).toEqual(['K9AAA', '579', 'COOK'])
    expect(await press(ALT_W), 'Alt+W is the strip’s: the browser gets nothing of it').toBe(false)
    expect(lineNow(), 'the whole line is wiped').toEqual(['', '599', ''])
    expect(await press(ALT_W)).toBe(false)
    expect(lineNow(), 'and put back').toEqual(['K9AAA', '579', 'COOK'])
  })

  it('any other key between the two presses ends the chance: the second Alt+W leaves the blank line blank', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await press(ALT_W)
    await press({ key: 'Alt', code: 'AltLeft', altKey: true }) // the Alt held for the next press is not a key
    await press({ key: 'ArrowLeft', code: 'ArrowLeft' })
    await press(ALT_W)
    expect(lineNow()).toEqual(['', '599', ''])
  })

  it('a bare Alt between the presses is not another key: the second Alt+W still puts the line back', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await press(ALT_W)
    await press({ key: 'Alt', code: 'AltLeft', altKey: true })
    await press(ALT_W)
    expect(lineNow()).toEqual(['K9AAA', '599', 'COOK'])
  })

  it('a line filled since the wipe is wiped by the next Alt+W, and that line is the one the press after puts back', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await press(ALT_W)
    type(callBox(), 'W9XYZ') // a click on a spot or the SCP line fills it without a key
    await press(ALT_W)
    expect(lineNow()).toEqual(['', '599', ''])
    await press(ALT_W)
    expect(lineNow()).toEqual(['W9XYZ', '599', ''])
  })

  it('wipes on a Mac too, where Option+W types ∑ — and nothing is typed', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    expect(await press({ key: '∑', code: 'KeyW', altKey: true })).toBe(false)
    expect(lineNow()).toEqual(['', '599', ''])
  })

  it('leaves the line alone with Ctrl, Shift or Cmd as well: AltGr+W (Ctrl+Alt) types on some keyboards', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    for (const extra of [{ ctrlKey: true }, { shiftKey: true }, { metaKey: true }]) {
      expect(await press({ ...ALT_W, ...extra }), JSON.stringify(extra)).toBe(true)
    }
    expect(lineNow()).toEqual(['K9AAA', '599', ''])
  })

  it('releases the serial number in flight, and the line put back takes the station’s number up again', async () => {
    render(<Strip />)
    type(callBox(), 'K9AAA')
    api.contestEntryReset.mockClear()
    await press(ALT_W)
    expect(api.contestEntryReset).toHaveBeenCalledTimes(1)
    expect(api.contestWorking).not.toHaveBeenCalled()
    await press(ALT_W)
    expect(api.contestWorking.mock.calls).toEqual([['K9AAA']])
  })

  it('puts back the call-history marks with the boxes they were on', async () => {
    render(<Strip fieldDay={party({ assistanceOn: ['Call history'] } as unknown as Partial<FieldDayStatus>)} />)
    await act(async () => new Promise((r) => setTimeout(r, 0)))
    type(callBox(), 'K9AAA')
    expect(qthBox().value, 'premise: the file filled his county').toBe('WILL')
    const marks = () => document.querySelectorAll('.le-fd-from-history').length
    expect(marks()).toBe(1)
    await press(ALT_W)
    expect(lineNow()).toEqual(['', '599', ''])
    expect(marks()).toBe(0)
    await press(ALT_W)
    expect(lineNow()).toEqual(['K9AAA', '599', 'WILL'])
    expect(marks(), 'the county is still the file’s, marked as such').toBe(1)
  })

  it('puts back a box typed over a call-history fill as typed: the file never writes over it', async () => {
    render(<Strip fieldDay={party({ assistanceOn: ['Call history'] } as unknown as Partial<FieldDayStatus>)} />)
    await act(async () => new Promise((r) => setTimeout(r, 0)))
    type(callBox(), 'K9AAA')
    expect(qthBox().value, 'premise: the file filled his county').toBe('WILL')
    type(qthBox(), 'COOK') // the operator types over it…
    expect(document.querySelectorAll('.le-fd-from-history')).toHaveLength(0)
    await press(ALT_W)
    await press(ALT_W)
    expect(lineNow(), '…and that is what comes back').toEqual(['K9AAA', '599', 'COOK'])
    expect(document.querySelectorAll('.le-fd-from-history')).toHaveLength(0)
  })

  it('is the strip’s only while its host is on screen, and never on the Remote page', async () => {
    render(<Strip active={false} />)
    type(callBox(), 'K9AAA')
    expect(await press(ALT_W)).toBe(true)
    expect(lineNow()).toEqual(['K9AAA', '599', ''])
    cleanup()
    render(<Strip remote />)
    type(callBox(), 'K9AAA')
    expect(await press(ALT_W)).toBe(true)
    expect(callBox().value).toBe('K9AAA')
  })
})

describe('Ctrl+O — the operator box', () => {
  it('asks for the operator box from anywhere in the cockpit, once a press, and the browser’s Open never runs', async () => {
    const asked = vi.fn()
    window.addEventListener(OPERATOR_BOX_ASK, asked)
    try {
      render(<Strip />)
      type(callBox(), 'K9AAA')
      expect(await press(CTRL_O)).toBe(false)
      expect(asked).toHaveBeenCalledTimes(1)
      expect(callBox().value, 'the line is untouched').toBe('K9AAA')
      for (const extra of [{ altKey: true }, { shiftKey: true }, { metaKey: true }]) await press({ ...CTRL_O, ...extra })
      expect(asked, 'only Ctrl+O itself').toHaveBeenCalledTimes(1)
    } finally {
      window.removeEventListener(OPERATOR_BOX_ASK, asked)
    }
  })

  it('asks nothing from a strip that is not on screen, or on the Remote page', async () => {
    const asked = vi.fn()
    window.addEventListener(OPERATOR_BOX_ASK, asked)
    try {
      render(<Strip active={false} />)
      expect(await press(CTRL_O)).toBe(true)
      cleanup()
      render(<Strip remote />)
      expect(await press(CTRL_O)).toBe(true)
      expect(asked).not.toHaveBeenCalled()
    } finally {
      window.removeEventListener(OPERATOR_BOX_ASK, asked)
    }
  })
})

describe('the stop keys pass by', () => {
  it('Esc and Space are never held, and neither wipes the line nor asks for the operator box', async () => {
    const asked = vi.fn()
    window.addEventListener(OPERATOR_BOX_ASK, asked)
    try {
      render(<Strip />)
      type(callBox(), 'K9AAA')
      await press(ALT_W)
      expect(await press({ key: 'Escape', code: 'Escape' })).toBe(true)
      expect(await press({ key: ' ', code: 'Space' })).toBe(true)
      expect(asked).not.toHaveBeenCalled()
      // …and each was another key: the wipe stays a wipe.
      await press(ALT_W)
      expect(lineNow()).toEqual(['', '599', ''])
    } finally {
      window.removeEventListener(OPERATOR_BOX_ASK, asked)
    }
  })
})
