// @vitest-environment jsdom
//
// A LOGGED CONTACT'S BOXES END WITH IT, WITH THE CONTEST LOGGER WINDOW OPEN. The main window's strip and
// the logger's show one contact in progress (`features/contestEntryShare`). When it is logged, each
// strip's after-log clear starts the next contact (the county box empty, the report the mode's own), and
// the shared entry still holds the logged contact's boxes until a window puts the next contact's
// (`contest_entry.rs`, `consume`). Whatever order the two land in, the next contact never starts with
// the logged contact's county, in either window or in the shared entry, so a plain Enter can never log
// it again.
//
// The engine is stood in for by `engine` below, as in LogEntry.sharedEntry.test.tsx: a put replaces the
// entry (one made on a contact already logged is refused), a log moves it on to the next contact (the
// call and the marks go, the boxes stay) and checks the claim as the engine does. Each window polls on its own: a poll is a re-render of THAT window with the snapshot the
// engine would hand it, and when it lands is chosen here, by the test. `engine.atLog` is the snapshot a
// window would get if its poll was built at the moment of the log, before any window put again.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, within } from '@testing-library/react'
import type { ReactElement } from 'react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, ContestEntryShared, FieldDayQso, FieldDayStatus } from '../types'
import type { EntryClaim } from '../api'
import { ENTRY_CHANGED, ENTRY_LOGGED } from '../features/contestEntryShare'

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
type Win = 'main' | 'logger'

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const ilqp = (log: FieldDayQso[]): FieldDayStatus =>
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

/** ARRL Field Day, class 2A from Illinois: a row carries the class and section it received. */
const fieldDay = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    ...ilqp(log),
    event: 'arrlfd',
    role: undefined,
    receives: [
      { key: 'CLASS', kind: 'pattern', required: true },
      { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
    ],
    composing: [
      { key: 'CLASS', raw: '2A' },
      { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
    ],
    composingText: '2A IL',
    composingTextPhone: '2A IL',
  }) as unknown as FieldDayStatus

// ── the engine, as the windows see it ──────────────────────────────────────────────────────────

const upper = (o: Record<string, string>) =>
  JSON.stringify(Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k].trim().toUpperCase()])))

const engine = {
  open: true,
  session: ilqp as (log: FieldDayQso[]) => FieldDayStatus,
  entry: { rev: 1, contact: 0, call: '', fields: {}, marks: null } as ContestEntryShared,
  /** The last contact logged from the entry, as its claim showed it, and the rev that log left. */
  logged: null as { call: string; fields: string; at: number } | null,
  log: [] as FieldDayQso[],
  /** What each log wrote, by value: the call, the received vector and the mode class. */
  rows: [] as { call: string; ex: [string, string][]; mode: string }[],
  /** The snapshot a poll built at the moment of the last log, before any window put again. */
  atLog: null as AppSnapshot | null,
  put(call: string, fields: Record<string, string>, marks: unknown, contact: number): number {
    if (!this.open) throw new Error('contestEntryClosed')
    if (contact < this.entry.contact) throw ENTRY_LOGGED
    this.entry = { rev: this.entry.rev + 1, contact: this.entry.contact, call, fields, marks }
    return this.entry.rev
  },
  /** `Engine::contest_log_entry`: the claim is checked (`admits`), the row written, the entry moved
   *  on (`consume`): the call and the marks go, the boxes stay. */
  write(call: string, ex: [string, string][], mode: string, claim?: EntryClaim): void {
    if (claim && this.open) {
      const shown = [claim.call.trim().toUpperCase(), upper(claim.fields)].join('|')
      if (this.logged && claim.rev < this.logged.at) {
        throw shown === [this.logged.call, this.logged.fields].join('|') ? ENTRY_LOGGED : ENTRY_CHANGED
      }
      const held = [this.entry.call.trim().toUpperCase(), upper(this.entry.fields)].join('|')
      if (claim.rev !== this.entry.rev && held !== shown) throw ENTRY_CHANGED
    }
    const value = (k: string) => ex.find(([key]) => key === k)?.[1] ?? ''
    this.rows.push({ call, ex, mode })
    this.log = [
      ...this.log,
      {
        call,
        class: value('CLASS'),
        section: value('SECTION'),
        band: '40m',
        mode,
        submode: mode === 'PH' ? 'SSB' : '',
        whenUnix: T0 + this.log.length * 60,
        rcvd: ex.map(([, v]) => v),
      } as FieldDayQso,
    ]
    if (claim && this.open) {
      this.entry = { ...this.entry, rev: this.entry.rev + 1, contact: this.entry.contact + 1, call: '', marks: null }
      this.logged = { call: claim.call.trim().toUpperCase(), fields: upper(claim.fields), at: this.entry.rev }
    }
    this.atLog = snapshot()
  },
  /** A county line (`contest_log_manual_rows`): one claim, a row per county, the entry moved on once. */
  writeRows(call: string, rows: [string, string][][], mode: string, claim?: EntryClaim): boolean[] {
    rows.forEach((ex, i) => this.write(call, ex, mode, i === 0 ? claim : undefined))
    return rows.map(() => true)
  },
}

