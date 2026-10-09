// @vitest-environment jsdom
//
// ⊞ PANELS ▸ ARRANGE IN THE FT COCKPIT (2026-10-07, the operator's "Arrange on FT's grid" and "Two
// saved arrangements").
//
// FT IS THE DEFAULT MODE, so the first thing this file holds is that nobody's FT screen changes on the
// update: until the operator moves a pane or adds a box, the lower region is today's tree, element for
// element, in both layouts, either rail side, with and without the callsign card and with panes hidden.
// The golden was captured from the cockpit as it was before Arrange came to FT; regenerate it only for a
// change that is MEANT to alter the stock tree: `NEXUS_REGEN_GOLDEN=1 vitest run
// src/components/OperateCockpit.arrange.test.tsx`.
//
// The tree is walked down to each pane and no further: a pane's own insides are not what Arrange
// changes, and other work changes them.
//
// THEN WHAT ARRANGE DOES, on the REAL panel record and the real ⊞ menu: a move puts the region in keyed
// columns that follow the placement, Classic and Roster each keep their own, the operator's example
// screen (the Call Roster and Rx Frequency together, Spots and POTA/SOTA boxes in the rail) can be built,
// a decode window's rows come back after a move, a double-click still calls the station through the
// cockpit's own handler, a pane moved up or down is not remounted, two adjacent feeds share a divider,
// the rail on the left stands first, and Reset brings today's tree back. The boxes' bodies are stubbed
// (a box's frame, picker and ✕ are real).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, DecodeRow } from '../types'
import { BOX_IDS, OPERATE_ARRANGE, OPERATE_PANELS, panelStorageKey, usePanelLayout } from '../features/panelState'
import type { OperateLayout, OperatePanelId, PanelLayoutApi } from '../features/panelState'
import { arrangeIds, type PaneMove } from '../features/panelPlace'
import type { BoxSource } from './panes/CockpitBox'
import { gripOf, paneBoxOf, pickUp, release, stubLayout } from './panes/PaneDrag.testkit'
import { t } from '../i18n'
import { startCq } from '../api'

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module (stop-line.api.testkit.ts says why); null answers, as the other FT
  // suites give, with the few a mount reads.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    getDeclination: vi.fn(async () => 0),
    resolveEntity: vi.fn(async () => 'United States'),
  }
})
// Canvas children only, and a box's body (a Conditions box: its own suites render it).
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./panes/BoxBody', () => ({ BoxBody: ({ pane }: { pane: string }) => <div data-testid={`box-body-${pane}`} /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  stations: [
    { call: 'W1ABC', grid: 'FN42', snr: -7, lastHeardSlot: 0, heardCount: 3, presence: 'live', worked: false, country: 'United States' },
  ],
  recentDecodes: [],
  conversations: [],
  highlights: [],
  harqRescues: 0,
  clearTick: 0,
  qso: null,
  link: { tier: 'FT8', periodSecs: 15 },
  radio: {
    dialMhz: 14.074,
    band: '20m',
    sideband: 'USB',
    slot: 0,
    source: 'native',
    sourceLabel: 'Native',
    nextSlotMs: 5000,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    txEven: true,
    txCycleAuto: true,
    txEnabled: false,
    txAllowed: true,
    transmitting: false,
    tuning: false,
    atu: true,
    qsoRecording: false,
    catOk: true,
    splitTxMhz: null,
    decodeDepth: 2,
  },
} as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

let api: PanelLayoutApi<OperatePanelId> | null = null
const noop = () => {}

/** What App lends the boxes on the desktop. */
const SOURCE: BoxSource = { myGrid: 'EN61', theme: 'dark', stations: [], prop: null, needByCall: new Map() }

interface LiveProps {
  layoutMode: OperateLayout
  selectedCall?: string | null
  snapOver?: Partial<AppSnapshot>
  onCall?: (call: string, grid?: string, message?: string, snr?: number, freq?: number) => void
  boxes?: BoxSource
  active?: boolean
}

/** The cockpit over a LIVE panel record (the hook App owns), so a move writes and the cockpit re-reads. */
function Live({ layoutMode, selectedCall = null, snapOver, onCall = noop, boxes, active = true }: LiveProps) {
  const panels = usePanelLayout(OPERATE_PANELS)
  api = panels
  return (
    <OperateCockpit
      snap={snapOver ? ({ ...snap, ...snapOver } as AppSnapshot) : snap}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={onCall}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onSetHoldTxFreq={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={selectedCall}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panels}
      boxes={boxes}
      active={active}
    />
  )
}

