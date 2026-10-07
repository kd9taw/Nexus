// WHERE A GRID COCKPIT'S PANES STAND (layout L3) — the pure half of ⊞ Panels ▸ Arrange. Phone lays its
// pane region out as up to three columns: `a` (the leading feeds), `b` (the rig strips) and `log`
// (the log form). Today each pane has one fixed place in that grouping; Arrange lets the operator
// move a pane up or down within its column, or into the column beside it, and the panel record
// keeps where each one stands (PanelLayout.place). Everything here is arithmetic on that record, so
// every rule is unit-tested without a cockpit.
//
// THE RULES (spec §3.2 Primitive 2):
//   · only a pane the cockpit ARRANGES may carry a place — its vocabulary id, listed in its
//     ArrangeSpec. An unknown id is dropped on load, so a stop control, which has no id, can never
//     be named, moved or hidden by a stored or hand-edited layout: unrepresentable, not guarded;
//   · every pane stands in exactly one column, and a pane the record does not name (one added in a
//     later release) lands at the end of its stock column;
//   · a PINNED pane keeps its column and only moves up or down in it (the operator's D9, "Pinned to
//     their column"): Phone's voice keyer. A pane that changes column changes its DOM parent, and
//     React cannot carry it across — the keyer would lose an over in flight and a recording. (The
//     log form has no id at all, so it cannot move; it stays at the foot of the log column.)
//   · orders are whole numbers from 0 in each column, re-numbered on load, so a hand-edited record
//     cannot leave a gap or a tie;
//   · the TIERS derive from the three-column placement exactly as they did from the fixed grouping:
//     at two tracks column `b` follows `a` in one column, at one track everything stacks a → b → log.
//     Until the operator moves anything the stock grouping of the narrower tiers is kept as it was
//     (Phone's feeds follow its rig strips there), so nobody's screen changes on the update.
//
// A column's ORDER on screen (`colOrder`, "swap columns") is read and kept by the record, but no
// cockpit renders it yet: its tracks are positional (cockpit-panes.css) and the column dividers
// size them by position, so a swap has to move the widths and the dividers with it first.
//
// THE LEFT SIDE (operator's pick, 2026-10-03: "Arrange gets a third place, 'Left side'"; Phone only):
// a full-height column beside the scope, outside the pane region, for the panes an ArrangeSpec lists in
// `leftSide` (Phone: Band Activity, Spots and Needed). The record keeps it APART from the placement
// (PanelLayout.leftSide, the panes there top to bottom): a pane on the left side keeps its place in
// the three columns, which is where it stands whenever the side does not show — a window under about
// 1280 px, where the stored choice is kept and never rewritten — and where ▶ takes it back. ◀ from the
// first column puts a listed pane there. Never the voice keyer, never the log form: a pane that
// changes parent is remounted, and the side comes and goes with the window's width.
//
// THE BOXES (2026-10-07: any pane in any area): a cockpit's six box slots (ArrangeSpec `boxes`), each
// showing an entry of the shared list. A box is listed in `columns` (its stock column) and moves like
// a pane, but where boxes stand is no part of the stock grouping: the cockpit's own panes keep their
// stock tiers whatever the boxes do (`isStockPlacement`), and below three tracks each box stands right
// after the own pane above it in its column (`regionGroups`), so adding one moves none of them.

/** The columns of a grid cockpit's pane region, in their stock order on screen. */
export type PaneColumn = 'a' | 'b' | 'log'
export const PANE_COLUMNS: readonly PaneColumn[] = ['a', 'b', 'log']

/** Where one pane stands: its column, and its place in it from the top (0-based). */
export interface PanePlace {
  col: PaneColumn
  order: number
}

export type PanePlacement<P extends string> = Partial<Record<P, PanePlace>>

/** What a cockpit lets the operator arrange. */
export interface ArrangeSpec<P extends string> {
  /** The stock three-column grouping, top to bottom: every pane the operator may move, each in
   *  exactly one column. A pane outside the region (a scope strip above it, the meters in the dock)
   *  is not listed, so it can never be given a place. */
  readonly columns: Readonly<Record<PaneColumn, readonly P[]>>
  /** The panes that keep their column: they move up and down in it only (D9). */
  readonly pinned: readonly P[]
  /** Below three tracks, the stock order of the one column `a` and `b` share, where it is not simply
   *  `a` then `b`. Used only while nothing has been arranged. */
  readonly stockMerged?: readonly P[]
  /** The panes that may stand on the LEFT SIDE (see the header). Absent: the cockpit has none. Each
   *  must be a pane whose remount loses nothing in flight, because the side shows and goes with the
   *  window's width — so never a pinned pane. */
  readonly leftSide?: readonly P[]
  /** The cockpit's BOXES (see the header), each also listed in `columns`. Absent: it has none. */
  readonly boxes?: readonly P[]
}

