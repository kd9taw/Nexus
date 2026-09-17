// @vitest-environment jsdom
//
// THE CONTEST STRIP IN A CONTEST THAT IS NOT FIELD DAY — rendered with the props the RTTY
// cockpit gives it (`RttyCockpit.tsx`, the LOG pane), and a `fieldDay` of the shape the
// engine sends for CQ WW: RST, a zone, and (for CQ WW RTTY) an optional QTH.
//
// What the Field Day strip got away with and a CQ WW log cannot:
//
//   1. RST came back BLANK after every contact (the prefill read a Field Day row's
//      class/section, which a CQ WW row does not have), so every second QSO needed the
//      report retyped.
//   2. A zone was any non-blank string — 41, 0 and "AB" all logged.
//   3. An OPTIONAL enum slot could not be left blank, so a DX station (who sends no QTH)
//      could not be logged at all.
//   4. Captions, header and button said Field Day.
//   5. Nothing could fill a box from outside (the RTTY grab needs to).
//   6. The cty.dat zone was not offered, and a USA/Canada station logged with no QTH went
//      by silently.
//
// ⚠️ jsdom lays out nothing; no assertion here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve(true)),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '20m', dialMhz: 14.08 },
  hunt: null,
} as unknown as AppSnapshot

/** CQ WW RTTY as the engine serialises it for a W/VE station. */
const cqwwRtty = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    event: 'cqww_rtty',
    log: [],
    role: 'w_ve',
    receives: [
      { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
      { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
      // An OPTIONAL enum: DX stations send no QTH. The contest's own QTH list, which the UI
      // carries as a guarded mirror of the rules seed.
      { key: 'QTH', kind: 'enum', required: false, domain: 'cqww_rtty_qth' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'ZN', raw: '5' },
      { key: 'QTH', raw: 'MA' },
    ],
    sentExchange: '5 MA',
    ...over,
  }) as unknown as FieldDayStatus

const row = (call: string): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'DIG', submode: 'RTTY', whenUnix: 1 }) as FieldDayQso

type Extra = { fillExchange?: { key: string; value: string; ts: number } | null }

/** Exactly the props `RttyCockpit` passes, plus the fill the grab drives. */
function strip(fieldDay: FieldDayStatus, extra: Extra = {}, fdMode: 'DIG' | 'CW' | 'PH' = 'DIG') {
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="RTTY"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode={fdMode}
      fdSubmode={fdMode === 'DIG' ? 'RTTY' : undefined}
      {...extra}
    />
  )
}

