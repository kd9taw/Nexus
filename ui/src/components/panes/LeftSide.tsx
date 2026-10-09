// THE LEFT SIDE (operator's pick, 2026-10-03; Phone only) — a full-height column beside the scope that
// ⊞ Panels ▸ Arrange fills with Band Activity, Spots or Needed (features/panelPlace). The cockpit
// decides WHEN it shows (a window about 1280 px wide or more, and a pane on it to show) and renders its
// frames as children; this draws the column and the divider on its right edge.
//
// THE WIDTH is the operator's: the record's `cols.leftSide` (CSS px), painted on the side as
// `--cockpit-left-w`, which the sheet clamps between its em floor and its share of the row
// (cockpit-panes.css `.cockpit-left`) on load and on every resize, without the stored width being
// rewritten. The divider measures the side it sizes and moves through exactly that range
// (features/paneColumns `leftSideRange`), so the keys and the drag stop where the column does. Its
// release is ONE commit through `setCols` — one undoable step in the panel record, which ⊞ Undo and
// Reset layout cover like any other change. RegionColumnSeams' width divider is the pattern.
//
// THE STOP LINE is not near any of this: the side holds only the panes its ArrangeSpec lists by id, and
// no control that stops a transmission has one. The header above and the TX dock below are the shell's.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): its words are the caller's.
import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode, type Ref } from 'react'
import { PaneSeam, elZoom } from '../PaneSeam'
import { leftSideRange, leftSideValue } from '../../features/paneColumns'
import type { PanelColId } from '../../features/panelState'

export interface LeftSideProps {
  /** The stored width, CSS px; absent is the sheet's default. */
  stored: number | undefined
  /** The record's column-width setter; absent, the side has no divider and keeps its stored width. */
  setCols?: (updates: Partial<Record<PanelColId, number | null>>) => void
  /** The side's accessible name (a landmark). */
  label: string
  /** The divider's accessible name. */
  widthLabel: string
  /** The side's column, the box its panes stand in (where a drag onto the side is drawn: panes/PaneDrag). */
  colRef?: Ref<HTMLDivElement>
  children: ReactNode
}

export function LeftSide({ stored, setCols, label, widthLabel, colRef, children }: LeftSideProps) {
  const ref = useRef<HTMLElement>(null)
  // Where the divider stands, measured: the side's rendered width and the range the sheet honours for
  // it. null while nothing is laid out (a hidden keep-alive host, or jsdom).
  const [view, setView] = useState<{ value: number; min: number; max: number } | null>(null)
  const settle = useCallback(() => {
    const side = ref.current
    const row = side?.parentElement
    if (!side || !row) return
    const z = elZoom(side)
    const w = side.getBoundingClientRect().width / z
    if (!(w > 0) || !(row.clientWidth > 0)) return
    // The floor is in the SIDE's em (a basis in em resolves against the element's own font).
    const font = parseFloat(getComputedStyle(side).fontSize)
    const { min, max } = leftSideRange(row.clientWidth, Number.isFinite(font) && font > 0 ? font : 16)
    setView((v) => (v && v.value === w && v.min === min && v.max === max ? v : { value: w, min, max }))
  }, [])
  // On mount, on every resize of the row (a window, a zoom) and of the side itself (a commit).
  useEffect(() => {
    settle()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => settle())
    if (ref.current) ro.observe(ref.current)
    if (ref.current?.parentElement) ro.observe(ref.current.parentElement)
    return () => ro.disconnect()
  }, [settle])

  return (
    <aside
      ref={ref}
      className="cockpit-left"
      aria-label={label}
      style={stored != null ? ({ '--cockpit-left-w': leftSideValue(stored) } as CSSProperties) : undefined}
    >
      <div className="cockpit-left-col" ref={colRef}>
        {children}
      </div>
      {/* The side's RIGHT edge: moving it right widens the side (grows +1). */}
      {setCols && <PaneSeam
        axis="x"
        className="cockpit-left-seam"
        label={widthLabel}
        value={view?.value ?? null}
        min={view?.min ?? 0}
        max={view?.max ?? 0}
        grows={1}
        onPaint={(px) => ref.current?.style.setProperty('--cockpit-left-w', leftSideValue(px))}
        // Back to what the record paints: its width, or no token at all (the sheet's default).
        onCancel={() => {
          const side = ref.current
          if (!side) return
          if (stored != null) side.style.setProperty('--cockpit-left-w', leftSideValue(stored))
          else side.style.removeProperty('--cockpit-left-w')
        }}
        onCommit={(px: number) => {
          setCols({ leftSide: px })
          setView((v) => (v ? { ...v, value: Math.round(px) } : v))
        }}
        onReset={() => setCols({ leftSide: null })}
      />}
    </aside>
  )
}
