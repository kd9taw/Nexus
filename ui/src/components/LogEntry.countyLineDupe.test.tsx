// @vitest-environment jsdom
//
// ⭐ **THE DUPE WARNING ON A COUNTY LINE** — `COOK/WILL` is judged county by county, as the log
// writes it.
//
// A county line logs one row per county, and the engine judges each row on its own key: at the
// Illinois QSO Party that is the call, the band, the mode class (CW and digital one) and the
// county each side sends. The while-typing warning judged the box WHOLE, and `COOK/WILL` is no
// row's county, so a station already worked from both counties said nothing when typed as their
// line, while each county alone warned. Every county a dupe reads as the one-county sentence
// naming them; some, which counties are dupes and which are new. The log is unchanged: one row
// per county, each a dupe or new as it is (`a_county_line_marks_only_the_county_already_worked`
// in the engine holds the marks).
//
// Rendered with the props `PhoneCockpit` passes and the ILQP `fieldDay` shape of
// `LogEntry.countyLine.test.tsx`, with the dupe rule and each row's dupe key as the engine
// serialises them (`LogEntry.partyClubDupe.test.tsx`). jsdom lays nothing out; nothing here is
// about geometry.
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
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '40m', dialMhz: 7.03 },
  hunt: null,
} as unknown as AppSnapshot

/** The engine's key for a 40 m CW row of this position, which sends KANE: call, band, mode
 *  class, the county received, the county sent. */
const dkey = (call: string, county: string) => [call, '40M', 'CW', county, 'KANE']

/** An Illinois QSO Party position in Kane whose log holds `call` worked on 40 m CW from each of
 *  `counties`, and whose club host holds `club` from the other positions. ILQP's dupe rule is
 *  `fd_rules.seed.json`'s: a station once per band and mode from each county, either way. */
const ilqp = (call: string, counties: string[], club: string[][] = []): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: counties.length,
    sections: 0,
    points: 0,
    role: 'in_state',
    log: counties.map((c) => ({ call, band: '40m', mode: 'CW', dkey: dkey(call, c) })),
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
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
    club: {
      syncState: 'synced',
      queued: 0,
      offlineSinceUnix: 0,
      hosting: false,
      event: 'W9XYZ IL QSO Party',
      hostCall: 'W9XYZ',
      score: 0,
      qsos: club.length,
      sections: 0,
      skewSecs: 0,
      dupes: [],
      dkeys: club,
      board: [],
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
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent === 'County / State')
  if (!cap) throw new Error('no received QTH box')
  return cap.closest('label')!.querySelector('input')! as HTMLInputElement
}
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
/** Every line under the strip: the warnings (`alert`) and the club's notes (`status`). */
const hints = () => [...document.querySelectorAll('.le-fd-verdicts .le-fd-hint')].map((n) => n.textContent)