async function settle() {
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}

/** The pane wrappers FT has always drawn: below one of these the tree is the pane's own. */
const PANE_WRAPPERS = ['cockpit-decodes', 'cockpit-decodes-side', 'cockpit-rxfreq', 'cockpit-roster', 'cockpit-roster-main']

/** The lower region's tree, element by element, down to each pane: tag, classes and every other
 *  attribute, sorted. A pane's insides (anything inside a pane wrapper, the Tx1–Tx6 machine, the card
 *  and a divider) are not walked. */
function treeOf(el: Element, depth = 0): string {
  const cls = el.getAttribute('class')
  const attrs = [...el.attributes]
    .filter((a) => a.name !== 'class')
    .map((a) => `${a.name}="${a.value}"`)
    .sort()
  const head = `${'  '.repeat(depth)}<${el.tagName.toLowerCase()}${cls ? ` class="${cls}"` : ''}${attrs.length ? ` ${attrs.join(' ')}` : ''}>`
  const leaf =
    el.classList.contains('tx-panel') ||
    el.classList.contains('recall-card') ||
    el.classList.contains('pane-splitter') ||
    PANE_WRAPPERS.some((w) => el.parentElement?.classList.contains(w))
  if (leaf) return head
  return [head, ...[...el.children].map((c) => treeOf(c, depth + 1))].join('\n')
}

/** Today's stock screens: both layouts, either rail side, the card shown or not, and panes hidden (the
 *  column collapse). Each is a fresh mount over a fresh record. */
const STOCK: Array<{ name: string; layoutMode: 'classic' | 'roster'; rail?: 'left'; call?: string; hidden?: OperatePanelId[] }> = [
  { name: 'classic', layoutMode: 'classic' },
  { name: 'classic, rail on the left', layoutMode: 'classic', rail: 'left' },
  { name: 'classic, the card shown', layoutMode: 'classic', call: 'W1ABC' },
  { name: 'classic, the card shown, rail on the left', layoutMode: 'classic', rail: 'left', call: 'W1ABC' },
  { name: 'classic, Stations hidden', layoutMode: 'classic', hidden: ['stations'] },
  { name: 'classic, Band Activity hidden', layoutMode: 'classic', hidden: ['bandActivity'] },
  { name: 'classic, Rx Frequency and Tx messages hidden', layoutMode: 'classic', hidden: ['rxfreq', 'txmsgs'] },
  { name: 'classic, the card shown, Stations hidden, rail on the left', layoutMode: 'classic', rail: 'left', call: 'W1ABC', hidden: ['stations'] },
  { name: 'roster', layoutMode: 'roster' },
  { name: 'roster, rail on the left', layoutMode: 'roster', rail: 'left' },
  { name: 'roster, the card shown', layoutMode: 'roster', call: 'W1ABC' },
  { name: 'roster, Call Roster hidden', layoutMode: 'roster', hidden: ['callRoster'] },
  { name: 'roster, Band Activity and Rx Frequency hidden', layoutMode: 'roster', hidden: ['bandActivity', 'rxfreq'] },
  { name: 'roster, Rx Frequency hidden, rail on the left', layoutMode: 'roster', rail: 'left', hidden: ['rxfreq'] },
]

async function stockTrees(): Promise<string> {
  const out: string[] = []
  for (const s of STOCK) {
    localStorage.clear()
    if (s.rail) localStorage.setItem('nexus.operate.railSide', s.rail)
    render(<Live layoutMode={s.layoutMode} selectedCall={s.call ?? null} />)
    await settle()
    for (const id of s.hidden ?? []) act(() => api!.setPanelState(id, 'removed'))
    await settle()
    const lower = document.querySelector('.cockpit-lower')
    expect(lower, `${s.name}: no lower region`).not.toBeNull()
    out.push(`## ${s.name}\n${treeOf(lower!)}\n`)
    cleanup()
  }
  localStorage.clear()
  return out.join('\n')
}

const GOLDEN = resolve(process.cwd(), 'src/components/__fixtures__/OperateCockpit.stock.golden.txt')

describe('until the operator arranges anything, FT draws today’s tree', () => {
  // Fourteen mounts of the whole FT screen: 0.4–0.7 s alone, so a budget past the 5 s default.
  it('every stock screen matches the golden captured before Arrange came to FT', async () => {
    expect(await stockTrees()).toBe(readFileSync(GOLDEN, 'utf8'))
  }, 30_000)

  // Regenerating is a deliberate act, never part of a normal run.
  it.skipIf(!process.env.NEXUS_REGEN_GOLDEN)('regenerate the golden', async () => {
    writeFileSync(GOLDEN, await stockTrees())
  })
})

