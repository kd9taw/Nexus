// Radix Tooltip styled with Nexus tokens. Headless primitive → accessibility
// (keyboard, ARIA, dismissal) for free; styling is ours. See ui/DESIGN.md.
import * as RT from '@radix-ui/react-tooltip'
import type { ReactNode } from 'react'

export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <RT.Provider delayDuration={350} skipDelayDuration={200}>
      {children}
    </RT.Provider>
  )
}

interface TooltipProps {
  content: ReactNode
  children: ReactNode
  side?: 'top' | 'right' | 'bottom' | 'left'
}

export function Tooltip({ content, children, side = 'right' }: TooltipProps) {
  return (
    <RT.Root>
      <RT.Trigger asChild>{children}</RT.Trigger>
      <RT.Portal>
        <RT.Content className="ui-tooltip" side={side} sideOffset={6} collisionPadding={8}>
          {/* Portaled to document.body, outside `.app`'s zoom:var(--ui-zoom), so the
              content rendered at 1/zoom of the app. Re-applying the zoom on an inner
              wrapper is positioning-safe: Floating UI places the OUTER popper element
              from getBoundingClientRect (visual px) of both anchor and floating box;
              the wrapper's zoom changes the box's layout size, which gBCR reports
              faithfully, so flip/collision math still holds. The arrow stays OUTSIDE
              the wrapper — Radix positions it absolutely against the popper box, and
              an ancestor zoom would mis-scale those offsets. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>{content}</div>
          <RT.Arrow className="ui-tooltip-arrow" />
        </RT.Content>
      </RT.Portal>
    </RT.Root>
  )
}
