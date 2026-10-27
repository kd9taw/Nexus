// @vitest-environment jsdom
//
// EVERY CONTEST CONTACT HAS A NUMBER.
//
// The engine numbers every row (`nr`: the serial it was sent in a serial contest, its place in the
// run otherwise; a removed contact's number stays spent, so the numbers can have gaps). The log
// table and the logger window's last contacts show it in a first column. A station older than the
// field sends no `nr`, and the tables keep the columns they had.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { createRef } from 'react'
import { render, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import { LastContacts } from './ContestLogger'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

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
}))

const T0 = Date.UTC(2026, 9, 18, 15, 0) / 1000
const row = (call: string, mins: number, nr?: number): FieldDayQso =>
  ({
    call,
    class: '',
    section: '',
    band: '20m',
    mode: 'CW',
    submode: '',
    whenUnix: T0 + mins * 60,
    mex: '599 KANE',
    rcvd: ['599', 'COOK'],
    ...(nr === undefined ? {} : { nr }),
  }) as FieldDayQso

// 15:00, 15:10, then a 45-minute break, 15:55, 16:05: on 0:20, off 0:45. The third contact was
// numbered 3 and removed, so the numbers read 1, 2, 4, 5.
const LOG = [row('K9AAA', 0, 1), row('W9BBB', 10, 2), row('N9DDD', 55, 4), row('K9EEE', 65, 5)]

const ILQP = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 4,
    multCount: 2,
    poweredPoints: 8,
    totalScore: 8,
    bonusPoints: 0,
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    sentExchange: '599 KANE',
    log,
    boards: [],
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => cleanup())

const texts = (root: ParentNode, sel: string) => [...root.querySelectorAll(sel)].map((n) => n.textContent)

describe('the contest log numbers every contact', () => {
  it('shows each row’s number in a first column, gaps and all', async () => {
    render(<ContestView fieldDay={ILQP(LOG)} onSetMode={() => {}} />)
    await settle()
    const log = document.querySelector('.fd-log')!
    expect(texts(log, '.fd-log-head .fd-col')[0]).toBe('#')
    expect(texts(log, '.fd-log-row .fd-col.nr')).toEqual(['1', '2', '4', '5'])
    expect(log.querySelector('.fd-log-head')!.hasAttribute('data-nr')).toBe(true)
  })

  it('keeps the old columns for a station that sends no numbers', async () => {
    render(<ContestView fieldDay={ILQP(LOG.map(({ nr: _nr, ...q }) => q as FieldDayQso))} onSetMode={() => {}} />)
    await settle()
    const log = document.querySelector('.fd-log')!
    expect(texts(log, '.fd-log-head .fd-col')[0]).toBe('Time')
    expect(log.querySelector('.fd-col.nr')).toBeNull()
    expect(log.querySelector('.fd-log-head')!.hasAttribute('data-nr')).toBe(false)
  })

  it('and so do the logger window’s last contacts, newest first', () => {
    render(<LastContacts fieldDay={ILQP(LOG)} style={{}} boxRef={createRef()} onEditDone={() => {}} />)
    const recent = document.querySelector('.contest-logger-recent')!
    expect(texts(recent, '.fd-log-head .fd-col')[0]).toBe('#')
    expect(texts(recent, '.fd-log-row .fd-col.nr')).toEqual(['5', '4', '2', '1'])
  })
})
