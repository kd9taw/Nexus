// @vitest-environment jsdom
//
// ⭐ ENTER SENDS MESSAGE IN THE CONTEST STRIP — by value, with the real step table and the real
// host hook (`features/esmHost.ts`), and a stand-in for the cockpit's send path that records what
// it was handed. The cockpits' own send paths are their files' (CwCockpit.esm, RttyCockpit.esm,
// PhoneCockpit.esm).
//
//   - one press is one message or a stated refusal, and a held Enter sends once;
//   - a refused press logs nothing; an accepted one logs once, through the one log path, with the
//     strip's values, and only once the send path took the message;
//   - a stop makes what went out count as not sent, and a stopped last message stays logged with
//     the strip saying so (rule 8 as the operator changed it);
//   - a box filled from call history counts as copied, and a Super Check Partial match counts only
//     once it is picked (rule 10); in S&P, an exchange only a fill completes waits for your call;
//   - ESM off, or from afar, and Enter is what it always was.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { useEffect } from 'react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'
import { esmLitKeys, useEsmHost, type EsmMessage } from '../features/esmHost'
import { CONTEST_LAYOUT_ROLES, CW_CONTEST_LAYOUT, type EsmRoleMap } from '../features/esmRoles'
import type { EsmGuards } from '../features/esm'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
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
    Promise.resolve({
      contest: 'ilqp',
      fileName: 'il-2026.txt',
      entries: { K9AAA: { Loc1: 'WILL' }, K9ABC: { Loc1: 'KANE' } },
    }),
  ),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** 2026-10-18 17:00Z. */
const T0 = 1_792_342_800
const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

/** The Illinois QSO Party as the engine serialises it: RST and county. */
const party = (log: FieldDayQso[] = [], over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
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

const radio = (over: Record<string, unknown> = {}) =>
  ({ radio: { band: '20m', dialMhz: 14.025, txEnabled: true, txAllowed: true, ...over }, hunt: null }) as unknown as AppSnapshot

const CW_ON: EsmGuards = { cockpit: 'cw', txEnabled: true, txAllowed: true, clockRepair: false }

/** What the test drives the host with, and what it reads back. */
const harness = {
  guards: CW_ON as EsmGuards,
  roles: CONTEST_LAYOUT_ROLES as EsmRoleMap,
  sent: [] as EsmMessage[],
  /** What the send path answers: null takes the message. */
  answer: (_m: EsmMessage): Promise<string | null> => Promise.resolve(null),
  host: null as ReturnType<typeof useEsmHost> | null,
}

function Strip({
  fieldDay,
  snap = radio(),
  on = true,
  remote = false,
}: {
  fieldDay: FieldDayStatus
  snap?: AppSnapshot
  on?: boolean
  remote?: boolean
}) {
  const esm = useEsmHost({
    cockpit: 'cw',
    on,
    callOnce: false,
    roles: harness.roles,
    slots: CW_CONTEST_LAYOUT,
    guards: () => harness.guards,
    send: (m) => {
      harness.sent.push(m)
      return harness.answer(m)
    },
  })
  useEffect(() => {
    harness.host = esm
  })
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode="CW"
      esm={esm.host}
      remote={
        remote
          ? { submit: vi.fn(), canSubmit: true, busy: false, pending: false, resetKey: 0, recall: () => null }
          : undefined
      }
    />
  )
}

