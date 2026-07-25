// A drag handle that sits BETWEEN two adjacent flex panes in a column and redistributes
// their flex-grow share on drag — the sibling-seam companion to Splitter.tsx. Where
// Splitter sizes ONE panel against a container var, this splits a region between two panes
// (e.g. Band Activity above / Rx Frequency below in the Operate side rail).
//
// Same discipline as Splitter: writes CSS vars LIVE during the drag (no React re-render, so
// it stays smooth) and commits to the panel record (panelState.setShares) only on release.
// Zoom-invariant by construction — the pointer position and the region rect both scale with
// --ui-zoom, so their ratio needs no correction. The share math lives in the pure, tested
// `seamShares`, so this file is only pointer plumbing.
import { seamShares } from '../features/panelState'

interface Props {
  /** The pane above the seam (grows when the seam is dragged down). */
  above: React.RefObject<HTMLElement | null>
  /** The pane below the seam (grows when the seam is dragged up). */
  below: React.RefObject<HTMLElement | null>
  /** The CSS custom property each pane reads for its flex-grow (e.g. "--pane-share"). */
  varName: string
  /** Commit the settled shares to the panel record on pointer-up. */
  onCommit: (aboveShare: number, belowShare: number) => void
  /** Accessible label for the separator. */
  label: string
}

export function SplitterSeam({ above, below, varName, onCommit, label }: Props) {
  const start = (e: React.PointerEvent<HTMLDivElement>) => {
    const a = above.current
    const b = below.current
    if (!a || !b) return
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    document.body.classList.add('resizing')
    // The combined region spans the TOP of the pane above to the BOTTOM of the pane below.
    const top = a.getBoundingClientRect().top
    const bottom = b.getBoundingClientRect().bottom
    const span = bottom - top
    if (span <= 0) return // collapsed/hidden region — never divide by it
    const sharesFor = (ev: PointerEvent): [number, number] =>
      seamShares((ev.clientY - top) / span)
    const paint = ([av, bv]: [number, number]) => {
      a.style.setProperty(varName, String(av))
      b.style.setProperty(varName, String(bv))
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
      className="pane-splitter horizontal seam"
      role="separator"
      aria-orientation="horizontal"
      aria-label={label}
      title={`Drag to resize (${label})`}
      onPointerDown={start}
    />
  )
}