/** The snapshot a window polls: the session, and the shared entry while the logger is open. */
function snapshot(): AppSnapshot {
  return {
    radio: { band: '40m', dialMhz: 7.04 },
    hunt: null,
    fieldDay: engine.session(engine.log),
    ...(engine.open ? { contestEntry: { ...engine.entry } } : {}),
  } as unknown as AppSnapshot
}

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

/** The main window's cockpit and the logger window, each on the snapshot it last polled. The main
 *  strip is KEYED by its cockpit, so a move from Phone to CW mounts the CW cockpit's strip, as the app
 *  does. */
function Windows({ main, logger, section }: { main: AppSnapshot; logger: AppSnapshot | null; section: ModeClass }) {
  return (
    <>
      <div data-testid="main" key={section}>
        {strip(main, section)}
      </div>
      {logger && <div data-testid="logger">{strip(logger, section, 'logger')}</div>}
    </>
  )
}

let view: { main: AppSnapshot; logger: AppSnapshot | null; section: ModeClass }
let rerender: (ui: ReactElement) => void = () => {}
function open(withLogger = true) {
  const s = snapshot()
  view = { main: s, logger: withLogger ? s : null, section: 'PH' }
  rerender = render(<Windows {...view} />).rerender
}
/** A poll lands in these windows: each re-renders on the snapshot the engine hands it now. */
function poll(...windows: Win[]) {
  const s = snapshot()
  for (const w of windows) view = { ...view, [w]: s }
  rerender(<Windows {...view} />)
}
/** A snapshot built earlier lands in this window now. */
function deliver(w: Win, s: AppSnapshot) {
  view = { ...view, [w]: s }
  rerender(<Windows {...view} />)
}

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
/** A few polls of both windows, so each has taken up whatever the other put. */
async function pollsSettle() {
  for (let i = 0; i < 3; i++) {
    poll('main', 'logger')
    await settle()
  }
}
const inWindow = (w: Win) => within(screen.getByTestId(w))
const callIn = (w: Win) => inWindow(w).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const rstIn = (w: Win) => inWindow(w).getByLabelText('RST') as HTMLInputElement
const qthIn = (w: Win) => inWindow(w).getByLabelText('County / State') as HTMLInputElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const enter = (w: Win) => fireEvent.keyDown(callIn(w), { key: 'Enter' })
const loggedAs = (call: string) => engine.rows.filter((r) => r.call === call).map((r) => r.ex)

/** K9BBB worked from Lake County with a typed 57, shown in both windows. */
async function aContactInProgress(w: Win = 'main') {
  open()
  await settle()
  type(callIn(w), 'K9BBB')
  type(rstIn(w), '57')
  type(qthIn(w), 'LAKE')
  await settle()
  poll('main', 'logger')
  await settle()
  expect(qthIn('logger').value, 'CONTROL: the contact in progress, logger window').toBe('LAKE')
  expect(qthIn('main').value, 'CONTROL: the contact in progress, main window').toBe('LAKE')
}

