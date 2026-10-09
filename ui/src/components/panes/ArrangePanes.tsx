// ⊞ PANELS ▸ ARRANGE (layout L3): where each pane of a grid cockpit stands. The three columns of the
// region, each with the panes on screen in it, top to bottom; every pane has ▲ ▼ to move it within its
// column and ◀ ▶ to move it into the column beside it. It rides in the ⊞ menu's `lead` slot, so the
// menu's own contract is unchanged, and its Undo and Reset cover a move like any other change.
//
// KEYBOARD FIRST: each move is a plain button with an accessible name that says which pane and which
// way ("Move Receiver up"), and a move that would do nothing is disabled rather than silently inert.
// A PINNED pane (the voice keyer: D9) has only ▲ ▼, and says why. The log form has no id and no
// entry: it stays at the foot of its column.
//
// FOCUS STAYS WITH THE PANE. A move pressed from a focused button can take that button away: it goes
// disabled at the end of its column, or its row is rebuilt in the pane's new column. Either way the
// browser drops focus to <body>, and a keyboard operator restarts Tab at the top of the document.
// So after such a move focus goes to the same button while it can still move the pane that way,
// else the opposite one, else any live button of that pane. A press from an unfocused button (a
// mouse click where the webview does not focus buttons) leaves focus alone.
//
// THE LEFT SIDE (2026-10-03, Phone): a cockpit whose ArrangeSpec lists `leftSide` gets a fourth group,
// first because it stands left of everything: the panes there with ▲ ▼ and ▶ (back to the column
// they stand in otherwise), and ◀ in column 1 puts a listed pane there. On a window too narrow for the
// side (`sideRoom` false) the group names what is stored there and says where those panes are now —
// in their columns, listed and moved there — and no move can reach the stored side, so a narrow
// window never rewrites it.
//
// THE BOXES (2026-10-07: any pane in any area): where the cockpit offers them (`onAddBox`), every place
// ends with "+ Add a box", which puts the first hidden box at its foot (the record's `addBox`); a box on
// screen is listed and moved like any pane, under the name of what it shows. With all six on screen the
// buttons stay where they are, disabled, and say why — the note explains, it never hides the control.
//
// FT'S COLUMNS (2026-10-07): a cockpit whose columns are not Phone's names them itself (`columns`, in their
// order on screen — FT's side rail can stand on the left, and Roster has two), and says what a narrower
// window does with them (`narrow`). ◀ ▶ then follow that order, and there is no log form to mention.
//
// BY DRAG TOO (2026-10-08): where the cockpit offers it (`onDrop`), a row is dragged by its name or its ⠿
// grip onto another place in the list (panes/PaneDrag), and the drop is the same moves the arrows make
// (the record's `dropPane`). The arrows stay, for the keyboard and screen readers; a drop is told to a
// screen reader, and focus stays with the pane as after an arrow when it was in the list.
//
// THE STOP LINE is not near this: only a pane with a vocabulary id can be listed or moved (the
// ArrangeSpec, features/panelPlace), and no control that stops a transmission has one. A move changes
// where a pane stands in the region or on the left side and nothing else — the header and the TX dock
// are neither, and no move can reach them.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The panes' names are the
// cockpit's (`labels`); the arrows are glyphs, not words.
import { useId, useLayoutEffect, useRef } from 'react'
import { getLocale, t } from '../../i18n'
import { PANE_COLUMNS, canMoveArranged, placedColumns, type ArrangeSpec, type PaneColumn, type PaneDrop, type PaneMove } from '../../features/panelPlace'
import type { PanelLayout } from '../../features/panelState'
import { PaneDropLayer, usePaneDrag, type DropArea, type PaneDropTarget } from './PaneDrag'

/** The four moves, in the order the buttons stand, with the glyph each shows. */
const MOVES: ReadonlyArray<readonly [PaneMove, string]> = [
  ['up', '▲'],
  ['down', '▼'],
  ['left', '◀'],
  ['right', '▶'],
]

const OPPOSITE: Readonly<Record<PaneMove, PaneMove>> = { up: 'down', down: 'up', left: 'right', right: 'left' }

