// @vitest-environment jsdom
//
// ⊞ ARRANGE BY DRAG IN THE COCKPITS THAT ARRANGE (2026-10-08): FT in both layouts with its side rail on either
// side, Phone with its left side, CW and JS8 — each rendered with the props App gives it (the stop-line
// testkit's cases), over a panel record that RECORDS what the cockpit asks of it.
//
// A DRAG MOVES THE RECORD EXACTLY AS THE ARROWS DO. For every arrow ⊞ Arrange offers, in the stock arrangement
// and in arranged ones: the arrow is pressed, and the pane is dragged by its title, with real pointer events
// through the cockpit's own handler, to the place that arrow puts it. The drop must name that place and pass
// the arguments the arrow passed, and so give the arrow's arrangement, field for field (the record's writer is
// one function for both: features/panelPlace.drop.test.ts). Then: Escape cancels; a press on a button in a
// title, on a pane's body, on Stop TX, Tune, PTT, the TX strip or the dock starts nothing; a release over the
// TX strip drops nothing; and no drag calls anything that keys or unkeys the radio.
//
// jsdom lays nothing out, so the boxes are stubbed (panes/PaneDrag.testkit): this proves the wiring and the
// arithmetic, and the real-browser runs prove the geometry.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent } from '@testing-library/react'
import * as api from '../api'
import { ALL_PANEL_VOCABULARIES, arrangeSpecOf, placeOf, type PanelLayoutApi, type PaneMoveAt, type PanelVocabulary } from '../features/panelState'
import { PANE_COLUMNS, arrangeIds, dropArranged, moveArranged, placedColumns, type Arrangement, type ArrangeSpec, type PaneColumn, type PaneDrop, type PaneMove } from '../features/panelPlace'
import { CASE_BY_NAME, panelsWith, settle, stopsOnScreen, type Case } from './stop-line.testkit'
import { centre, columnsOf, gripOf, paneBoxOf, pickUp, pressEscape, release, stubLayout } from './panes/PaneDrag.testkit'

vi.mock('../api', async (importOriginal) =>
  (await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
)
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./panes/BoxBody', () => ({ BoxBody: () => <div data-testid="box-body-stub" /> }))

/** What the cockpit asked of the record. */
interface Asked {
  kind: 'move' | 'drop'
  id: string
  move?: PaneMove
  drop?: PaneDrop<string>
  shown: (id: string) => boolean
  sideShows: boolean
  at?: PaneMoveAt
}
function recording(base: PanelLayoutApi<string>, asked: Asked[]): PanelLayoutApi<string> {
  return {
    ...base,
    movePane: (id, move, shown, sideShows = false, at) => void asked.push({ kind: 'move', id, move, shown, sideShows, at }),
    dropPane: (id, drop, shown, sideShows = false, at) => void asked.push({ kind: 'drop', id, drop, shown, sideShows, at }),
  }
}

interface Setting {
  name: string
  c: Case<string>
  region: string
  /** Phone's left side has room (a window about 1280 px wide or wider). */
  wide?: boolean
  rail?: 'left'
}
const SETTINGS: Setting[] = [
  { name: 'FT Classic', c: CASE_BY_NAME.operateClassic, region: '.cockpit-lower' },
  { name: 'FT Classic, rail on the left', c: CASE_BY_NAME.operateClassic, region: '.cockpit-lower', rail: 'left' },
  { name: 'FT Roster', c: CASE_BY_NAME.operateRoster, region: '.cockpit-lower' },
  { name: 'FT Roster, rail on the left', c: CASE_BY_NAME.operateRoster, region: '.cockpit-lower', rail: 'left' },
  { name: 'Phone, the left side in room', c: CASE_BY_NAME.phone, region: '.cockpit-panes', wide: true },
  { name: 'CW', c: CASE_BY_NAME.cw, region: '.cockpit-panes' },
  { name: 'JS8', c: CASE_BY_NAME.js8, region: '.cockpit-panes' },
]
const vocabOf = (c: Case<string>) => ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)! as PanelVocabulary<string>

let unstub: (() => void) | null = null
function setUp(s: Setting) {
  localStorage.clear()
  if (s.rail) localStorage.setItem('nexus.operate.railSide', s.rail)
  document.documentElement.style.setProperty('--vw-eff', s.wide ? '1920px' : '1100px')
  unstub = stubLayout(s.region)
}
beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(async () => {
  cleanup()
  unstub?.()
  unstub = null
  document.documentElement.style.removeProperty('--vw-eff')
  localStorage.clear()
  // A release after a drag holds back the click it sends until the next task.
  await new Promise((r) => setTimeout(r, 0))
})