// ── WHAT ARRANGE DOES ───────────────────────────────────────────────────────────────────────────────
const lower = () => document.querySelector('.cockpit-lower')!
const stacks = () => [...lower().querySelectorAll(':scope > .op-stack')] as HTMLElement[]
/** An arranged column's panes, top to bottom, by id (the card is the `.recall-card` it always was). */
const idsIn = (col: Element) =>
  [...col.children]
    .filter((c) => !c.classList.contains('pane-splitter'))
    .map((c) => c.getAttribute('data-pane') ?? (c.classList.contains('recall-card') ? 'recall' : `?${c.className}`))
const rendered = () => stacks().map(idsIn)
const seamsIn = (col: Element) => [...col.querySelectorAll(':scope > [role="separator"]')].map((s) => s.getAttribute('aria-label'))
const pane = (id: string) => document.querySelector<HTMLElement>(`.cockpit-lower [data-pane="${id}"]`)
const bodyOf = (b: string) => pane(b)?.querySelector('[data-testid^="box-body-"]')?.getAttribute('data-testid')
const all = () => true
function move(id: OperatePanelId, way: PaneMove, layout: OperateLayout) {
  act(() => api!.movePane!(id, way, all, false, { layout }))
}
/** One stock screen of the golden, by its name. */
const goldenSection = (name: string) => readFileSync(GOLDEN, 'utf8').split(/\n(?=## )/).find((p) => p.startsWith(`## ${name}\n`))
const decode = (over: Partial<DecodeRow>): DecodeRow => ({
  from: 'W1ABC',
  snr: -7,
  dtSec: 0.1,
  freqHz: 1200,
  message: 'CQ W1ABC FN42',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

describe('⊞ Arrange in FT', () => {
  it('a move draws the region as keyed columns that follow the placement, on the stock grid', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    expect(lower().hasAttribute('data-arranged'), 'arranged before anything moved').toBe(false)
    move('rxfreq', 'left', 'roster')
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect(rendered()).toEqual([['callRoster', 'rxfreq'], ['bandActivity']])
    // The rail is still an aside; the grid, its collapse and the divider between its columns are FT's own.
    expect(stacks().map((s) => s.tagName)).toEqual(['DIV', 'ASIDE'])
    expect(lower().classList.contains('roster')).toBe(true)
    expect(lower().getAttribute('data-cols')).toBe('two')
    expect([...lower().querySelectorAll(':scope > .op-colseam')].map((d) => d.getAttribute('aria-label'))).toEqual(['Call Roster / side rail'])
  })

  it('Classic and Roster each keep their own arrangement, in one record', async () => {
    const view = render(<Live layoutMode="roster" />)
    await settle()
    move('rxfreq', 'left', 'roster')
    view.rerender(<Live layoutMode="classic" />)
    await settle()
    // Classic was never arranged: it is today's tree.
    expect(lower().hasAttribute('data-arranged')).toBe(false)
    expect(`## classic\n${treeOf(lower())}\n`).toBe(goldenSection('classic'))
    move('txmsgs', 'right', 'classic')
    expect(rendered()).toEqual([['bandActivity'], ['rxfreq'], ['stations', 'txmsgs']])
    view.rerender(<Live layoutMode="roster" />)
    await settle()
    expect(rendered()).toEqual([['callRoster', 'rxfreq'], ['bandActivity']])
    view.rerender(<Live layoutMode="classic" />)
    await settle()
    expect(rendered()).toEqual([['bandActivity'], ['rxfreq'], ['stations', 'txmsgs']])
    const stored = JSON.parse(localStorage.getItem(panelStorageKey('operate'))!)
    expect(Object.keys(stored.places).sort()).toEqual(['classic', 'roster'])
    expect(stored.place, 'FT keeps no single placement').toBeUndefined()
  })

  it('the operator’s example: Call Roster and Rx Frequency together, Spots and POTA/SOTA boxes in the rail', async () => {
    render(<Live layoutMode="roster" boxes={SOURCE} />)
    await settle()
    move('rxfreq', 'left', 'roster')
    act(() => api!.setPanelState('bandActivity', 'removed'))
    act(() => api!.addBox!('log', 'roster'))
    act(() => api!.setBox!('box1', 'spotsBoard'))
    act(() => api!.addBox!('log', 'roster'))
    act(() => api!.setBox!('box2', 'pota'))
    await settle()
    expect(rendered()).toEqual([['callRoster', 'rxfreq'], ['box1', 'box2']])
    expect([bodyOf('box1'), bodyOf('box2')]).toEqual(['box-body-spots', 'box-body-pota'])
    // Two fill panes side by side in the main column share a divider; boxes take none.
    expect(stacks().map(seamsIn)).toEqual([['Call Roster / Rx Frequency'], []])
  })

  it('the callsign card keeps its size in the arranged columns, and only there: the column scrolls instead', async () => {
    // The operator's "Card keeps its size": a second flat class on the card (cockpit-panes.css computes
    // what it does), added in the arranged branch only, so today's tree keeps the card it always drew.
    const card = () => document.querySelector('.cockpit-lower .recall-card')!
    render(<Live layoutMode="classic" selectedCall="W1ABC" />)
    await settle()
    expect(card().classList.contains('cockpit-recall'), 'fixture: the bounded card is on screen').toBe(true)
    expect(card().classList.contains('cockpit-recall-kept'), 'today’s tree changed').toBe(false)
    move('stations', 'up', 'classic')
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect([...card().classList]).toEqual(expect.arrayContaining(['recall-card', 'cockpit-recall', 'cockpit-recall-kept']))
  })

  it('a decode window’s rows come back after a move: its history is the cockpit’s, not the pane’s', async () => {
    const view = render(<Live layoutMode="roster" snapOver={{ recentDecodes: [decode({})] }} />)
    await settle()
    const inBand = () => document.querySelector('.cockpit-decodes-side')?.textContent ?? ''
    expect(inBand(), 'fixture: Band Activity shows the decode').toContain('CQ W1ABC FN42')
    // The next period decodes nothing: the row now lives only in the history.
    view.rerender(<Live layoutMode="roster" snapOver={{ recentDecodes: [] }} />)
    await settle()
    expect(inBand(), 'fixture: the history holds the row').toContain('CQ W1ABC FN42')
    move('bandActivity', 'left', 'roster')
    await settle()
    expect(rendered()[0], 'Band Activity did not move to the main column').toContain('bandActivity')
    expect(inBand(), 'Band Activity came back empty after the move').toContain('CQ W1ABC FN42')
  })

  it('the sorts come back after the first move too: they are the cockpit’s, not the panes’', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    const bandSort = () => document.querySelector<HTMLSelectElement>('.cockpit-lower .operate-decodes:not(.compact) .od-sort select')!
    const rosterSort = () => document.querySelector('.cockpit-lower .operate-roster .or-th.active')?.textContent
    const callHeader = () => within(document.querySelector<HTMLElement>('.cockpit-lower .operate-roster')!).getByRole('button', { name: /^Call( [▲▼])?$/ })
    expect([bandSort().value, rosterSort()], 'fixture: the defaults').toEqual(['time', 'Need ▼'])
    fireEvent.change(bandSort(), { target: { value: 'dt' } })
    fireEvent.click(callHeader())
    expect([bandSort().value, rosterSort()], 'fixture: the picks took').toEqual(['dt', 'Call ▲'])
    // The first arranging act: the region leaves today's tree, remounting its panes.
    move('rxfreq', 'left', 'roster')
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect([bandSort().value, rosterSort()]).toEqual(['dt', 'Call ▲'])
  })

  it('a double-click on a decode calls the station through the cockpit’s own handler, wherever the pane stands', async () => {
    const calls: unknown[][] = []
    const onCall = (...a: unknown[]) => void calls.push(a)
    const row = () => document.querySelector('.cockpit-decodes-side .decode-row') as HTMLElement
    render(<Live layoutMode="roster" snapOver={{ recentDecodes: [decode({})] }} onCall={onCall} />)
    await settle()
    fireEvent.doubleClick(row())
    const stock = calls.splice(0)
    expect(stock, 'fixture: the stock double-click called nobody').toHaveLength(1)
    for (const way of ['left', 'up'] as PaneMove[]) {
      move('bandActivity', way, 'roster')
      await settle()
      fireEvent.doubleClick(row())
      expect(calls.splice(0), `after a move ${way}, the double-click called differently`).toEqual(stock)
    }
  })

  it('a pane moved up or down in its column is moved, not remounted; one that changes column is the only one remounted', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    move('rxfreq', 'up', 'roster') // the first arranging act draws the arranged columns
    await settle()
    expect(rendered()).toEqual([['callRoster'], ['rxfreq', 'bandActivity']])
    const [cr, rx, ba] = ['callRoster', 'rxfreq', 'bandActivity'].map(pane)
    move('rxfreq', 'down', 'roster')
    await settle()
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq']])
    expect([pane('callRoster') === cr, pane('rxfreq') === rx, pane('bandActivity') === ba]).toEqual([true, true, true])
    move('rxfreq', 'left', 'roster')
    await settle()
    expect(rendered()).toEqual([['callRoster', 'rxfreq'], ['bandActivity']])
    expect([pane('callRoster') === cr, pane('rxfreq') === rx, pane('bandActivity') === ba]).toEqual([true, false, true])
  })

  it('two fill panes adjacent in a column share a divider, sized from the record, and lose it apart', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    move('rxfreq', 'up', 'roster')
    await settle()
    // Rx Frequency over Band Activity in the rail: a pair, at the pair's stock total (1 + 1.6, so 1.3 a share).
    expect(stacks().map(seamsIn)).toEqual([[], ['Rx Frequency / Band Activity']])
    act(() => api!.setShares({ rxfreq: 1.2, bandActivity: 0.8 }))
    const grow = (id: string) => pane(id)!.style.getPropertyValue('--pane-share')
    expect([grow('rxfreq'), grow('bandActivity')].map(Number)).toEqual([1.2 * 1.3, 0.8 * 1.3])
    move('bandActivity', 'left', 'roster')
    await settle()
    expect(stacks().map(seamsIn)).toEqual([['Call Roster / Band Activity'], []])
    // Rx Frequency stands alone: its stock weight, not half of a pair.
    expect(grow('rxfreq')).toBe('')
  })

  it('the rail on the left stands first, and ⊞ Arrange lists it first with ◀ ▶ following the screen', async () => {
    localStorage.setItem('nexus.operate.railSide', 'left')
    render(<Live layoutMode="roster" />)
    await settle()
    move('rxfreq', 'up', 'roster')
    await settle()
    const rail = stacks().find((s) => s.tagName === 'ASIDE')!
    expect(rail.classList.contains('op-stack-lead')).toBe(true)
    expect(stacks().filter((s) => s.classList.contains('op-stack-lead'))).toEqual([rail])
    expect(lower().getAttribute('data-rail')).toBe('left')
    fireEvent.click(document.querySelector('.panels-menu-btn')!)
    expect([...document.querySelectorAll('.panels-arrange-colhead')].map((h) => h.textContent)).toEqual([
      t('operate.arrange.rail'),
      t('operate.arrange.main'),
    ])
    // The Call Roster stands in the main column, right of the rail: ◀ takes it to the rail, ▶ goes nowhere.
    const left = screen.getByRole('button', { name: t('panels.arrange.left.aria', { pane: 'Call Roster' }) }) as HTMLButtonElement
    const right = screen.getByRole('button', { name: t('panels.arrange.right.aria', { pane: 'Call Roster' }) }) as HTMLButtonElement
    expect([left.disabled, right.disabled]).toEqual([false, true])
    fireEvent.click(left)
    expect(api!.layout.places?.roster?.callRoster?.col).toBe('log')
  })

  it('a column the moves empty is not drawn, and the grid collapses as the stock one does', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    move('callRoster', 'right', 'roster')
    await settle()
    expect(rendered()).toEqual([['bandActivity', 'rxfreq', 'callRoster']])
    expect(lower().getAttribute('data-cols')).toBe('one')
    expect(lower().querySelector(':scope > .op-colseam'), 'a divider over a template with nothing to divide').toBeNull()
  })

  it('Reset takes FT back to today’s tree', async () => {
    render(<Live layoutMode="roster" boxes={SOURCE} />)
    await settle()
    move('rxfreq', 'left', 'roster')
    act(() => api!.addBox!('a', 'roster'))
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    act(() => api!.reset())
    await settle()
    expect(`## roster\n${treeOf(lower())}\n`).toBe(goldenSection('roster'))
  })

  it('⊞ Arrange names FT’s own columns, in each layout, and offers boxes only where the window lends them', async () => {
    for (const [layoutMode, heads] of [
      ['classic', [t('panels.arrange.column.a'), t('panels.arrange.column.b'), t('operate.arrange.rail')]],
      ['roster', [t('operate.arrange.main'), t('operate.arrange.rail')]],
    ] as Array<[OperateLayout, string[]]>) {
      for (const lend of [true, false]) {
        render(<Live layoutMode={layoutMode} boxes={lend ? SOURCE : undefined} />)
        await settle()
        fireEvent.click(document.querySelector('.panels-menu-btn')!)
        expect([...document.querySelectorAll('.panels-arrange-colhead')].map((h) => h.textContent), layoutMode).toEqual(heads)
        expect(screen.queryByText(t('panels.arrange.logForm')), 'FT has no log form').toBeNull()
        expect(screen.getByText(t('operate.arrange.narrow'))).toBeTruthy()
        expect(document.querySelectorAll('.panels-arrange-add').length, `${layoutMode}, lent: ${lend}`).toBe(lend ? heads.length : 0)
        cleanup()
      }
    }
  })
})

