// @vitest-environment jsdom
//
// THE CLUB BOARD AS A BAND × MODE GRID, above its table: the contest's bands across, its own mode
// classes down (the Summary's `modeColumns`), each position's name in the cell of its band and
// class. On the host's own screen each name is coloured by its link, with a mark and a word beside
// the colour, never the colour alone; this laptop is marked as such; a removed position is not in
// the grid; one with no band yet is named on a line under it. On a board without link words, a
// position the host has not heard lately is greyed and marked ⚠ in the grid exactly when its line in
// the table is. Read here the way a screen reader reads it, as a table with headers, on every host of
// the board but Remote (RemoteFieldDay.test.tsx).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, within } from '@testing-library/react'
import { ContestView, FdClubSection } from './ContestView'
import { DetachedPanel } from '../DetachedPanel'
import { getSettings, subscribeSnapshot } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { AppSnapshot, DupeRule, FdClubBoardRow, FdClubStatus, FieldDayStatus } from '../types'

// THE BUDGET: the house budget for a real-render file; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  // The host's club log under its board: none listed here.
  fdClubLogNewest: vi.fn(async () => null),
  fdClubGivePosition: vi.fn(async () => ({ outcome: 'given' })),
  fdClubRemovePosition: vi.fn(async () => ({ outcome: 'done' })),
  fdClubAllowBack: vi.fn(async () => ({ outcome: 'done' })),
  fdClubImport: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
  // The pop-out window's own loaders.
  subscribeSnapshot: vi.fn(() => () => {}),
  getBandPlan: vi.fn(async () => []),
  getPropagation: vi.fn(async () => null),
  getNeedAlerts: vi.fn(async () => []),
  getFdRuleset: vi.fn(async () => null),
  selectPeer: vi.fn(async () => null),
}))

const row = (posid: string, posName: string, extra: Partial<FdClubBoardRow>): FdClubBoardRow => ({
  posid,
  posName,
  band: '20m',
  mode: 'CW',
  operator: 'W9ABC',
  qsos: 10,
  rate: 4,
  lastSeenSecs: 1,
  ...extra,
})

/** The Illinois QSO Party's rules data as the screen has it: its bands, and CW and digital as one mode. */
const ILQP_BANDS = ['160m', '80m', '40m', '20m', '15m', '10m', '6m', '2m']
const ILQP_RULE: DupeRule = {
  byCall: true,
  byBand: true,
  byModeClass: true,
  byFields: ['QTH'],
  bySentFields: ['QTH'],
  modeClassGroups: [['CW', 'DIG']],
  logDupes: false,
}

/** The host's board: its own position, one of each link a position can be in, two on one cell (CW
 *  and digital fold into one class at the party), a removed one, and one with no band yet. */
const BOARD: FdClubBoardRow[] = [
  row('aaaa0001', 'Comms Trailer', { band: '20m', mode: 'PH', link: 'host' }),
  row('bbbb0002', 'CW tent', { band: '40m', mode: 'CW', link: 'connected', heardSecs: 3 }),
  row('cccc0003', 'Digital', { band: '40m', mode: 'DIG', link: 'reconnecting', heardSecs: 12 }),
  row('dddd0004', 'GOTA', { band: '6m', mode: 'PH', link: 'lost', heardSecs: 4 * 60 + 10 }),
  row('eeee0005', 'VHF tent', { band: '2m', mode: 'PH', link: 'removed', heardSecs: 2 * 3600 + 1 }),
  row('ffff0006', 'Rover', { band: '', mode: 'CW', link: 'connected', heardSecs: 1 }),
]
const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC QSO Party',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: BOARD,
}
/** The same board as a position's laptop is sent it: no link words on any line. */
const POSITION: FdClubStatus = {
  ...HOST,
  hosting: false,
  board: BOARD.filter((r) => r.link !== 'removed').map(({ link: _link, heardSecs: _heard, ...r }) => r),
}
const fd = (club: FdClubStatus, extra: Partial<FieldDayStatus> = {}): FieldDayStatus => ({
  composing: [{ key: 'QTH', raw: 'COOK' }],
  running: false,
  state: 'Listening',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'ilqp',
  bands: ILQP_BANDS,
  dupeRule: ILQP_RULE,
  club,
  ...extra,
})

