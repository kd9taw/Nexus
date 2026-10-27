// @vitest-environment jsdom
//
// THE SIGNAL REPORT FOLLOWS THE MODE, WITH THE CONTEST LOGGER WINDOW OPEN. While that window is open
// the main window's strip and the logger's show one contact (`features/contestEntryShare`), and each
// cockpit has a strip of its own: moving from Phone to CW takes the Phone strip away and brings the
// CW one, which takes the shared contact up. The logger's strip stays, and follows the section.
//
// A report the operator did not type is the mode's own — 59 on phone, 599 on CW — in either window,
// and a report the operator typed is kept for the contact it was typed for, and only that one.
//
// The engine is stood in for by `engine` below, as in LogEntry.sharedEntry.test.tsx: a put replaces
// the entry (one made on a contact already logged is refused), a log moves it on to the next contact
// and keeps its boxes. A poll is a re-render with the snapshot the engine would hand both windows;
// when it lands against a strip's own clear is chosen here, by the test.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, within } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, ContestEntryShared, FieldDayQso, FieldDayStatus } from '../types'
import { ENTRY_LOGGED } from '../features/contestEntryShare'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestEntryPut: vi.fn(),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(() => Promise.resolve({ outcome: 'refused', refusal: 'changed' })),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 0 })),
  getScpCalls: vi.fn(() => Promise.resolve([])),
  getCallHistory: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800

type ModeClass = 'PH' | 'CW'

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const party = (log: FieldDayQso[]): FieldDayStatus =>
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
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'MCHE', domain: 'il_counties' },
    ],
    composingText: '599 MCHE',
    composingTextPhone: '59 MCHE',
    assistanceOn: [],
  }) as unknown as FieldDayStatus

// ── the engine, as the two windows see it ───────────────────────────────────────────────

const engine = {
  open: true,
  entry: { rev: 1, contact: 0, call: '', fields: {}, marks: null } as ContestEntryShared,
  log: [] as FieldDayQso[],
  /** What each log asked for, by value: the call, the received vector and the mode class. */
  logged: [] as { call: string; ex: [string, string][]; mode: string }[],
  put(call: string, fields: Record<string, string>, marks: unknown, contact: number): number {
    if (!this.open) throw new Error('contestEntryClosed')
    if (contact < this.entry.contact) throw ENTRY_LOGGED
    this.entry = { rev: this.entry.rev + 1, contact: this.entry.contact, call, fields, marks }
    return this.entry.rev
  },
  write(call: string, ex: [string, string][], mode: string): void {
    this.logged.push({ call, ex, mode })
    this.log = [
      ...this.log,
      { call, class: '', section: '', band: '40m', mode, submode: mode === 'PH' ? 'SSB' : '', whenUnix: T0 + this.log.length * 60, rcvd: ex.map(([, v]) => v) } as FieldDayQso,
    ]
    // `consume`: the call and the marks go; the boxes stay until a window puts the next contact's.
    if (this.open) this.entry = { ...this.entry, rev: this.entry.rev + 1, contact: this.entry.contact + 1, call: '', marks: null }
  },
}

/** The snapshot both windows poll: the session, and the shared entry while the logger is open. */
const poll = (): AppSnapshot =>
  ({
    radio: { band: '40m', dialMhz: 7.04 },
    hunt: null,
    fieldDay: party(engine.log),
    ...(engine.open ? { contestEntry: { ...engine.entry } } : {}),
  }) as unknown as AppSnapshot

/** A strip as each window mounts it: the cockpits pass their own mode class, and the logger window
 *  passes the one its section maps to (`loggerMode`). */
function strip(snap: AppSnapshot, fdMode: ModeClass, role?: 'logger') {
  return (
    <LogEntry
      snap={snap}
      mode={fdMode === 'PH' ? 'SSB' : 'CW'}
      defaultRst={fdMode === 'PH' ? '59' : '599'}
      exchange="terrestrial"
      titled={false}
      fieldDay={snap.fieldDay}
      fdMode={fdMode}
      sharedEntry={role}
    />
  )
}

/** The main window's cockpit and the logger window. The main strip is KEYED by its cockpit, so a
 *  move from Phone to CW mounts the CW cockpit's strip as the app does; the logger's is one strip. */
function Windows({ snap, section, logger = true }: { snap: AppSnapshot; section: ModeClass; logger?: boolean }) {
  return (
    <>
      <div data-testid="main" key={section}>
        {strip(snap, section)}
      </div>
      {logger && <div data-testid="logger">{strip(snap, section, 'logger')}</div>}
    </>
  )
}

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const inWindow = (w: string) => within(screen.getByTestId(w))
const callIn = (w: string) => inWindow(w).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const rstIn = (w: string) => inWindow(w).getByLabelText('RST') as HTMLInputElement
const qthIn = (w: string) => inWindow(w).getByLabelText('County / State') as HTMLInputElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const lastLogged = () => engine.logged[engine.logged.length - 1]

