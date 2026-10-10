// @vitest-environment jsdom
//
// ⭐ THE CONTEST SCREEN'S HALF OF "REMOVE THE NEWEST CONTACT": Remove on the newest row (and only
// there), asked before it acts, and the Removed list with Restore. Nothing is deleted — the
// engine keeps a removed contact and the list reads it back. Never offered when observing from
// afar.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'
import type { RemovedContest } from '../api'
import { StreamInputDispatcher } from '../remote-native/stream-input'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  contestRemoved: vi.fn(async () => []),
  contestRemoveLast: vi.fn(),
  contestRestore: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

const party = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    rulesYear: 2026,
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

const LOG = [row('K9AAA', 'COOK', T0), row('W9BBB', 'LAKE', T0 + 60)]
const KEPT: RemovedContest = { id: 7, removedUnix: T0 + 150, rows: [row('K9OLD', 'WILL', T0 + 30)] }

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

async function show(log: FieldDayQso[], observed = false) {
  render(
    <ContestView
      fieldDay={party(log)}
      onSetMode={() => {}}
      fdActive
      tier="FT8"
      observation={observed ? { fdOperator: '', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] } : undefined}
    />,
  )
  await settle()
}

const removeButtons = () => [...document.querySelectorAll<HTMLButtonElement>('.fd-log .fd-remove')]
const note = () => document.querySelector('.fd-removed-note')?.textContent ?? null

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('Remove sits on the newest row and asks first', () => {
  it('is on the newest row alone', async () => {
    await show(LOG)
    expect(removeButtons()).toHaveLength(1)
    expect(removeButtons()[0].closest('.fd-log-row')?.textContent).toContain('W9BBB')
  })

  it('names the contact on the first click, and removes it on the second', async () => {
    api.contestRemoveLast.mockResolvedValueOnce({
      outcome: 'removed',
      entry: { id: 2, removedUnix: T0 + 90, rows: [LOG[1]] },
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    await show(LOG)
    fireEvent.click(removeButtons()[0])
    expect(removeButtons()[0].getAttribute('aria-pressed')).toBe('true')
    expect(note()).toBe('Remove W9BBB · 20m CW · 17:01 · 599 LAKE? Click Remove again.')
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(removeButtons()[0])
    })
    await settle()
    expect(api.contestRemoveLast).toHaveBeenCalledWith('W9BBB', T0 + 60)
    expect(note()).toBe(
      'Removed W9BBB · 20m CW · 17:01 · 599 LAKE from the contest log. Restore it on the contest screen.',
    )
    expect(api.contestRemoved, 'the list is read again after the removal').toHaveBeenCalledTimes(2)
  })
})

describe('the Removed list keeps them, with Restore', () => {
  it('lists a removed contact and restores it by its id', async () => {
    api.contestRemoved.mockResolvedValue([KEPT])
    api.contestRestore.mockResolvedValueOnce({ outcome: 'restored', entry: KEPT })
    await show(LOG)
    expect(screen.getByText('Removed (1)')).toBeTruthy()
    const item = document.querySelector('.fd-removed-row')!
    expect(item.textContent).toContain('K9OLD · 20m CW · 17:00 · 599 WILL')
    expect(item.textContent).toContain('removed 17:02')
    await act(async () => {
      fireEvent.click(screen.getByText('Restore'))
    })
    await settle()
    expect(api.contestRestore).toHaveBeenCalledWith(7)
    expect(note()).toBe('Restored K9OLD to the contest log.')
    api.contestRemoved.mockResolvedValue([])
  })

  it('says why a restore was refused', async () => {
    api.contestRemoved.mockResolvedValue([KEPT])
    api.contestRestore.mockResolvedValueOnce({ outcome: 'refused', refusal: 'workedAgain' })
    await show(LOG)
    await act(async () => {
      fireEvent.click(screen.getByText('Restore'))
    })
    await settle()
    expect(note()).toBe('K9OLD was worked again on that band and mode after it was removed, so it was not restored.')
    api.contestRemoved.mockResolvedValue([])
  })
})

describe('never from afar', () => {
  it('offers no Remove and no Removed list when observing', async () => {
    api.contestRemoved.mockResolvedValue([KEPT])
    await show(LOG, true)
    expect(removeButtons()).toHaveLength(0)
    expect(document.querySelector('.fd-removed')).toBeNull()
    expect(api.contestRemoved).not.toHaveBeenCalled()
    api.contestRemoved.mockResolvedValue([])
  })
})

// jsdom never lays out: `elementFromPoint` does not exist, so each streamed press says what is
// under it.
describe('a press through the Remote stream removes nothing', () => {
  let under: Element | null = null
  let stream: StreamInputDispatcher | null = null
  beforeEach(() => {
    under = null
    Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
    stream = new StreamInputDispatcher(window)
  })
  afterEach(() => {
    stream?.dispose()
    stream = null
    delete (document as { elementFromPoint?: unknown }).elementFromPoint
  })
  const pointer = (action: 'down' | 'up') =>
    ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
      modifiers: 0, pointerType: 'mouse', clicks: 1 })

  it('refuses Remove clicked twice through the stream, and says why; CONTROL: at the station it removes', async () => {
    api.contestRemoveLast.mockResolvedValue({
      outcome: 'removed',
      entry: { id: 2, removedUnix: T0 + 90, rows: [LOG[1]] },
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    await show(LOG)
    for (let i = 0; i < 2; i++) {
      under = removeButtons()[0]
      await act(async () => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
    }
    await settle()
    expect(api.contestRemoveLast).not.toHaveBeenCalled()
    expect(note()).toMatch(/Only at the station/)
    fireEvent.click(removeButtons()[0])
    await act(async () => { fireEvent.click(removeButtons()[0]) })
    await settle()
    expect(api.contestRemoveLast).toHaveBeenCalledWith('W9BBB', T0 + 60)
  })
})