/** Every pane an ArrangeSpec lists, in stock order (a, then b, then log). */
export function arrangeIds<P extends string>(spec: ArrangeSpec<P>): P[] {
  return PANE_COLUMNS.flatMap((c) => [...spec.columns[c]])
}

/** A pane's stock column. */
export function stockColumn<P extends string>(spec: ArrangeSpec<P>, id: P): PaneColumn | null {
  for (const c of PANE_COLUMNS) if (spec.columns[c].includes(id)) return c
  return null
}

function isColumn(v: unknown): v is PaneColumn {
  return typeof v === 'string' && (PANE_COLUMNS as readonly string[]).includes(v)
}

/**
 * A valid placement from any input, or undefined for "nothing arranged". Only listed panes keep a
 * place; a pinned pane is held in its stock column; each column's orders are re-numbered 0..n in
 * the order they were stored (ties broken by the stock order), so a hand-edited or foreign record
 * cannot leave a gap, a duplicate or a pane in a column it may not stand in.
 */
export function coercePlacement<P extends string>(spec: ArrangeSpec<P>, raw: unknown): PanePlacement<P> | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const src = raw as Record<string, unknown>
  const ids = arrangeIds(spec)
  const kept: Array<{ id: P; col: PaneColumn; order: number; stock: number }> = []
  ids.forEach((id, stock) => {
    const v = src[id] as { col?: unknown; order?: unknown } | undefined
    if (!v || typeof v !== 'object' || !isColumn(v.col) || typeof v.order !== 'number' || !Number.isFinite(v.order)) return
    const col = spec.pinned.includes(id) ? stockColumn(spec, id)! : v.col
    kept.push({ id, col, order: v.order, stock })
  })
  if (kept.length === 0) return undefined
  const out: PanePlacement<P> = {}
  for (const c of PANE_COLUMNS) {
    kept
      .filter((k) => k.col === c)
      .sort((x, y) => x.order - y.order || x.stock - y.stock)
      .forEach((k, i) => (out[k.id] = { col: c, order: i }))
  }
  return out
}

/** A valid column order (a permutation of the three columns), or undefined for the stock one. */
export function coerceColumnOrder(raw: unknown): PaneColumn[] | undefined {
  if (!Array.isArray(raw) || raw.length !== PANE_COLUMNS.length) return undefined
  if (!raw.every(isColumn) || new Set(raw).size !== PANE_COLUMNS.length) return undefined
  return [...raw] as PaneColumn[]
}

/** The three-column grouping a placement gives: each column's placed panes in order, then the panes
 *  it does not name, at the end of their stock column in stock order. Every listed pane appears
 *  exactly once, shown or not. */
export function placedColumns<P extends string>(spec: ArrangeSpec<P>, place: PanePlacement<P> | undefined): Record<PaneColumn, P[]> {
  const out: Record<PaneColumn, P[]> = { a: [], b: [], log: [] }
  for (const c of PANE_COLUMNS) {
    const placed = arrangeIds(spec)
      .filter((id) => place?.[id]?.col === c)
      .sort((x, y) => place![x]!.order - place![y]!.order)
    const rest = spec.columns[c].filter((id) => place?.[id] == null)
    out[c] = [...placed, ...rest]
  }
  return out
}

/** One rendered column group: which column it is (the key its div keeps) and the panes in it. */
export interface RegionGroup<P extends string> {
  col: PaneColumn
  ids: P[]
}

/**
 * The groups a region renders at `tracks` columns, keeping only the panes `shown` says are on screen.
 * Three tracks: a | b | log. Fewer: one column holding a then b (or the stock merged order while the
 * placement is the stock one), then the log column; at one track the sheet stacks the two groups. A
 * group with nothing in it is still returned — whether an empty column renders is the cockpit's
 * call, and its track count already collapses (useRegionCols).
 *
 * ⊞ Arrange moves panes in the THREE-column placement at every tier (its menu lists the three
 * columns), and the narrower tiers derive from it; so once anything is arranged, the merged column
 * is simply a then b.
 */
