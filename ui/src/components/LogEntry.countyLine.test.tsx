// @vitest-environment jsdom
//
// ⭐ **A COUNTY LINE IN ONE ENTRY** — `COOK/DUPG` in the QTH box logs one contact per county.
//
// The Illinois QSO Party's 2026 rules: "Contacts with/by stations at the border of 2/3/4
// counties count as 2/3/4 counties and 2/3/4 QSOs." The station on the line sends its counties
// joined by `/`, which is also how N1MM Logger+ takes them in its exchange box, so the strip
// takes them that way: each part completed and resolved exactly as a single county is, and
// Enter logs every county as its own row in ONE engine call (`contestLogManualRows`), so the
// rows share one time, band and mode. A part that is not on the sponsor's chart refuses the
// whole line with the reason — a county line is never half-logged on a typo.
//
// Rendered with the props `PhoneCockpit` passes and the ILQP `fieldDay` shape of
// `LogEntry.countyTypeahead.test.tsx`. jsdom lays nothing out; nothing here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { ILQP_COUNTIES } from '../features/ilqpQth'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  // Every row entered the log, unless a test says otherwise.
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
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
const { pushToast } = (await import('../toast')) as unknown as { pushToast: ReturnType<typeof vi.fn> }

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** The Illinois QSO Party as the engine serialises it: RST plus ONE QTH box drawing on the
 *  county list and the state/province list (a `oneOf`, whose free-text arm is how a DX
 *  station's country gets in). */
const ilqp = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

function renderStrip(exchange: 'terrestrial' | 'satellite' = 'terrestrial') {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange={exchange}
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={ilqp()}
      fdMode="CW"
    />,
  )
}

/** A RECEIVED box by its caption, scoped to the row of boxes the operator types into. */
const box = (caption: string): HTMLInputElement => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find(
    (n) => n.textContent === caption,
  )
  if (!cap) throw new Error(`no received box captioned ${caption}`)
  return cap.closest('label')!.querySelector('input')! as HTMLInputElement
}
const qthBox = () => box('QTH')
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const logButton = () => document.querySelector('.le-fd-log-btn') as HTMLButtonElement
const hits = () => [...document.querySelectorAll('.le-fd-suggest .le-fd-hit-code')].map((n) => n.textContent)
const verdict = () => [...document.querySelectorAll('.le-fd-verdicts [role="alert"]')].map((n) => n.textContent)
const toasts = (kind: string) =>
  pushToast.mock.calls.filter(([, k]) => (k ?? 'error') === kind).map(([text]) => text)
const COUNTY_CODES = new Set(ILQP_COUNTIES.map((c) => c.code))

/** Type a contact into the strip and press Enter from the QTH box. */
function enter(call: string, qth: string) {
  fireEvent.change(callBox(), { target: { value: call } })
  fireEvent.change(qthBox(), { target: { value: qth } })
  fireEvent.keyDown(qthBox(), { key: 'Enter' })
}

/** The one row a county contributes: the strip's own vector with that county in the QTH slot. */
const row = (county: string) => [
  ['RST', '599'],
  ['QTH', county],
]

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  pushToast.mockClear()
})

