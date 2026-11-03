// @vitest-environment jsdom
//
// #357 — THE CALL ROSTER'S PARK COLUMN. A POTA hunter asked for a column of its own for the park,
// sortable so the parks stay at the top of the list while hunting.
//
// The roster knows a station is out at a park two ways, and the column reads both:
//   · a POTA/SOTA spot names the activation (the need alert's `park`, the reference itself);
//   · the station's own frame says so (`CQ POTA`, `CQ SOTA`: the CQ modifier), with no reference.
// A spot's reference wins over the bare word. Everything else is a blank, and blanks sort LAST in
// both directions: the direction orders the parks among themselves, it never sinks them.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import type { NeedAlert, NeedTag, Station } from '../types'
import { NEED_TIER } from '../features/needs'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt } from '../cssCascade'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100

function station(call: string, over: Partial<Station> = {}): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'active',
    worked: false,
    country: 'United States',
    ...over,
  }
}

const parkAlert = (call: string, reference: string, program = 'POTA', tags: NeedTag[] = ['Pota']): NeedAlert => ({
  call,
  entity: 'United States',
  band: '20m',
  zone: 4,
  tags,
  priority: NEED_TIER[tags[0]] ?? 0,
  headline: '',
  mode: 'Digital',
  freqMhz: 14.074,
  park: { program, reference },
})

const STATIONS = [
  station('W1LOUD', { snr: 5 }), // no park, loudest: the default sort would put it high
  station('K2PARK', { snr: -15 }), // spotted at US-1234
  station('N3CQ', { snr: -12, cqDir: 'POTA' }), // CQ POTA, never spotted
  station('W4SOTA', { snr: -18 }), // spotted on a summit
  station('KA5ABC', { snr: 0 }), // no park
]
const ALERTS = new Map<string, NeedAlert[]>([
  ['K2PARK', [parkAlert('K2PARK', 'US-1234')]],
  ['W4SOTA', [parkAlert('W4SOTA', 'W4C/CM-001', 'SOTA', ['Sota'])]],
])

function mount() {
  return render(
    <OperateRoster
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={ALERTS}
      band="20m"
      feedMode="FT8"
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

const rowOf = (call: string): HTMLElement | null =>
  (screen.queryByText(call)?.closest('[role="row"]') as HTMLElement | null | undefined) ?? null
const parkCell = (call: string) => rowOf(call)?.querySelector('.or-park')?.textContent ?? null
/** The data rows' calls, top to bottom. */
const order = () =>
  [...document.querySelectorAll('.or-row:not(.or-header) .or-call')].map((c) => c.firstChild?.textContent)

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('#357 the Call Roster Park column', () => {
  it('has a Park heading the operator can sort by', () => {
    mount()
    expect(screen.getByRole('button', { name: /^Park/ })).not.toBeNull()
  })

  it('shows the spotted reference, the bare program for a CQ POTA, and a dash for the rest', () => {
    mount()
    expect(parkCell('K2PARK')).toBe('US-1234')
    expect(parkCell('W4SOTA')).toBe('W4C/CM-001')
    expect(parkCell('N3CQ')).toBe('POTA')
    expect(parkCell('W1LOUD')).toBe('—')
    expect(parkCell('KA5ABC')).toBe('—')
  })

  it('sorting by Park puts every park on top, and they STAY on top when the direction flips', () => {
    mount()
    const head = screen.getByRole('button', { name: /^Park/ })
    fireEvent.click(head)
    const parks = new Set(['K2PARK', 'N3CQ', 'W4SOTA'])
    const first = order()
    expect(new Set(first.slice(0, 3)), `parks not on top: ${first.join(' ')}`).toEqual(parks)
    fireEvent.click(head) // the other direction
    const second = order()
    expect(new Set(second.slice(0, 3)), `parks sank on the flip: ${second.join(' ')}`).toEqual(parks)
    // …and the direction did reorder the parks among themselves.
    expect(second.slice(0, 3)).toEqual([...first.slice(0, 3)].reverse())
  })

  it('the control: the default Need sort does not group the parks', () => {
    mount()
    expect(order()[0]).toBe('W1LOUD')
  })
})

// THE COLUMN TAKES NO ROOM UNTIL IT HAS SOMETHING TO SAY. A Park track on every roster grew the row's
// minimum width by a whole column for an operator who never hunts parks, at the 1024×768 floor where
// the roster sits on its minimum. So the column (header, cells and grid track) appears only while a
// station on the roster is at a park, or while the roster is sorted by Park. Measured on the cascade
// the RENDERED roster reaches: the track list that wins on its rows, and that header and data rows
// carry exactly as many cells as that list has tracks (so they can never fall out of line).

const RULES = parseRules(readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''))
/** The winning grid-template-columns on a rendered row, split into its tracks. */
function tracksOf(row: Element): string[] {
  const won = winnerAt(RULES, 'dark', chainOf(row), 'grid-template-columns')
  expect(won, 'no grid-template-columns reaches the roster row').not.toBeNull()
  // Top-level tracks: split on whitespace outside parentheses.
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of won!.value) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (/\s/.test(ch) && depth === 0) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) out.push(cur)
  return out
}
/** The sum of each track's px floor at Normal text — the row's minimum width before gaps. */
const floorPx = (tracks: string[]) =>
  tracks.reduce((sum, t) => sum + Number(/(\d+(?:\.\d+)?)px/.exec(t)?.[1] ?? 0), 0)

function mountWith(stations: Station[], alerts: Map<string, NeedAlert[]>, sort?: { key: 'park' | 'need'; dir: 'asc' | 'desc' }) {
  return render(
    <OperateRoster
      stations={stations}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={alerts}
      band="20m"
      feedMode="FT8"
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
      sort={sort}
      onSort={sort ? () => {} : undefined}
    />,
  )
}
const header = () => document.querySelector('.or-row.or-header')!
const firstRow = () => document.querySelector('.or-row:not(.or-header)')!

describe('#357 the Park column takes no width until it is needed', () => {
  it('with no station at a park: no Park heading, no Park cells, and the row keeps its old ten tracks', () => {
    mountWith([station('W1LOUD'), station('KA5ABC')], new Map())
    expect(screen.queryByRole('button', { name: /^Park/ })).toBeNull()
    expect(document.querySelector('.or-park')).toBeNull()
    const tracks = tracksOf(firstRow())
    expect(tracks).toHaveLength(10)
    expect(header().children).toHaveLength(10)
    expect(firstRow().children).toHaveLength(10)
    // The floor main shipped: Call 124 · Calling 68 · Need 105 · Country 50 · State 40 · Grid 54 ·
    // Dist 56 · Brg 48 · SNR 44 · Age 48.
    expect(floorPx(tracks)).toBe(637)
  })

  it('with a station at a park: the Park track joins, after Need, and every row carries its cell', () => {
    mountWith(STATIONS, ALERTS)
    const tracks = tracksOf(firstRow())
    expect(tracks).toHaveLength(11)
    expect(tracks[3]).toContain('60px')
    expect(floorPx(tracks)).toBe(697)
    expect(header().children).toHaveLength(11)
    for (const row of document.querySelectorAll('.or-row:not(.or-header)')) expect(row.children).toHaveLength(11)
  })

  it('sorted by Park, the column stays even when no station is at a park, so the sort stays on screen', () => {
    mountWith([station('W1LOUD')], new Map(), { key: 'park', dir: 'asc' })
    expect(screen.getByRole('button', { name: /^Park/ })).not.toBeNull()
    expect(tracksOf(firstRow())).toHaveLength(11)
  })
})