const CAPTION = 'Positions by band and mode'

/** The grid as a screen reader reads it: the column heads, and each row's head with the names in
 *  each of its cells. */
function readGrid(root: HTMLElement = document.body) {
  const table = within(root).getByRole('table', { name: CAPTION })
  const [head, ...body] = within(table).getAllByRole('row')
  return {
    cols: within(head).getAllByRole('columnheader').map((c) => c.textContent ?? ''),
    rows: body.map((r) => ({
      head: within(r).getByRole('rowheader').textContent ?? '',
      cells: within(r)
        .getAllByRole('cell')
        .map((c) => [...c.querySelectorAll('.club-grid-name')].map((n) => n.textContent ?? '')),
    })),
  }
}
/** The names in the cell a reader finds by its band's column head and its mode's row head. */
function cellAt(band: string, mode: string): string[] {
  const { cols, rows } = readGrid()
  const col = cols.indexOf(band)
  const r = rows.find((x) => x.head === mode)
  expect(col, `a ${band} column`).toBeGreaterThan(0)
  expect(r, `a ${mode} row`).toBeTruthy()
  return r!.cells[col - 1]
}
/** Every name the grid places, cell by cell. */
const placed = () => readGrid().rows.flatMap((r) => r.cells.flat())

/** A position's name in the grid or on its no-band line, by the name it shows. */
function chipOf(name: string, root: ParentNode = document): HTMLElement {
  const chips = [...root.querySelectorAll<HTMLElement>('.club-grid-block .club-grid-pos')].filter(
    (c) => c.querySelector('.club-grid-name')?.textContent === name,
  )
  expect(chips, `one ${name} in the grid`).toHaveLength(1)
  return chips[0]
}
/** What a name says of its link: its link class, the mark beside it (hidden from a reader, who gets
 *  the word; the space after it is the line's break point), the word a reader hears, and what
 *  hovering it says. */
function linkOf(chip: HTMLElement) {
  const mark = chip.querySelector('.club-grid-mark')
  return {
    class: [...chip.classList].filter((c) => c.startsWith('link-')).join(' '),
    mark: mark?.textContent?.trim() ?? '',
    markHidden: mark?.getAttribute('aria-hidden') ?? null,
    word: chip.querySelector('.sr-only')?.textContent ?? '',
    title: chip.getAttribute('title') ?? '',
  }
}

afterEach(() => {
  cleanup()
  vi.mocked(getSettings).mockImplementation(async () => ({ ...defaultSettings }) as never)
  vi.mocked(subscribeSnapshot).mockImplementation(() => () => {})
})

