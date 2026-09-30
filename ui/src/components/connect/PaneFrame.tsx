// One grid slot: a header (pane title, a content-picker to reassign the slot, and ✕ to close
// it) over a body that renders the pane's full panel, falling back to its one-sentence
// projection when there is no data yet. The picker auto-lists every registry entry, so B2/B3
// panes appear with no change here.
//
// The Basic/Expert detail toggle was removed 2026-07-26 (operator) — every pane renders in full.
// `def.basic()` is NOT the removed mode: it is the loading / no-data / offline hint for every
// pane, reached whenever `def.expert()` returns null. Deleting it would blank a pane that is
// simply waiting on a feed.
//
// CLOSE + RESIZE (2026-09-13). ✕ closes the SLOT (panelState 'removed'); ⊞ Panels in the
// Connect header brings it back with the same pane in it. No `className`/`style` prop — the
// frame cannot size itself (the pane-grid contract). A rail frame's split is a typed `share`
// placed inline, exactly like CockpitPaneFrame's `weight`: `--connect-share` is the variable
// the rail separator paints during a drag, and `--connect-pane-flex` is the xs stack's
// content-height override. A strip frame takes no share; its grid sizes it.
//
// THE BOX'S OWN TEXT SIZE (2026-09-29, ⋯ ▸ A− / A+). A typed `textScale` placed inline on the
// BODY as `--box-text-scale`, the same kind of placement input as the share: styles.css multiplies
// the app's --text-scale by it for everything inside the body, and the head — the title, the picker,
// the ⋯ and ✕ — stays at the app's size, so a box's head is the same height at every size and the
// strip's row does not jump. The factor is written only when it is not 1: a box at the app's size
// renders exactly the DOM it rendered before the control existed. Every prop here is optional, so a
// host that passes none of them gets today's frame.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every pane's name
// arrives already translated from the registry (`panes.tsx`, resolved through getters); the
// picker's B2/B3 groups are named by their tier code, which is not prose. The ✕ uses the
// cockpit frame's own words (`pane.hide.*`) — one gesture, one sentence, in every view.
import type { CSSProperties } from 'react'
import { t } from '../../i18n'
import { PANES, paneById } from './panes'
import { BoxMenu } from './BoxMenu'
import type { PaneContext } from './paneContext'
import type { PaneId, SlotId } from '../../features/connectConfig'

export function PaneFrame({
  slotId,
  paneId,
  ctx,
  onAssign,
  share,
  onHide,
  textScale,
  onTextScale,
}: {
  slotId: SlotId
  paneId: PaneId
  ctx: PaneContext
  onAssign: (slotId: SlotId, paneId: PaneId) => void
  /** Rail frames only: this pane's flex share of its rail (default split 1:1). */
  share?: number
  /** Close this slot. Omitted ⇒ no ✕. */
  onHide?: () => void
  /** This box's text size, a factor on the app's Text size (⋯ ▸ A− / A+). Omitted ⇒ 1. */
  textScale?: number
  /** Change it. Omitted ⇒ the ⋯ menu offers no text size. */
  onTextScale?: (factor: number) => void
}) {
  const def = paneById(paneId)
  if (!def) return null
  const body = def.expert(ctx) // null when there is no data yet → falls back to basic() below
  const scale = textScale ?? 1
  return (
    <section
      className="pane-frame"
      data-slot={slotId}
      data-pane={paneId}
      style={
        share === undefined
          ? undefined
          : ({ '--connect-share': share, flex: 'var(--connect-pane-flex, var(--connect-share) 1 0)' } as CSSProperties)
      }
    >
      <header className="pane-head">
        <span className="pane-title">{def.title}</span>
        <div className="pane-acts">
          <select
            className="pane-pick"
            value={paneId}
            aria-label={t('connect.slot.pick.aria', { slot: slotId })}
            title={t('connect.slot.pick.title')}
            onChange={(e) => onAssign(slotId, e.target.value as PaneId)}
          >
            {(['core', 'b2', 'b3'] as const).map((cat) => {
              const items = PANES.filter((p) => p.category === cat)
              return items.length ? (
                <optgroup
                  key={cat}
                  label={cat === 'core' ? t('connect.slot.group.core') : cat.toUpperCase()}
                >
                  {items.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.title}
                    </option>
                  ))}
                </optgroup>
              ) : null
            })}
          </select>
          {onTextScale && <BoxMenu title={def.title} textScale={scale} onTextScale={onTextScale} />}
          {onHide && (
            <button
              type="button"
              className="pane-close"
              onClick={onHide}
              aria-label={t('pane.hide.aria', { title: def.title })}
              title={t('pane.hide.title')}
            >
              ✕
            </button>
          )}
        </div>
      </header>
      <div
        className="pane-body"
        style={scale === 1 ? undefined : ({ '--box-text-scale': scale } as CSSProperties)}
      >
        {body ?? <p className="pane-basic">{def.basic(ctx)}</p>}
      </div>
    </section>
  )
}