const moveName = (move: PaneMove, pane: string): string =>
  move === 'up'
    ? t('panels.arrange.up.aria', { pane })
    : move === 'down'
      ? t('panels.arrange.down.aria', { pane })
      : move === 'left'
        ? t('panels.arrange.left.aria', { pane })
        : t('panels.arrange.right.aria', { pane })

/** `Intl.ListFormat` where the runtime has it (every webview Nexus ships in does); else a comma list. */
const ListFormat = (
  Intl as unknown as {
    ListFormat?: new (locale: string, o: { style: string; type: string }) => { format(items: string[]): string }
  }
).ListFormat
const listOf = (names: string[], type: 'conjunction' | 'disjunction'): string =>
  ListFormat ? new ListFormat(getLocale(), { style: 'long', type }).format(names) : names.join(', ')

const columnName = (col: PaneColumn): string =>
  col === 'a' ? t('panels.arrange.column.a') : col === 'b' ? t('panels.arrange.column.b') : t('panels.arrange.column.log')

export interface ArrangePanesProps<P extends string> {
  spec: ArrangeSpec<P>
  layout: PanelLayout<P>
  /** Whether a pane is on screen: only those are listed, and a move steps past the others. */
  shown: (id: P) => boolean
  /** The panes' operator-facing names (the ⊞ entries' own). */
  labels: Readonly<Record<P, string>>
  /** Whether this window has room for the cockpit's left side (absent: it has none). */
  sideRoom?: boolean
  onMove: (id: P, move: PaneMove) => void
  /** A row dropped by drag onto a place in the list (the record's `dropPane`, with the arguments `onMove`'s
   *  arrows pass). Absent: the rows are not dragged. */
  onDrop?: (id: P, drop: PaneDrop<P>) => void
  /** "+ Add a box" at the foot of `area` (the record's `addBox`). Absent: no box is offered here — the
   *  cockpit has none, or the window lends them nothing (the hosted Remote page). */
  onAddBox?: (area: PaneColumn | 'side') => void
  /** Every box is on screen: the add buttons stay, disabled, and say why. */
  boxesFull?: boolean
  /** The cockpit's columns as they stand on screen, each with its name and its "+ Add a box" name, where
   *  they are not Phone's a | b | log (FT: see the header). */
  columns?: ReadonlyArray<{ col: PaneColumn; name: string; addAria: string }>
  /** What a narrower window does with the columns, where it is not what Phone's do. */
  narrow?: string
}

/** Each place's "+ Add a box", named for where it adds one: a whole sentence per place. */
const ADD_ARIA: Readonly<Record<PaneColumn | 'side', () => string>> = {
  a: () => t('panels.box.add.a.aria'),
  b: () => t('panels.box.add.b.aria'),
  log: () => t('panels.box.add.log.aria'),
  side: () => t('panels.box.add.side.aria'),
}