describe('the club board as a band × mode grid', () => {
  it('places each position in the cell of its band and the contest’s own mode class, two in one cell together', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    const { cols, rows } = readGrid()
    expect(cols).toEqual(['Mode', ...ILQP_BANDS])
    // The party counts CW and digital as one mode, so they are one row, as on its Summary.
    expect(rows.map((r) => r.head)).toEqual(['CW/DIG', 'PH'])
    expect(cellAt('40m', 'CW/DIG')).toEqual(['CW tent', 'Digital'])
    expect(cellAt('20m', 'PH')).toEqual(['Comms Trailer'])
    expect(cellAt('6m', 'PH')).toEqual(['GOTA'])
    // …and nowhere else: four names, each once.
    expect(placed().sort()).toEqual(['CW tent', 'Comms Trailer', 'Digital', 'GOTA'])
  })

  it('colours each name by its link, with a mark beside the colour and the word for a reader', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(linkOf(chipOf('CW tent'))).toEqual({
      class: 'link-connected',
      mark: '✓',
      markHidden: 'true',
      word: 'Connected',
      title: 'Connected (heard 3 s ago)',
    })
    expect(linkOf(chipOf('Digital'))).toEqual({
      class: 'link-reconnecting',
      mark: '↻',
      markHidden: 'true',
      word: 'Reconnecting',
      title: 'Reconnecting (heard 12 s ago)',
    })
    expect(linkOf(chipOf('GOTA'))).toEqual({
      class: 'link-lost',
      mark: '✕',
      markHidden: 'true',
      word: 'Lost',
      title: 'Lost (heard 4 min ago)',
    })
    // The host's own position is this laptop, as its line in the table says.
    expect(linkOf(chipOf('Comms Trailer'))).toEqual({
      class: 'link-self',
      mark: '⌂',
      markHidden: 'true',
      word: 'This laptop',
      title: 'This laptop',
    })
  })

  it('says what each colour and mark is, in a legend under the grid', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    const legend = document.querySelector('.club-grid-legend') as HTMLElement
    // Every name already says its word to a reader; the legend is for the eye.
    expect(legend.getAttribute('aria-hidden')).toBe('true')
    expect(
      [...legend.querySelectorAll<HTMLElement>('.club-grid-pos')].map((k) => [
        [...k.classList].find((c) => c.startsWith('link-')),
        k.textContent,
      ]),
    ).toEqual([
      ['link-connected', '✓ Connected'],
      ['link-reconnecting', '↻ Reconnecting'],
      ['link-lost', '✕ Lost'],
      ['link-self', '⌂ This laptop'],
    ])
  })

  it('leaves a removed position out of the grid, and keeps it in the table with its Allow back', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} onRemovePosition={vi.fn()} onAllowBack={vi.fn()} />)
    expect(placed()).not.toContain('VHF tent')
    expect(cellAt('2m', 'PH')).toEqual([])
    expect(document.querySelector('.club-grid-block')?.textContent).not.toContain('VHF tent')
    // CONTROL: the table still lists it, with the button that lets it back.
    const table = document.querySelector('[data-club-board]') as HTMLElement
    expect(table.textContent).toContain('VHF tent')
    expect(within(table).getByRole('button', { name: 'Allow back' })).toBeTruthy()
    // …and a position the host let back is in the grid again, in its cell.
    cleanup()
    render(
      <FdClubSection
        club={{ ...HOST, board: BOARD.map((r) => (r.link === 'removed' ? { ...r, link: 'connected' as const } : r)) }}
        bands={ILQP_BANDS}
        dupeRule={ILQP_RULE}
        fieldDay={false}
      />,
    )
    expect(cellAt('2m', 'PH')).toEqual(['VHF tent'])
  })

  it('names a position with no band yet on a line under the grid, never in a cell', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(placed()).not.toContain('Rover')
    const line = document.querySelector('.club-grid-waiting') as HTMLElement
    expect(line.textContent).toBe('No band yet:✓ RoverConnected')
    expect(linkOf(chipOf('Rover', line)).class).toBe('link-connected')
    // CONTROL: once it reports a band it is in that cell, and the line is gone.
    cleanup()
    render(
      <FdClubSection
        club={{ ...HOST, board: BOARD.map((r) => (r.posName === 'Rover' ? { ...r, band: '80m' } : r)) }}
        bands={ILQP_BANDS}
        dupeRule={ILQP_RULE}
        fieldDay={false}
      />,
    )
    expect(cellAt('80m', 'CW/DIG')).toEqual(['Rover'])
    expect(document.querySelector('.club-grid-waiting')).toBeNull()
  })

  it('gives a band the contest does not use a column of its own, so a station on it still shows', () => {
    render(
      <FdClubSection
        club={{ ...HOST, board: [...BOARD, row('abab0007', 'WARC tent', { band: '30m', mode: 'CW', link: 'connected', heardSecs: 2 })] }}
        bands={ILQP_BANDS}
        dupeRule={ILQP_RULE}
        fieldDay={false}
      />,
    )
    expect(readGrid().cols).toEqual(['Mode', ...ILQP_BANDS, '30m'])
    expect(cellAt('30m', 'CW/DIG')).toEqual(['WARC tent'])
  })

  it('runs Field Day on its own bands and three classes, which its rules data does not list', () => {
    render(<FdClubSection club={HOST} />)
    const { cols, rows } = readGrid()
    expect(cols).toEqual(['Mode', '160m', '80m', '40m', '20m', '15m', '10m', '6m', '2m', '1.25m', '70cm'])
    expect(rows.map((r) => r.head)).toEqual(['CW', 'DIG', 'PH'])
    expect(cellAt('40m', 'CW')).toEqual(['CW tent'])
    expect(cellAt('40m', 'DIG')).toEqual(['Digital'])
  })

  it('a contest that names no bands and counts no mode class shows the bands and classes its positions are on', () => {
    const cqww: DupeRule = { ...ILQP_RULE, byModeClass: false, modeClassGroups: [], byFields: [], bySentFields: [], logDupes: true }
    render(<FdClubSection club={HOST} dupeRule={cqww} fieldDay={false} />)
    const { cols, rows } = readGrid()
    expect(cols).toEqual(['Mode', '40m', '20m', '6m'])
    expect(rows.map((r) => r.head)).toEqual(['CW', 'DIG', 'PH'])
    expect(placed().sort()).toEqual(['CW tent', 'Comms Trailer', 'Digital', 'GOTA'])
  })

  it('on a position’s board colours nothing, and marks this laptop by its own position id', () => {
    render(<FdClubSection club={POSITION} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} ownPosid="dddd0004" />)
    expect(cellAt('6m', 'PH')).toEqual(['GOTA'])
    expect(linkOf(chipOf('GOTA'))).toEqual({ class: 'link-self', mark: '⌂', markHidden: 'true', word: 'This laptop', title: 'This laptop' })
    for (const name of ['CW tent', 'Digital', 'Comms Trailer']) {
      expect(linkOf(chipOf(name)), name).toEqual({ class: '', mark: '', markHidden: null, word: '', title: '' })
    }
    const legend = document.querySelector('.club-grid-legend') as HTMLElement
    expect(legend.textContent).toBe('⚠ Not heard for more than 15 s⌂ This laptop')
    // CONTROL: with no id of this laptop's, no name is marked, and the legend says only what ⚠ is.
    cleanup()
    render(<FdClubSection club={POSITION} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(document.querySelectorAll('.club-grid-block .link-self')).toHaveLength(0)
    expect(document.querySelector('.club-grid-legend')?.textContent).toBe('⚠ Not heard for more than 15 s')
    expect(placed().sort()).toEqual(['CW tent', 'Comms Trailer', 'Digital', 'GOTA'])
  })

  it('sits above the positions table, which keeps every row', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    const block = document.querySelector('.club-grid-block') as HTMLElement
    const table = document.querySelector('[data-club-board]') as HTMLElement
    expect(block.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    for (const r of BOARD) expect(table.querySelector(`[data-club-row="${r.posid}"]`), r.posName).not.toBeNull()
  })

  it('says each link in the table in the colour the grid gives it: Lost in red, Reconnecting in amber', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    const table = document.querySelector('[data-club-board]') as HTMLElement
    expect(within(table).getByText('Lost').style.color).toBe('var(--alert-critical)')
    expect(within(table).getByText('Reconnecting').style.color).toBe('var(--alert-warning)')
    expect(within(table).getAllByText('Connected').map((w) => w.style.color)).toEqual([
      'var(--status-confirmed)',
      'var(--status-confirmed)',
    ])
  })

  it('draws no grid for a board with nobody on it yet', () => {
    render(<FdClubSection club={{ ...HOST, board: [] }} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.getByText(/no positions heard yet/i)).toBeTruthy()
  })
})

