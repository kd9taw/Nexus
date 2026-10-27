// @vitest-environment jsdom
//
// THE COUNTRY ON THE CONTEST ENTRY — the call's country as the contest places it (`contestPlace`, the
// scorer's own resolve), read-only on the Call caption's line: "Fed. Rep. of Germany · CQ 14" in a
// contest that receives a CQ zone, the country alone in one that does not. The country and the zone come
// from that one answer, and the contest is asked by its event, so a WAE area is named as that contest's
// scorer counts it: on its own in CQ WW (IT9 is Sicily), its DXCC entity elsewhere (IT9 is Italy).
// The zone stays the zone box's placeholder and never becomes its value.
//
// ⚠️ jsdom lays out nothing; that the label moves no box is the browser measurement's to say.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
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
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.25 }, hunt: null } as unknown as AppSnapshot

/** The contest's own placing of a call: the entity its scorer counts, and the prefix's CQ zone. */
const PLACE: Record<string, { entity: string; cqZone: number | null }> = {
  DL1ABC: { entity: 'Fed. Rep. of Germany', cqZone: 14 },
  W6XYZ: { entity: 'United States', cqZone: 3 },
  IT9XYZ: { entity: 'Sicily', cqZone: 15 },
}
/** The callsign card's entity, which folds a WAE area into its DXCC entity (IT9 is Italy there). */
const CARD: Record<string, string> = { DL1ABC: 'Fed. Rep. of Germany', W6XYZ: 'United States', IT9XYZ: 'Italy' }
const placeAll = () => {
  api.resolveEntity.mockImplementation((c: string) => Promise.resolve(CARD[c] ?? null))
  api.contestPlace.mockImplementation((c: string) => Promise.resolve(PLACE[c] ?? null))
}

/** CQ WW SSB as the engine serialises it: RST and a CQ zone. */
const cqww = {
  running: true,
  state: 'run',
  qsoCount: 0,
  sections: 0,
  points: 0,
  event: 'cqww_ssb',
  log: [],
  role: '',
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
  ],
  composing: [
    { key: 'RST', raw: '59' },
    { key: 'ZN', raw: '4' },
  ],
} as unknown as FieldDayStatus

/** A contest with no zone in its exchange (a serial number). */
const serial = {
  ...cqww,
  event: 'wpx_ssb',
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'NR', kind: 'number', required: true, min: 1, max: 99999 },
  ],
  composing: [
    { key: 'RST', raw: '59' },
    { key: 'NR', raw: '1' },
  ],
} as unknown as FieldDayStatus

const strip = (fieldDay: FieldDayStatus) => (
  <LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" titled={false} fieldDay={fieldDay} fdMode="PH" />
)
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const country = () => document.querySelector<HTMLElement>('.le-fd-country')
const zoneBox = () =>
  [...document.querySelectorAll('.le-fd-big .le-fd-field')]
    .find((l) => l.querySelector('.le-fd-cap')?.textContent === 'Zone')!
    .querySelector('input') as HTMLInputElement
const typeCall = async (call: string) => {
  await act(async () => {
    fireEvent.change(callBox(), { target: { value: call } })
  })
}

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  api.resolveEntity.mockImplementation(() => Promise.resolve(null))
  api.contestPlace.mockImplementation(() => Promise.resolve(null))
})

describe('the country on the contest entry', () => {
  it('shows the country and its CQ zone on the Call caption line in a CQ-zone contest, and fills nothing', async () => {
    placeAll()
    render(strip(cqww))
    expect(country()?.textContent ?? '').toBe('')
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe(t('logEntry.contest.country.zone', { country: 'Fed. Rep. of Germany', zone: '14' }))
    expect(country()!.textContent).toBe('Fed. Rep. of Germany · CQ 14')
    // On the Call caption's line, after the DUPE badge's live region and outside it.
    const caption = callBox().closest('label')!.querySelector('.le-fd-cap-marked')!
    expect(country()!.parentElement).toBe(caption)
    expect(country()!.closest('[aria-live]')).toBeNull()
    expect(caption.querySelector('.le-fd-dupe')!.compareDocumentPosition(country()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // The zone is still only the box's hint.
    expect(zoneBox().placeholder).toBe('14')
    expect(zoneBox().value).toBe('')
    // It follows the call, and goes when the call does.
    await typeCall('W6XYZ')
    expect(country()!.textContent).toBe('United States · CQ 3')
    await typeCall('')
    expect(country()?.textContent ?? '').toBe('')
  })

  it('shows the country alone in a contest with no zone in its exchange', async () => {
    placeAll()
    render(strip(serial))
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe('Fed. Rep. of Germany')
  })

  it('names a WAE area as the contest counts it, Sicily for IT9 in CQ WW and Italy elsewhere; CONTROL: a plain call as before', async () => {
    placeAll()
    // The backend's answer follows the event it is asked for: only CQ WW counts Sicily on its own.
    api.contestPlace.mockImplementation((c: string, event: string) =>
      Promise.resolve(c === 'IT9XYZ' && !event.startsWith('cqww') ? { entity: 'Italy', cqZone: 15 } : (PLACE[c] ?? null)),
    )
    render(strip(cqww))
    await typeCall('IT9XYZ')
    expect(api.contestPlace).toHaveBeenLastCalledWith('IT9XYZ', 'cqww_ssb')
    expect(country()!.textContent).toBe('Sicily · CQ 15')
    expect(zoneBox().placeholder).toBe('15')
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe('Fed. Rep. of Germany · CQ 14')
    cleanup()
    render(strip(serial))
    await typeCall('IT9XYZ')
    expect(api.contestPlace).toHaveBeenLastCalledWith('IT9XYZ', 'wpx_ssb')
    expect(country()!.textContent).toBe('Italy')
  })

  it("never pairs one call's country with another's zone while the new call's answer is on its way", async () => {
    placeAll()
    render(strip(cqww))
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe('Fed. Rep. of Germany · CQ 14')
    let answer: (v: unknown) => void = () => {}
    api.contestPlace.mockImplementationOnce(() => new Promise((r) => (answer = r)))
    await typeCall('W6XYZ')
    expect(country()?.textContent ?? '', 'nothing of DL1ABC is left on W6XYZ').toBe('')
    expect(zoneBox().placeholder).toBe('')
    await act(async () => answer(PLACE.W6XYZ))
    expect(country()!.textContent).toBe('United States · CQ 3')
    expect(zoneBox().placeholder).toBe('3')
  })

  it('is not shown in Field Day, where every station is W/VE; CONTROL: the same call in CQ WW is', async () => {
    placeAll()
    const fd = {
      ...cqww,
      event: 'arrlfd',
      receives: [
        { key: 'CLASS', kind: 'fdClass', required: true },
        { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
      ],
      composing: [],
    } as unknown as FieldDayStatus
    render(strip(fd))
    await typeCall('W6XYZ')
    expect(country()).toBeNull()
    cleanup()
    render(strip(cqww))
    await typeCall('W6XYZ')
    expect(country()!.textContent).toBe('United States · CQ 3')
  })

  it('shows nothing for a call the country file cannot place', async () => {
    render(strip(cqww))
    await typeCall('ZZ9ZZZ')
    expect(country()?.textContent ?? '').toBe('')
  })
})
