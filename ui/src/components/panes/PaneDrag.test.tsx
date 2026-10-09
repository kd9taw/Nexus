// @vitest-environment jsdom
//
// DRAGGING A PANE BY ITS TITLE — the engine, on its own. First the geometry, pure, on boxes given by hand:
// which place and which pane a point names, where the line goes, where an empty place's zone stands. Then the
// press itself, on a small host with the boxes stubbed (jsdom lays nothing out): a mouse lifts after a few px,
// a finger after a still hold; a press on a control in a title, or in a pane's body, starts nothing; Escape
// and a release outside cancel; a pinned pane shows no place it cannot reach; the click after a drag is
// swallowed; a drop is told to a screen reader. The cockpits' own drags are in their arrange tests.
import { useRef } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { LIFT_PX, PaneDropLayer, TOUCH_HOLD_MS, dropAt, measurePlaces, usePaneDrag, type Box, type MeasuredPlace } from './PaneDrag'
import type { ArrangeSpec, PaneDrop } from '../../features/panelPlace'

const announced = vi.hoisted(() => [] as string[])
vi.mock('../../announce', () => ({ announce: (text: string) => announced.push(text) }))

const box = (left: number, top: number, width: number, height: number): Box => ({ left, top, width, height })
const place = (b: Box, areas: MeasuredPlace<string>['areas'], panes: Array<[string, Box]> = []): MeasuredPlace<string> => ({
  box: b,
  zone: false,
  areas,
  panes: panes.map(([id, pb]) => ({ id, box: pb })),
})

describe('the geometry: which place and which pane a point names', () => {
  // Three columns, 300 px wide with 12 px gaps; panes 100 px tall, 8 px apart.
  const cols: MeasuredPlace<string>[] = [
    place(box(0, 0, 300, 600), ['a'], [['p1', box(0, 0, 300, 100)], ['p2', box(0, 108, 300, 100)]]),
    place(box(312, 0, 300, 600), ['b'], [['q1', box(312, 0, 300, 100)], ['q2', box(312, 108, 300, 100)]]),
    place(box(624, 0, 300, 600), ['log'], []),
  ]
  const areaOf = (id: string) => (id.startsWith('p') ? 'a' : 'b') as 'a' | 'b'

  it('between two panes: right above the lower one, the line in the gap between them', () => {
    const hit = dropAt(cols, 400, 120, 'p1', areaOf)!
    expect(hit.drop).toEqual({ area: 'b', before: 'q2' })
    expect(hit.line!.top + hit.line!.height / 2).toBeCloseTo(104, 5)
    expect(hit.line!.left).toBe(316)
    expect(hit.line!.width).toBe(292)
  })
  it('over the top half of the first pane: above it; below the last pane: at the foot', () => {
    expect(dropAt(cols, 400, 20, 'p1', areaOf)!.drop).toEqual({ area: 'b', before: 'q1' })
    expect(dropAt(cols, 400, 400, 'p1', areaOf)!.drop).toEqual({ area: 'b', before: null })
  })
  it('a column with nothing in it takes the pane at its foot, the line at its top', () => {
    const hit = dropAt(cols, 700, 300, 'p1', areaOf)!
    expect(hit.drop).toEqual({ area: 'log', before: null })
    expect(hit.line!.top).toBeLessThan(10)
  })
  it('the dragged pane’s own box counts for nothing: over itself it names its own place', () => {
    // p1 dragged, the point over p1's lower half: the only other pane is p2, below — so "above p2".
    expect(dropAt(cols, 100, 80, 'p1', areaOf)!.drop).toEqual({ area: 'a', before: 'p2' })
  })
  it('in the gap between two columns, the nearer column; far outside every place, nothing', () => {
    expect(dropAt(cols, 305, 300, 'p1', areaOf)!.index).toBe(0)
    expect(dropAt(cols, 307, 300, 'p1', areaOf)!.index).toBe(1)
    expect(dropAt(cols, 1000, 300, 'p1', areaOf)).toBeNull()
    expect(dropAt(cols, 400, 700, 'p1', areaOf)).toBeNull()
  })
  it('one element drawing two places: the pane it lands above names the place; its foot is the last place', () => {
    const merged = [place(box(0, 0, 300, 600), ['a', 'b'], [['p1', box(0, 0, 300, 100)], ['q1', box(0, 108, 300, 100)]])]
    expect(dropAt(merged, 100, 20, 'x', areaOf)!.drop).toEqual({ area: 'a', before: 'p1' })
    expect(dropAt(merged, 100, 120, 'x', areaOf)!.drop).toEqual({ area: 'b', before: 'q1' })
    expect(dropAt(merged, 100, 400, 'x', areaOf)!.drop).toEqual({ area: 'b', before: null })
  })
})

