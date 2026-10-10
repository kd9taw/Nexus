// @vitest-environment jsdom
//
// ⭐ ONE CONTACT IN PROGRESS, IN TWO WINDOWS — the main window's contest strip and the contest
// logger window's, while that window is open (`features/contestEntryShare`).
//
// The engine is the medium between the windows, so it is stood in for here by `engine` below:
// the shared entry, its rev, and the claim rule a log is checked against (the real one, and the
// two-threads race on it, are pinned in `tempo-app`'s `contest_entry_tests.rs`). A poll is a
// re-render with the snapshot the engine would hand each window. Window-level keys (Esc, Ctrl+D)
// are tested with one strip mounted at a time: in the app each strip has a window of its own.
//
//   - typed in either strip, it shows in the other at its next snapshot;
//   - every hint follows the shared contact in both: the dupe verdict, Super Check Partial, the
//     call-history mark, the take-back line;
//   - Enter in both at the same moment logs it once, and the strip that lost says so;
//   - a contact changed in the other window is not logged from a stale view;
//   - the logger's Esc clears the entry, the main window's Esc does not;
//   - a logger opened mid-contact shows that contact;
//   - with the logger window closed nothing is shared and the log commands get exactly what they
//     always got.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, within } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, ContestEntryShared, FieldDayQso, FieldDayStatus } from '../types'
import type { EntryClaim } from '../api'
import { ENTRY_CHANGED, ENTRY_LOGGED } from '../features/contestEntryShare'
import { t } from '../i18n'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestEntryPut: vi.fn(),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(() => Promise.resolve({ outcome: 'refused', refusal: 'changed' })),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 3 })),
  getScpCalls: vi.fn(() => Promise.resolve(['K9AAA', 'K9AAB', 'W9XYZ'])),
  getCallHistory: vi.fn(() =>
    Promise.resolve({ contest: 'ilqp', fileName: 'il-2026.txt', entries: { K9AAB: { Loc1: 'COOK' } } }),
  ),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
const toast = (await import('../toast')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** 2026-10-18 17:00Z. */
const T0 = 1_792_342_800

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'PH', submode: 'SSB', whenUnix, rcvd: ['59', qth] }) as FieldDayQso

/** An Illinois QSO Party session, as the engine serialises it. */
const party = (log: FieldDayQso[] = []): FieldDayStatus =>
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
      { key: 'RST', raw: '59' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    assistanceOn: ['Super Check Partial', 'Call history'],
  }) as unknown as FieldDayStatus

// ── the engine, as the two windows see it ───────────────────────────────────────────────

const engine = {
  open: true,
  entry: { rev: 1, call: '', fields: {}, marks: null } as ContestEntryShared,
  /** The rev the last log left the entry at, and the call it logged. */
  logged: null as { call: string; at: number } | null,
  rows: [] as string[],
  put(call: string, fields: Record<string, string>, marks: unknown): number {
    if (!this.open) throw new Error('contestEntryClosed')
    this.entry = { rev: this.entry.rev + 1, call, fields, marks }
    return this.entry.rev
  },
  /** The claim rule (`contest_entry.rs`): older than the last log, or not the entry as it stands. */
  log(call: string, claim: EntryClaim | undefined): void {
    if (claim && this.open) {
      if (this.logged && claim.rev < this.logged.at) throw this.logged.call === claim.call ? ENTRY_LOGGED : ENTRY_CHANGED
      if (claim.rev !== this.entry.rev && claim.call !== this.entry.call) throw ENTRY_CHANGED
    }
    this.rows.push(call)
    if (claim && this.open) {
      this.entry = { ...this.entry, rev: this.entry.rev + 1, call: '', marks: null }
      this.logged = { call: claim.call, at: this.entry.rev }
    }
  },
}

/** The snapshot a window polls: the session, and the shared entry while the logger is open. */
const poll = (fieldDay: FieldDayStatus): AppSnapshot =>
  ({
    radio: { band: '20m', dialMhz: 14.25 },
    hunt: null,
    fieldDay,
    ...(engine.open ? { contestEntry: { ...engine.entry } } : {}),
  }) as unknown as AppSnapshot

function strip(snap: AppSnapshot, role?: 'logger') {
  return (
    <LogEntry
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      titled={false}
      fieldDay={snap.fieldDay}
      fdMode="PH"
      sharedEntry={role}
    />
  )
}

/** Both windows, one above the other (Enter is the input's own key, so one document serves). */
function Both({ snap }: { snap: AppSnapshot }) {
  return (
    <>
      <div data-testid="main">{strip(snap)}</div>
      <div data-testid="logger">{strip(snap, 'logger')}</div>
    </>
  )
}

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const callIn = (w: string) => within(screen.getByTestId(w)).getByPlaceholderText('W1AW') as HTMLInputElement
const qthIn = (w: string) => within(screen.getByTestId(w)).getAllByRole('textbox')[2] as HTMLInputElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const toasts = () => toast.pushToast.mock.calls.map((c) => c[0] as string)