/** A position's board with the named positions last heard by the host that many seconds before it
 *  sent the board. */
const silentFor = (ages: Record<string, number>): FdClubStatus => ({
  ...POSITION,
  board: POSITION.board.map((r) => (r.posName in ages ? { ...r, lastSeenSecs: ages[r.posName] } : r)),
})
/** What a name says of the host not having heard it lately: whether it is greyed, the mark after it
 *  (hidden from a reader, who gets the word), every word a reader hears, and what hovering it says. */
function quietOf(chip: HTMLElement) {
  const mark = chip.querySelector('.club-grid-name ~ .club-grid-mark')
  return {
    greyed: chip.classList.contains('stale'),
    mark: mark?.textContent ?? '',
    markHidden: mark?.getAttribute('aria-hidden') ?? null,
    words: [...chip.querySelectorAll('.sr-only')].map((w) => w.textContent ?? ''),
    title: chip.getAttribute('title') ?? '',
  }
}
/** Whether the positions table marks a position's line as not heard: ⚠ after its name. */
function tableMarks(posid: string): boolean {
  const name = document.querySelector(`[data-club-board] [data-club-row="${posid}"]`) as HTMLElement
  return name.textContent?.endsWith(' ⚠') ?? false
}
const NOT_HEARD = 'Not heard for more than 15 s'