/** Both windows and the shared entry start the next contact: no county, the mode's own report. */
function expectTheNextContact(when: string) {
  expect(qthIn('main').value, `${when}: the next contact's county, main window`).toBe('')
  expect(qthIn('logger').value, `${when}: the next contact's county, logger window`).toBe('')
  expect(engine.entry.fields.QTH ?? '', `${when}: the next contact's county, shared entry`).toBe('')
  expect(rstIn('main').value, `${when}: the next contact's report, main window`).toBe('59')
  expect(rstIn('logger').value, `${when}: the next contact's report, logger window`).toBe('59')
}

beforeEach(() => {
  engine.open = true
  engine.session = ilqp
  engine.entry = { rev: 1, contact: 0, call: '', fields: {}, marks: null }
  engine.logged = null
  engine.log = []
  engine.rows = []
  engine.atLog = null
  api.contestEntryPut.mockImplementation(
    (call: string, fields: Record<string, string>, marks: unknown, contact: number) =>
      Promise.resolve(engine.put(call, fields, marks, contact)),
  )
  api.contestLogManual.mockImplementation(
    (call: string, ex: [string, string][], mode: string, _sub: unknown, claim?: EntryClaim) =>
      new Promise((ok, refuse) => {
        try {
          engine.write(call, ex, mode, claim)
          ok({})
        } catch (e) {
          refuse(e)
        }
      }),
  )
  api.contestLogManualRows.mockImplementation(
    (call: string, rows: [string, string][][], mode: string, _sub: unknown, claim?: EntryClaim) =>
      new Promise((ok, refuse) => {
        try {
          ok(engine.writeRows(call, rows, mode, claim))
        } catch (e) {
          refuse(e)
        }
      }),
  )
})

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('a logged contact’s county never carries into the next contact, with the logger window open', () => {
  it('the snapshot carrying the contact lands in the same render as the main window’s own clear', async () => {
    await aContactInProgress()
    enter('main')
    // The log answers, and the poll carrying the contact lands in that same render, in both windows.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    await pollsSettle()
    expectTheNextContact('after the polls')
    // The logger starts the next call; a plain Enter in the main window must not log Lake for it.
    type(callIn('logger'), 'K9CCC')
    await settle()
    await pollsSettle()
    expect(callIn('main').value, 'CONTROL: the next call reached the main window').toBe('K9CCC')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'a plain Enter on the next call').toEqual([])
  })

  it('the same, with Enter in the logger window', async () => {
    await aContactInProgress('logger')
    enter('logger')
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('the logger window polls once the main window has put its own clear, before the main window polls', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    poll('logger')
    await settle()
    expect(qthIn('logger').value, 'the logger window, at its first snapshot after the contact').toBe('')
    expect(rstIn('logger').value, 'the logger window’s report, then').toBe('59')
    // The logger operator starts the next call before the main window's next poll.
    type(callIn('logger'), 'K9DDD')
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
    expect(callIn('main').value, 'CONTROL: the next call reached the main window').toBe('K9DDD')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9DDD'), 'a plain Enter on the next call').toEqual([])
  })

  it('the logger window’s poll was built at the moment of the log, and lands after the main window’s own put', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    const atLog = engine.atLog!
    expect(atLog.contestEntry?.fields.QTH, 'CONTROL: the entry the engine kept at the log').toBe('LAKE')
    deliver('logger', atLog)
    await settle()
    expect(qthIn('logger').value, 'the logger window, on the snapshot of the moment of the log').toBe('')
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('the operator moves to the CW cockpit as the contact is logged: its new strip starts the next contact', async () => {
    await aContactInProgress()
    enter('main')
    // The Phone cockpit's strip goes with the log in flight, and the CW cockpit's mounts on the poll
    // carrying the contact; the logger window follows the section on the same poll.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      view = { ...view, section: 'CW' }
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    expect(qthIn('main').value, 'the CW cockpit’s strip, as it mounts').toBe('')
    await pollsSettle()
    expect(qthIn('main').value, 'the CW cockpit’s strip').toBe('')
    expect(qthIn('logger').value, 'the logger window, on CW').toBe('')
    expect(engine.entry.fields.QTH ?? '', 'the shared entry').toBe('')
    expect(rstIn('main').value, 'the CW report').toBe('599')
  })

  it('CONTROL: a county typed in the other window for the NEXT contact, landing with this window’s clear, is kept', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // The logger window sees the contact first; its operator types the next station's call and county.
    poll('logger')
    await settle()
    type(callIn('logger'), 'K9DDD')
    type(qthIn('logger'), 'COOK')
    await settle()
    // The main window's first snapshot after the contact carries them.
    poll('main')
    await settle()
    expect(callIn('main').value, 'the next call, main window').toBe('K9DDD')
    expect(qthIn('main').value, 'the next county, main window').toBe('COOK')
    await pollsSettle()
    expect(qthIn('main').value, 'the next county, main window, after the polls').toBe('COOK')
    expect(qthIn('logger').value, 'the next county, logger window').toBe('COOK')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9DDD'), 'the next contact, as typed for it').toEqual([[['RST', '59'], ['QTH', 'COOK']]])
  })

  it('CONTROL: two people at one moment, Esc in the logger window as the main window logs: the county ends in both once the logger window sees the contact', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // The logger window has not polled since the contact went in. Its Esc clears the line it still
    // shows and puts it with the logged contact's county (Clear keeps the boxes): a put made on a
    // contact already logged, which the engine refuses, so the entry stays the next contact.
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(callIn('logger').value, 'CONTROL: the logger window cleared its line').toBe('')
    expect(engine.entry.fields.QTH, 'the logger window’s Esc, refused: the next contact’s county').toBe('')
    // Each window then sees the contact, and the county ends in both.
    poll('main')
    await settle()
    poll('logger')
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('CONTROL: a contact with no race clears as before, in both windows', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('CONTROL: typed in the logger window while the main window is idle, it shows in both', async () => {
    open()
    await settle()
    type(callIn('logger'), 'K9EEE')
    type(qthIn('logger'), 'DUPA')
    await settle()
    poll('main')
    await settle()
    expect(callIn('main').value).toBe('K9EEE')
    expect(qthIn('main').value).toBe('DUPA')
  })

  it('CONTROL: with the logger window closed, a contact clears as before', async () => {
    engine.open = false
    open(false)
    await settle()
    type(callIn('main'), 'K9BBB')
    type(rstIn('main'), '57')
    type(qthIn('main'), 'LAKE')
    enter('main')
    await settle()
    poll('main')
    await settle()
    expect(loggedAs('K9BBB')).toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    expect(callIn('main').value, 'the call').toBe('')
    expect(qthIn('main').value, 'the county').toBe('')
    expect(rstIn('main').value, 'the report').toBe('59')
    expect(api.contestEntryPut, 'nothing shared').not.toHaveBeenCalled()
  })

  it('CONTROL: Field Day keeps the class and section it logged for the next contact, in both windows', async () => {
    engine.session = fieldDay
    open()
    await settle()
    const box = (w: Win, i: number) => inWindow(w).getAllByRole('textbox')[i] as HTMLInputElement
    type(callIn('main'), 'K9FFF')
    type(box('main', 1), '3A')
    type(box('main', 2), 'WI')
    await settle()
    poll('main', 'logger')
    await settle()
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9FFF'), 'logged').toEqual([[['CLASS', '3A'], ['SECTION', 'WI']]])
    // As the log lands, before any snapshot shows it: the class and section stay.
    expect(callIn('main').value, 'the call, as the log lands').toBe('')
    expect(box('main', 1).value, 'the class, as the log lands').toBe('3A')
    expect(box('main', 2).value, 'the section, as the log lands').toBe('WI')
    expect(engine.entry.fields, 'what the main window put').toMatchObject({ CLASS: '3A', SECTION: 'WI' })
    // The logger window's snapshot, built at the moment of the log: the entry the log left.
    deliver('logger', engine.atLog!)
    await settle()
    await pollsSettle()
    for (const w of ['main', 'logger'] as const) {
      expect(callIn(w).value, `${w}: the call`).toBe('')
      expect(box(w, 1).value, `${w}: the class carried`).toBe('3A')
      expect(box(w, 2).value, `${w}: the section carried`).toBe('WI')
    }
  })
})

// TWO PEOPLE AT ONE MOMENT. A window that changes the line before it shows the log is changing a
// contact already logged: the engine refuses that change, so the logged call never comes back to
// both windows, where a county and Enter would log it a second time.
describe('a change made in one window before it shows the log never brings the logged contact back', () => {
  it('an edit in the logger window as the main window logs: the shared entry stays the next contact', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // The logger window has not polled since the contact went in; its operator corrects the county.
    type(qthIn('logger'), 'COOK')
    await settle()
    expect(engine.entry.call, 'the shared entry’s call, after the logger window’s edit').toBe('')
    expect(engine.entry.fields.QTH ?? '', 'the shared entry’s county, after the logger window’s edit').toBe('')
    poll('main')
    await settle()
    expect(callIn('main').value, 'the main window, at its next snapshot').toBe('')
    poll('logger')
    await settle()
    expect(callIn('logger').value, 'the logger window, at its first snapshot after the contact').toBe('')
    await pollsSettle()
    expect(callIn('main').value, 'the main window, after the polls').toBe('')
    expect(callIn('logger').value, 'the logger window, after the polls').toBe('')
    expectTheNextContact('after the polls')
  })

  it('the same edit, then a county and Enter in the main window: no second row for the logged call', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    type(qthIn('logger'), 'COOK')
    await settle()
    poll('main')
    await settle()
    poll('logger')
    await settle()
    await pollsSettle()
    type(qthIn('main'), 'DUPA')
    await settle()
    expect(qthIn('main').value, 'CONTROL: the county typed in the main window').toBe('DUPA')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'the one row K9BBB was logged as').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
  })

  it('the logger window’s first snapshot after the contact lands while its refused edit is on its way: it shows the next contact', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    type(qthIn('logger'), 'COOK')
    // Before that edit is answered.
    poll('logger')
    await settle()
    await settle()
    expect(callIn('logger').value, 'the logger window, once its edit is answered').toBe('')
    expect(qthIn('logger').value, 'the logger window’s county, then').toBe('')
    expect(engine.entry.call, 'the shared entry').toBe('')
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('Enter in the logger window, an edit in the main window at that moment: the same', async () => {
    await aContactInProgress('logger')
    enter('logger')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    // The main window has not polled since the contact went in; its operator corrects the county.
    type(qthIn('main'), 'COOK')
    await settle()
    expect(engine.entry.call, 'the shared entry’s call, after the main window’s edit').toBe('')
    poll('logger')
    await settle()
    poll('main')
    await settle()
    await pollsSettle()
    expect(callIn('main').value, 'the main window, after the polls').toBe('')
    expect(callIn('logger').value, 'the logger window, after the polls').toBe('')
    expectTheNextContact('after the polls')
  })

  it('CONTROL: the main window’s own typing straight after its Enter lands, reaches the logger window and logs', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // No snapshot has shown the contact yet: the main operator types the next call at once.
    type(callIn('main'), 'K9CCC')
    await settle()
    expect(engine.entry.call, 'the shared entry, as the main window typed it').toBe('K9CCC')
    await pollsSettle()
    expect(callIn('logger').value, 'the next call, logger window').toBe('K9CCC')
    type(qthIn('main'), 'COOK')
    await settle()
    await pollsSettle()
    for (const w of ['main', 'logger'] as const) {
      expect(callIn(w).value, `${w}: the next call`).toBe('K9CCC')
      expect(qthIn(w).value, `${w}: the next county`).toBe('COOK')
    }
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'the next contact, as typed').toEqual([[['RST', '59'], ['QTH', 'COOK']]])
  })

  it('CONTROL: the same after a county-line contact, logged as one row per county', async () => {
    open()
    await settle()
    type(callIn('main'), 'K9LLL')
    type(qthIn('main'), 'COOK/DUPG')
    await settle()
    poll('main', 'logger')
    await settle()
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9LLL'), 'CONTROL: one row per county').toEqual([
      [['RST', '59'], ['QTH', 'COOK']],
      [['RST', '59'], ['QTH', 'DUPG']],
    ])
    type(callIn('main'), 'K9CCC')
    await settle()
    expect(engine.entry.call, 'the shared entry, as the main window typed it').toBe('K9CCC')
    await pollsSettle()
    expect(callIn('logger').value, 'the next call, logger window').toBe('K9CCC')
  })
})

