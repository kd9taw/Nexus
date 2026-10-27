// @vitest-environment jsdom
//
// ALT+M IN THE CONTEST STRIP: mark the station's dial on the band map with the call in the entry, as
// N1MM's Mark (Alt+M) does (features/freqMarkers.ts). One press, one marker, with the call as typed
// (or none); the browser gets nothing of the key. Only while the strip's host is on screen, and never
// on the hosted Remote page. The strip also publishes its call for the band map's right-click.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { ENTRY_CALL_KEY } from '../features/freqMarkers'

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
  addFreqMarker: vi.fn(() => Promise.resolve([{ id: 1, dialHz: 14_025_000, band: '20M', call: 'K9AAA' }])),
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
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
/** A key on the window, as the browser delivers it; true when nothing held it. */
const press = async (init: KeyboardEventInit) => {
  let free = true
  await act(async () => {
    free = fireEvent.keyDown(window, init)
  })
  return free
}
const ALT_M = { key: 'm', code: 'KeyM', altKey: true }

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  localStorage.clear()
})

describe('Alt+M — mark this frequency', () => {
  it('marks the dial with the call in the entry, once per press, and holds the key from the browser', async () => {
    render(<Strip />)
    type(callBox(), 'k9aaa')
    expect(await press(ALT_M)).toBe(false)
    expect(api.addFreqMarker.mock.calls).toEqual([['K9AAA']])
    // Held down, it is still one press.
    await press({ ...ALT_M, repeat: true })
    expect(api.addFreqMarker).toHaveBeenCalledTimes(1)
  })

  it('marks with no call when the entry is empty', async () => {
    render(<Strip />)
    await press(ALT_M)
    expect(api.addFreqMarker.mock.calls).toEqual([['']])
  })

  it('is not Ctrl+Alt+M (AltGr), and does nothing while the strip is off screen or on Remote', async () => {
    render(<Strip />)
    await press({ ...ALT_M, ctrlKey: true })
    cleanup()
    render(<Strip active={false} />)
    await press(ALT_M)
    cleanup()
    render(<Strip remote />)
    await press(ALT_M)
    expect(api.addFreqMarker).not.toHaveBeenCalled()
  })

  it('publishes its call for the band map, and not while off screen', async () => {
    render(<Strip />)
    type(callBox(), 'w9xyz')
    expect(localStorage.getItem(ENTRY_CALL_KEY)).toBe('W9XYZ')
    cleanup()
    localStorage.clear()
    render(<Strip active={false} />)
    type(callBox(), 'k9zzz')
    expect(localStorage.getItem(ENTRY_CALL_KEY)).toBeNull()
  })
})
