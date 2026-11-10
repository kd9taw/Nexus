// The Now-Bar alert lane: renders persistent status items (from status.ts) as
// compact, tiered chips so degradations are never silent. Reduced-motion-aware
// (the critical pulse is CSS that the global reduced-motion rules disable; the
// ⚑ glyph still carries it). Part of Phase-0 honest-state.
import { useEffect, useState, type ReactNode } from 'react'
import { AlertTriangle, Flag, Info } from 'lucide-react'
import { subscribeStatus, type StatusItem, type StatusTier } from '../status'
import { Tooltip, TooltipProvider } from './ui/Tooltip'

const TIER_ICON = {
  critical: Flag,
  warning: AlertTriangle,
  info: Info,
} as const

function chipClass(tier: StatusTier): string {
  return `status-chip tier-${tier}`
}

/** ALWAYS DRAWN, items or none (cockpit-panes.css `.status-lane`): the lane stands where the bar's free room
 *  is, so a box that came and went with a message moved the groups after it, the bar's own Stop TX among them.
 *
 *  `lead`: the top bar's REC chip, drawn in the lane so it takes the lane's zero width: a recording that
 *  starts or stops never wraps the bar.
 *
 *  `keepCritical`: this screen has NO transmitter alarm cell (TxAlarmBanner, on the operating screens' TX
 *  strip), so the lane is the only place a critical item shows. Then the lane ALWAYS keeps the room for a
 *  critical item's flag (`.status-lane-keep`), so one arriving never wraps the bar; critical items are drawn
 *  first and announced as alerts, not politely. With the cell, the lane may be cut: every critical item is
 *  beside Stop TX. */
export function StatusLane({ lead, keepCritical = false }: { lead?: ReactNode; keepCritical?: boolean } = {}) {
  const [items, setItems] = useState<StatusItem[]>([])
  useEffect(() => subscribeStatus(setItems), [])

  const critical = items.filter((it) => it.tier === 'critical')
  const keep = keepCritical

  const chip = (it: StatusItem) => {
    const Icon = TIER_ICON[it.tier]
    // Keyed by its words as well as its id: the critical pulse runs a few times and then
    // holds, so a different alarm under the same id (the audio line names several) has to
    // be a new chip to pulse again. A reworded tooltip is the same alarm and keeps its chip.
    return (
      <Tooltip key={`${it.id}\u0000${it.message}`} side="bottom" content={it.detail ?? it.message}>
        <span className={chipClass(it.tier)} role={keep && it.tier === 'critical' ? 'alert' : undefined}>
          <Icon size={13} strokeWidth={2} aria-hidden="true" />
          <span className="status-chip-msg">{it.message}</span>
        </span>
      </Tooltip>
    )
  }

  return (
    <TooltipProvider>
      <div className={keep ? 'status-lane status-lane-keep' : 'status-lane'} role="status" aria-live="polite">
        {/* The bar's gap, given back inside the lane (its margin cancels it for the line breaker). */}
        <span className="status-lane-gap" aria-hidden="true" />
        {/* Critical first, ahead of REC, so the kept flag is a critical item's (the store sorts critical first). */}
        {keep && critical.map(chip)}
        {lead}
        {(keep ? items.filter((it) => it.tier !== 'critical') : items).map(chip)}
      </div>
    </TooltipProvider>
  )
}
