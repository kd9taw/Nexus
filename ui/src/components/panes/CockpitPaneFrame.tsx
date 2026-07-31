// THE COCKPIT PANE FRAME — Connect's PaneFrame (components/connect/PaneFrame.tsx),
// promoted so every cockpit's operator-content blocks render through one box.
//
// It reuses the SHIPPED .pane-frame / .pane-head / .pane-body CSS family verbatim
// (styles.css ~1497): a bounded frame whose body is `flex:1; min-height:0; overflow:auto`
// with the thin visible scrollbar. That family is view-agnostic and already carries
// Connect's panes; it is deliberately NOT forked or edited here, and cockpit-panes.test.ts
// fails if this rebuild starts restyling it.
//
// What it deliberately does NOT have:
//   - no `className` / `style` prop. A pane must declare no size of its own — the grid cell
//     sizes the frame (design3 §5 contract rule 2). Without a styling hook on the frame,
//     "just give this pane a min-height" is not expressible; content styling attaches
//     inside the body, where it can only ever scroll.
//   - no picker. Connect's slot picker belongs to Connect's assignable grid; a cockpit's
//     placement is the fixed responsive template, and visibility is the ⊞ Panels menu.
//
// TX-safety: this is a layout wrapper and nothing more. TX chrome (PTT, send, abort, Tune,
// Stop TX) does not render through it — it lives in the shell's pinned .cockpit-txdock,
// outside the pane region, with no id in any pane vocabulary.
import type { ReactNode } from 'react'

export function CockpitPaneFrame({
  title,
  paneId,
  children,
  rows,
  onPopOut,
  onRemove,
  actions,
}: {
  /** Head label. Also the accessible name of the frame (a landmark per pane). */
  title: string
  /** Optional stable id for tests / pop-out slugs. A data attribute, NOT a class: it is
   *  not a styling hook (see above). */
  paneId?: string
  children: ReactNode
  /** Row WEIGHT in the column grid (grid-row span), default 1. This is how a cockpit says
   *  "prominent": a transcript/feed pane spans more `minmax(0,1fr)` rows than a one-line
   *  control strip, so equal-share rows stop starving the DECODE/Band-Activity feeds while
   *  inflating a chip row to feed height (fix-round, 2026-07-31). It is a PLACEMENT input
   *  to the region's grid — the grid still sizes the cell — not the pane sizing itself,
   *  so contract rule 2 (no min-height/flex/overflow of a pane's own) holds. At the 1-col
   *  tier rows are `auto`, where a span changes nothing. */
  rows?: number
  /** Tear this pane off into its own window (open_panel_window). Omitted ⇒ no button. */
  onPopOut?: () => void
  /** Hide this pane (panelState 'removed'). Omitted ⇒ the pane cannot be removed — which
   *  is how a pane with no id in the view's vocabulary stays put. */
  onRemove?: () => void
  /** Pane-supplied head controls (filters, a mode chip). Rendered before pop-out/remove. */
  actions?: ReactNode
}) {
  return (
    <section
      className="pane-frame"
      data-pane={paneId}
      aria-label={title}
      // Inline, not a class: a per-pane styling hook is what this component refuses to
      // have, and an inline placement cannot be outranked or forked in either sheet.
      style={rows && rows > 1 ? { gridRow: `span ${rows}` } : undefined}
    >
      <header className="pane-head">
        <span className="pane-title">{title}</span>
        <div className="cockpit-pane-acts">
          {actions}
          {onPopOut && (
            <button
              type="button"
              className="cockpit-popout"
              onClick={onPopOut}
              aria-label={`Open ${title} in its own window`}
              title="Open this pane in its own window (for a second monitor)"
            >
              ⧉
            </button>
          )}
          {onRemove && (
            <button
              type="button"
              className="cockpit-popout"
              onClick={onRemove}
              aria-label={`Hide ${title}`}
              title="Hide this pane (restore it from the ⊞ Panels menu)"
            >
              ✕
            </button>
          )}
        </div>
      </header>
      <div className="pane-body">{children}</div>
    </section>
  )
}