const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const boxes = () => [...document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')]
const qthBox = () => boxes()[1]
const line = () => document.querySelector('.le-fd-esm-line')?.textContent ?? null
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
/** Enter in a box, as the browser delivers it, and everything it starts. */
const enter = async (el: HTMLInputElement, init: Partial<KeyboardEventInit> = {}) => {
  await act(async () => {
    fireEvent.keyDown(el, { key: 'Enter', ...init })
  })
}
/** What reached the one log path: (call, exchange). */
const logged = () => api.contestLogManual.mock.calls.map((c) => [c[0], c[1]])
const run = async () => {
  await act(async () => {
    harness.host!.noteKey('F1') // the key the set's CQ is on
  })
}

afterEach(() => {
  cleanup()
  harness.guards = CW_ON
  harness.roles = CONTEST_LAYOUT_ROLES
  harness.sent = []
  harness.answer = () => Promise.resolve(null)
  harness.host = null
  for (const f of Object.values(api)) f.mockClear()
})

describe('Enter Sends Message — one press, one message', () => {
  it('runs a contact: CQ, his call and the exchange, then TU, which logs it with the strip’s values', async () => {
    render(<Strip fieldDay={party()} />)
    await run()
    await enter(callBox())
    expect(harness.sent.map((m) => [m.role, m.text, m.call])).toEqual([['cq', 'CQ TEST DE {MYCALL} {MYCALL} K', '']])

    type(callBox(), 'K9AAA')
    await enter(callBox())
    expect(harness.sent[harness.sent.length - 1]).toMatchObject({ role: 'callExch', keys: ['F2'], text: '! {RST} {EXCH}', call: 'K9AAA' })
    expect(document.activeElement, 'the caret goes to his first box to copy the exchange').toBe(boxes()[0])

    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent[harness.sent.length - 1]).toMatchObject({ role: 'tu', keys: ['F3'], text: 'TU {MYCALL}', call: 'K9AAA' })
    expect(logged(), 'the contact logs once, through the one log path').toEqual([
      ['K9AAA', [['RST', '599'], ['QTH', 'COOK']]],
    ])
    expect(harness.sent).toHaveLength(3)
    expect(callBox().value, 'and the strip has cleared for the next one').toBe('')
  })

  it('searches and pounces: my call, then my exchange, which logs it', async () => {
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    await enter(callBox())
    expect(harness.sent.map((m) => [m.role, m.text])).toEqual([['myCall', '{MYCALL}']])
    expect(logged()).toEqual([])
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent.map((m) => [m.role, m.text])).toEqual([
      ['myCall', '{MYCALL}'],
      ['exch', 'TU {RST} {EXCH}'],
    ])
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })

  it('logs only once the send path has TAKEN the last message, and the message names the strip’s call', async () => {
    let take: (why: string | null) => void = () => {}
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox()) // S&P, complete: my exchange, and the contact logs…
    expect(harness.sent[harness.sent.length - 1]).toMatchObject({ role: 'exch', call: 'K9AAA' })
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
    cleanup()
    api.contestLogManual.mockClear()
    harness.sent = []

    // …but not before the path has answered: a pending send logs nothing yet.
    harness.answer = () => new Promise((resolve) => (take = resolve))
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toHaveLength(1)
    expect(logged(), 'nothing logs while the message is not taken').toEqual([])
    expect(callBox().value, 'the strip still holds the contact').toBe('K9AAA')
    await act(async () => take(null))
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })

  it('logs nothing when the send path refuses, and says why', async () => {
    harness.answer = () => Promise.resolve('Not sent: TX is off, and Enter never turns it on.')
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toHaveLength(1)
    expect(logged()).toEqual([])
    expect(line()).toBe('Nothing was sent, and nothing was logged: Not sent: TX is off, and Enter never turns it on.')
    expect(callBox().value, 'the contact stays in the strip').toBe('K9AAA')
  })

  it('refuses up front on the cockpit’s guards: nothing is handed to the send path and nothing logs', async () => {
    harness.guards = { ...CW_ON, txEnabled: false }
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
    expect(logged()).toEqual([])
    expect(line()).toBe('TX is off, and Enter never turns it on. Turn TX on yourself, then press Enter.')
  })

  it('sends once for a held Enter', async () => {
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    await enter(callBox())
    await enter(callBox(), { repeat: true })
    await enter(callBox(), { repeat: true })
    expect(harness.sent.map((m) => m.role)).toEqual(['myCall'])
  })

  it('refuses an own dupe by name, and sends and logs nothing', async () => {
    render(<Strip fieldDay={party([row('K9AAA', 'COOK', T0)])} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
    expect(logged()).toEqual([])
    expect(line()).toBe('A dupe of your own log: Enter sends nothing and logs nothing.')
  })

  it('refuses a step the set does not map by name (decision 4), and Alt+Enter still logs', async () => {
    harness.roles = { cq: ['F1'], callExch: ['F2'], myCall: ['F4'], again: ['F7'] } // no TU, no S&P exchange
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
    expect(logged()).toEqual([])
    expect(line()).toBe('Your S&P exchange: no key is mapped, so Enter sends nothing and logs nothing at that step.')
    await enter(qthBox(), { altKey: true })
    expect(harness.sent, 'Alt+Enter sends nothing').toEqual([])
    expect(logged(), '…and logs').toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })

  it('steps aside for a set with no step mapped: Enter logs exactly as with ESM off', async () => {
    harness.roles = {}
    render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })
})