describe('a position the host has not heard lately, in the grid as in the table', () => {
  it('on a position’s board greys its name and marks it ⚠, with its word for a reader; a fresh one not', () => {
    render(
      <FdClubSection
        club={silentFor({ GOTA: 250, Digital: 16, 'CW tent': 15, Rover: 40 })}
        bands={ILQP_BANDS}
        dupeRule={ILQP_RULE}
        fieldDay={false}
      />,
    )
    for (const [name, secs] of [['GOTA', 250], ['Digital', 16]] as const)
      expect(quietOf(chipOf(name)), name).toEqual({
        greyed: true,
        mark: ' ⚠',
        markHidden: 'true',
        words: [NOT_HEARD],
        title: `Last heard ${secs} s ago`,
      })
    // Heard 15 s ago is not yet past it, as its line in the table says; nor is one heard a second ago.
    for (const name of ['CW tent', 'Comms Trailer'])
      expect(quietOf(chipOf(name)), name).toEqual({ greyed: false, mark: '', markHidden: null, words: [], title: '' })
    // Greyed where it is, never moved: in its cell, and on the no-band line.
    expect(cellAt('6m', 'PH')).toEqual(['GOTA'])
    expect(cellAt('40m', 'CW/DIG')).toEqual(['CW tent', 'Digital'])
    expect(quietOf(chipOf('Rover', document.querySelector('.club-grid-waiting')!)).greyed).toBe(true)
  })

  it('marks exactly the positions whose line the table marks, at every age', () => {
    const seen = new Set<boolean>()
    for (const secs of [0, 1, 14, 15, 16, 17, 60, 250, 3600]) {
      render(<FdClubSection club={silentFor({ GOTA: secs })} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
      const table = tableMarks('dddd0004')
      const grid = quietOf(chipOf('GOTA'))
      expect({ secs, greyed: grid.greyed, marked: grid.mark === ' ⚠' }).toEqual({ secs, greyed: table, marked: table })
      seen.add(table)
      cleanup()
    }
    // CONTROL: the ages cross the table's threshold, so the grid was compared with both of its answers.
    expect(seen).toEqual(new Set([true, false]))
  })

  it('on the host’s own screen the link word says it: no name greys or takes ⚠, and the link colours stay', () => {
    // Every presence aged past the threshold: the host's board marks by each line's link instead.
    render(
      <FdClubSection
        club={{ ...HOST, board: BOARD.map((r) => ({ ...r, lastSeenSecs: 250 })) }}
        bands={ILQP_BANDS}
        dupeRule={ILQP_RULE}
        fieldDay={false}
      />,
    )
    const block = document.querySelector('.club-grid-block') as HTMLElement
    expect(block.querySelectorAll('.stale')).toHaveLength(0)
    expect(block.textContent).not.toContain('⚠')
    expect(linkOf(chipOf('GOTA'))).toEqual({ class: 'link-lost', mark: '✕', markHidden: 'true', word: 'Lost', title: 'Lost (heard 4 min ago)' })
    expect(linkOf(chipOf('CW tent')).class).toBe('link-connected')
    expect(linkOf(chipOf('Comms Trailer')).class).toBe('link-self')
    // CONTROL: the table marks none of their lines ⚠ either.
    expect(BOARD.map((r) => tableMarks(r.posid))).toEqual(BOARD.map(() => false))
  })

  it('this laptop not heard keeps its own mark and edge, greys and takes ⚠, and a reader hears both words', () => {
    render(<FdClubSection club={silentFor({ GOTA: 250 })} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} ownPosid="dddd0004" />)
    const chip = chipOf('GOTA')
    expect(linkOf(chip).class).toBe('link-self')
    expect([...chip.querySelectorAll('.club-grid-mark')].map((m) => m.textContent)).toEqual(['⌂ ', ' ⚠'])
    expect(quietOf(chip)).toEqual({
      greyed: true,
      mark: ' ⚠',
      markHidden: 'true',
      words: ['This laptop', NOT_HEARD],
      title: 'Last heard 250 s ago',
    })
  })

  it('says what ⚠ is in the legend wherever a name can carry it, and never on the host’s own screen', () => {
    const keys = () =>
      [...document.querySelectorAll<HTMLElement>('.club-grid-legend .club-grid-pos')].map((k) => [k.className, k.textContent])
    // A position's board, nobody unheard right now: the key is there all the same, as the host's
    // link colours are.
    render(<FdClubSection club={POSITION} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(keys()).toEqual([['club-grid-pos stale', `⚠ ${NOT_HEARD}`]])
    // CONTROL: the host's own screen, where no name can carry it, keys its link colours and nothing more.
    cleanup()
    render(<FdClubSection club={{ ...HOST, board: BOARD.map((r) => ({ ...r, lastSeenSecs: 250 })) }} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect(keys()).toEqual([
      ['club-grid-pos link-connected', '✓ Connected'],
      ['club-grid-pos link-reconnecting', '↻ Reconnecting'],
      ['club-grid-pos link-lost', '✕ Lost'],
      ['club-grid-pos link-self', '⌂ This laptop'],
    ])
  })
})

describe('every host of the board draws the grid from the contest it runs', () => {
  it('the Contest screen: the contest’s bands and classes, and this laptop by its settings', async () => {
    vi.mocked(getSettings).mockImplementation(async () => ({ ...defaultSettings, fdPositionId: 'bbbb0002' }) as never)
    render(<ContestView fieldDay={fd(POSITION)} onSetMode={() => {}} />)
    await act(async () => {})
    expect(readGrid().cols).toEqual(['Mode', ...ILQP_BANDS])
    expect(readGrid().rows.map((r) => r.head)).toEqual(['CW/DIG', 'PH'])
    expect(linkOf(chipOf('CW tent')).class).toBe('link-self')
  })

  it('the logger window, which shows this laptop’s own Contest screen', async () => {
    vi.mocked(getSettings).mockImplementation(async () => ({ ...defaultSettings, fdPositionId: 'bbbb0002' }) as never)
    render(<ContestView fieldDay={fd(HOST)} logger />)
    await act(async () => {})
    expect(cellAt('40m', 'CW/DIG')).toEqual(['CW tent', 'Digital'])
    expect(linkOf(chipOf('GOTA')).class).toBe('link-lost')
  })

  it('the board’s own pop-out window, at the glance size', async () => {
    vi.mocked(getSettings).mockImplementation(async () => ({ ...defaultSettings, fdPositionId: 'cccc0003' }) as never)
    const snap = { link: { tier: 'FT8' }, radio: {}, fieldDay: fd(POSITION) } as unknown as AppSnapshot
    vi.mocked(subscribeSnapshot).mockImplementation(((cb: (s: AppSnapshot) => void) => {
      cb(snap)
      return () => {}
    }) as never)
    render(<DetachedPanel panel="fdclub" />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(readGrid().cols).toEqual(['Mode', ...ILQP_BANDS])
    expect(cellAt('40m', 'CW/DIG')).toEqual(['CW tent', 'Digital'])
    expect(linkOf(chipOf('Digital')).class).toBe('link-self')
    expect((document.querySelector('.club-grid-block') as HTMLElement).dataset.glance).toBe('')
  })

  it('CONTROL: the docked board is not at the glance size', () => {
    render(<FdClubSection club={HOST} bands={ILQP_BANDS} dupeRule={ILQP_RULE} fieldDay={false} />)
    expect((document.querySelector('.club-grid-block') as HTMLElement).dataset.glance).toBeUndefined()
  })
})
