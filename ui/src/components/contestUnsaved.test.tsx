// @vitest-environment jsdom
//
// THE CONTEST JOURNAL COULD NOT BE WRITTEN, said on both surfaces that show the contest: the
// Contest screen (`ContestView`) and the contest log strip the cockpits use (`LogEntry`). The
// engine sends `fieldDay.unsaved` while the journal's writes are failing and drops it when one
// lands; each surface shows one red line, in the catalog's words, and none without it.
//
// ⚠️ jsdom lays out nothing; no assertion here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { ContestView } from './ContestView'
import { LogEntry } from './LogEntry'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve(true)),
  contestWorking: vi.fn(() => Promise.resolve(true)),
  contestEntryReset: vi.fn(() => Promise.resolve(true)),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const snap = { radio: { band: '20m', dialMhz: 14.03 }, hunt: null } as unknown as AppSnapshot

/** CQ WW CW as the engine sends it, with `over` on top. */
const cqww = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    multCount: 0,
    poweredPoints: 0,
    totalScore: 0,
    bonusPoints: 0,
    event: 'cqww_cw',
    role: '',
    receives: [
      { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
      { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'ZN', raw: '4' },
    ],
    sentExchange: '4',
    log: [],
    boards: [],
    ...over,
  }) as unknown as FieldDayStatus

// 2026-11-28 00:05:00 UTC.
const LAST_SAVED = Date.UTC(2026, 10, 28, 0, 5) / 1000
const since = t('fieldDay.unsaved.since', { time: '00:05' })
const session = t('fieldDay.unsaved.session')

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
const lines = () => [...document.querySelectorAll('.fd-unsaved')]

afterEach(() => cleanup())

describe('a contest journal that cannot be written is said in red', () => {
  it('premise: the catalog has the words, with the time in them', () => {
    expect(since).toContain('00:05 UTC')
    expect(since).toMatch(/not being saved to disk.*Export a Cabrillo file now\.$/)
    expect(session).toMatch(/this session.*Export a Cabrillo file now\.$/)
  })

  it('on the Contest screen: one line, as an alert, gone when a write lands', async () => {
    const view = render(<ContestView fieldDay={cqww({ unsaved: { failedWrites: 2, lastSavedUnix: LAST_SAVED } })} onSetMode={() => {}} />)
    await settle()
    expect(lines().map((l) => l.textContent)).toEqual([since])
    expect(lines()[0].getAttribute('role')).toBe('alert')
    view.rerender(<ContestView fieldDay={cqww({ unsaved: { failedWrites: 1 } })} onSetMode={() => {}} />)
    expect(lines().map((l) => l.textContent)).toEqual([session])
    // CONTROL: the next good write drops it, and the line goes.
    view.rerender(<ContestView fieldDay={cqww()} onSetMode={() => {}} />)
    expect(lines()).toEqual([])
  })

  it('on the contest log strip: one line, as an alert, gone when a write lands', () => {
    const strip = (fd: FieldDayStatus) => (
      <LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} fieldDay={fd} fdMode="CW" />
    )
    const view = render(strip(cqww({ unsaved: { failedWrites: 2, lastSavedUnix: LAST_SAVED } })))
    expect(lines().map((l) => l.textContent)).toEqual([since])
    expect(lines()[0].getAttribute('role')).toBe('alert')
    view.rerender(strip(cqww({ unsaved: { failedWrites: 1 } })))
    expect(lines().map((l) => l.textContent)).toEqual([session])
    view.rerender(strip(cqww()))
    expect(lines()).toEqual([])
  })
})
