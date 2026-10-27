// @vitest-environment jsdom
//
// ⭐ **THE EARLY "WORKED" WARNING** — at a QSO party a station is worked once per band and mode
// FROM EACH COUNTY, so the contest strip cannot call a contact a dupe until its county is in (a
// rover in a new county scores). Waiting for the county let operators work many dupes. So the
// moment the call is in, a station already in this log on this band and mode turns the Call box
// AMBER and says where it was worked from; it turns RED (the DUPE badge, unchanged) only when the
// county typed is one already logged, and a new county clears it.
//
// The ILQP `fieldDay` shape of `LogEntry.countyLineDupe.test.tsx`, with each row's dupe key as
// the engine serialises it. jsdom lays nothing out; nothing here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn((_call: string, rows: unknown[]) => Promise.resolve(rows.map(() => true))),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestLoggedQth: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestPlace: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const snap = {
  radio: { band: '40m', dialMhz: 7.03 },
  hunt: null,
} as unknown as AppSnapshot

/** The engine's key for a row of this position: call, band, mode class, county received, county sent. */
const dkey = (call: string, band: string, county: string, sent = 'KANE') => [call, band.toUpperCase(), 'CW', county, sent]

/** An Illinois QSO Party position in `sending` whose log holds the given rows. */
const ilqp = (rows: { call: string; band: string; county: string; sent?: string }[], sending = 'KANE'): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: rows.length,
    sections: 0,
    points: 0,
    role: 'in_state',
    log: rows.map((r) => ({ call: r.call, band: r.band, mode: 'CW', dkey: dkey(r.call, r.band, r.county, r.sent) })),
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: sending, domain: 'il_counties' },
    ],
    dupeModeGroups: [['CW', 'DIG']],
    dupeRule: {
      byCall: true,
      byBand: true,
      byModeClass: true,
      byFields: ['QTH'],
      bySentFields: ['QTH'],
      modeClassGroups: [['CW', 'DIG']],
    },
  }) as unknown as FieldDayStatus

function renderStrip(fd: FieldDayStatus) {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fd}
      fdMode="CW"
    />,
  )
}

const qthBox = (): HTMLInputElement => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent?.startsWith('County / State'))
  if (!cap) throw new Error('no received QTH box')
  return cap.closest('label')!.querySelector('input')! as HTMLInputElement
}
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
/** What the Call box says about itself: 'maybe' (amber), 'dupe' (red) or 'new'. */
const shade = () =>
  callBox().classList.contains('le-fd-input-dupe')
    ? 'dupe'
    : callBox().classList.contains('le-fd-input-maybe')
      ? 'maybe'
      : 'new'
const badge = () => document.querySelector('.le-fd-big .le-fd-dupe')?.textContent?.trim() ?? ''

const WORKED = [
  { call: 'K9AAA', band: '40m', county: 'COOK' },
  { call: 'K9AAA', band: '40m', county: 'DUPA' },
  { call: 'K9AAA', band: '20m', county: 'LAKE' },
]

afterEach(cleanup)

describe('the contest strip warns of a possible dupe as soon as the call is in', () => {
  it('a call already logged on this band and mode turns amber and names the counties, before any county', () => {
    renderStrip(ilqp(WORKED))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    expect(shade()).toBe('maybe')
    // LAKE was 20 m: another band is a new contact, so it is not listed.
    expect(badge()).toBe('Worked: COOK, DUPA')
  })

  it('a county already logged turns it red (the DUPE the strip always showed)', () => {
    renderStrip(ilqp(WORKED))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    expect(shade()).toBe('dupe')
    expect(badge()).toMatch(/^DUPE/)
  })

  it('a rover in a new county is a new contact: no amber, no red', () => {
    renderStrip(ilqp(WORKED))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'WILL' } })
    expect(shade()).toBe('new')
    expect(badge()).toBe('')
  })

  it('narrows to the counties that still fit what is typed', () => {
    renderStrip(ilqp(WORKED))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'DU' } })
    expect(shade()).toBe('maybe')
    expect(badge()).toBe('Worked: DUPA')
  })

  it('a call never worked here, or worked only on another band, stays plain', () => {
    renderStrip(ilqp(WORKED))
    fireEvent.change(callBox(), { target: { value: 'K9BBB' } })
    expect([shade(), badge()]).toEqual(['new', ''])
    cleanup()
    renderStrip(ilqp([{ call: 'K9AAA', band: '20m', county: 'COOK' }]))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    expect([shade(), badge()]).toEqual(['new', ''])
  })

  it('after this station moves county, contacts sent from the old one are no warning', () => {
    // Every row was worked sending KANE; this position now sends DEKA.
    renderStrip(ilqp(WORKED, 'DEKA'))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    expect([shade(), badge()]).toEqual(['new', ''])
  })

  it('a contest whose dupe key names no county keeps the plain DUPE, with no amber step', () => {
    const fd = ilqp(WORKED)
    const triple = {
      ...fd,
      dupeRule: { ...fd.dupeRule!, byFields: [], bySentFields: [] },
      log: fd.log.map((q) => ({ ...q, dkey: [q.call, q.band.toUpperCase(), 'CW'] })),
    } as FieldDayStatus
    renderStrip(triple)
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    expect(shade()).toBe('dupe')
  })
})
