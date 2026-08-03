// The ⊞ Panels control — the ONLY way to remove or restore a cockpit panel. A
// persistent ✕ on every panel header was considered and rejected: it would sit inches
// from a decode list the operator clicks all night, with no confirm, and hover-reveal
// would break the always-on accessibility posture. Removal is menu-only, keyboard
// reachable, and never more than one click from Undo / Reset — both ship here, before
// the operator can make a mess.
//
// Only the panels the CURRENT layout renders are listed; TX controls are not panels at
// all, so they have no entry here by construction (see features/panelState).
//
// Some panels are conditional on the station (rig-scope controls need the radio's own
// panadapter streaming; the DSP panes need the rig to report those fields over CAT; CW's
// Sent Echo is empty until the first over), so an operator can tick one and see nothing
// happen. A `note` answers that: the entry says, in its own accessible description, why
// there is nothing on screen for it right now and what would change that.
//
// THE NOTE EXPLAINS; IT NEVER REFUSES. Availability and preference are two questions and
// this menu only asks one of them: "do you want this panel?" A round of this shipped with
// unavailable entries `aria-disabled` and the toggle suppressed, which conflated them and
// cost the operator control in the state he is most likely to exercise it — CW's Sent Echo
// is empty at EVERY session start, so an operator who wants it gone had to wait for a
// transmission before he could untick it. Recording the preference now is also the honest
// answer: it takes effect the moment the pane can mount. And the greyed look that came with
// aria-disabled was `opacity` on the focusable element, which composites its FOCUS RING too
// — the change made to keep the entry keyboard-reachable is what made its focus indicator
// hard to see. Both problems are deleted, not worked around, by leaving the box alone.
import { useEffect, useId, useRef, useState } from 'react'
import type { PanelState } from '../features/panelState'

export interface PanelsMenuItem {
  id: string
  label: string
  state: PanelState
  /** Why this panel has nothing on screen right now, and what would change that (no
   *  native scope streaming; TX meters read on transmit). Shown under the entry and
   *  attached to the checkbox as its accessible description. The entry stays a plain,
   *  operable checkbox — this annotates it, it does not disable it. */
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
            // Truthiness, not `!= null`: an empty note carries no id, so a blank string
            // cannot leave a checkbox pointing at an empty description.
            const whyId = it.note ? `${uid}-${it.id}` : undefined
            return (
              <div key={it.id} className="panels-menu-item">
                <label className="panels-menu-check">
                  <input
                    type="checkbox"
                    checked={it.state !== 'removed'}
                    aria-describedby={whyId}
                    onChange={(e) => onToggle(it.id, e.target.checked)}
                  />
                  <span>{it.label}</span>
                  {it.state === 'popped' && <span className="panels-menu-tag">popped out</span>}
                </label>
                {it.note && (
                  <span className="panels-menu-why" id={whyId}>
                    {it.note}
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