describe('every FT pane in both layouts (the operator’s pick)', () => {
  // Classic can place the Call Roster, and Roster the Tx messages and Stations, in any column. Each layout's
  // ⊞ Panels offers them unticked, so each layout opens as it always did; ticked, one stands where that
  // layout's stock arrangement lists it and moves like any pane; its ✕ takes it out of that layout only.
  const tick = (label: string) => {
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    const box = screen.getByLabelText(label) as HTMLInputElement
    const was = box.checked
    fireEvent.click(box)
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    return was
  }

  it('Classic: the Call Roster, unticked until added, stands under Band Activity, moves, and leaves with its ✕', async () => {
    const onCall = vi.fn()
    const view = render(<Live layoutMode="classic" onCall={onCall} />)
    await settle()
    expect(`## classic\n${treeOf(lower())}\n`, 'Classic changed before anything was added').toBe(goldenSection('classic'))
    expect(tick('Call Roster'), 'Classic offered the Call Roster ticked').toBe(false)
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect(rendered()).toEqual([['bandActivity', 'callRoster'], ['rxfreq', 'txmsgs'], ['stations']])
    // It is the Call Roster: a double-click on a station calls through the cockpit's own handler.
    fireEvent.doubleClick(within(pane('callRoster')!).getByText('W1ABC'))
    expect(onCall).toHaveBeenCalled()
    expect(onCall.mock.calls[0][0]).toBe('W1ABC')
    move('callRoster', 'right', 'classic')
    await settle()
    expect(rendered()).toEqual([['bandActivity'], ['rxfreq', 'txmsgs', 'callRoster'], ['stations']])
    fireEvent.click(screen.getByRole('button', { name: 'Hide Call Roster' }))
    await settle()
    expect(rendered()).toEqual([['bandActivity'], ['rxfreq', 'txmsgs'], ['stations']])
    // Taken out of Classic only: Roster's main column still holds it.
    view.rerender(<Live layoutMode="roster" />)
    await settle()
    expect(document.querySelector('.cockpit-roster-main'), 'the ✕ in Classic hid the Call Roster in Roster').not.toBeNull()
  })

  it('Roster: the Tx messages and Stations stand in the rail under Rx Frequency, and Tx6 calls CQ from there', async () => {
    const view = render(<Live layoutMode="roster" />)
    await settle()
    expect(`## roster\n${treeOf(lower())}\n`, 'Roster changed before anything was added').toBe(goldenSection('roster'))
    expect(tick('Tx Messages')).toBe(false)
    expect(tick('Stations')).toBe(false)
    await settle()
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq', 'txmsgs', 'stations']])
    // The Tx messages start a transmission wherever they stand, as they always could; no stop control is in
    // them (Stop TX stays in the QSO strip, which no layout lists).
    expect(within(pane('txmsgs')!).queryByRole('button', { name: /^stop tx$/i })).toBeNull()
    expect(document.querySelector('.cockpit-qso')?.querySelector('.op-btn.stop'), 'Stop TX left the QSO strip').not.toBeNull()
    vi.mocked(startCq).mockClear()
    fireEvent.click(within(pane('txmsgs')!).getByTitle('Call CQ (Alt+6)'))
    await settle()
    expect(startCq).toHaveBeenCalled()
    move('txmsgs', 'left', 'roster')
    await settle()
    expect(rendered()).toEqual([['callRoster', 'txmsgs'], ['bandActivity', 'rxfreq', 'stations']])
    // Classic is untouched: it was never arranged, and draws its own Tx messages and Stations as it always did.
    view.rerender(<Live layoutMode="classic" />)
    await settle()
    expect(`## classic\n${treeOf(lower())}\n`).toBe(goldenSection('classic'))
  })

  it('a stored record that adds a pane and places nothing still draws it, in the arranged columns', async () => {
    localStorage.setItem(panelStorageKey('operate'), JSON.stringify({ v: 2, state: {}, share: {}, extras: { classic: ['callRoster'] } }))
    render(<Live layoutMode="classic" />)
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect(rendered()).toEqual([['bandActivity', 'callRoster'], ['rxfreq', 'txmsgs'], ['stations']])
  })

  it('Reset takes both layouts back as they shipped, the added panes with them', async () => {
    const view = render(<Live layoutMode="roster" />)
    await settle()
    tick('Stations')
    await settle()
    act(() => api!.reset())
    await settle()
    expect(lower().hasAttribute('data-arranged')).toBe(false)
    expect(`## roster\n${treeOf(lower())}\n`).toBe(goldenSection('roster'))
    view.rerender(<Live layoutMode="classic" />)
    await settle()
    expect(`## classic\n${treeOf(lower())}\n`).toBe(goldenSection('classic'))
  })
})