describe('the geometry: where a place with nothing on screen stands', () => {
  const host = box(0, 0, 1000, 600)
  const drawn = (b: Box | null, areas: MeasuredPlace<string>['areas']) => ({ box: b, areas, panes: [] })
  it('between two drawn columns: a zone centred on the gap, the host’s height', () => {
    const got = measurePlaces([drawn(box(0, 0, 400, 600), ['a']), drawn(null, ['b']), drawn(box(412, 0, 588, 600), ['log'])], host, false)
    expect(got[1].zone).toBe(true)
    expect(got[1].box.left + got[1].box.width / 2).toBeCloseTo(406, 5)
    expect(got[1].box.top).toBe(0)
    expect(got[1].box.height).toBe(600)
  })
  it('first or last on screen: at the host’s left or right edge; two in a row stand side by side', () => {
    const left = measurePlaces([drawn(null, ['side']), drawn(box(0, 0, 1000, 600), ['a'])], host, false)
    expect(left[0].box.left).toBeGreaterThanOrEqual(0)
    expect(left[0].box.left).toBeLessThan(10)
    const two = measurePlaces([drawn(box(0, 0, 600, 600), ['a']), drawn(null, ['b']), drawn(null, ['log'])], host, false)
    expect(right(two[2].box)).toBeLessThanOrEqual(1000)
    expect(right(two[2].box)).toBeGreaterThan(990)
    expect(right(two[1].box)).toBeLessThanOrEqual(two[2].box.left)
  })
  it('stacked: a band across the host, between the two drawn places', () => {
    const got = measurePlaces([drawn(box(0, 0, 1000, 200), ['a']), drawn(null, ['b']), drawn(box(0, 208, 1000, 300), ['log'])], host, false)
    expect(got[1].box.left).toBe(0)
    expect(got[1].box.width).toBe(1000)
    expect(got[1].box.top + got[1].box.height / 2).toBeCloseTo(204, 5)
  })
  it('a point inside a zone drops the pane at the foot of that place', () => {
    const got = measurePlaces([drawn(box(0, 0, 400, 600), ['a']), drawn(null, ['b']), drawn(box(412, 0, 588, 600), ['log'])], host, false)
    expect(dropAt(got, 406, 300, 'p1', () => 'a')!.drop).toEqual({ area: 'b', before: null })
  })
})
const right = (b: Box) => b.left + b.width

// ── THE PRESS, on a small host ───────────────────────────────────────────────────────────────────────

type Id = 'one' | 'two' | 'three'
const SPEC: ArrangeSpec<Id> = { columns: { a: ['one', 'two'], b: ['three'], log: [] }, pinned: ['two'] }
const LABELS: Record<Id, string> = { one: 'One', two: 'Two', three: 'Three' }
const drops: Array<[Id, PaneDrop<Id>]> = []
const clicks: string[] = []

function Host({ enabled = true }: { enabled?: boolean }) {
  const root = useRef<HTMLDivElement>(null)
  const region = useRef<HTMLDivElement>(null)
  const a = useRef<HTMLDivElement>(null)
  const b = useRef<HTMLDivElement>(null)
  const drag = usePaneDrag<Id>({
    root,
    region,
    enabled,
    spec: SPEC,
    arrangement: {},
    shown: () => true,
    sideShows: false,
    targets: () => [
      { el: a.current, areas: ['a'] },
      { el: b.current, areas: ['b'] },
    ],
    labels: LABELS,
    names: { a: 'Column 1', b: 'Column 2' },
    onDrop: (id, drop) => drops.push([id, drop]),
  })
  const pane = (id: Id) => (
    <section key={id} id={`pane-${id}`}>
      <header data-pane-grip={id} id={`grip-${id}`}>
        <span id={`title-${id}`}>{LABELS[id]}</span>
        <button type="button" id={`close-${id}`} onClick={() => clicks.push(`close ${id}`)}>
          ✕
        </button>
      </header>
      <div id={`body-${id}`}>
        <button type="button" id={`row-${id}`} onClick={() => clicks.push(`row ${id}`)}>
          row
        </button>
      </div>
    </section>
  )
  return (
    <div ref={root} id="root">
      <div id="region" ref={region}>
        <div ref={a} id="col-a">
          {pane('one')}
          {pane('two')}
        </div>
        <div ref={b} id="col-b">
          {pane('three')}
        </div>
        <PaneDropLayer drag={drag} host={region} />
      </div>
      <div id="strip">
        <button type="button" id="stop">
          Stop TX
        </button>
      </div>
    </div>
  )
}