/** Where a pane stands in an arrangement, and the pane on screen right below it there. */
function placeOfPane(spec: ArrangeSpec<string>, arr: Arrangement<string>, id: string, shown: (id: string) => boolean, side: boolean): PaneDrop<string> {
  const onSide = side && spec.leftSide != null && (arr.leftSide ?? []).includes(id)
  const area: PaneColumn | 'side' = onSide ? 'side' : PANE_COLUMNS.find((c) => placedColumns(spec, arr.place)[c].includes(id))!
  const list =
    area === 'side'
      ? (arr.leftSide ?? []).filter(shown)
      : placedColumns(spec, arr.place)[area].filter((x) => shown(x) && !(side && (arr.leftSide ?? []).includes(x)))
  return { area, before: list[list.indexOf(id) + 1] ?? null }
}

/** The point on screen that names `drop` for `id` — over the pane it lands above, below the last pane of its
 *  place, or in the zone or the empty column that stands for its place. */
function pointFor(s: Setting, spec: ArrangeSpec<string>, arr: Arrangement<string>, id: string, drop: PaneDrop<string>, shown: (id: string) => boolean, side: boolean) {
  if (drop.before != null) {
    const r = paneBoxOf(s.region, drop.before)!.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + 10 }
  }
  const area = drop.area
  const others =
    area === 'side'
      ? (arr.leftSide ?? []).filter((x) => shown(x) && x !== id)
      : placedColumns(spec, arr.place)[area].filter((x) => shown(x) && x !== id && !(side && (arr.leftSide ?? []).includes(x)))
  const drawn = others.filter((x) => paneBoxOf(s.region, x) != null)
  if (drawn.length > 0) {
    const r = paneBoxOf(s.region, drawn[drawn.length - 1])!.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.bottom + 3 }
  }
  const zone = document.querySelector<HTMLElement>(`.pane-drop-zone[data-drop-area="${area}"]`)
  if (zone) {
    const left = parseFloat(zone.style.left)
    const top = parseFloat(zone.style.top)
    return { x: left + parseFloat(zone.style.width) / 2, y: top + parseFloat(zone.style.height) / 2 }
  }
  // A column drawn with no pane in it (the log column, which holds the log form).
  const empty = columnsOf(document.querySelector(s.region)!).filter((col) => !col.querySelector('[data-pane-grip]'))
  expect(empty.length, `${s.name}: no zone and no single empty column stands for ${area}`).toBe(1)
  return centre(empty[0].getBoundingClientRect())
}

const openMenu = () => {
  if (!document.querySelector('.panels-arrange')) fireEvent.click(document.querySelector('.panels-menu-btn')!)
}

/** Every arrow on offer in `arr`, pressed and then dragged: the drag must store what the arrow stores. */
async function dragEqualsArrows(s: Setting, arr: Arrangement<string>): Promise<number> {
  const c = s.c
  const vocab = vocabOf(c)
  const spec = arrangeSpecOf(vocab, c.layout)!
  const extras = spec.extra
  const asked: Asked[] = []
  const base = panelsWith<string>([], arr.place, arr.leftSide, c.layout, extras)
  c.render(recording(base, asked))
  await settle()
  const start: Arrangement<string> = { place: placeOf(vocab, base.layout, c.layout), leftSide: base.layout.leftSide }
  openMenu()
  const arrows = [...document.querySelectorAll<HTMLButtonElement>('button[data-arrange]')].filter((b) => !b.disabled).map((b) => b.dataset.arrange!)
  expect(arrows.length, `${s.name}: ⊞ Arrange offered no arrow`).toBeGreaterThan(3)
  let compared = 0
  for (const name of arrows) {
    openMenu()
    asked.length = 0
    fireEvent.click(document.querySelector<HTMLButtonElement>(`button[data-arrange="${name}"]`)!)
    const pressed = asked.find((a) => a.kind === 'move')!
    expect(pressed, `${s.name}: the arrow ${name} asked nothing`).toBeDefined()
    const arrow = moveArranged(spec, start, pressed.id, pressed.move!, pressed.shown, pressed.sideShows, pressed.at?.order)
    expect(arrow, `${s.name}: the arrow ${name} is offered but moves nothing`).not.toBeNull()
    const want = placeOfPane(spec, arrow!, pressed.id, pressed.shown, pressed.sideShows)
    // The drag, by the pane's own title, to the place the arrow put it.
    asked.length = 0
    const grip = gripOf(s.region, pressed.id)
    expect(grip, `${s.name}: ${pressed.id} has no grip in the region`).not.toBeNull()
    pickUp(grip!)
    const to = pointFor(s, spec, start, pressed.id, want, pressed.shown, pressed.sideShows)
    fireEvent.pointerMove(window, { clientX: to.x, clientY: to.y, pointerId: 7, pointerType: 'mouse' })
    release(to)
    // The click a release after a drag sends is held back until the next task; let it go before the next press.
    await new Promise((r) => setTimeout(r, 0))
    const dropped = asked.find((a) => a.kind === 'drop')
    expect(dropped, `${s.name}: dragging ${pressed.id} to where ${name} puts it dropped nothing`).toBeDefined()
    expect(dropped!.drop, `${s.name}: ${name}`).toEqual(want)
    // The arrows' own arguments …
    const ids = arrangeIds(spec)
    expect(ids.map(dropped!.shown), `${s.name}: the drop steps past other panes than ${name} does`).toEqual(ids.map(pressed.shown))
    expect(dropped!.sideShows).toBe(pressed.sideShows)
    expect(dropped!.at).toEqual(pressed.at)
    // … so the same arrangement, field for field.
    expect(dropArranged(spec, start, dropped!.id, dropped!.drop!, dropped!.shown, dropped!.sideShows, dropped!.at?.order), `${s.name}: ${name}`).toEqual(arrow)
    compared++
  }
  cleanup()
  return compared
}

