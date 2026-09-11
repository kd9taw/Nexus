// Radix DropdownMenu styled with Nexus tokens — context/overflow menus (e.g.
// per-row actions on decodes/roster in P2). Accessible roving focus for free.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useState, type ReactNode } from 'react'
import { useStationData } from '../../stationAccess'

export interface MenuItem {
  label: string
  onSelect: () => void
  disabled?: boolean
  icon?: ReactNode
}

interface MenuProps {
  trigger: ReactNode
  items: MenuItem[]
  /** Navigation can remain available while station readings are stale. Station
   * action menus retain their existing data requirement by default. */
  requiresStationData?: boolean
  className?: string
}

export function Menu({ trigger, items, requiresStationData = true, className }: MenuProps) {
  const available = useStationData()
  const [open, setOpen] = useState(false)
  return (
    <RM.Root open={open && (!requiresStationData || available)} onOpenChange={setOpen}>
      <RM.Trigger asChild>{trigger}</RM.Trigger>
      <RM.Portal>
        <RM.Content className={['ui-menu', className].filter(Boolean).join(' ')} sideOffset={4} align="end" collisionPadding={8}>
          {/* Same portal-zoom re-application as Dialog/Tooltip (see Tooltip.tsx for
              why an inner wrapper is positioning-safe): the portal escapes `.app`'s
              zoom:var(--ui-zoom), so content must re-apply it. Item roving focus is
              context-based, so the extra div does not break keyboard navigation. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            {items.map((it, i) => (
              <RM.Item
                key={i}
                className="ui-menu-item"
                disabled={it.disabled}
                onSelect={it.onSelect}
              >
                {it.icon && <span className="ui-menu-icon">{it.icon}</span>}
                {it.label}
              </RM.Item>
            ))}
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