beforeEach(() => {
  engine.open = true
  engine.entry = { rev: 1, call: '', fields: {}, marks: null }
  engine.logged = null
  engine.rows = []
  api.contestEntryPut.mockImplementation((call: string, fields: Record<string, string>, marks: unknown) =>
    Promise.resolve(engine.put(call, fields, marks)),
  )
  api.contestLogManual.mockImplementation(
    (call: string, _ex: unknown, _m: unknown, _s: unknown, claim?: EntryClaim) =>
      new Promise((ok, refuse) => {
        try {
          engine.log(call, claim)
          ok({})
        } catch (e) {
          refuse(e)
        }
      }),
  )
})

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  for (const f of Object.values(toast)) f.mockClear()
})

describe('one contact in progress, in two windows', () => {
  it('shows what is typed in either strip in the other, as it is typed', async () => {
    const fd = party()
    const { rerender } = render(<Both snap={poll(fd)} />)
    await settle()
    type(callIn('logger'), 'K9A')
    await settle()
    expect(api.contestEntryPut).toHaveBeenLastCalledWith('K9A', expect.any(Object), expect.any(Object))
    rerender(<Both snap={poll(fd)} />)
    await settle()
    expect(callIn('main').value).toBe('K9A')
    // …and the other way: the operator finishes the call in the main window.
    type(callIn('main'), 'K9ABC')
    type(qthIn('main'), 'COOK')
    await settle()
    rerender(<Both snap={poll(fd)} />)
    await settle()
    expect(callIn('logger').value).toBe('K9ABC')
    expect(qthIn('logger').value).toBe('COOK')
  })

  it('every hint follows the shared contact in both: call history, dupe, Super Check Partial', async () => {
    const fd = party([row('K9AAA', 'COOK', T0)])
    const { rerender } = render(<Both snap={poll(fd)} />)
    await settle()
    // A call the history file holds: the logger's strip fills the box and marks it, and the
    // operator's shows the same box with the same mark.
    type(callIn('logger'), 'K9AAB')
    await settle()
    expect(qthIn('logger').value).toBe('COOK')
    expect(qthIn('logger').classList.contains('le-fd-input-history')).toBe(true)
    rerender(<Both snap={poll(fd)} />)
    await settle()
    expect(callIn('main').value).toBe('K9AAB')
    expect(qthIn('main').value).toBe('COOK')
    expect(qthIn('main').classList.contains('le-fd-input-history')).toBe(true)
    // The logger corrects the call to a station this log already has from Cook on this band and
    // mode (the box keeps Cook, now typed)…
    type(callIn('logger'), 'K9AAA')
    type(qthIn('logger'), 'COOK')
    await settle()
    rerender(<Both snap={poll(fd)} />)
    await settle()
    // …and the operator's own strip says it is a dupe, in the engine's own words.
    const dupe = t('logEntry.fd.dupe.own', { call: 'K9AAA', band: '20m', mode: 'PH' })
    for (const w of ['main', 'logger']) {
      expect(within(screen.getByTestId(w)).getByText(dupe), w).toBeTruthy()
      // Super Check Partial offers the same calls under both strips.
      const scp = within(screen.getByTestId(w)).getByRole('group', { name: t('logEntry.scp.label') })
      expect(within(scp).getByText('K9AAA'), w).toBeTruthy()
      // …and the history mark went with the fill in both.
      expect(qthIn(w).classList.contains('le-fd-input-history'), w).toBe(false)
    }
  })

  it('logs it ONCE when Enter is pressed in both windows at the same moment', async () => {
    const fd = party()
    const { rerender } = render(<Both snap={poll(fd)} />)
    await settle()
    type(callIn('logger'), 'K9ABC')
    type(qthIn('logger'), 'COOK')
    await settle()
    rerender(<Both snap={poll(fd)} />)
    await settle()
    expect(callIn('main').value).toBe('K9ABC')
    // Both press Enter before either hears back.
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    fireEvent.keyDown(callIn('logger'), { key: 'Enter' })
    await settle()
    await settle()
    expect(api.contestLogManual).toHaveBeenCalledTimes(2)
    const claims = api.contestLogManual.mock.calls.map((c) => c[4] as EntryClaim)
    expect(claims.map((c) => [c.rev, c.call])).toEqual([[claims[0].rev, 'K9ABC'], [claims[0].rev, 'K9ABC']])
    expect(engine.rows, 'one contact, one row').toEqual(['K9ABC'])
    // The strip that lost the race says why, and claims no log of its own.
    expect(toasts()).toContain(t('logEntry.shared.logged', { call: 'K9ABC' }))
    expect(toasts().filter((m) => m.startsWith(t('logEntry.fd.logged', { call: 'K9ABC', exchange: '59 COOK', mode: 'PH' })))).toHaveLength(1)
  })

  it('does not log a contact the other window changed since this one showed it', async () => {
    const fd = party()
    render(strip(poll(fd), 'logger'))
    await settle()
    const box = screen.getByPlaceholderText('W1AW') as HTMLInputElement
    type(box, 'K9ABC')
    type(screen.getAllByRole('textbox')[2] as HTMLInputElement, 'COOK')
    await settle()
    // The operator corrects the call in the main window; this window has not polled since.
    engine.put('K9ABD', {}, null)
    fireEvent.keyDown(box, { key: 'Enter' })
    await settle()
    await settle()
    expect(engine.rows).toEqual([])
    expect(toasts()).toContain(t('logEntry.shared.changed'))
    expect(box.value, 'nothing logged, so nothing cleared').toBe('K9ABC')
  })

  it('a logger opened mid-contact shows that contact, and never puts its own blank over it', async () => {
    engine.open = false
    const fd = party()
    const { rerender } = render(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    type(callIn('main'), 'K9ABC')
    await settle()
    expect(api.contestEntryPut, 'closed: nothing is shared').not.toHaveBeenCalled()
    // The logger window opens: a blank shared entry. The main strip puts what it holds…
    engine.open = true
    rerender(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    expect(engine.entry.call).toBe('K9ABC')
    // …and the logger's strip, mounting on it, takes it up and puts nothing.
    api.contestEntryPut.mockClear()
    cleanup()
    render(<div data-testid="logger">{strip(poll(fd), 'logger')}</div>)
    await settle()
    expect(callIn('logger').value).toBe('K9ABC')
    expect(api.contestEntryPut).not.toHaveBeenCalled()
  })

  it("a logger opened on a blank entry puts nothing of its own, not even its class guess", async () => {
    // Field Day: the strip's first guess at the class is this station's own (2A), so the logger's
    // strip is not blank when it mounts. Only the main window's strip may seed the shared entry,
    // or a logger opening at the moment the operator types would put its guess over the call.
    const fd = {
      ...party(),
      event: 'arrlfd',
      receives: [
        { key: 'CLASS', kind: 'pattern', required: true },
        { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
      ],
      composing: [
        { key: 'CLASS', raw: '2A' },
        { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
      ],
    } as unknown as FieldDayStatus
    render(<div data-testid="logger">{strip(poll(fd), 'logger')}</div>)
    await settle()
    expect((within(screen.getByTestId('logger')).getAllByRole('textbox')[1] as HTMLInputElement).value).toBe('2A')
    expect(api.contestEntryPut).not.toHaveBeenCalled()
    // Control: the main window's strip, mounting on the same blank entry with the same guess, seeds it.
    cleanup()
    render(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    expect(api.contestEntryPut).toHaveBeenCalledTimes(1)
  })

  it("the logger's Esc clears the entry; the main window's Esc is the cockpit's and clears nothing", async () => {
    const fd = party()
    render(<div data-testid="logger">{strip(poll(fd), 'logger')}</div>)
    await settle()
    type(callIn('logger'), 'K9ABC')
    await settle()
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(callIn('logger').value).toBe('')
    expect(api.contestEntryReset).toHaveBeenCalledTimes(1)
    expect(engine.entry.call, 'cleared in the shared entry too').toBe('')

    cleanup()
    api.contestEntryReset.mockClear()
    render(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    type(callIn('main'), 'K9ABD')
    await settle()
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(callIn('main').value).toBe('K9ABD')
    expect(api.contestEntryReset).not.toHaveBeenCalled()
    expect(engine.entry.call).toBe('K9ABD')
    // Nor does anything of the logger's take the main window's F-keys: they are its cockpit's.
    expect(fireEvent.keyDown(window, { key: 'F2', code: 'F2' }), 'F2 left to the main window').toBe(true)
  })

  it('the take-back line follows in both windows, and the second Ctrl+D may come from either', async () => {
    const fd = party([row('K9AAA', 'COOK', T0)])
    render(<div data-testid="logger">{strip(poll(fd), 'logger')}</div>)
    await settle()
    fireEvent.keyDown(window, { key: 'd', code: 'KeyD', ctrlKey: true })
    await settle()
    const armed = document.querySelector('.le-fd-remove-line')?.textContent ?? ''
    expect(armed).toContain('K9AAA')
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
    // The operator's window, at its next snapshot, shows the same question…
    cleanup()
    render(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    expect(document.querySelector('.le-fd-remove-line')?.textContent).toBe(armed)
    // …and answers it.
    fireEvent.keyDown(window, { key: 'd', code: 'KeyD', ctrlKey: true })
    await settle()
    expect(api.contestRemoveLast).toHaveBeenCalledTimes(1)
    expect(api.contestRemoveLast).toHaveBeenCalledWith('K9AAA', T0)
  })

  it('with the logger window closed, nothing is shared and the log gets what it always got', async () => {
    engine.open = false
    const fd = party()
    render(<div data-testid="main">{strip(poll(fd))}</div>)
    await settle()
    type(callIn('main'), 'K9ABC')
    type(within(screen.getByTestId('main')).getAllByRole('textbox')[2] as HTMLInputElement, 'COOK')
    await settle()
    fireEvent.keyDown(callIn('main'), { key: 'Enter' })
    await settle()
    expect(api.contestEntryPut).not.toHaveBeenCalled()
    expect(api.contestLogManual).toHaveBeenCalledWith('K9ABC', [['RST', '59'], ['QTH', 'COOK']], 'PH', undefined)
    expect(api.contestLogManual.mock.calls[0]).toHaveLength(4)
    expect(engine.rows).toEqual(['K9ABC'])
  })
})