beforeEach(() => {
  engine.open = true
  engine.entry = { rev: 1, contact: 0, call: '', fields: {}, marks: null }
  engine.log = []
  engine.logged = []
  api.contestEntryPut.mockImplementation(
    (call: string, fields: Record<string, string>, marks: unknown, contact: number) =>
      Promise.resolve(engine.put(call, fields, marks, contact)),
  )
  api.contestLogManual.mockImplementation((call: string, ex: [string, string][], mode: string) => {
    engine.write(call, ex, mode)
    return Promise.resolve({})
  })
})

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the report follows the mode with the logger window open', () => {
  it('a move from Phone to CW after a phone contact: both windows offer 599, and the CW contact logs it', async () => {
    const { rerender } = render(<Windows snap={poll()} section="PH" />)
    await settle()
    type(callIn('main'), 'K9AAA')
    type(qthIn('main'), 'COOK')
    await settle()
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    await settle()
    await settle()
    expect(lastLogged(), 'the phone contact').toEqual({ call: 'K9AAA', ex: [['RST', '59'], ['QTH', 'COOK']], mode: 'PH' })
    // The snapshot carrying the contact, after the strip's own clear has reached the engine; then
    // one more, so each window has taken up what the other put.
    for (let i = 0; i < 2; i++) {
      rerender(<Windows snap={poll()} section="PH" />)
      await settle()
    }
    expect(rstIn('main').value, 'CONTROL: the next phone contact').toBe('59')
    expect(rstIn('logger').value, 'CONTROL: the next phone contact, logger').toBe('59')
    // A box nobody has touched is absent from the entry (`ContestEntryDto::fields`): the windows
    // share no report for the next contact, so each shows its own mode's.
    expect(engine.entry.fields, 'the shared entry after the contact').not.toHaveProperty('RST')

    // The operator moves to the CW cockpit; the logger window follows the section.
    for (let i = 0; i < 2; i++) {
      rerender(<Windows snap={poll()} section="CW" />)
      await settle()
    }
    expect(rstIn('main').value, 'the CW cockpit').toBe('599')
    expect(rstIn('logger').value, 'the logger window, on CW').toBe('599')
    type(callIn('main'), 'K9EEE')
    type(qthIn('main'), 'MCDN')
    await settle()
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    await settle()
    await settle()
    expect(lastLogged(), 'the CW contact').toEqual({ call: 'K9EEE', ex: [['RST', '599'], ['QTH', 'MCDN']], mode: 'CW' })

    // …and back to Phone after it.
    for (let i = 0; i < 3; i++) {
      rerender(<Windows snap={poll()} section="PH" />)
      await settle()
    }
    expect(rstIn('main').value, 'back on the Phone cockpit').toBe('59')
    expect(rstIn('logger').value, 'the logger window, back on phone').toBe('59')
  })

  it('a report typed for one contact is not offered for the next, when the log lands with the strip’s own clear', async () => {
    const { rerender } = render(<Windows snap={poll()} section="PH" />)
    await settle()
    type(callIn('main'), 'K9BBB')
    type(rstIn('main'), '57')
    type(qthIn('main'), 'LAKE')
    await settle()
    rerender(<Windows snap={poll()} section="PH" />)
    await settle()
    expect(rstIn('logger').value, 'CONTROL: a typed report reaches the other window').toBe('57')
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    // The snapshot carrying the contact lands in the same render as the strip's own clear.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      rerender(<Windows snap={poll()} section="PH" />)
    })
    expect(lastLogged(), 'logged as typed').toEqual({ call: 'K9BBB', ex: [['RST', '57'], ['QTH', 'LAKE']], mode: 'PH' })
    for (let i = 0; i < 3; i++) {
      rerender(<Windows snap={poll()} section="PH" />)
      await settle()
    }
    expect(rstIn('main').value, 'the next contact, main window').toBe('59')
    expect(rstIn('logger').value, 'the next contact, logger window').toBe('59')
    // …nor the county typed for it, in either window or in the shared entry.
    expect(qthIn('main').value, 'the next contact county, main window').toBe('')
    expect(qthIn('logger').value, 'the next contact county, logger window').toBe('')
    expect(engine.entry.fields.QTH ?? '', 'the next contact county, shared entry').toBe('')
  })

  it('CONTROL: a report typed for the contact in progress crosses the move to CW, in both windows', async () => {
    const { rerender } = render(<Windows snap={poll()} section="PH" />)
    await settle()
    type(callIn('main'), 'K9CCC')
    type(rstIn('main'), '57')
    type(qthIn('main'), 'COOK')
    await settle()
    for (const section of ['PH', 'CW', 'CW'] as const) {
      rerender(<Windows snap={poll()} section={section} />)
      await settle()
    }
    expect(rstIn('main').value, 'the CW cockpit took the contact up').toBe('57')
    expect(rstIn('logger').value, 'the logger window').toBe('57')
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    await settle()
    await settle()
    expect(lastLogged()).toEqual({ call: 'K9CCC', ex: [['RST', '57'], ['QTH', 'COOK']], mode: 'CW' })
  })

  it('Clear ends the contact: a report typed for it goes back to the mode’s own', async () => {
    engine.open = false
    render(<Windows snap={poll()} section="PH" logger={false} />)
    await settle()
    type(callIn('main'), 'K9FFF')
    type(rstIn('main'), '57')
    await settle()
    expect(rstIn('main').value, 'CONTROL: typed').toBe('57')
    fireEvent.click(inWindow('main').getByTitle('Clear the log fields'))
    await settle()
    expect(callIn('main').value, 'cleared').toBe('')
    expect(rstIn('main').value, 'the next contact').toBe('59')
  })

  it('CONTROL: with the logger window closed, the CW cockpit starts on 599 after a phone contact', async () => {
    engine.open = false
    const { rerender } = render(<Windows snap={poll()} section="PH" logger={false} />)
    await settle()
    type(callIn('main'), 'K9AAA')
    type(qthIn('main'), 'COOK')
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    await settle()
    rerender(<Windows snap={poll()} section="PH" logger={false} />)
    await settle()
    expect(rstIn('main').value, 'the next phone contact').toBe('59')
    rerender(<Windows snap={poll()} section="CW" logger={false} />)
    await settle()
    expect(rstIn('main').value, 'the CW cockpit').toBe('599')
  })
})