export function regionGroups<P extends string>(
  spec: ArrangeSpec<P>,
  place: PanePlacement<P> | undefined,
  tracks: 1 | 2 | 3,
  shown: (id: P) => boolean,
): RegionGroup<P>[] {
  const cols = placedColumns(spec, place)
  const keep = (ids: readonly P[]) => ids.filter(shown)
  if (tracks === 3) return PANE_COLUMNS.map((c) => ({ col: c, ids: keep(cols[c]) }))
  const merged = spec.stockMerged && isStockPlacement(spec, place) ? mergeBoxes(spec, cols, spec.stockMerged) : [...cols.a, ...cols.b]
  return [
    { col: 'a', ids: keep(merged) },
    { col: 'log', ids: keep(cols.log) },
  ]
}

/** The stock merged column with the boxes of columns a and b in it: each box right after the own pane
 *  above it in its column (boxes under the same pane in their order), a box at the top of its column
 *  before that column's first own pane. The own panes keep the stock order exactly. */
function mergeBoxes<P extends string>(spec: ArrangeSpec<P>, cols: Record<PaneColumn, P[]>, stock: readonly P[]): P[] {
  const boxes = spec.boxes ?? []
  const out = stock.filter((id) => !boxes.includes(id))
  for (const c of ['a', 'b'] as const) {
    const first = out.indexOf(cols[c].find((id) => !boxes.includes(id)) as P)
    let at = first < 0 ? out.length - 1 : first - 1
    for (const id of cols[c]) {
      if (boxes.includes(id)) out.splice(++at, 0, id)
      else at = out.indexOf(id)
    }
  }
  return out
}

export type PaneMove = 'up' | 'down' | 'left' | 'right'

/** Every listed pane's place, written out in full: what a move stores, so a later release's new pane
 *  is the only one the record does not name. */
function materialize<P extends string>(cols: Record<PaneColumn, P[]>): PanePlacement<P> {
  const out: PanePlacement<P> = {}
  for (const c of PANE_COLUMNS) cols[c].forEach((id, i) => (out[id] = { col: c, order: i }))
  return out
}

/**
 * The placement after one move of `id`, or null when the move does nothing (a pane already at the top
 * of its column moving up, a pinned pane moving sideways, a column with no neighbour that way). Up
 * and down step past panes that are hidden, so every move changes what is on screen; left and right
 * go to the neighbouring column on screen (`colOrder`, stock a | b | log) and put the pane at the
 * foot of it. The result names every listed pane.
 */
export function movePane<P extends string>(
  spec: ArrangeSpec<P>,
  place: PanePlacement<P> | undefined,
  colOrder: readonly PaneColumn[] | undefined,
  id: P,
  move: PaneMove,
  shown: (id: P) => boolean,
): PanePlacement<P> | null {
  const cols = placedColumns(spec, place)
  const from = PANE_COLUMNS.find((c) => cols[c].includes(id))
  if (!from) return null
  if (move === 'up' || move === 'down') {
    const list = cols[from]
    const at = list.indexOf(id)
    const step = move === 'up' ? -1 : 1
    let to = at + step
    while (to >= 0 && to < list.length && !shown(list[to])) to += step
    if (to < 0 || to >= list.length) return null
    list.splice(at, 1)
    list.splice(to, 0, id)
    return materialize(cols)
  }
  if (spec.pinned.includes(id)) return null
  const order = colOrder ?? PANE_COLUMNS
  const to = order[order.indexOf(from) + (move === 'left' ? -1 : 1)]
  if (!to) return null
  cols[from] = cols[from].filter((x) => x !== id)
  cols[to] = [...cols[to], id]
  return materialize(cols)
}

/** Whether a move would do anything — what the ⊞ Arrange buttons' `disabled` reads. */
export function canMovePane<P extends string>(
  spec: ArrangeSpec<P>,
  place: PanePlacement<P> | undefined,
  colOrder: readonly PaneColumn[] | undefined,
  id: P,
  move: PaneMove,
  shown: (id: P) => boolean,
): boolean {
  return movePane(spec, place, colOrder, id, move, shown) != null
}

/** The stock placement written out in full: what the grouping is before anything moved. */
export function stockPlacement<P extends string>(spec: ArrangeSpec<P>): PanePlacement<P> {
  return materialize(placedColumns(spec, undefined))
}

/** Whether a placement is the stock one (in effect), so the menu can say nothing has been arranged.
 *  The boxes are no part of it (see the header): only the cockpit's own panes are compared. */