describe('FT’s boxes', () => {
  it('“+ Add a box” puts one at the foot of the column it is pressed in, showing the first entry not on screen', async () => {
    render(<Live layoutMode="classic" boxes={SOURCE} />)
    await settle()
    fireEvent.click(document.querySelector('.panels-menu-btn')!)
    fireEvent.click(screen.getByRole('button', { name: t('panels.box.add.b.aria') }))
    await settle()
    expect(rendered()).toEqual([['bandActivity'], ['rxfreq', 'txmsgs', 'box1'], ['stations']])
    expect(bodyOf('box1')).toBe('box-body-advisory')
  })

  it('an FT that is not on screen mounts no box (its keep-alive host stays mounted), and one that lends none draws none', async () => {
    const record = JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' }, places: { roster: { box1: { col: 'a', order: 1 } } } })
    for (const [props, drawn] of [
      [{ boxes: SOURCE, active: false }, false],
      [{ boxes: undefined, active: true }, false],
      [{ boxes: SOURCE, active: true }, true],
    ] as Array<[Partial<LiveProps>, boolean]>) {
      localStorage.setItem(panelStorageKey('operate'), record)
      render(<Live layoutMode="roster" {...props} />)
      await settle()
      expect(pane('box1') != null, JSON.stringify(props)).toBe(drawn)
      if (drawn) expect(bodyOf('box1')).toBe('box-body-clock')
      cleanup()
    }
  })

  it('where no box is lent, a box in the record arranges nothing: the pop-out and the hosted page draw today’s tree', async () => {
    // A box added on the desktop in Roster's main column: Classic has no placement, and stands the box at
    // the foot of its rail.
    const record = JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' }, places: { roster: { box1: { col: 'a', order: 1 } } } })
    for (const layoutMode of ['roster', 'classic'] as const) {
      localStorage.setItem(panelStorageKey('operate'), record)
      render(<Live layoutMode={layoutMode} />)
      await settle()
      expect(`## ${layoutMode}\n${treeOf(lower())}\n`, `${layoutMode}, no box lent`).toBe(goldenSection(layoutMode))
      cleanup()
      localStorage.setItem(panelStorageKey('operate'), record)
      render(<Live layoutMode={layoutMode} boxes={SOURCE} />)
      await settle()
      expect(pane('box1'), `${layoutMode}: the desktop draws the box`).not.toBeNull()
      cleanup()
    }
  })

  it('a box’s ✕ hides it and ends nothing; with the last box gone and nothing moved, FT keeps the arranged columns until Reset', async () => {
    render(<Live layoutMode="roster" boxes={SOURCE} />)
    await settle()
    act(() => api!.addBox!('log', 'roster'))
    await settle()
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq', 'box1']])
    fireEvent.click(within(pane('box1')!).getByRole('button', { name: /hide/i }))
    await settle()
    expect(api!.stateOf('box1')).toBe('removed')
    // The box's placement stays in the record (its column is where it comes back), so the region stays
    // arranged rather than flipping its panes back and forth.
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq']])
  })

  it('every box of FT is listed in both layouts, at the foot of the side rail until placed', () => {
    for (const layout of ['classic', 'roster'] as const) {
      const spec = OPERATE_ARRANGE[layout]
      expect([...(spec.boxes ?? [])]).toEqual([...BOX_IDS])
      expect(spec.columns.log.slice(-BOX_IDS.length)).toEqual([...BOX_IDS])
      expect(arrangeIds(spec).filter((id) => (BOX_IDS as readonly string[]).includes(id))).toEqual([...BOX_IDS])
    }
  })
})

