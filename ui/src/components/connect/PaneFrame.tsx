// One grid slot: a header (pane title + a content-picker to reassign the slot) over a
// body that renders the pane's full panel, falling back to its one-sentence projection when
// there is no data yet. The picker auto-lists every registry entry, so B2/B3 panes appear with
// no change here.
//
// The Basic/Expert detail toggle was removed 2026-07-26 (operator) — every pane renders in full.
// `def.basic()` is NOT the removed mode: it is the loading / no-data / offline hint for every
// pane, reached whenever `def.expert()` returns null. Deleting it would blank a pane that is
// simply waiting on a feed.
import type { CSSProperties } from 'react'
import { PANES, paneById } from './panes'
import type { PaneContext } from './paneContext'
import type { PaneId, SlotId } from '../../features/connectConfig'

export function PaneFrame({
  slotId,
  paneId,
  ctx,
  onAssign,
  style,
}: {
  slotId: SlotId
  paneId: PaneId
  ctx: PaneContext
  onAssign: (slotId: SlotId, paneId: PaneId) => void
  style?: CSSProperties // { gridArea: slotId } for the 4 rail frames; omitted inside the strip
}) {
  const def = paneById(paneId)
  if (!def) return null
  const body = def.expert(ctx) // null when there is no data yet → falls back to basic() below
  return (
    <section className="pane-frame" data-slot={slotId} data-pane={paneId} style={style}>
      <header className="pane-head">
        <span className="pane-title">{def.title}</span>
        <select
          className="pane-pick"
          value={paneId}
          aria-label={`Choose what the ${slotId} slot shows`}
          title="Choose what this slot shows"
          onChange={(e) => onAssign(slotId, e.target.value as PaneId)}
        >
          {(['core', 'b2', 'b3'] as const).map((cat) => {
            const items = PANES.filter((p) => p.category === cat)
            return items.length ? (
              <optgroup key={cat} label={cat === 'core' ? 'Panels' : cat.toUpperCase()}>
                {items.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.title}
                  </option>
                ))}
              </optgroup>
            ) : null
          })}
        </select>
      </header>
      <div className="pane-body">{body ?? <p className="pane-basic">{def.basic(ctx)}</p>}</div>
    </section>
  )
}