const box = (cap: string) =>
  [...document.querySelectorAll('.le-fd-big .le-fd-field')]
    .find((l) => l.querySelector('.le-fd-cap')?.textContent === cap)!
    .querySelector('input') as HTMLInputElement
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('1 — the report keeps its default across contacts', () => {
  it('starts at 599 and comes back to 599 after a logged contact, not blank', async () => {
    const view = render(strip(cqwwRtty()))
    expect(box('RST').value).toBe('599')
    fireEvent.change(callBox(), { target: { value: 'DL1ABC' } })
    fireEvent.change(box('Zone'), { target: { value: '14' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    })
    expect(api.contestLogManual).toHaveBeenCalledWith(
      'DL1ABC',
      [
        ['RST', '599'],
        ['ZN', '14'],
        ['QTH', ''],
      ],
      'DIG',
      'RTTY',
    )
    // The snapshot now carries the contact — the moment the prefill used to blank RST.
    view.rerender(strip(cqwwRtty({ qsoCount: 1, log: [row('DL1ABC')] })))
    expect(box('RST').value).toBe('599')
    // The zone and QTH are the NEXT station's, so they start empty.
    expect(box('Zone').value).toBe('')
    expect(box('QTH').value).toBe('')
  })

  it('is 59 on a phone position', () => {
    render(strip(cqwwRtty(), {}, 'PH'))
    expect(box('RST').value).toBe('59')
  })

  it('leaves Field Day exactly as it was — class and section carry over, no report box', async () => {
    const fd = {
      running: true,
      state: 'Idle',
      qsoCount: 0,
      sections: 0,
      points: 0,
      event: 'arrlfd',
      log: [],
      role: '',
      receives: [
        { key: 'CLASS', kind: 'pattern', required: true },
        { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
      ],
      composing: [
        { key: 'CLASS', raw: '3A' },
        { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
      ],
    } as unknown as FieldDayStatus
    const view = render(strip(fd, {}, 'CW'))
    expect(box('Class').value).toBe('3A')
    view.rerender(
      strip(
        { ...fd, qsoCount: 1, log: [{ ...row('K1ABC'), class: '2A', section: 'EMA' }] },
        {},
        'CW',
      ),
    )
    expect(box('Class').value).toBe('2A')
    expect(box('Section').value).toBe('EMA')
  })
})

describe('2 — a zone is a number from 1 to 40', () => {
  it('refuses 0, 41 and letters, and accepts 1 and 40', () => {
    render(strip(cqwwRtty()))
    fireEvent.change(callBox(), { target: { value: 'DL1ABC' } })
    const logBtn = () => screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement
    for (const bad of ['0', '41', 'AB', '']) {
      fireEvent.change(box('Zone'), { target: { value: bad } })
      expect(logBtn().disabled, `zone ${JSON.stringify(bad)}`).toBe(true)
    }
    fireEvent.change(box('Zone'), { target: { value: '41' } })
    expect(screen.getByRole('alert').textContent).toBe('Zone must be a number from 1 to 40.')
    for (const good of ['1', '05', '40']) {
      fireEvent.change(box('Zone'), { target: { value: good } })
      expect(logBtn().disabled, `zone ${good}`).toBe(false)
    }
  })
})

describe('3 — an optional enum slot may be left blank', () => {
  it('logs with QTH blank, and still refuses a QTH that is not in its domain', () => {
    render(strip(cqwwRtty()))
    fireEvent.change(callBox(), { target: { value: 'DL1ABC' } })
    fireEvent.change(box('Zone'), { target: { value: '14' } })
    const logBtn = () => screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement
    expect(logBtn().disabled, 'blank optional QTH').toBe(false)
    // POSITIVE CONTROL: optional is not "anything" — a value must still be a member.
    fireEvent.change(box('QTH'), { target: { value: 'ZZZ' } })
    expect(logBtn().disabled, 'bogus QTH').toBe(true)
    // An ARRL section is not a CQ WW RTTY QTH: Eastern Massachusetts sends MA.
    fireEvent.change(box('QTH'), { target: { value: 'EMA' } })
    expect(logBtn().disabled, 'a section is not a QTH').toBe(true)
    fireEvent.change(box('QTH'), { target: { value: 'MA' } })
    expect(logBtn().disabled, 'real QTH').toBe(false)
    // The sponsor's own three-letter Canadian call areas are real QTHs too.
    fireEvent.change(box('QTH'), { target: { value: 'PEI' } })
    expect(logBtn().disabled, 'PEI').toBe(false)
  })
})

describe('4 — captions, header and button come from the contest, not Field Day', () => {
  it('reads the contest’s slots and says contest log', () => {
    render(strip(cqwwRtty()))
    const caps = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].map((n) => n.textContent)
    expect(caps).toEqual(['Call', 'RST', 'Zone', 'QTH'])
    expect(document.querySelector('.le-fd-chip')!.textContent).toBe('CONTEST LOG')
    expect(document.querySelector('.le-fd-header')!.textContent).not.toMatch(/Field Day/)
    expect(screen.queryByRole('button', { name: 'Log FD' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Log' })).toBeTruthy()
  })

  it('keeps Field Day’s own words in Field Day', () => {
    render(
      strip(
        cqwwRtty({
          event: 'wfd',
          receives: [
            { key: 'CLASS', kind: 'pattern', required: true },
            { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
          ],
        }),
      ),
    )
    expect(document.querySelector('.le-fd-chip')!.textContent).toBe('FD LOG')
    expect(screen.getByRole('button', { name: 'Log FD' })).toBeTruthy()
  })
})

describe('5 — the fill API: one slot, refilled on every new ts, focus left alone', () => {
  it('fills the named box, refills the same value after an edit, and ignores unknown slots', () => {
    const view = render(strip(cqwwRtty()))
    callBox().focus()
    view.rerender(strip(cqwwRtty(), { fillExchange: { key: 'ZN', value: '14', ts: 1 } }))
    expect(box('Zone').value).toBe('14')
    expect(document.activeElement, 'focus did not move').toBe(callBox())
    // The operator overtypes it; the same grab again (a new ts) puts it back.
    fireEvent.change(box('Zone'), { target: { value: '15' } })
    view.rerender(strip(cqwwRtty(), { fillExchange: { key: 'ZN', value: '14', ts: 2 } }))
    expect(box('Zone').value).toBe('14')
    // A re-render with the SAME ts is not a new fill.
    fireEvent.change(box('Zone'), { target: { value: '16' } })
    view.rerender(strip(cqwwRtty(), { fillExchange: { key: 'ZN', value: '14', ts: 2 } }))
    expect(box('Zone').value).toBe('16')
    // A value is uppercased like typing does, and a slot this contest does not receive is
    // not invented.
    view.rerender(strip(cqwwRtty(), { fillExchange: { key: 'QTH', value: 'nwt', ts: 3 } }))
    expect(box('QTH').value).toBe('NWT')
    view.rerender(strip(cqwwRtty(), { fillExchange: { key: 'CLASS', value: '3A', ts: 4 } }))
    const caps = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].map((n) => n.textContent)
    expect(caps).toEqual(['Call', 'RST', 'Zone', 'QTH'])
  })
})

describe('6 — the country file’s zone is a hint, and a W/VE contact with no QTH is flagged', () => {
  it('shows the cty.dat zone as the placeholder and never as a value', async () => {
    api.contestZoneHint.mockImplementation(() => Promise.resolve(5))
    render(strip(cqwwRtty()))
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'W1AW' } })
    })
    expect(api.contestZoneHint).toHaveBeenCalledWith('W1AW')
    expect(box('Zone').placeholder).toBe('5')
    expect(box('Zone').value).toBe('')
    api.contestZoneHint.mockImplementation(() => Promise.resolve(null))
  })

  it('warns — never refuses — when a USA or Canada station has no QTH', async () => {
    api.resolveEntity.mockImplementation((c: string) =>
      Promise.resolve(c.startsWith('W') ? 'United States' : c.startsWith('VE') ? 'Canada' : 'Fed. Rep. of Germany'),
    )
    render(strip(cqwwRtty()))
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'W1AW' } })
    })
    fireEvent.change(box('Zone'), { target: { value: '5' } })
    const warning = () => screen.queryByRole('status')
    expect(warning()?.textContent).toMatch(/W1AW/)
    expect((screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement).disabled).toBe(false)
    // Adding the QTH clears it.
    fireEvent.change(box('QTH'), { target: { value: 'CT' } })
    expect(warning()).toBeNull()
    // POSITIVE CONTROL: the same blank QTH from a DX station is not flagged…
    fireEvent.change(box('QTH'), { target: { value: '' } })
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'DL1ABC' } })
    })
    expect(warning()).toBeNull()
    // …and Canada is flagged like the USA.
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'VE3XYZ' } })
    })
    expect(warning()?.textContent).toMatch(/VE3XYZ/)
    api.resolveEntity.mockImplementation(() => Promise.resolve(null))
  })
})

