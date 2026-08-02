// A drag handle that sits BETWEEN two adjacent panes and redistributes their share on
// drag — the sibling-seam companion to Splitter.tsx. Where Splitter sizes ONE panel
// against a container var, this splits a region between two panes (e.g. Band Activity
// above / Rx Frequency below in the Operate side rail, or the Rx-Frequency column
// beside the Stations roster in the Classic grid).
//
// Same discipline as Splitter: writes CSS vars LIVE during the drag (no React re-render,
// so it stays smooth) and commits to the panel record (panelState.setShares) only on
// release. Zoom-invariant by construction — the pointer position and the region rect
// both scale with --ui-zoom, so their ratio needs no correction. The share math lives in
// the pure, tested `seamShares`, so this file is only pointer plumbing.
//
// Two paint modes:
// - flex (default): the numeric share lands as `varName` on EACH pane, whose flex-grow
//   reads it (`flex: var(--pane-share, 1) 1 0`).
// - grid columns (`columnsOn` set): the share lands as an `fr` TOKEN in `${varName}-a`
//   / `${varName}-b` on the grid CONTAINER — a grid template cannot read a variable off
//   its children, so painting the panes would be a silently dead drag.
import { seamShares } from '../features/panelState'

interface Props {
  /** The pane above/left of the seam (grows when the seam is dragged down/right). */
  above: React.RefObject<HTMLElement | null>
  /** The pane below/right of the seam (grows when the seam is dragged up/left). */
  below: React.RefObject<HTMLElement | null>
  /** The CSS custom property the drag drives (e.g. "--pane-share", "--op-col"). */
  varName: string
  /** Commit the settled shares to the panel record on pointer-up. */
  onCommit: (aboveShare: number, belowShare: number) => void
  /** Accessible label for the separator. */
  label: string
  /** Drag axis: 'y' (default) splits stacked panes; 'x' splits side-by-side columns. */
  axis?: 'x' | 'y'
  /** Grid-column mode: the container whose template consumes `${varName}-a`/`-b`. */
  columnsOn?: React.RefObject<HTMLElement | null>
}

export function SplitterSeam({ above, below, varName, onCommit, label, axis = 'y', columnsOn }: Props) {
  const start = (e: React.PointerEvent<HTMLDivElement>) => {
    const a = above.current
    const b = below.current
    if (!a || !b) return
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    document.body.classList.add('resizing')
    // The combined region spans the start of the pane above/left to the end of the
    // pane below/right.
    const ra = a.getBoundingClientRect()
    const rb = b.getBoundingClientRect()
    const lo = axis === 'x' ? ra.left : ra.top
    const hi = axis === 'x' ? rb.right : rb.bottom
    const span = hi - lo
    if (span <= 0) return // collapsed/hidden region — never divide by it
    const sharesFor = (ev: PointerEvent): [number, number] =>
      seamShares(((axis === 'x' ? ev.clientX : ev.clientY) - lo) / span)
    const paint = ([av, bv]: [number, number]) => {
      const c = columnsOn?.current
      if (c) {
        c.style.setProperty(`${varName}-a`, `${av}fr`)
        c.style.setProperty(`${varName}-b`, `${bv}fr`)
      } else {
        a.style.setProperty(varName, String(av))
        b.style.setProperty(varName, String(bv))
      }
    }
    const move = (ev: PointerEvent) => paint(sharesFor(ev))
    const up = (ev: PointerEvent) => {
      const [av, bv] = sharesFor(ev)
      paint([av, bv])
      onCommit(av, bv)
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.classList.remove('resizing')
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <div
      className={`pane-splitter ${axis === 'x' ? 'col-seam' : 'horizontal'} seam`}
      role="separator"
      aria-orientation={axis === 'x' ? 'vertical' : 'horizontal'}
      aria-label={label}
      title={`Drag to resize (${label})`}
      onPointerDown={start}
    />
  )
}
