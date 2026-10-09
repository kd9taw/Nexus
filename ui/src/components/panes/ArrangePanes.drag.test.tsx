// @vitest-environment jsdom
//
// ⊞ PANELS ▸ ARRANGE BY DRAG (2026-10-08): the list's rows are dragged by their name or their ⠿ grip, and a drop
// is the arrows' moves. For every arrow the list offers — Phone with and without room for its left side, FT's
// Classic and Roster — the arrow is pressed and the row is dragged to the place the arrow puts it, with real
// pointer events: the drop names that place with the arguments the arrow passed, so it gives the arrow's
// arrangement. Then over the REAL panel record: the drop is stored as one step, it is told to a screen
// reader, focus stays with the pane as after an arrow, Escape cancels, and a press on an arrow drags nothing.
// jsdom lays nothing out: the list's boxes are stubbed (the groups one above the other, 24 px rows).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { ArrangePanes } from './ArrangePanes'
import { boxLabels } from './CockpitBox'
import { LIFT_PX } from './PaneDrag'
import { OPERATE_ARRANGE, PHONE_PANELS, usePanelLayout, type OperatePanelId, type PhonePanelId } from '../../features/panelState'
import { PANE_COLUMNS, dropArranged, moveArranged, placedColumns, type Arrangement, type ArrangeSpec, type PaneColumn, type PaneDrop, type PaneMove } from '../../features/panelPlace'
import { t } from '../../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "Phone: every arrow on offer", takes 0.40 s and 0.28 s on one core
// (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core, past vitest's 5 s
// default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const announced = vi.hoisted(() => [] as string[])
vi.mock('../../announce', () => ({ announce: (text: string) => announced.push(text) }))

const PHONE_LABELS: Record<PhonePanelId, string> = {
  scope: 'Scope',
  rigscope: 'Rig scope',
  txmeters: 'TX meters',
  receiver: 'Receiver',
  transmitter: 'Transmitter',
  bandActivity: 'Band Activity',
  voiceKeyer: 'Voice Keyer',
  spots: 'Spots',
  needed: 'Needed',
  ...boxLabels({}),
}
const FT_LABELS = { bandActivity: 'Band Activity', rxfreq: 'Rx Frequency', txmsgs: 'Tx messages', stations: 'Stations', callRoster: 'Call Roster', recall: 'Callsign card' } as Record<OperatePanelId, string>
const PHONE_SHOWN = new Set<string>(['bandActivity', 'voiceKeyer', 'spots', 'receiver', 'transmitter', 'needed'])

/** jsdom lays nothing out: the list's groups one above the other, each row 24 px tall, 300 px wide. */
let realRect: typeof Element.prototype.getBoundingClientRect
function stubList() {
  realRect = Element.prototype.getBoundingClientRect
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const root = document.querySelector('.panels-arrange')
    if (!root) return realRect.call(this)
    const kids = [...root.children] as HTMLElement[]
    let y = 0
    const at = new Map<Element, DOMRect>()
    for (const k of kids) {
      const rows = [...k.children].filter((c) => c.classList.contains('panels-arrange-pane'))
      const h = 20 + rows.length * 24 + 20
      at.set(k, rect(0, y, 300, h))
      rows.forEach((r, i) => at.set(r, rect(0, y + 20 + i * 24, 300, 24)))
      y += h + 4
    }
    if (this === root) return rect(0, 0, 300, y)
    return at.get(this) ?? realRect.call(this)
  }
}
const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, x: left, y: top, right: left + width, bottom: top + height, toJSON: () => ({}) }) as DOMRect

beforeEach(() => {
  localStorage.clear()
  announced.length = 0
  stubList()
})
afterEach(async () => {
  cleanup()
  Element.prototype.getBoundingClientRect = realRect
  await new Promise((r) => setTimeout(r, 0))
})

const rowOf = (id: string) => document.querySelector(`.panels-arrange-name[data-pane-grip="${id}"]`)!.closest('.panels-arrange-pane')!
function dragRow(id: string, to: { x: number; y: number }) {
  const name = document.querySelector(`.panels-arrange-name[data-pane-grip="${id}"]`)!
  const r = rowOf(id).getBoundingClientRect()
  fireEvent.pointerDown(name, { clientX: 20, clientY: r.top + 5, pointerId: 3, button: 0, pointerType: 'mouse' })
  fireEvent.pointerMove(window, { clientX: 20, clientY: r.top + 5 + LIFT_PX + 3, pointerId: 3, pointerType: 'mouse' })
  fireEvent.pointerMove(window, { clientX: to.x, clientY: to.y, pointerId: 3, pointerType: 'mouse' })
  fireEvent.pointerUp(window, { clientX: to.x, clientY: to.y, pointerId: 3, pointerType: 'mouse' })
}
/** The point that names `drop` in the list: over the row it lands above, or just under the last row of its
 *  group (or under the group's head when it has none). */