describe('Enter Sends Message — what counts as a stop', () => {
  it('a stop after his call and the exchange went out makes them count as not sent: the next Enter sends them again', async () => {
    render(<Strip fieldDay={party()} />)
    await run()
    type(callBox(), 'K9AAA')
    await enter(callBox()) // his call and the exchange
    type(qthBox(), 'COOK')
    await act(async () => harness.host!.noteStop()) // Esc, Stop TX…
    expect(line()).toBe('His call and your exchange stopped, so it counts as not sent: the next Enter sends it again.')
    await enter(qthBox())
    expect(harness.sent.map((m) => m.role), 'sent again rather than logged on').toEqual(['callExch', 'callExch'])
    expect(logged()).toEqual([])
    await enter(qthBox())
    expect(harness.sent[harness.sent.length - 1]?.role).toBe('tu')
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })

  it('the TX latch falling is a stop too (the watchdog among them)', async () => {
    const { rerender } = render(<Strip fieldDay={party()} />)
    await run()
    type(callBox(), 'K9AAA')
    await enter(callBox())
    rerender(<Strip fieldDay={party()} snap={radio({ txEnabled: false })} />)
    expect(line()).toBe('His call and your exchange stopped, so it counts as not sent: the next Enter sends it again.')
    rerender(<Strip fieldDay={party()} snap={radio({ txEnabled: true })} />)
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent.map((m) => m.role)).toEqual(['callExch', 'callExch'])
    expect(logged()).toEqual([])
  })

  it('a stopped TU stays logged, and the strip says so and how to take it back', async () => {
    const { rerender } = render(<Strip fieldDay={party()} />)
    await run()
    type(callBox(), 'K9AAA')
    await enter(callBox())
    type(qthBox(), 'COOK')
    await enter(qthBox()) // TU: logged
    expect(logged()).toHaveLength(1)
    await act(async () => harness.host!.noteStop())
    expect(line()).toBe('TU stopped. K9AAA is logged. Ctrl+D twice removes it.')
    // The snapshot then brings the logged row: the note is about that contact and stays…
    rerender(<Strip fieldDay={party([row('K9AAA', 'COOK', T0)])} />)
    expect(line()).toBe('TU stopped. K9AAA is logged. Ctrl+D twice removes it.')
    // …until the next key, whichever it is.
    fireEvent.keyDown(window, { key: 'W' })
    expect(line()).toBe(null)
  })

  it('says why it cannot be taken back while club sync is on', async () => {
    const club = { club: { hosting: false } } as unknown as Partial<FieldDayStatus>
    render(<Strip fieldDay={party([], club)} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox()) // S&P exchange: logged
    await act(async () => harness.host!.noteStop())
    expect(line()).toBe(
      'Your S&P exchange stopped. K9AAA is logged, and club sync is on, so it cannot be removed: the club log already has it. Edit the club’s Cabrillo file before you send it in.',
    )
  })

  it('a stop after the message’s over has ended changes no note', async () => {
    const { rerender } = render(<Strip fieldDay={party()} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    rerender(<Strip fieldDay={party([row('K9AAA', 'COOK', T0)])} snap={radio({ txBusyReason: 'CW is keying' })} />)
    rerender(<Strip fieldDay={party([row('K9AAA', 'COOK', T0)])} snap={radio({ txBusyReason: null })} />)
    await act(async () => harness.host!.noteStop())
    expect(line()).toBe(null)
  })

  it('Ctrl+D takes over the line from a stopped note', async () => {
    render(<Strip fieldDay={party([row('W9BBB', 'LAKE', T0)])} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    await act(async () => harness.host!.noteStop())
    expect(line()).toBe('Your S&P exchange stopped. K9AAA is logged. Ctrl+D twice removes it.')
    fireEvent.keyDown(window, { key: 'd', code: 'KeyD', ctrlKey: true })
    expect(line()).toBe(null)
    expect(document.querySelector('.le-fd-remove-line')?.textContent).toMatch(/^Remove W9BBB/)
  })
})

describe('Enter Sends Message — what it reads as a complete exchange', () => {
  it('never takes the last contact’s exchange, carried into the boxes, for this one (Field Day)', async () => {
    const fd = {
      ...party(),
      event: 'arrlfd',
      role: '',
      receives: [
        { key: 'CLASS', kind: 'pattern', required: true },
        { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
      ],
      composing: [
        { key: 'CLASS', raw: '1D' },
        { key: 'SECTION', raw: 'IL' },
      ],
    } as unknown as FieldDayStatus
    const w1aw = { ...row('W1AW', '', T0), class: '3A', section: 'CT', rcvd: ['3A', 'CT'] } as FieldDayQso
    const { rerender } = render(<Strip fieldDay={fd} />)
    rerender(<Strip fieldDay={{ ...fd, log: [w1aw], qsoCount: 1 } as FieldDayStatus} />)
    expect(boxes().map((b) => b.value), 'premise: the last contact’s exchange is in the boxes').toEqual(['3A', 'CT'])
    type(callBox(), 'K9AAA')
    await enter(callBox())
    expect(harness.sent.map((m) => m.role), 'S&P: my call, not my exchange').toEqual(['myCall'])
    expect(logged(), 'and nothing logged with W1AW’s exchange').toEqual([])
    // Typed for this contact, it is his: then the exchange goes, and the contact logs.
    type(boxes()[0], '2A')
    type(boxes()[1], 'IL')
    await enter(boxes()[1])
    expect(harness.sent[harness.sent.length - 1]?.role).toBe('exch')
    expect(logged()).toEqual([['K9AAA', [['CLASS', '2A'], ['SECTION', 'IL']]]])
  })
})

describe('Enter Sends Message — call history and Super Check Partial (rule 10)', () => {
  /** Both aids on, as the engine reports them. The file has K9AAA in Will County. */
  const aided = () =>
    party([], { assistanceOn: ['Super Check Partial', 'Call history'] } as unknown as Partial<FieldDayStatus>)
  /** Let the mount's reads (the SCP list, the call-history file) settle. */
  const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
  const scpShown = () => [...document.querySelectorAll('.le-scp button')].map((b) => b.textContent)

  it('Run: a box filled from call history counts as copied, so TU goes and the contact logs with the fill', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    await run()
    type(callBox(), 'K9AAA')
    expect(qthBox().value, 'premise: the file filled his county').toBe('WILL')
    await enter(callBox())
    await enter(callBox())
    expect({ sent: harness.sent.map((m) => [m.role, m.text, m.call]), logged: logged(), line: line() }).toEqual({
      sent: [
        ['callExch', '! {RST} {EXCH}', 'K9AAA'],
        ['tu', 'TU {MYCALL}', 'K9AAA'],
      ],
      logged: [['K9AAA', [['RST', '599'], ['QTH', 'WILL']]]],
      line: null,
    })
  })

  // In S&P the file completes his exchange as the call is typed, before he has sent a thing, so
  // the first Enter sends your call and the next your exchange, which logs: N1MM's order.
  /** What the press sent and logged, and what the dock lights for the next Enter. */
  const outcome = () => ({
    sent: harness.sent.map((m) => [m.role, m.text, m.call]),
    logged: logged(),
    lit: esmLitKeys(harness.host!.preview),
  })
  const MY_CALL = ['myCall', '{MYCALL}']
  const MY_EXCH = ['exch', 'TU {RST} {EXCH}']

  it('S&P: when only the file completes his exchange, the first Enter sends your call and logs nothing; the next sends your exchange and logs the fill', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    type(callBox(), 'K9AAA')
    expect(qthBox().value, 'premise: the file filled his county').toBe('WILL')
    await enter(callBox())
    expect(outcome()).toEqual({ sent: [[...MY_CALL, 'K9AAA']], logged: [], lit: ['F6'] })
    expect(document.activeElement, 'the cursor stays in Call, as in N1MM').toBe(callBox())
    expect(line()).toBe(null)
    await enter(callBox())
    expect(outcome()).toEqual({
      sent: [
        [...MY_CALL, 'K9AAA'],
        [...MY_EXCH, 'K9AAA'],
      ],
      logged: [['K9AAA', [['RST', '599'], ['QTH', 'WILL']]]],
      lit: ['F4'],
    })
    expect(line()).toBe(null)
  })

  it('S&P: after your call, type over the fill with what he sends, and that is what logs', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    type(callBox(), 'K9AAA')
    await enter(callBox())
    type(qthBox(), 'LAKE')
    await enter(qthBox())
    expect(outcome()).toEqual({
      sent: [
        [...MY_CALL, 'K9AAA'],
        [...MY_EXCH, 'K9AAA'],
      ],
      logged: [['K9AAA', [['RST', '599'], ['QTH', 'LAKE']]]],
      lit: ['F4'],
    })
  })

  it('S&P: type over the fill before you call and the exchange is yours: the dock lights it, and the first Enter sends it and logs what you typed', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    type(callBox(), 'K9AAA')
    expect(esmLitKeys(harness.host!.preview), 'premise: the fill lights your call').toEqual(['F4'])
    type(qthBox(), 'LAKE')
    expect(esmLitKeys(harness.host!.preview), 'typed over, the dock lights your exchange').toEqual(['F6'])
    await enter(qthBox())
    expect(outcome()).toEqual({
      sent: [[...MY_EXCH, 'K9AAA']],
      logged: [['K9AAA', [['RST', '599'], ['QTH', 'LAKE']]]],
      lit: ['F4'],
    })
  })

  it('S&P: an exchange you type for a call the file does not hold sends and logs at the first Enter, as before', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    type(callBox(), 'W9XYZ')
    expect(qthBox().value, 'premise: nothing filled').toBe('')
    type(qthBox(), 'LAKE')
    await enter(qthBox())
    expect(outcome()).toEqual({
      sent: [[...MY_EXCH, 'W9XYZ']],
      logged: [['W9XYZ', [['RST', '599'], ['QTH', 'LAKE']]]],
      lit: ['F4'],
    })
  })

  it('S&P: your call went to the call in the strip only: corrected to another call the file holds, Enter sends your call again first', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    type(callBox(), 'K9AAA')
    await enter(callBox())
    type(callBox(), 'K9ABC')
    expect(qthBox().value, 'premise: the file fills K9ABC’s county').toBe('KANE')
    await enter(callBox())
    expect(outcome()).toEqual({
      sent: [
        [...MY_CALL, 'K9AAA'],
        [...MY_CALL, 'K9ABC'],
      ],
      logged: [],
      lit: ['F6'],
    })
    await enter(callBox())
    expect(outcome().logged).toEqual([['K9ABC', [['RST', '599'], ['QTH', 'KANE']]]])
  })

  it('what you type over the fill is what logs', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    await run()
    type(callBox(), 'K9AAA')
    expect(qthBox().value, 'premise: the file filled his county').toBe('WILL')
    await enter(callBox())
    type(qthBox(), 'LAKE')
    await enter(qthBox())
    expect(harness.sent.map((m) => m.role)).toEqual(['callExch', 'tu'])
    expect(logged(), 'LAKE as typed, not the file’s WILL').toEqual([['K9AAA', [['RST', '599'], ['QTH', 'LAKE']]]])
  })

  it('an SCP match is never sent or logged until you pick it', async () => {
    render(<Strip fieldDay={aided()} />)
    await settle()
    await run()
    type(callBox(), 'W9XY')
    expect(scpShown(), 'premise: SCP offers W9XYZ').toEqual(['W9XYZ'])
    await enter(callBox())
    expect(harness.sent.map((m) => [m.role, m.call]), 'the call as typed, not the match').toEqual([['callExch', 'W9XY']])
    fireEvent.click(screen.getByRole('button', { name: 'W9XYZ' }))
    await enter(callBox())
    type(qthBox(), 'LAKE')
    await enter(qthBox())
    expect(harness.sent.map((m) => [m.role, m.call])).toEqual([
      ['callExch', 'W9XY'],
      ['callExch', 'W9XYZ'],
      ['tu', 'W9XYZ'],
    ])
    expect(logged()).toEqual([['W9XYZ', [['RST', '599'], ['QTH', 'LAKE']]]])
  })
})