// ── BY DRAG (2026-10-08): the same moves, made by dragging a pane by its title ──────────────────────────
// The drop is the arrows' moves (cockpit-drag.test.tsx holds that for every arrow); here, over the REAL
// record, what the screen does after one: a pane dragged up or down in its column is moved, not remounted;
// the one dragged into another column is the only one remounted; the sorts and a double-click's call hold.
describe('by drag: the same moves, by a pane’s title', () => {
  let unstub: (() => void) | null = null
  beforeEach(() => {
    unstub = stubLayout('.cockpit-lower')
  })
  afterEach(async () => {
    unstub?.()
    unstub = null
    await new Promise((r) => setTimeout(r, 0))
  })
  const above = (id: string) => {
    const r = paneBoxOf('.cockpit-lower', id)!.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + 5 }
  }
  const below = (id: string) => {
    const r = paneBoxOf('.cockpit-lower', id)!.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.bottom + 3 }
  }
  async function dragTo(id: string, to: { x: number; y: number }) {
    pickUp(gripOf('.cockpit-lower', id)!, to)
    release(to)
    await settle()
    // The click a release after a drag sends is held back until the next task.
    await new Promise((r) => setTimeout(r, 0))
  }
  const bandSort = () => document.querySelector<HTMLSelectElement>('.cockpit-lower .operate-decodes:not(.compact) .od-sort select')!
  const rosterSort = () => document.querySelector('.cockpit-lower .operate-roster .or-th.active')?.textContent

  it('dragged in its column a pane is moved, not remounted; dragged into another it is the only one remounted; the sorts hold', async () => {
    render(<Live layoutMode="roster" />)
    await settle()
    fireEvent.change(bandSort(), { target: { value: 'dt' } })
    fireEvent.click(within(document.querySelector<HTMLElement>('.cockpit-lower .operate-roster')!).getByRole('button', { name: /^Call( [▲▼])?$/ }))
    expect([bandSort().value, rosterSort()], 'fixture: the picks took').toEqual(['dt', 'Call ▲'])
    // The first arranging act, by drag: Rx Frequency above Band Activity in the rail.
    await dragTo('rxfreq', above('bandActivity'))
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    expect(rendered()).toEqual([['callRoster'], ['rxfreq', 'bandActivity']])
    const [cr, rx, ba] = ['callRoster', 'rxfreq', 'bandActivity'].map(pane)
    await dragTo('rxfreq', below('bandActivity'))
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq']])
    expect([pane('callRoster') === cr, pane('rxfreq') === rx, pane('bandActivity') === ba], 'a drag within a column remounted a pane').toEqual([true, true, true])
    await dragTo('rxfreq', above('callRoster'))
    expect(rendered()).toEqual([['rxfreq', 'callRoster'], ['bandActivity']])
    expect([pane('callRoster') === cr, pane('bandActivity') === ba], 'a drag into another column remounted another pane').toEqual([true, true])
    expect(pane('rxfreq') === rx, 'the pane that changed column kept its node: React cannot carry it across').toBe(false)
    expect([bandSort().value, rosterSort()], 'a sort was lost on the way').toEqual(['dt', 'Call ▲'])
    // Undo takes the one drop back, as it takes one arrow back.
    act(() => api!.undo())
    await settle()
    expect(rendered()).toEqual([['callRoster'], ['bandActivity', 'rxfreq']])
    // A real FT mount and three drags, 0.8 s in the full suite on a loaded box: past the 5 s default only under
    // far more load than that, the budget the real-render files in this tree carry.
  }, 15_000)

  it('a double-click on a decode still calls the station after a drag, through the cockpit’s own handler', async () => {
    const calls: unknown[][] = []
    render(<Live layoutMode="roster" snapOver={{ recentDecodes: [decode({})] }} onCall={(...a: unknown[]) => void calls.push(a)} />)
    await settle()
    const row = () => document.querySelector('.cockpit-lower .operate-decodes:not(.compact) .decode-row') as HTMLElement
    fireEvent.doubleClick(row())
    const stock = calls.splice(0)
    expect(stock, 'fixture: the stock double-click called nobody').toHaveLength(1)
    await dragTo('bandActivity', above('callRoster'))
    expect(rendered()[0][0]).toBe('bandActivity')
    fireEvent.doubleClick(row())
    expect(calls.splice(0)).toEqual(stock)
  }, 15_000)
})