// THE WINDOW THAT LOGGED CLEARS ONCE. Its after-log clear runs as its log lands; the snapshot that
// first shows the contact there clears nothing more, so the next contact typed (or filled from call
// history) in between stays. A window that did not log clears when it first sees the contact, once.
describe('the next contact typed in the window that logged, before its next snapshot, stays', () => {
  /** What a window's strip shows: the call, the report and the county. */
  const shown = (w: Win) => ({ call: callIn(w).value, rst: rstIn(w).value, qth: qthIn(w).value })
  const theNext = { call: 'K9CCC', rst: '55', qth: 'COOK' }

  it('logger window closed: the next call, report and county typed straight after Enter stay, and log', async () => {
    engine.open = false
    open(false)
    await settle()
    type(callIn('main'), 'K9BBB')
    type(rstIn('main'), '57')
    type(qthIn('main'), 'LAKE')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'CONTROL: logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    // No snapshot has shown the contact yet: the operator types the next one.
    type(callIn('main'), 'K9CCC')
    type(rstIn('main'), '55')
    type(qthIn('main'), 'COOK')
    await settle()
    poll('main')
    await settle()
    expect(shown('main'), 'the next contact, after the snapshot').toEqual({ call: 'K9CCC', rst: '55', qth: 'COOK' })
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'the next contact, as typed for it').toEqual([[['RST', '55'], ['QTH', 'COOK']]])
  })

  /** K9BBB logged with Enter in `w`, then the next contact typed there before `w` polls. */
  async function theNextContactTypedIn(w: Win) {
    await aContactInProgress(w)
    enter(w)
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'CONTROL: logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    type(callIn(w), 'K9CCC')
    type(rstIn(w), '55')
    type(qthIn(w), 'COOK')
    await settle()
    poll(w)
    await settle()
    expect(shown(w), `${w}: the next contact, at its first snapshot after the contact`).toEqual(theNext)
    await pollsSettle()
    for (const v of ['main', 'logger'] as const) {
      expect(shown(v), `${v}: the next contact, after the polls`).toEqual(theNext)
    }
    enter(w)
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'the next contact, as typed for it').toEqual([[['RST', '55'], ['QTH', 'COOK']]])
  }

  it('logger window open, Enter in the main window: the same, and the logger window shows them', async () => {
    await theNextContactTypedIn('main')
  })

  it('logger window open, Enter in the logger window: the same, and the main window shows them', async () => {
    await theNextContactTypedIn('logger')
  })

  it('logger window closed: the county call history fills for the next call stays, marked, and logs', async () => {
    engine.session = (log) => ({ ...ilqp(log), assistanceOn: ['Call history'] }) as FieldDayStatus
    api.getCallHistory.mockResolvedValueOnce({
      contest: 'ilqp',
      fileName: 'il-2026.txt',
      entries: { K9CCC: { Loc1: 'COOK' } },
    })
    engine.open = false
    open(false)
    await settle()
    // A filled box's label also says where it came from ("history").
    const qth = () => inWindow('main').getByLabelText(/County \/ State/) as HTMLInputElement
    const filled = () => ({ qth: qth().value, marked: qth().classList.contains('le-fd-input-history') })
    type(callIn('main'), 'K9BBB')
    type(qth(), 'LAKE')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'CONTROL: logged as typed').toEqual([[['RST', '59'], ['QTH', 'LAKE']]])
    type(callIn('main'), 'K9CCC')
    await settle()
    expect(filled(), 'CONTROL: call history filled the next county').toEqual({ qth: 'COOK', marked: true })
    poll('main')
    await settle()
    expect(filled(), 'the filled county, after the snapshot').toEqual({ qth: 'COOK', marked: true })
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'the next contact, with the filled county').toEqual([[['RST', '59'], ['QTH', 'COOK']]])
  })

  it('logger window closed: after a county-line contact, the next call and county typed before the snapshot stay', async () => {
    engine.open = false
    open(false)
    await settle()
    type(callIn('main'), 'K9LLL')
    type(qthIn('main'), 'COOK/DUPG')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9LLL'), 'CONTROL: one row per county').toEqual([
      [['RST', '59'], ['QTH', 'COOK']],
      [['RST', '59'], ['QTH', 'DUPG']],
    ])
    type(callIn('main'), 'K9CCC')
    type(qthIn('main'), 'LAKE')
    await settle()
    poll('main')
    await settle()
    expect(shown('main'), 'the next contact, after the snapshot').toEqual({ call: 'K9CCC', rst: '59', qth: 'LAKE' })
  })

  it('CONTROL: the logger window, which did not log, clears at its first snapshot showing the contact, and only then', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    poll('logger')
    await settle()
    expect(shown('logger'), 'the logger window, at its first snapshot after the contact').toEqual({ call: '', rst: '59', qth: '' })
    // Its operator types the next contact; its next snapshot shows the same log.
    type(callIn('logger'), 'K9DDD')
    type(rstIn('logger'), '55')
    type(qthIn('logger'), 'DUPA')
    await settle()
    poll('logger')
    await settle()
    const typed = { call: 'K9DDD', rst: '55', qth: 'DUPA' }
    expect(shown('logger'), 'the logger window, at its next snapshot').toEqual(typed)
    await pollsSettle()
    expect(shown('main'), 'the main window, after the polls').toEqual(typed)
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9DDD'), 'the next contact, as typed for it').toEqual([[['RST', '55'], ['QTH', 'DUPA']]])
  })

  it('CONTROL: a snapshot shows the contact before its log answers; a contact logged elsewhere after it still starts the next one', async () => {
    engine.session = fieldDay
    engine.open = false
    open(false)
    await settle()
    const box = (i: number) => inWindow('main').getAllByRole('textbox')[i] as HTMLInputElement
    // The contact is in the log, and the answer waits on the journal: the snapshot shows it first.
    let answer: (r: unknown) => void = () => {}
    api.contestLogManual.mockImplementationOnce((call: string, ex: [string, string][], mode: string) => {
      engine.write(call, ex, mode)
      return new Promise((ok) => {
        answer = ok
      })
    })
    type(callIn('main'), 'K9FFF')
    type(box(1), '3A')
    type(box(2), 'WI')
    enter('main')
    await settle()
    expect(loggedAs('K9FFF'), 'CONTROL: logged').toEqual([[['CLASS', '3A'], ['SECTION', 'WI']]])
    poll('main')
    await settle()
    await act(async () => answer({}))
    await settle()
    expect(callIn('main').value, 'CONTROL: the call, once the log answers').toBe('')
    // The FT sequencer logs a contact: class 2B from Connecticut.
    engine.write('W1AW', [['CLASS', '2B'], ['SECTION', 'CT']], 'DIG')
    poll('main')
    await settle()
    expect({ class: box(1).value, section: box(2).value }, 'what the next contact starts with').toEqual({
      class: '2B',
      section: 'CT',
    })
  })

  it('CONTROL: an Enter the engine refuses (a dupe) clears for no contact: one logged elsewhere after it still starts the next one', async () => {
    engine.session = fieldDay
    engine.open = false
    open(false)
    await settle()
    const box = (i: number) => inWindow('main').getAllByRole('textbox')[i] as HTMLInputElement
    api.contestLogManual.mockImplementationOnce(() => Promise.reject('K9FFF is a dupe on this band/mode'))
    type(callIn('main'), 'K9FFF')
    type(box(1), '3A')
    type(box(2), 'WI')
    enter('main')
    await settle()
    await settle()
    expect(callIn('main').value, 'CONTROL: the refused contact stays on screen').toBe('K9FFF')
    expect(engine.rows, 'CONTROL: nothing logged').toEqual([])
    // The operator abandons it, and the FT sequencer logs a contact: class 2B from Connecticut.
    type(callIn('main'), '')
    engine.write('W1AW', [['CLASS', '2B'], ['SECTION', 'CT']], 'DIG')
    poll('main')
    await settle()
    expect({ class: box(1).value, section: box(2).value }, 'what the next contact starts with').toEqual({
      class: '2B',
      section: 'CT',
    })
  })
})