/** A few arrangements the arrows build, past the stock one. */
function arranged(s: Setting): Array<Arrangement<string>> {
  const spec = arrangeSpecOf(vocabOf(s.c), s.c.layout)!
  const ids = arrangeIds(spec).filter((id) => !(spec.boxes ?? []).includes(id))
  const out: Array<Arrangement<string>> = []
  let arr: Arrangement<string> = {}
  const MOVES: PaneMove[] = ['right', 'down', 'left', 'up', 'right', 'right']
  for (let i = 0; out.length < 2 && i < 40; i++) {
    const next = moveArranged(spec, arr, ids[(i * 7) % ids.length], MOVES[i % MOVES.length], () => true, s.wide === true)
    if (next) arr = next
    if (i % 9 === 8) out.push(arr)
  }
  return out
}

/** Each setting's budget: three mounts (stock and two arranged), every arrow pressed and dragged in each. The
 *  heaviest, FT Classic with its rail on the left, took 4.3 s alone, 33.0 s in the full suite on a loaded box,
 *  54.8 s at a fifth of a CPU and 73.5 s at a tenth (a SIGSTOP/SIGCONT duty cycle), where it had also run past
 *  the first budget of 60 s once. 240 s is the arrangement sweeps' budget (stop-line.testkit): 3.3× the tenth. */
const DRAG_EQUALS_ARROWS_BUDGET_MS = 240_000

describe('a drag moves the record exactly as the arrows do', () => {
  it.each(SETTINGS.map((s) => [s.name, s] as const))(
    '%s: every arrow on offer, in the stock arrangement and in arranged ones',
    async (_name, s) => {
      setUp(s)
      let compared = await dragEqualsArrows(s, {})
      for (const arr of arranged(s)) compared += await dragEqualsArrows(s, arr)
      expect(compared).toBeGreaterThan(8)
    },
    DRAG_EQUALS_ARROWS_BUDGET_MS,
  )
})

/** The stop controls of a case, on screen. */
const stops = (c: Case<string>) => [...stopsOnScreen(c.stopControls).values()].flat()
const TX_CALLS = ['callStation', 'startCq', 'sendCw', 'stopCw', 'setPtt', 'playVoiceMessage', 'stopVoice', 'setTxEnabled', 'setTune', 'haltTx', 'pskSend', 'js8Send', 'js8SendCommand'] as const