export function isStockPlacement<P extends string>(spec: ArrangeSpec<P>, place: PanePlacement<P> | undefined): boolean {
  if (place == null) return true
  const now = placedColumns(spec, place)
  const own = (ids: readonly P[]) => ids.filter((id) => !spec.boxes?.includes(id)).join('\u0000')
  return PANE_COLUMNS.every((c) => own(now[c]) === own(spec.columns[c]))
}

/** The placement with `id` at the foot of column `col`, every other pane where it was: where "+ Add a
 *  box" puts a box. The result names every listed pane. */
export function placeAtFoot<P extends string>(spec: ArrangeSpec<P>, place: PanePlacement<P> | undefined, id: P, col: PaneColumn): PanePlacement<P> {
  const cols = placedColumns(spec, place)
  for (const c of PANE_COLUMNS) cols[c] = cols[c].filter((x) => x !== id)
  cols[col].push(id)
  return materialize(cols)
}

// ── THE LEFT SIDE ────────────────────────────────────────────────────────────────────────────────

/** The panes on the left side from any input, or undefined for "nothing there": only the panes the
 *  spec lists in `leftSide`, each once, in the order stored. An id outside that list — the voice keyer,
 *  a rig strip, a hand-edited stop control — is dropped, so a stored or hand-edited record cannot put
 *  one there. */
export function coerceLeftSide<P extends string>(spec: ArrangeSpec<P>, raw: unknown): P[] | undefined {
  if (!spec.leftSide || !Array.isArray(raw)) return undefined
  const allowed = spec.leftSide as readonly string[]
  const out: P[] = []
  for (const v of raw) if (typeof v === 'string' && allowed.includes(v) && !out.includes(v as P)) out.push(v as P)
  return out.length > 0 ? out : undefined
}

/** What ⊞ Arrange moves: the three-column placement and the panes on the left side. */
export interface Arrangement<P extends string> {
  place?: PanePlacement<P>
  leftSide?: P[]
}

/**
 * The arrangement after one move of `id`, or null when it does nothing. `sideShows` is whether the
 * left side is on screen on this window (wide enough, and the cockpit has one).
 *
 * While it shows, a pane on it moves ▲ ▼ among the panes there (past the hidden ones), ▶ takes it
 * back to its place in the columns, and ◀ does nothing; ◀ on a listed pane in the first column puts
 * it at the foot of the side; and every other move is `movePane` among the panes in the columns.
 * While it does not show, its panes stand in their columns and move there like any other, and the
 * side itself is kept exactly as stored — no move reaches it.
 */
export function moveArranged<P extends string>(
  spec: ArrangeSpec<P>,
  arr: Arrangement<P>,
  id: P,
  move: PaneMove,
  shown: (id: P) => boolean,
  sideShows: boolean,
): Arrangement<P> | null {
  const side = arr.leftSide ?? []
  const keep = (place: PanePlacement<P> | null): Arrangement<P> | null => (place ? { ...arr, place } : null)
  if (!sideShows || !spec.leftSide) return keep(movePane(spec, arr.place, undefined, id, move, shown))
  if (side.includes(id)) {
    if (move === 'left') return null
    if (move === 'right') {
      const rest = side.filter((x) => x !== id)
      return { place: arr.place, ...(rest.length > 0 ? { leftSide: rest } : {}) }
    }
    const at = side.indexOf(id)
    const step = move === 'up' ? -1 : 1
    let to = at + step
    while (to >= 0 && to < side.length && !shown(side[to])) to += step
    if (to < 0 || to >= side.length) return null
    const next = side.filter((x) => x !== id)
    next.splice(to, 0, id)
    return { ...arr, leftSide: next }
  }
  if (move === 'left' && spec.leftSide.includes(id) && placedColumns(spec, arr.place)[PANE_COLUMNS[0]].includes(id)) {
    return { ...arr, leftSide: [...side, id] }
  }
  return keep(movePane(spec, arr.place, undefined, id, move, (x) => shown(x) && !side.includes(x)))
}

/** Whether a move would do anything — what the ⊞ Arrange buttons' `disabled` reads. */
export function canMoveArranged<P extends string>(
  spec: ArrangeSpec<P>,
  arr: Arrangement<P>,
  id: P,
  move: PaneMove,
  shown: (id: P) => boolean,
  sideShows: boolean,
): boolean {
  return moveArranged(spec, arr, id, move, shown, sideShows) != null
}