function pointFor(drop: PaneDrop<string>, dragged: string) {
  if (drop.before != null) return { x: 150, y: rowOf(drop.before).getBoundingClientRect().top + 5 }
  const group = document.querySelector(`.panels-arrange > [data-arrange-area="${drop.area}"]`)!
  const rows = [...group.querySelectorAll('.panels-arrange-pane')].filter((r) => !r.querySelector(`[data-pane-grip="${dragged}"]`))
  if (rows.length > 0) return { x: 150, y: rows[rows.length - 1].getBoundingClientRect().bottom + 2 }
  return { x: 150, y: group.getBoundingClientRect().top + 22 }
}

interface Setting {
  name: string
  spec: ArrangeSpec<string>
  labels: Record<string, string>
  shown: (id: string) => boolean
  sideRoom?: boolean
  columns?: Array<{ col: PaneColumn; name: string; addAria: string }>
}
const ftColumns = (cols: readonly PaneColumn[]) => cols.map((col) => ({ col, name: `Column ${col}`, addAria: `Add to ${col}` }))
const SETTINGS: Setting[] = [
  { name: 'Phone', spec: PHONE_PANELS.arrange! as ArrangeSpec<string>, labels: PHONE_LABELS, shown: (id) => PHONE_SHOWN.has(id) },
  { name: 'Phone, room for the left side', spec: PHONE_PANELS.arrange! as ArrangeSpec<string>, labels: PHONE_LABELS, shown: (id) => PHONE_SHOWN.has(id), sideRoom: true },
  { name: 'FT Classic', spec: OPERATE_ARRANGE.classic as ArrangeSpec<string>, labels: FT_LABELS, shown: (id) => id !== 'callRoster' && !id.startsWith('box'), columns: ftColumns(['a', 'b', 'log']) },
  { name: 'FT Roster, rail on the left', spec: OPERATE_ARRANGE.roster as ArrangeSpec<string>, labels: FT_LABELS, shown: (id) => !id.startsWith('box'), columns: ftColumns(['log', 'a']) },
]

describe('a row dragged in the list moves the record exactly as its arrows do', () => {
  it.each(SETTINGS.map((s) => [s.name, s] as const))('%s: every arrow on offer', async (_name, s) => {
    const asked: Array<{ id: string; move?: PaneMove; drop?: PaneDrop<string> }> = []
    const arr: Arrangement<string> = {}
    render(
      <ArrangePanes
        spec={s.spec}
        layout={{ v: 2, state: {}, share: {} }}
        shown={s.shown}
        labels={s.labels}
        sideRoom={s.sideRoom}
        columns={s.columns}
        onMove={(id, move) => asked.push({ id, move })}
        onDrop={(id, drop) => asked.push({ id, drop })}
      />,
    )
    const order = s.columns?.map((c) => c.col)
    const side = s.sideRoom === true
    const arrows = [...document.querySelectorAll<HTMLButtonElement>('button[data-arrange]')].filter((b) => !b.disabled)
    expect(arrows.length).toBeGreaterThan(4)
    for (const b of arrows) {
      asked.length = 0
      fireEvent.click(b)
      const { id, move } = asked[0]
      const arrow = moveArranged(s.spec, arr, id, move!, s.shown, side, order)!
      expect(arrow, `${s.name}: ${b.dataset.arrange} is offered but moves nothing`).not.toBeNull()
      // The place the arrow puts it, and the row right under it there.
      const onSide = side && (arrow.leftSide ?? []).includes(id)
      const area: PaneColumn | 'side' = onSide ? 'side' : PANE_COLUMNS.find((c) => placedColumns(s.spec, arrow.place)[c].includes(id))!
      const list = area === 'side' ? (arrow.leftSide ?? []).filter(s.shown) : placedColumns(s.spec, arrow.place)[area].filter((x) => s.shown(x) && !(arrow.leftSide ?? []).includes(x))
      const want: PaneDrop<string> = { area, before: list[list.indexOf(id) + 1] ?? null }
      asked.length = 0
      dragRow(id, pointFor(want, id))
      expect(asked, `${s.name}: dragging ${id} to where ${b.dataset.arrange} puts it`).toEqual([{ id, drop: want }])
      expect(dropArranged(s.spec, arr, id, want, s.shown, side, order)).toEqual(arrow)
      // The click a release after a drag sends is held back until the next task; let it go before the next press.
      await new Promise((r) => setTimeout(r, 0))
    }
  })
})

