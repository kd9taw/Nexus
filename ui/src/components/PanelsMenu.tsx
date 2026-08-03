// The ⊞ Panels control — the ONLY way to remove or restore a cockpit panel. A
// persistent ✕ on every panel header was considered and rejected: it would sit inches
// from a decode list the operator clicks all night, with no confirm, and hover-reveal
// would break the always-on accessibility posture. Removal is menu-only, keyboard
// reachable, and never more than one click from Undo / Reset — both ship here, before
// the operator can make a mess.
//
// Only the panels the CURRENT layout renders are listed. Anything that can STOP a
// transmission is not a panel at all, so it has no entry here by construction (see
// features/panelState) — an entry can cost you a sender, never the way to shut one up.
//
// A listed entry must be able to CHANGE something. Some panels are conditional on the
// station (rig-scope controls need the radio's own panadapter streaming), so ticking
// them can be a no-op — the operator unticks, nothing falls away, and the menu has lied.
// Two affordances answer that, and both carry their reason in the entry: `unavailable`
// (the panel cannot render at all right now — not checkable, and the reason says what
// would make it appear) and `note` (it works, but its panel only has something to show
// at certain times). Same rule DSP_FUNCS already follows in the cockpits: never offer a
// dead control.
//
// An unavailable entry is `aria-disabled`, NEVER the `disabled` attribute. A disabled
// control is removed from the tab order, so an operator driving the menu from the
// keyboard or a screen reader would never land on the one entry whose whole purpose is to
// explain itself — the reason exists for them first. aria-disabled keeps it focusable and
// announces "dimmed/unavailable" with its description; the toggle is suppressed in the
// handler instead, so it is reachable AND cannot act.
import { useEffect, useId, useRef, useState } from 'react'
import type { PanelState } from '../features/panelState'

export interface PanelsMenuItem {
  id: string
  label: string
  state: PanelState
  /** Why checking this entry would change nothing right now. Present ⇒ the entry is
   *  listed but NOT checkable, with this reason under it. The reason IS the disable
   *  signal, so a disabled entry without one is unrepresentable. */
  unavailable?: string
  /** Standing note for an entry that works but whose panel is only populated sometimes
   *  (TX meters read on transmit). Shown the same way; the entry stays checkable. */
  note?: string
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
  // Reason/note ids for aria-describedby. The reason sits OUTSIDE the <label> on
  // purpose: inside it, it would join the checkbox's accessible NAME, so a screen
  // reader would read the whole sentence every time focus lands on the box.
  const uid = useId()
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
          {items.map((it) => {
            // An unavailable entry stays TICKED while it is docked — it is switched on,
            // there is just nothing streaming for it to show — and it becomes checkable
            // again by itself the moment the station can render it.
            // Truthiness, not `!= null`: an empty reason must read as "available", or a
            // blank string would disable an entry with nothing on it to explain why.
            const why = it.unavailable || it.note
            const whyId = why ? `${uid}-${it.id}` : undefined
            return (
              <div key={it.id} className={`panels-menu-item${it.unavailable ? ' unavailable' : ''}`}>
                <label className="panels-menu-check">
                  <input
                    type="checkbox"
                    checked={it.state !== 'removed'}
                    aria-disabled={it.unavailable ? true : undefined}
                    aria-describedby={whyId}
                    onChange={(e) => {
                      // Reachable, but not actionable. `disabled` would have done this
                      // for us — at the cost of the tab stop, which is the one thing this
                      // entry cannot afford to lose. So refuse here instead: the browser
                      // has already flipped the box by the time a change event exists, so
                      // put it back to what the layout says and never call onToggle.
                      // Pointer and keyboard both arrive through this one path.
                      if (it.unavailable) {
                        e.currentTarget.checked = it.state !== 'removed'
                        return
                      }
                      onToggle(it.id, e.target.checked)
                    }}
                  />
                  <span>{it.label}</span>
                  {it.state === 'popped' && <span className="panels-menu-tag">popped out</span>}
                </label>
                {why && (
                  <span className="panels-menu-why" id={whyId}>
                    {why}
                  </span>
                )}
              </div>
            )
          })}
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
