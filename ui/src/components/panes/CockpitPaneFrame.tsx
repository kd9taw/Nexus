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
  fit,
  weight,
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
  /** The pane's ROLE in its flex column. 'content' = a control strip (DSP chips, a rig
   *  scope row): exactly content height, always — the first build's uniform 1fr rows made
   *  every strip a grower, and a one-row chip strip inflated to half a column of empty
   *  panel (operator, 2026-07-31). Omitted = FILL: the pane splits the column's remaining
   *  height with its fill siblings by `weight`. Both are PLACEMENT inputs carried inline
   *  from typed props — the column sizes the frame; a pane still cannot size itself, so
   *  contract rule 2 (no min-height/flex/overflow of a pane's own) holds. At the 1-col
   *  scrolling tier the region's --cockpit-pane-flex overrides fill to content-height.
   *  The fill min-height consults --cockpit-fill-min (default 0): the REGION-LESS
   *  cockpits (RTTY/SSTV — bare fill frames, no .cockpit-panes) set it on their shell
   *  rule so the frame keeps a floor under the shell's scrolling deficit valve. A sheet
   *  rule on .pane-frame cannot supply that floor — this inline declaration outranks
   *  every selector (the point of inline placement), which is exactly how
   *  `.rtty-cockpit > .pane-frame { min-height: 10em }` once shipped dead. The knob is
   *  fenced to those two shell rules by cockpit-panes.test.ts: inherited into a
   *  region's tier-2/3 `overflow: hidden` it would be the clip bug reborn. */
  fit?: 'content'
  /** Fill share among fill siblings, default 1 (CW gives DECODE 3). Ignored with fit. */
  weight?: number
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
      data-fit={fit ?? 'fill'}
      aria-label={title}
      // Inline, not a class: a per-pane styling hook is what this component refuses to
      // have, and an inline placement cannot be outranked or forked in either sheet.
      style={
        fit === 'content'
          ? { flex: '0 0 auto' }
          : { flex: `var(--cockpit-pane-flex, ${weight ?? 1} 1 0)`, minHeight: 'var(--cockpit-fill-min, 0)' }
      }
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