describe('a county line in the ILQP QTH box', () => {
  it('completes the part after the slash and logs COOK/DUP as two contacts, COOK and DUPG', async () => {
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK/DUP' } })
    // The list completes the LAST part, from the county chart: DuPage's code is DUPG.
    expect(hits()).toEqual(['DUPG'])
    fireEvent.mouseDown(document.querySelector('.le-fd-suggest button')!)
    expect(qthBox().value).toBe('COOK/DUPG')
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogManualRows).toHaveBeenCalledTimes(1))
    expect(api.contestLogManualRows.mock.calls[0]).toEqual([
      'K9NR',
      [row('COOK'), row('DUPG')],
      'CW',
      undefined,
    ])
    // ONE action: nothing went through the single-contact command as well.
    expect(api.contestLogManual).not.toHaveBeenCalled()
    await vi.waitFor(() =>
      expect(toasts('success')).toEqual(['Logged K9NR 599 COOK/DUPG (CW): 2 contacts, one per county']),
    )
  })

  it('logs four counties as four contacts, in the order they were typed', async () => {
    renderStrip()
    enter('W9AWE', 'COOK/DUPG/KANE/WILL')
    await vi.waitFor(() => expect(api.contestLogManualRows).toHaveBeenCalledTimes(1))
    expect(api.contestLogManualRows.mock.calls[0][1]).toEqual([
      row('COOK'),
      row('DUPG'),
      row('KANE'),
      row('WILL'),
    ])
    expect(api.contestLogManual).not.toHaveBeenCalled()
  })

  it('takes county NAMES part by part, as the single box does', async () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'Cook/DuPage' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('COOK/DUPG')
    // …and straight to Enter, with nothing resolving it on the way.
    enter('K9NR', 'St Clair/Madison')
    await vi.waitFor(() => expect(api.contestLogManualRows).toHaveBeenCalledTimes(1))
    expect(api.contestLogManualRows.mock.calls[0][1]).toEqual([row('SCLA'), row('MADN')])
  })

  it('offers only counties after the slash — a state is never half of a county line', () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'COOK/IN' } })
    expect(hits().length).toBeGreaterThan(0)
    expect(hits().filter((c) => !COUNTY_CODES.has(c ?? ''))).toEqual([])
    // POSITIVE CONTROL: the same two letters alone are Indiana, offered from the state list.
    fireEvent.change(qthBox(), { target: { value: 'IN' } })
    expect(hits()).toContain('IN')
  })

  it('refuses the whole line, with the reason, when a part is not on the sponsor’s chart', async () => {
    renderStrip()
    enter('K9NR', 'COOK/XYZ')
    expect(verdict()).toEqual([
      "XYZ is not a county on the sponsor's list, so this county line cannot be logged.",
    ])
    expect(logButton().disabled).toBe(true)
    // A prefix is never guessed onto the air: DUP left unpicked is not DuPage.
    enter('K9NR', 'COOK/DUP')
    expect(verdict()).toEqual([
      "DUP is not a county on the sponsor's list, so this county line cannot be logged.",
    ])
    // …and a state is not a county either.
    enter('K9NR', 'COOK/IN')
    expect(verdict()).toEqual([
      "IN is not a county on the sponsor's list, so this county line cannot be logged.",
    ])
    await Promise.resolve()
    expect(api.contestLogManualRows).not.toHaveBeenCalled()
    expect(api.contestLogManual).not.toHaveBeenCalled()
  })

  it('refuses five counties, a county named twice and an unfinished line', async () => {
    renderStrip()
    enter('K9NR', 'COOK/DUPG/KANE/WILL/LAKE')
    expect(verdict()).toEqual(['A county line counts at most 4 counties.'])
    enter('K9NR', 'COOK/DUPG/COOK')
    expect(verdict()).toEqual(['COOK is in this county line twice.'])
    enter('K9NR', 'COOK/')
    expect(verdict()).toEqual(['Finish the county line: 2 to 4 counties joined by /.'])
    expect(logButton().disabled).toBe(true)
    await Promise.resolve()
    expect(api.contestLogManualRows).not.toHaveBeenCalled()
    expect(api.contestLogManual).not.toHaveBeenCalled()
  })

  it('logs the counties that are new and says which one was a dupe', async () => {
    // The engine refused DUPG on its own key: K9NR in DuPage is already in the log on this
    // band and mode. COOK and KANE are new and entered the log.
    api.contestLogManualRows.mockImplementationOnce(() => Promise.resolve([true, false, true]))
    renderStrip()
    enter('K9NR', 'COOK/DUPG/KANE')
    await vi.waitFor(() => expect(toasts('error')).toHaveLength(1))
    expect(toasts('success')).toEqual(['Logged K9NR 599 COOK/KANE (CW): 2 contacts, one per county'])
    expect(toasts('error')).toEqual(['K9NR in DUPG is a dupe on this band/mode, not logged again'])
    // The contact is in the log, so the strip moves on to the next one.
    await vi.waitFor(() => expect(callBox().value).toBe(''))
  })

  it('keeps the entry when every county was a dupe, and says so', async () => {
    api.contestLogManualRows.mockImplementationOnce(() => Promise.resolve([false, false]))
    renderStrip()
    enter('K9NR', 'COOK/DUPG')
    await vi.waitFor(() => expect(toasts('error')).toHaveLength(1))
    expect(toasts('error')).toEqual(['K9NR in COOK, DUPG are dupes on this band/mode, not logged again'])
    expect(toasts('success')).toEqual([])
    // Nothing was logged, exactly as a single refused contact leaves the strip.
    expect(callBox().value).toBe('K9NR')
  })

  it('leaves a single county exactly as it was', async () => {
    renderStrip()
    enter('K9NR', 'COOK')
    await vi.waitFor(() => expect(api.contestLogManual).toHaveBeenCalledTimes(1))
    expect(api.contestLogManual.mock.calls[0]).toEqual(['K9NR', row('COOK'), 'CW', undefined])
    expect(api.contestLogManualRows).not.toHaveBeenCalled()
  })
})
