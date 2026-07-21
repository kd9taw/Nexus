// The ⊞ Panels control — the ONLY way to remove or restore a cockpit panel. A
// persistent ✕ on every panel header was considered and rejected: it would sit inches
// from a decode list the operator clicks all night, with no confirm, and hover-reveal
// would break the always-on accessibility posture. Removal is menu-only, keyboard
// reachable, and never more than one click from Undo / Reset — both ship here, before
// the operator can make a mess.
//
// Only the panels the CURRENT layout renders are listed; TX controls are not panels at
// all, so they have no entry here by construction (see features/panelState).
import { useEffect, useRef, useState } from 'react'
import type { PanelState } from '../features/panelState'

export interface PanelsMenuItem {
  id: string
  label: string
  state: PanelState
}

interface Props {
  /** Panels present in the current layout, in menu order. */
  items: readonly PanelsMenuItem[]
  /** Tick ⇒ dock it, untick ⇒ remove it. */
  onToggle: (id: string, show: boolean) => void
  /** Restore the layout as it was before the last change. */
  onUndo: () => void
  canUndo: boolean
  /** Put every panel back (stock layout). */
  onReset: () => void
}

export function PanelsMenu({ items, onToggle, onUndo, canUndo, onReset }: Props) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  // The menu overlays the header, so a click anywhere else closes it rather than
  // leaving it sitting on top of the controls beneath.
  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', onDown)
    return () => document.removeEventListener('pointerdown', onDown)
  }, [open])

  const hidden = items.filter((i) => i.state === 'removed').length
  return (
    <div className="panels-menu" ref={rootRef}>
      <button
        type="button"
        className={`panels-menu-btn${open || hidden > 0 ? ' active' : ''}`}
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title="Show or hide the panels on this screen — untick one and its neighbours expand into the space it leaves"
      >
        ⊞ Panels{hidden > 0 ? ` · ${hidden} hidden` : ''}
      </button>
      {open && (
        <div
          className="panels-menu-pop"
          role="group"
          aria-label="Panels on this screen"
          // Escape closes the menu. It deliberately does NOT stop propagating: Escape
          // is the abort key and must still reach the cockpit's halt handler.
          onKeyDown={(e) => {
            if (e.key === 'Escape') setOpen(false)
          }}
        >
          {items.map((it) => (
            <label key={it.id} className="panels-menu-item">
              <input
                type="checkbox"
                checked={it.state !== 'removed'}
                onChange={(e) => onToggle(it.id, e.target.checked)}
              />
              <span>{it.label}</span>
              {it.state === 'popped' && <span className="panels-menu-tag">popped out</span>}
            </label>
          ))}
          <div className="panels-menu-actions">
            <button
              type="button"
              onClick={onUndo}
              disabled={!canUndo}
              title="Put the layout back the way it was before the last change"
            >
              Undo last change
            </button>
            <button type="button" onClick={onReset} title="Show every panel again (the stock layout)">
              Reset layout
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