export function ArrangePanes<P extends string>({ spec, layout, shown, labels, sideRoom = false, onMove, onDrop, onAddBox, boxesFull = false, columns, narrow }: ArrangePanesProps<P>) {
  const uid = useId()
  const rootRef = useRef<HTMLDivElement>(null)
  // The move just pressed from a focused button, until the render that shows it.
  const pending = useRef<{ id: P; move: PaneMove } | null>(null)
  // The place whose "+ Add a box" was just pressed from a focused button, until the render that shows it.
  const pendingAdd = useRef<PaneColumn | 'side' | null>(null)
  const cols = placedColumns(spec, layout.place)
  // The left side: what is stored there, and whether it is on screen here.
  const side = layout.leftSide ?? []
  const sideOn = spec.leftSide != null && sideRoom
  const arr = { place: layout.place, leftSide: layout.leftSide }
  // The columns in their order on screen: the cockpit's own, or Phone's a | b | log.
  const places = columns ?? PANE_COLUMNS.map((col) => ({ col, name: columnName(col), addAria: ADD_ARIA[col]() }))
  const order = columns?.map((c) => c.col)
  const can = (id: P, move: PaneMove) => canMoveArranged(spec, arr, id, move, shown, sideOn, order)
  // By drag: the places are the list's own groups, one above the other, and a drop runs the arrows' moves.
  const drag = usePaneDrag<P>({
    root: rootRef,
    region: rootRef,
    enabled: onDrop != null,
    spec,
    arrangement: arr,
    shown,
    sideShows: sideOn,
    order,
    stacked: true,
    targets: () =>
      [...(rootRef.current?.querySelectorAll<HTMLElement>(':scope > [data-arrange-area]') ?? [])].map(
        (el): PaneDropTarget => ({ el, areas: [el.dataset.arrangeArea as DropArea] }),
      ),
    labels,
    names: { ...Object.fromEntries(places.map((pl) => [pl.col, pl.name])), side: t('panels.arrange.side') },
    onDrop: (id, drop) => {
      // As after an arrow pressed from the keyboard: focus stays with the pane, if it was in the list.
      if (rootRef.current?.contains(document.activeElement)) pending.current = { id, move: 'up' }
      onDrop?.(id, drop)
    },
  })
  // A pane on the side is listed there while the side shows; otherwise it is in its column.
  const inColumn = (id: P) => shown(id) && !(sideOn && side.includes(id))
  const moveLabel = (id: P, move: PaneMove): string =>
    sideOn && side.includes(id) && move === 'right'
      ? t('panels.arrange.fromSide.aria', { pane: labels[id] })
      : sideOn && move === 'left' && spec.leftSide!.includes(id) && cols[PANE_COLUMNS[0]].includes(id)
        ? t('panels.arrange.toSide.aria', { pane: labels[id] })
        : moveName(move, labels[id])
  const paneRow = (id: P, onSide: boolean) => {
    const pinned = spec.pinned.includes(id)
    const pinnedId = pinned ? `${uid}-${id}-pinned` : undefined
    return (
      <div key={id} className="panels-arrange-pane">
        <div className="panels-arrange-row">
          {onDrop && (
            <span className="panels-arrange-grip" data-pane-grip={id} aria-hidden="true" title={t('panels.drag.grip.title', { pane: labels[id] })}>
              ⠿
            </span>
          )}
          <span className="panels-arrange-name" data-pane-grip={onDrop ? id : undefined}>
            {labels[id]}
          </span>
          <span className="panels-arrange-moves">
            {MOVES.map(([move, glyph]) =>
              // A pinned pane has no ◀ ▶; a pane on the side has no ◀ (nothing stands left of it).
              (pinned && (move === 'left' || move === 'right')) || (onSide && move === 'left') ? null : (
                <button
                  key={move}
                  type="button"
                  className="panels-arrange-btn"
                  aria-label={moveLabel(id, move)}
                  aria-describedby={pinnedId}
                  title={moveLabel(id, move)}
                  // The moves follow the columns' order on screen: the cockpit's (`columns`), else a | b
                  // | log — no cockpit renders a stored column order yet (as the hook's moves do).
                  disabled={!can(id, move)}
                  data-arrange={`${id} ${move}`}
                  onClick={(e) => {
                    if (document.activeElement === e.currentTarget) pending.current = { id, move }
                    onMove(id, move)
                  }}
                >
                  {glyph}
                </button>
              ),
            )}
          </span>
        </div>
        {pinned && (
          <span className="panels-menu-why" id={pinnedId}>
            {t('panels.arrange.pinned')}
          </span>
        )}
      </div>
    )
  }
  // "+ Add a box", the last thing in its place. A press from a focused button keeps focus there while the
  // button can still add; the sixth box disables it, and focus then goes to that place's last live move.
  const addButton = (area: PaneColumn | 'side') =>
    onAddBox ? (
      <button
        type="button"
        className="panels-arrange-add"
        aria-label={places.find((p) => p.col === area)?.addAria ?? ADD_ARIA[area]()}
        aria-describedby={boxesFull ? `${uid}-full` : undefined}
        disabled={boxesFull}
        data-add={area}
        onClick={(e) => {
          if (document.activeElement === e.currentTarget) pendingAdd.current = area
          onAddBox(area)
        }}
      >
        {t('panels.box.add')}
      </button>
    ) : null
  useLayoutEffect(() => {
    const area = pendingAdd.current
    if (!area) return
    pendingAdd.current = null
    const add = rootRef.current?.querySelector<HTMLButtonElement>(`button[data-add="${area}"]`)
    if (!add || !add.disabled) return
    const live = [...(add.parentElement?.querySelectorAll<HTMLButtonElement>('button[data-arrange]') ?? [])].filter((b) => !b.disabled)
    const to = live[live.length - 1]
    if (to) {
      to.focus({ preventScroll: true })
      to.scrollIntoView?.({ block: 'nearest' })
    }
  })
  useLayoutEffect(() => {
    const p = pending.current
    if (!p) return
    pending.current = null
    const buttons = [...(rootRef.current?.querySelectorAll<HTMLButtonElement>('button[data-arrange]') ?? [])]
    for (const move of [p.move, OPPOSITE[p.move], ...MOVES.map(([m]) => m)]) {
      const b = buttons.find((x) => x.dataset.arrange === `${p.id} ${move}`)
      if (b && !b.disabled) {
        if (document.activeElement !== b) {
          b.focus({ preventScroll: true })
          b.scrollIntoView?.({ block: 'nearest' })
        }
        return
      }
    }
  })
  return (
    <div className="panels-arrange" role="group" aria-labelledby={`${uid}-head`} ref={rootRef} data-pane-drag={onDrop ? '' : undefined}>
      <span className="panels-arrange-head" id={`${uid}-head`}>
        {t('panels.arrange.heading')}
      </span>
      {onAddBox && boxesFull && (
        <span className="panels-menu-why" id={`${uid}-full`}>
          {t('panels.box.full')}
        </span>
      )}
      {spec.leftSide && (
        <div className="panels-arrange-col" role="group" aria-labelledby={`${uid}-side`} data-arrange-area={sideOn ? 'side' : undefined}>
          <span className="panels-arrange-colhead" id={`${uid}-side`}>
            {t('panels.arrange.side')}
          </span>
          {sideOn ? (
            <>
              <span className="panels-menu-why">
                {t('panels.arrange.side.how', {
                  // The cockpit's own panes by name, and the boxes as one: any box in column 1 goes there too.
                  panes: listOf(
                    [
                      ...spec.leftSide.filter((id) => !spec.boxes?.includes(id)).map((id) => labels[id]),
                      ...(spec.boxes?.some((id) => spec.leftSide!.includes(id)) ? [t('panels.arrange.side.aBox')] : []),
                    ],
                    'disjunction',
                  ),
                })}
              </span>
              {side.filter(shown).map((id) => paneRow(id, true))}
              {addButton('side')}
            </>
          ) : side.some(shown) ? (
            // Too narrow here: what is kept for a wider window, named — those panes are listed (and
            // move) in their columns below, where they are on screen.
            <span className="panels-menu-why">
              {t('panels.arrange.side.kept', {
                count: side.filter(shown).length,
                panes: listOf(side.filter(shown).map((id) => labels[id]), 'conjunction'),
              })}
            </span>
          ) : (
            <span className="panels-menu-why">{t('panels.arrange.side.narrow')}</span>
          )}
        </div>
      )}
      {places.map(({ col, name }) => {
        const ids = cols[col].filter(inColumn)
        return (
          <div key={col} className="panels-arrange-col" role="group" aria-labelledby={`${uid}-${col}`} data-arrange-area={col}>
            <span className="panels-arrange-colhead" id={`${uid}-${col}`}>
              {name}
            </span>
            {col === 'log' && !columns && <span className="panels-menu-why">{t('panels.arrange.logForm')}</span>}
            {ids.map((id) => paneRow(id, false))}
            {addButton(col)}
          </div>
        )
      })}
      <span className="panels-menu-why">{narrow ?? t('panels.arrange.narrow')}</span>
      <PaneDropLayer drag={drag} host={rootRef} />
    </div>
  )
}