describe('7 — a band the contest does not use is flagged, never refused', () => {
  it('says so on 30 m, not on 20 m, and still offers Log', () => {
    const bands = ['80m', '40m', '20m', '15m', '10m']
    const view = render(strip(cqwwRtty({ bands } as Partial<FieldDayStatus>)))
    const hint = () => document.querySelector('.le-fd-header .le-fd-hint')!.textContent
    expect(hint()).toBe('20m · contacts go to the contest log')
    const offBand = { ...snap, radio: { band: '30m', dialMhz: 10.142 } } as unknown as AppSnapshot
    view.rerender(
      <LogEntry
        onOpenLogbook={() => {}}
        snap={offBand}
        mode="RTTY"
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        fieldDay={cqwwRtty({ bands } as Partial<FieldDayStatus>)}
        fdMode="DIG"
        fdSubmode="RTTY"
      />,
    )
    expect(hint()).toBe('30m is not a band this contest uses · contacts still go to the contest log')
    fireEvent.change(callBox(), { target: { value: 'JA1ABC' } })
    fireEvent.change(box('Zone'), { target: { value: '25' } })
    expect((screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement).disabled).toBe(false)
    // CONTROL: a contest that names no bands never says it.
    view.rerender(
      <LogEntry
        onOpenLogbook={() => {}}
        snap={offBand}
        mode="RTTY"
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        fieldDay={cqwwRtty()}
        fdMode="DIG"
        fdSubmode="RTTY"
      />,
    )
    expect(hint()).toBe('30m · contacts go to the contest log')
  })
})
