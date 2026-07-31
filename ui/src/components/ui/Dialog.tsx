// Radix Dialog styled with Nexus tokens — the modal/command-palette primitive
// (the P2 command palette will build on this). Accessible focus-trap + ESC for
// free. See ui/DESIGN.md.
import * as RD from '@radix-ui/react-dialog'
import type { ReactNode } from 'react'

interface DialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  /** Hide the visible title but keep it for screen readers. */
  hideTitle?: boolean
  description?: string
  children: ReactNode
}

export function Dialog({ open, onOpenChange, title, hideTitle, description, children }: DialogProps) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className="ui-dialog-overlay" />
        <RD.Content className="ui-dialog">
          {/* The portal lands on document.body — OUTSIDE `.app`'s zoom:var(--ui-zoom) —
              so dialog content rendered at 1/zoom of the app (1.54x too large at
              auto-65; 0.57x at pinned 175, inverting the accessibility setting).
              Re-apply the zoom on this inner wrapper ONLY: the .ui-dialog box itself
              must stay unzoomed so its top/max-height/width rules keep resolving in
              real viewport units (raw vh/vw is correct there for the same reason it
              is wrong inside .app). Title/description sit inside so they scale too. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <RD.Title className={hideTitle ? 'sr-only' : 'ui-dialog-title'}>{title}</RD.Title>
            {description && <RD.Description className="ui-dialog-desc">{description}</RD.Description>}
            {children}
          </div>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  )
}