describe('Enter Sends Message — off, and from afar, Enter is what it always was', () => {
  it('ESM off: Enter logs, and nothing reaches the send path', async () => {
    render(<Strip fieldDay={party()} on={false} />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
    expect(logged()).toEqual([['K9AAA', [['RST', '599'], ['QTH', 'COOK']]]])
  })

  it('on the hosted page or in the native client: no ESM, whatever the switch says', async () => {
    render(<Strip fieldDay={party()} remote />)
    type(callBox(), 'K9AAA')
    type(qthBox(), 'COOK')
    await enter(qthBox())
    expect(harness.sent).toEqual([])
  })

  it('moves the caret only between the strip’s own boxes, without scrolling', async () => {
    const focus = vi.spyOn(HTMLElement.prototype, 'focus')
    render(<Strip fieldDay={party()} />)
    await run()
    type(callBox(), 'K9AAA')
    focus.mockClear()
    await enter(callBox())
    const moved = focus.mock.contexts as HTMLElement[]
    expect(moved.length).toBeGreaterThan(0)
    for (const el of moved) expect(el.closest('.le-fd-big'), 'inside the strip').not.toBeNull()
    for (const args of focus.mock.calls) expect(args[0]).toEqual({ preventScroll: true })
    focus.mockRestore()
  })
})
