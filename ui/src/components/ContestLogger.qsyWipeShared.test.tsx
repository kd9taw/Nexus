// @vitest-environment jsdom
//
// TWO WINDOWS, ONE LINE, OVER A FAKE ENGINE ENTRY (a put replaces the entry and bumps its rev, as
// `contest_entry.rs` does): a wipe in one window must empty EVERY box of the shared line in both
// windows, so nothing of the wiped contact rides into the next call. A window that takes the entry
// up keeps the boxes the entry does not hold (`takeUp`), so a wipe puts each box empty rather than
// leaving it out.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, fireEvent } from '@testing-library/react'
import { useEffect, useState } from 'react'
import { ContestLargeEntry } from './ContestLogger'
import { setQsyWipe } from '../features/qsyWipe'
import type { AppSnapshot, FieldDayStatus } from '../types'

type E = { rev: number; contact: number; call: string; fields: Record<string, string>; marks: unknown }
const eng: { entry: E | undefined; subs: Set<() => void> } = { entry: undefined, subs: new Set() }
const bump = () => eng.subs.forEach((f) => f())

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    contestLogManualRows: vi.fn(async () => []),
    searchParks: vi.fn(async () => []),
    askLog: vi.fn(() => new Promise(() => {})),
    contestEntryPut: vi.fn(async (call: string, fields: Record<string, string>, marks: unknown, contact: number) => {
      if (!eng.entry) throw 'contestEntryClosed'
      if (contact < eng.entry.contact) throw 'contestEntryLogged'
      eng.entry = { ...eng.entry, rev: eng.entry.rev + 1, call, fields: { ...fields }, marks }
      setTimeout(bump, 0)
      return eng.entry.rev
    }),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const party = (log: unknown[] = []): FieldDayStatus =>
  ({
    running: true, state: 'Idle', qsoCount: log.length, sections: 0, points: 0, log,
    event: 'ilqp', role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [ { key: 'RST', raw: '599' }, { key: 'QTH', raw: 'KANE', domain: 'il_counties' } ],
  }) as unknown as FieldDayStatus

let dial = 14.025
let fd = party()
let loggerOpen = true
let mainOpen = true
const snapOf = (): AppSnapshot =>
  ({
    mycall: 'KD9TAW', mygrid: 'EN52',
    radio: { band: '20m', dialMhz: dial, sideband: 'CW', operatingMode: 'cw', rigKeyed: false },
    link: { tier: 'CW' }, hunt: null, bandChangeTick: 1,
    ...(eng.entry ? { contestEntry: { ...eng.entry } } : {}),
  }) as unknown as AppSnapshot

const win = (logger: boolean) => (
  <div data-w={logger ? 'logger' : 'main'}>
    <ContestLargeEntry snap={snapOf()} fieldDay={fd} fdActive fdRuleset={null} title="Contest"
      sharedEntry={logger ? 'logger' : undefined} operator="" onSaveOperator={() => {}}
      satellite={null} dopplerTracking={false} entryOnly />
  </div>
)
function Harness() {
  const [, set] = useState(0)
  useEffect(() => { const f = () => set((n) => n + 1); eng.subs.add(f); return () => { eng.subs.delete(f) } }, [])
  return <>{mainOpen && win(false)}{loggerOpen && win(true)}</>
}
const q = (w: string, sel: string) => document.querySelector<HTMLInputElement>(`[data-w="${w}"] ${sel}`)
const call = (w: string) => q(w, 'input.le-fd-input-call')?.value
const boxes = (w: string) => [...document.querySelectorAll<HTMLInputElement>(`[data-w="${w}"] input.le-fd-input-code`)].map((b) => b.value)
const notices = () => document.querySelectorAll('.le-band-cleared').length
const wait = (ms: number) => act(async () => { vi.advanceTimersByTime(ms); await Promise.resolve(); await Promise.resolve() })
const settle = async () => { for (let i = 0; i < 6; i++) await wait(10) }
const typeCall = async (w: string, v: string) => { fireEvent.change(q(w, 'input.le-fd-input-call')!, { target: { value: v } }); await settle() }
const typeBox = async (w: string, i: number, v: string) => {
  const b = document.querySelectorAll<HTMLInputElement>(`[data-w="${w}"] input.le-fd-input-code`)[i]
  fireEvent.change(b, { target: { value: v } }); await settle()
}
let view: ReturnType<typeof render>
const rr = async () => { view.rerender(<Harness />); await settle() }
const qsy = async (to: number) => { dial = to; await rr(); await wait(2600); await settle() }

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
  setQsyWipe(true)
  dial = 14.025; fd = party(); loggerOpen = true; mainOpen = true
  eng.entry = { rev: 10, contact: 0, call: '', fields: {}, marks: null }
})
afterEach(() => { view?.unmount(); setQsyWipe(false); vi.useRealTimers() })

describe('a wipe of the shared line empties every box in both windows', () => {
  it('the QSY wipe: QTH COOK never rides into the next call', async () => {
    view = render(<Harness />); await settle()
    await typeCall('main', 'K9AAA')
    await typeBox('main', 1, 'COOK')
    expect(boxes('logger')[1]).toBe('COOK')
    await qsy(14.03)
    expect(notices()).toBe(1)
    expect([call('main'), boxes('main')[1]], 'the main window\'s copy is wiped too').toEqual(['', ''])
    expect([call('logger'), boxes('logger')[1]]).toEqual(['', ''])
    await typeCall('main', 'W1BBB')
    expect(boxes('main')[1]).toBe('')
    expect(boxes('logger')[1]).toBe('')
  })

  it('Alt+W and Ctrl+W put every box empty to the engine, so the other window\'s copy empties too', async () => {
    // One window on screen: a keydown reaches every strip in a test's one document, and the
    // claim is about what the wipe sends, which the other window merges over its own boxes.
    mainOpen = false
    for (const key of [{ altKey: true }, { ctrlKey: true }]) {
      view = render(<Harness />); await settle()
      await typeCall('logger', 'K9AAA')
      await typeBox('logger', 1, 'COOK')
      expect(eng.entry?.fields.QTH).toBe('COOK')
      await act(async () => { fireEvent.keyDown(window, { key: 'w', code: 'KeyW', ...key }) })
      await settle()
      expect(eng.entry?.call, JSON.stringify(key)).toBe('')
      expect(eng.entry?.fields.QTH, `${JSON.stringify(key)}: the box goes as empty, not left out`).toBe('')
      view.unmount()
      eng.entry = { rev: 10, contact: 0, call: '', fields: {}, marks: null }
    }
  })
})