describe('what a drag never does', () => {
  it.each(SETTINGS.map((s) => [s.name, s] as const))(
    '%s: Escape cancels; a press on a control, a body, a stop control, the strip or the dock starts nothing; nothing keys',
    async (_name, s) => {
      setUp(s)
      const c = s.c
      const asked: Asked[] = []
      const spec = arrangeSpecOf(vocabOf(c), c.layout)!
      c.render(recording(panelsWith<string>([], undefined, undefined, c.layout, spec.extra), asked))
      await settle()
      const id = arrangeIds(spec).find((x) => gripOf(s.region, x) != null)!
      const cols = columnsOf(document.querySelector(s.region)!)
      const far = centre(cols[cols.length - 1].getBoundingClientRect())

      // Escape, mid-drag over a place: nothing drawn after it, nothing dropped on release.
      pickUp(gripOf(s.region, id)!, far)
      expect(document.querySelector('.pane-drop-layer'), `${s.name}: the drag drew nothing`).not.toBeNull()
      pressEscape()
      expect(document.querySelector('.pane-drop-layer')).toBeNull()
      release(far)
      expect(asked.filter((a) => a.kind === 'drop'), `${s.name}: Escape did not cancel`).toEqual([])

      // A release over the TX strip (or FT's QSO strip) drops nothing.
      const strip = document.querySelector('.cockpit-txstrip, .cockpit-qso')
      expect(strip, `${s.name}: no TX strip on screen`).not.toBeNull()
      pickUp(gripOf(s.region, id)!, far, centre(strip!.getBoundingClientRect()))
      release(centre(strip!.getBoundingClientRect()))
      expect(asked.filter((a) => a.kind === 'drop'), `${s.name}: a release over the TX strip dropped the pane`).toEqual([])

      // Nothing outside a title, and no control inside one, lifts anything.
      const from: Element[] = [
        ...stops(c),
        strip!,
        ...[document.querySelector('.cockpit-txdock')].filter((x): x is Element => x != null),
        // A pane's own ✕, and a button anywhere in a pane.
        ...[...document.querySelectorAll(`${s.region} [data-pane-grip] button`)].slice(0, 3),
        ...[...document.querySelectorAll(`${s.region} .pane-body button, ${s.region} .pane-body`)].slice(0, 3),
      ]
      expect(stops(c).length, `${s.name}: no stop control found`).toBeGreaterThan(0)
      for (const el of from) {
        fireEvent.pointerDown(el, { clientX: 5, clientY: 5, pointerId: 9, button: 0, pointerType: 'mouse' })
        fireEvent.pointerMove(window, { clientX: far.x, clientY: far.y, pointerId: 9, pointerType: 'mouse' })
        expect(document.querySelector('.pane-drop-layer'), `${s.name}: a press on ${el.outerHTML.slice(0, 80)} lifted a pane`).toBeNull()
        fireEvent.pointerUp(window, { clientX: far.x, clientY: far.y, pointerId: 9, pointerType: 'mouse' })
      }
      expect(asked.filter((a) => a.kind === 'drop')).toEqual([])

      // ⊞ Arrange's list stands inside the cockpit: a row dragged there is the list's drag alone, never the
      // cockpit's too (one layer, the list's).
      await new Promise((r) => setTimeout(r, 0))
      openMenu()
      const row = document.querySelector(`.panels-arrange [data-pane-grip="${id}"]`)
      expect(row, `${s.name}: ⊞ Arrange lists no row for ${id}`).not.toBeNull()
      fireEvent.pointerDown(row!, { clientX: 5, clientY: 5, pointerId: 11, button: 0, pointerType: 'mouse' })
      fireEvent.pointerMove(window, { clientX: 5, clientY: 40, pointerId: 11, pointerType: 'mouse' })
      const layers = [...document.querySelectorAll('.pane-drop-layer')]
      expect(layers.map((l) => l.parentElement?.className), `${s.name}: a row dragged in the list lifted the cockpit's pane too`).toEqual(['panels-arrange'])
      pressEscape()
      fireEvent.pointerUp(window, { clientX: 5, clientY: 40, pointerId: 11, pointerType: 'mouse' })

      // A drag that lands: still nothing that keys or unkeys the radio, and the stop controls where they were.
      // (The presses above on PTT and the stop controls, and Escape, do key and unkey: they are those controls.)
      await new Promise((r) => setTimeout(r, 0))
      for (const name of TX_CALLS) vi.mocked(api[name] as unknown as () => void).mockClear()
      const before = stops(c).map((b) => (b as HTMLButtonElement).disabled)
      pickUp(gripOf(s.region, id)!, far)
      release(far)
      expect(asked.filter((a) => a.kind === 'drop').length, `${s.name}: the control drag dropped nothing`).toBe(1)
      for (const name of TX_CALLS) expect(vi.mocked(api[name] as unknown as () => void), `${s.name}: a drag called ${name}`).not.toHaveBeenCalled()
      expect(stops(c).map((b) => (b as HTMLButtonElement).disabled)).toEqual(before)
    },
    30_000,
  )
})