/** jsdom lays nothing out: each element of the host gets the box it would have. */
function layout() {
  const boxes: Record<string, Box> = {
    root: box(0, 0, 624, 700),
    region: box(0, 0, 624, 600),
    'col-a': box(0, 0, 300, 600),
    'col-b': box(312, 0, 300, 600),
    'pane-one': box(0, 0, 300, 100),
    'pane-two': box(0, 108, 300, 100),
    'pane-three': box(312, 0, 300, 100),
    strip: box(0, 610, 624, 40),
  }
  for (const [id, b] of Object.entries(boxes)) {
    const el = document.getElementById(id)!
    el.getBoundingClientRect = () => ({ ...b, x: b.left, y: b.top, right: b.left + b.width, bottom: b.top + b.height, toJSON: () => b }) as DOMRect
  }
  for (const id of ['region', 'col-a', 'col-b']) {
    const el = document.getElementById(id)!
    Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => boxes[id].width })
    Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => boxes[id].height })
  }
}
const $ = (id: string) => document.getElementById(id)!
const press = (id: string, x: number, y: number, over: Partial<PointerEventInit> = {}) =>
  fireEvent.pointerDown($(id), { clientX: x, clientY: y, pointerId: 1, button: 0, pointerType: 'mouse', ...over })
const move = (x: number, y: number, over: Partial<PointerEventInit> = {}) =>
  fireEvent.pointerMove(window, { clientX: x, clientY: y, pointerId: 1, pointerType: 'mouse', ...over })
const release = (x: number, y: number, over: Partial<PointerEventInit> = {}) =>
  fireEvent.pointerUp(window, { clientX: x, clientY: y, pointerId: 1, pointerType: 'mouse', ...over })
const layer = () => document.querySelector('.pane-drop-layer')

beforeEach(() => {
  drops.length = 0
  clicks.length = 0
  announced.length = 0
})
afterEach(async () => {
  cleanup()
  vi.useRealTimers()
  // A release after a drag holds back the click it sends until the next task: let it go, so it cannot eat the
  // next test's first click.
  await new Promise((r) => setTimeout(r, 0))
})

