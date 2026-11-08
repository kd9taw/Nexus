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

/** `lead`: the top bar's REC chip, drawn first in the lane so it takes the lane's zero width (styles.css
 *  `.status-lane`): a recording that starts or stops never wraps the bar. */
export function StatusLane({ lead }: { lead?: ReactNode } = {}) {
  const [items, setItems] = useState<StatusItem[]>([])
  useEffect(() => subscribeStatus(setItems), [])

  if (items.length === 0 && !lead) return null

  return (
    <TooltipProvider>
      <div className="status-lane" role="status" aria-live="polite">
        {lead}
        {items.map((it) => {
          const Icon = TIER_ICON[it.tier]
          // Keyed by its words as well as its id: the critical pulse runs a few times and then
          // holds, so a different alarm under the same id (the audio line names several) has to
          // be a new chip to pulse again. A reworded tooltip is the same alarm and keeps its chip.
          return (
            <Tooltip key={`${it.id}\u0000${it.message}`} side="bottom" content={it.detail ?? it.message}>
              <span className={chipClass(it.tier)}>
                <Icon size={13} strokeWidth={2} aria-hidden="true" />
                <span className="status-chip-msg">{it.message}</span>
              </span>
            </Tooltip>
          )
        })}
      </div>
    </TooltipProvider>
  )
}
