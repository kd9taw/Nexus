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
// TX-safety: this is a layout wrapper and nothing more. The operator must never be unable
// to stop a transmission, so his last resort (PTT, abort, Tune, Stop TX) does not render
// through it — it lives in the shell's pinned .cockpit-txdock or the header, with no id in
// any pane vocabulary. A TRANSMITTING pane may sit in a frame and several do (Phone's voice
// keyer; Operate's Tx messages and its decode/roster panes, which use raw .panel divs but
// answer to the same rule): being a sender has no bearing on whether a pane may be hidden.
// See cockpit-panes.css, "THE STOP LINE".
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The pane's own NAME
// arrives from the cockpit and is interpolated as data; the two head buttons' words are here.
import type { CSSProperties, ReactNode, Ref } from 'react'
import { t } from '../../i18n'
import { PaneCloseButton } from './PaneCloseButton'

/** A frame's placement in its column, from its ROLE — CockpitPaneFrame's own inline style (its props
 *  below say what each input is). Exported for the panes that draw their own head and so cannot sit in
 *  this frame, but must take a column's room exactly as a frame does: FT's arranged columns (2026-10-07).
 *  A placement input like the frame's own, never a size a pane declares. */
export function paneRoleStyle({ fit, weight, share, split }: { fit?: 'content'; weight?: number; share?: number; split?: number }): CSSProperties {
  return fit === 'content'
    ? { flex: '0 0 auto' }
    : share != null || split != null
      ? ({
          ...(share != null ? { '--pane-share': share } : {}),
          flex: `var(--cockpit-pane-flex, var(--pane-share, ${weight ?? 1}) 1 0)`,
          minHeight: `min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, ${split ?? 1}) / ${split ?? 1}), 100%)`,
        } as CSSProperties)
      : { flex: `var(--cockpit-pane-flex, ${weight ?? 1} 1 0)`, minHeight: 'var(--cockpit-fill-min, 0)' }
}

export function CockpitPaneFrame({
  title,
  paneId,
  children,
  fit,
  weight,
  share,
  split,
  paneRef,
  onPopOut,
  onRemove,
  hideNote,
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
  /** The OPERATOR'S share for a fill pane that a PaneSeam splits (the panel record's
   *  `share`) — `weight`, carried as `--pane-share` on the frame itself so the seam can repaint
   *  it live mid-drag, and so the record's value (React's) is what stands after a release, a
   *  Reset or an Undo. Still a typed number from the host, never a size the pane declares.
   *  With a `split` other than 1 it is the pane's GROW, the record's share × `split` — the unit
   *  the divider paints (PaneSeam `scale`). Omitted ⇒ `weight`. Ignored with fit. */
  share?: number
  /** Set on a fill pane that a PaneSeam splits (with or without a `share` yet): the grow a
   *  share of 1 stands for — the mean stock `weight` of the two panes the divider splits, so the
   *  pair's shares sum to 2 (seamShares) while their grows keep the pair's stock total and every
   *  other fill pane in the column stays where it was. Default 1.
   *
   *  THE FLOOR FOLLOWS THE SHARE: `--cockpit-fill-min × share` (the pane's stock floor at the
   *  stock split), so the pair's floors always add up to the two stock floors and the divider
   *  divides them too. It has to: in a column too short for every fill pane's floor, each pane
   *  sits ON its floor and a share alone moves nothing — Phone's Spots / Needed divider did
   *  exactly that at 1024×768 and 1366×768 (layout L2, measured in Chrome), rewriting the record
   *  while neither pane moved a pixel. Still written to yield: never more than the column. */
  split?: number
  /** The frame's own box, for a PaneSeam to measure and repaint. Omitted ⇒ no ref. */
  paneRef?: Ref<HTMLElement>
  /** Tear this pane off into its own window (open_panel_window). Omitted ⇒ no button. */
  onPopOut?: () => void
  /** Hide this pane (panelState 'removed'). Omitted ⇒ the pane cannot be removed — which
   *  is how a pane with no id in the view's vocabulary stays put. Comes from
   *  `panelHost.closeProps(id)`, so it is the SAME act as the ⊞ Panels tick. */
  onRemove?: () => void
  /** What this hide ENDS, in the cockpit's own words (panelHost `endsOnHide`) — THE
   *  PRACTICE half of THE STOP LINE, put before the act. Omitted for a hide that ends
   *  nothing, which is every pane but Phone's voice keyer. */
  hideNote?: string
  /** Pane-supplied head controls (filters, a mode chip). Rendered before pop-out/remove. */
  actions?: ReactNode
}) {
  return (
    <section
      className="pane-frame"
      ref={paneRef}
      data-pane={paneId}
      data-fit={fit ?? 'fill'}
      aria-label={title}
      // Inline, not a class: a per-pane styling hook is what this component refuses to
      // have, and an inline placement cannot be outranked or forked in either sheet.
      style={paneRoleStyle({ fit, weight, share, split })}
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
              aria-label={t('pane.popOut.aria', { title })}
              title={t('pane.popOut.title')}
            >
              ⧉
            </button>
          )}
          {/* The pane's own ✕ — one shared component so every removable pane in the app
              closes the same way, with the same accessible name and the same consequence
              copy. It renders nothing when `onRemove` is absent. */}
          <PaneCloseButton title={title} onRemove={onRemove} hideNote={hideNote} />
        </div>
      </header>
      <div className="pane-body">{children}</div>
    </section>
  )
}