function Live({ sideRoom = false }: { sideRoom?: boolean }) {
  const panels = usePanelLayout(PHONE_PANELS, 'main')
  const shown = (id: PhonePanelId) => PHONE_SHOWN.has(id)
  return (
    <>
      <ArrangePanes
        spec={PHONE_PANELS.arrange!}
        layout={panels.layout}
        shown={shown}
        labels={PHONE_LABELS}
        sideRoom={sideRoom}
        onMove={(id, m) => panels.movePane!(id, m, shown, sideRoom)}
        onDrop={(id, d) => panels.dropPane!(id, d, shown, sideRoom)}
      />
      <span data-testid="place">{JSON.stringify(placedColumns(PHONE_PANELS.arrange!, panels.layout.place))}</span>
      <span data-testid="undo">{String(panels.canUndo)}</span>
    </>
  )
}
const placed = () => JSON.parse(screen.getByTestId('place').textContent!) as Record<PaneColumn, string[]>

describe('over the real panel record', () => {
  it('a drop is stored in one step and told to a screen reader', () => {
    render(<Live />)
    dragRow('transmitter', pointFor({ area: 'a', before: 'voiceKeyer' }, 'transmitter'))
    expect(placed().a.slice(0, 3)).toEqual(['bandActivity', 'transmitter', 'voiceKeyer'])
    expect(screen.getByTestId('undo').textContent).toBe('true')
    expect(announced).toEqual([t('panels.drag.dropped.above', { pane: 'Transmitter', place: t('panels.arrange.column.a'), next: 'Voice Keyer' })])
  })
  it('focus stays with the pane, as after an arrow pressed from the keyboard — and a mouse that had no focus there takes none', () => {
    render(<Live />)
    const up = screen.getByRole('button', { name: t('panels.arrange.up.aria', { pane: 'Needed' }) })
    up.focus()
    dragRow('transmitter', pointFor({ area: 'log', before: null }, 'transmitter'))
    const now = document.activeElement as HTMLElement
    expect(now.dataset.arrange?.startsWith('transmitter '), `focus went to ${now.outerHTML.slice(0, 60)}`).toBe(true)
    // Focus elsewhere than the list: a drop leaves it there.
    ;(document.activeElement as HTMLElement).blur()
    dragRow('receiver', pointFor({ area: 'log', before: null }, 'receiver'))
    expect(document.activeElement).toBe(document.body)
  })
  it('Escape cancels: nothing stored, no step spent, nothing told', () => {
    render(<Live />)
    const name = document.querySelector('.panels-arrange-name[data-pane-grip="transmitter"]')!
    fireEvent.pointerDown(name, { clientX: 20, clientY: 30, pointerId: 3, button: 0, pointerType: 'mouse' })
    fireEvent.pointerMove(window, { clientX: 20, clientY: 60, pointerId: 3, pointerType: 'mouse' })
    expect(document.querySelector('.pane-drop-layer')).not.toBeNull()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    fireEvent.pointerUp(window, { clientX: 20, clientY: 60, pointerId: 3, pointerType: 'mouse' })
    expect(screen.getByTestId('undo').textContent).toBe('false')
    expect(localStorage.length).toBe(0)
    expect(announced).toEqual([])
  })
  it('a press on an arrow drags nothing (it is the arrow’s), and a list with no drop has no grip', () => {
    render(<Live />)
    const up = screen.getByRole('button', { name: t('panels.arrange.up.aria', { pane: 'Transmitter' }) })
    fireEvent.pointerDown(up, { clientX: 250, clientY: 30, pointerId: 3, button: 0, pointerType: 'mouse' })
    fireEvent.pointerMove(window, { clientX: 250, clientY: 200, pointerId: 3, pointerType: 'mouse' })
    expect(document.querySelector('.pane-drop-layer')).toBeNull()
    fireEvent.pointerUp(window, { clientX: 250, clientY: 200, pointerId: 3, pointerType: 'mouse' })
    expect(screen.getByTestId('undo').textContent).toBe('false')
    cleanup()
    render(<ArrangePanes spec={PHONE_PANELS.arrange!} layout={{ v: 2, state: {}, share: {} }} shown={() => true} labels={PHONE_LABELS} onMove={() => {}} />)
    expect(document.querySelector('[data-pane-grip]')).toBeNull()
    expect(document.querySelector('[data-pane-drag]')).toBeNull()
  })
})
