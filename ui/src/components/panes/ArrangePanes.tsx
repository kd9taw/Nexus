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
// THE STOP LINE is not near this: only a pane with a vocabulary id can be listed or moved (the
// ArrangeSpec, features/panelPlace), and no control that stops a transmission has one. A move changes
// where a pane stands in the region and nothing else — the header and the TX dock are not the
// region's, and no move can reach them.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The panes' names are the
// cockpit's (`labels`); the arrows are glyphs, not words.
import { useId, useLayoutEffect, useRef } from 'react'
import { t } from '../../i18n'
import { PANE_COLUMNS, canMovePane, placedColumns, type ArrangeSpec, type PaneColumn, type PaneMove } from '../../features/panelPlace'
import type { PanelLayout } from '../../features/panelState'

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

const columnName = (col: PaneColumn): string =>
  col === 'a' ? t('panels.arrange.column.a') : col === 'b' ? t('panels.arrange.column.b') : t('panels.arrange.column.log')

export interface ArrangePanesProps<P extends string> {
  spec: ArrangeSpec<P>
  layout: PanelLayout<P>
  /** Whether a pane is on screen: only those are listed, and a move steps past the others. */
  shown: (id: P) => boolean
  /** The panes' operator-facing names (the ⊞ entries' own). */
  labels: Readonly<Record<P, string>>
  onMove: (id: P, move: PaneMove) => void
}

export function ArrangePanes<P extends string>({ spec, layout, shown, labels, onMove }: ArrangePanesProps<P>) {
  const uid = useId()
  const cols = placedColumns(spec, layout.place)
  const rootRef = useRef<HTMLDivElement>(null)
  // The move just pressed from a focused button, until the render that shows it.
  const pending = useRef<{ id: P; move: PaneMove } | null>(null)
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
    <div className="panels-arrange" role="group" aria-labelledby={`${uid}-head`} ref={rootRef}>
      <span className="panels-arrange-head" id={`${uid}-head`}>
        {t('panels.arrange.heading')}
      </span>
      {PANE_COLUMNS.map((col) => {
        const ids = cols[col].filter(shown)
        return (
          <div key={col} className="panels-arrange-col" role="group" aria-labelledby={`${uid}-${col}`}>
            <span className="panels-arrange-colhead" id={`${uid}-${col}`}>
              {columnName(col)}
            </span>
            {col === 'log' && <span className="panels-menu-why">{t('panels.arrange.logForm')}</span>}
            {ids.map((id) => {
              const pinned = spec.pinned.includes(id)
              const pinnedId = pinned ? `${uid}-${id}-pinned` : undefined
              return (
                <div key={id} className="panels-arrange-pane">
                  <div className="panels-arrange-row">
                    <span className="panels-arrange-name">{labels[id]}</span>
                    <span className="panels-arrange-moves">
                      {MOVES.map(([move, glyph]) =>
                        pinned && (move === 'left' || move === 'right') ? null : (
                          <button
                            key={move}
                            type="button"
                            className="panels-arrange-btn"
                            aria-label={moveName(move, labels[id])}
                            aria-describedby={pinnedId}
                            title={moveName(move, labels[id])}
                            // The columns on screen are a | b | log: no cockpit renders a stored
                            // column order yet, so the moves follow the stock one (as the hook's do).
                            disabled={!canMovePane(spec, layout.place, undefined, id, move, shown)}
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
            })}
          </div>
        )
      })}
      <span className="panels-menu-why">{t('panels.arrange.narrow')}</span>
    </div>
  )
}