describe('the press', () => {
  it('a mouse lifts the pane after a few px, shows where it lands, and a release there drops it', () => {
    render(<Host />)
    layout()
    press('title-one', 50, 20)
    move(50, 20 + LIFT_PX - 2)
    expect(layer(), 'lifted before it moved far enough').toBeNull()
    move(50, 40)
    expect(layer()).not.toBeNull()
    expect(document.body.classList.contains('pane-dragging')).toBe(true)
    // Over column 2, below its one pane: at its foot.
    move(400, 300)
    release(400, 300)
    expect(drops).toEqual([['one', { area: 'b', before: null }]])
    expect(layer()).toBeNull()
    expect(document.body.classList.contains('pane-dragging')).toBe(false)
    expect(announced).toEqual(['One moved: Column 2, at the foot'])
  })
  it('the line and the hot place are drawn where the pointer is, in the host’s own box', () => {
    render(<Host />)
    layout()
    press('title-three', 400, 20)
    move(400, 60)
    // Into column 1, between One and Two.
    move(100, 104)
    const line = document.querySelector<HTMLElement>('.pane-drop-line')!
    expect(line, 'no line').not.toBeNull()
    expect(parseFloat(line.style.top) + parseFloat(line.style.height) / 2).toBeCloseTo(104, 5)
    expect(document.querySelector('.pane-drop-place.hot')?.getAttribute('data-drop-area')).toBe('a')
    release(100, 104)
    expect(drops).toEqual([['three', { area: 'a', before: 'two' }]])
    expect(announced).toEqual(['Three moved: Column 1, above Two'])
  })
  it('Escape cancels: nothing is dropped, nothing is drawn — and the key is not swallowed', () => {
    render(<Host />)
    layout()
    press('title-one', 50, 20)
    move(400, 300)
    expect(layer()).not.toBeNull()
    const key = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    let seen = false
    const later = (e: KeyboardEvent) => (seen = e.key === 'Escape')
    window.addEventListener('keydown', later)
    act(() => {
      window.dispatchEvent(key)
    })
    window.removeEventListener('keydown', later)
    expect(seen, 'Escape did not reach the window’s other listeners').toBe(true)
    expect(key.defaultPrevented).toBe(false)
    expect(layer()).toBeNull()
    release(400, 300)
    expect(drops).toEqual([])
    expect(announced).toEqual([])
  })
  it('a release outside every place cancels — over the TX strip too', () => {
    render(<Host />)
    layout()
    press('title-one', 50, 20)
    move(400, 300)
    move(300, 630)
    expect(document.querySelector('.pane-drop-line')).toBeNull()
    release(300, 630)
    expect(drops).toEqual([])
  })
  it('a press on a button in a title, in a pane’s body, or outside every pane starts nothing; the button keeps its click', () => {
    render(<Host />)
    layout()
    for (const id of ['close-one', 'body-one', 'row-one', 'stop', 'strip']) {
      press(id, 50, 20)
      move(400, 300)
      expect(layer(), `a press on #${id} lifted a pane`).toBeNull()
      release(400, 300)
    }
    expect(drops).toEqual([])
    fireEvent.click($('close-one'))
    expect(clicks).toEqual(['close one'])
  })
  it('a pinned pane is offered only its own column: no line in another, and a release there drops nothing', () => {
    render(<Host />)
    layout()
    // The control: a pane that is not pinned is offered both columns.
    press('title-one', 50, 20)
    move(50, 40)
    expect([...document.querySelectorAll('.pane-drop-place')].map((p) => p.getAttribute('data-drop-area'))).toEqual(['a', 'b'])
    release(50, 40)
    press('title-two', 50, 120)
    move(50, 140)
    expect([...document.querySelectorAll('.pane-drop-place')].map((p) => p.getAttribute('data-drop-area'))).toEqual(['a'])
    move(400, 300)
    expect(document.querySelector('.pane-drop-line')).toBeNull()
    release(400, 300)
    expect(drops).toEqual([])
    // In its own column it moves, as its ▲ does.
    press('title-two', 50, 120)
    move(50, 140)
    move(50, 20)
    release(50, 20)
    expect(drops).toEqual([['two', { area: 'a', before: 'one' }]])
  })
  it('the click a release sends after a drag is swallowed; a plain click on a title is not', () => {
    render(<Host />)
    layout()
    press('title-one', 50, 20)
    move(400, 300)
    release(400, 300)
    fireEvent.click($('row-three'))
    expect(clicks, 'the click after the drag reached the row it landed on').toEqual([])
    press('title-one', 50, 20)
    release(50, 20)
    fireEvent.click($('row-three'))
    expect(clicks).toEqual(['row three'])
  })
  it('a finger lifts the pane only after a still hold; a finger that moves first is scrolling', () => {
    vi.useFakeTimers()
    render(<Host />)
    layout()
    const touch = { pointerType: 'touch', isPrimary: true }
    press('title-one', 50, 20, touch)
    move(50, 40, touch)
    act(() => vi.advanceTimersByTime(TOUCH_HOLD_MS + 50))
    expect(layer(), 'a swipe on a title lifted the pane').toBeNull()
    release(50, 40, touch)
    press('title-one', 50, 20, touch)
    act(() => vi.advanceTimersByTime(TOUCH_HOLD_MS + 50))
    expect(layer()).not.toBeNull()
    // Up, the screen does not scroll under the finger.
    const tm = new Event('touchmove', { bubbles: true, cancelable: true })
    window.dispatchEvent(tm)
    expect(tm.defaultPrevented).toBe(true)
    move(400, 300, touch)
    release(400, 300, touch)
    expect(drops).toEqual([['one', { area: 'b', before: null }]])
  })
  it('a window that does not arrange starts nothing', () => {
    render(<Host enabled={false} />)
    layout()
    press('title-one', 50, 20)
    move(400, 300)
    expect(layer()).toBeNull()
    release(400, 300)
    expect(drops).toEqual([])
  })
})