/** Type a contact into the strip, as far as the QTH box, and press nothing. */
function typed(call: string, qth: string) {
  fireEvent.change(callBox(), { target: { value: call } })
  fireEvent.change(qthBox(), { target: { value: qth } })
}
const row = (county: string) => [
  ['RST', '599'],
  ['QTH', county],
]
const DUPE = "Dupe: K9AAA is already in this position's log on 40m CW"

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the dupe warning on a county line', () => {
  it('warns for the line when every county is already in the log, as each county alone does', () => {
    for (const line of ['COOK/WILL', 'COOK+WILL', 'Cook/Will', 'WILL/COOK']) {
      renderStrip(ilqp('K9AAA', ['COOK', 'WILL']))
      // CONTROL first: each county alone, in the same strip, warns as it always did.
      for (const county of ['COOK', 'WILL']) {
        typed('K9AAA', county)
        expect(hints(), county).toEqual([DUPE])
      }
      typed('K9AAA', line)
      const counties = line === 'WILL/COOK' ? 'WILL and COOK' : 'COOK and WILL'
      expect(hints(), line).toEqual([`${DUPE} for ${counties}`])
      cleanup()
    }
  })

  it('names which counties are dupes and which are new, in the order typed', () => {
    renderStrip(ilqp('K9AAA', ['COOK']))
    typed('K9AAA', 'COOK/WILL')
    expect(hints()).toEqual(['COOK is a dupe; WILL is new'])
    typed('K9AAA', 'WILL+COOK')
    expect(hints()).toEqual(['COOK is a dupe; WILL is new'])
    cleanup()
    renderStrip(ilqp('K9AAA', ['COOK', 'WILL']))
    typed('K9AAA', 'COOK/DUPG/WILL')
    expect(hints()).toEqual(['COOK and WILL are dupes; DUPG is new'])
    cleanup()
    renderStrip(ilqp('K9AAA', ['DUPG']))
    typed('K9AAA', 'COOK/DUPG/WILL')
    expect(hints()).toEqual(['DUPG is a dupe; COOK and WILL are new'])
  })

  it('puts the DUPE badge on the Call box only when every county of the line is a dupe', () => {
    const badge = () => document.querySelector('.le-fd-field-call .le-fd-dupe')!.textContent
    renderStrip(ilqp('K9AAA', ['COOK', 'WILL']))
    typed('K9AAA', 'COOK/WILL')
    // A row with no time on it (an older station) says where it was worked, not when.
    expect(badge()).toBe('DUPE 40m CW')
    // One new county makes the line a contact that scores: the badge goes, as the sentence does.
    typed('K9AAA', 'COOK/DUPG')
    expect(hints()).toEqual(['COOK is a dupe; DUPG is new'])
    expect(badge()).toBe('')
    expect(callBox().classList.contains('le-fd-input-dupe')).toBe(false)
  })

  it('CONTROL: a line whose every county is new says nothing, as a new county does', () => {
    renderStrip(ilqp('K9AAA', ['COOK', 'WILL']))
    for (const [call, qth] of [
      ['K9AAA', 'DUPG/KANE'],
      ['K9AAA', 'DUPG'],
      ['K9BBB', 'COOK/WILL'],
      ['K9BBB', 'COOK'],
    ]) {
      typed(call, qth)
      expect(hints(), `${call} ${qth}`).toEqual([])
    }
  })

  it('judges a county typed by NAME as the code the log will write', () => {
    renderStrip(ilqp('K9AAA', ['DUPG']))
    // CONTROL first: the code, as before.
    typed('K9AAA', 'DUPG')
    expect(hints()).toEqual([DUPE])
    typed('K9AAA', 'DuPage')
    expect(hints()).toEqual([DUPE])
  })

  it('logs one row per county whatever the warning says: the engine marks each dupe', async () => {
    for (const worked of [['COOK', 'WILL'], ['COOK'], []]) {
      renderStrip(ilqp('K9AAA', worked))
      typed('K9AAA', 'COOK/WILL')
      fireEvent.keyDown(qthBox(), { key: 'Enter' })
      await vi.waitFor(() => expect(api.contestLogManualRows).toHaveBeenCalledTimes(1))
      expect(api.contestLogManualRows.mock.calls[0], worked.join('/')).toEqual([
        'K9AAA',
        [row('COOK'), row('WILL')],
        'CW',
        undefined,
      ])
      expect(api.contestLogManual).not.toHaveBeenCalled()
      cleanup()
      for (const f of Object.values(api)) f.mockClear()
    }
  })

  it('names the counties another position of the club already worked', () => {
    const club = "Club dupe: another position already worked K9AAA on 40m CW — logging is allowed but adds no points"
    renderStrip(ilqp('K9AAA', [], [dkey('K9AAA', 'COOK')]))
    // CONTROL first: the one county, as before.
    typed('K9AAA', 'COOK')
    expect(hints()).toEqual([club])
    typed('K9AAA', 'COOK/WILL')
    expect(hints()).toEqual([`${club} for COOK`])
    // CONTROL: a line the club has not worked says nothing.
    typed('K9AAA', 'DUPG/WILL')
    expect(hints()).toEqual([])
    cleanup()
    // Worked here from COOK and by the club from WILL: WILL is new to this position's log, and the
    // club's note names it.
    renderStrip(ilqp('K9AAA', ['COOK'], [dkey('K9AAA', 'WILL')]))
    typed('K9AAA', 'COOK/WILL')
    expect(hints()).toEqual(['COOK is a dupe; WILL is new', `${club} for WILL`])
  })
})
