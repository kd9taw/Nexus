// The ⊞ Panels control — the ONLY way to remove or restore a cockpit panel. A
// persistent ✕ on every panel header was considered and rejected: it would sit inches
// from a decode list the operator clicks all night, with no confirm, and hover-reveal
// would break the always-on accessibility posture. Removal is menu-only, keyboard
// reachable, and never more than one click from Undo / Reset — both ship here, before
// the operator can make a mess.
//
// Only the panels the CURRENT layout renders are listed. The operator's way to STOP a
// transmission is not a panel at all, so it has no entry here by construction (see
// features/panelState, THE STOP LINE) — an entry can cost you a sender, never the way to
// shut one up. Senders ARE listed, several of them: Operate's Tx messages, its decode panes
// and rosters, Phone's voice keyer. That is the rule holding, not a hole in it.
//
// A listed pane whose hide ENDS something in flight carries a `note` saying what the tick
// ends. That is courtesy rather than the safety rule (a stop the operator did not ask for
// reads as a dropout), and it is required of exactly those panes — a note on an entry whose
// hide ends nothing teaches him to ignore the next one.
//
// THE TICK IS NOT THE ONLY BUTTON IN THIS MENU THAT HIDES A PANE. "Undo last change"
// restores the layout as it was before the last change, and when that layout had such a pane
// unticked, pressing it unmounts it exactly as a tick does — so it carries the same
// consequence line, in the same words, before the press (`undoNote`). "Reset layout" does
// not need one: it applies the empty record, and an absent state means 'docked', so reset
// can only ever put panes back.
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
  /** What the operator needs to know about this entry before he touches it. Two kinds,
   *  one field, because both answer at the same moment and in the same place:
   *    · why this panel has nothing on screen right now, and what would change that (no
   *      native scope streaming; TX meters read on transmit);
   *    · what unticking it will END (Phone's Voice Keyer stops a message and discards a
   *      recording — courtesy before the act, not the price of a menu entry; see THE STOP
   *      LINE, "the practice").
   *  Shown under the entry and attached to the checkbox as its accessible description.
   *  The entry stays a plain, operable checkbox — this annotates it, never disables it.
   *  Not shown once the panel IS removed: by then the first kind is no longer why the
   *  screen looks as it does, and the second warns about an act already taken. */
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
  /** What THIS undo will END, when the layout it restores had a pane unticked whose hide
   *  ends something. THE PRACTICE half of THE STOP LINE puts the consequence before the act,
   *  and Undo is a second button that reaches the same teardown as the tick — untick the
   *  voice keyer, tick it back, start a recording, press Undo, and the take is binned.
   *  Undefined when the undo ends nothing, which is every undo in every cockpit but Phone's
   *  keyer today.
   *
   *  Reset needs no twin of this: it applies the empty record and an absent state means
   *  'docked', so Reset can only ever put panes BACK. */
  undoNote?: string
  /** Put every panel back (stock layout). */
  onReset: () => void
}

export function PanelsMenu({ items, onToggle, onUndo, canUndo, undoNote, onReset }: Props) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const btnRef = useRef<HTMLButtonElement>(null)
  // Note / state-tag ids for aria-describedby. BOTH sit OUTSIDE the <label>: text inside a
  // label joins the checkbox's accessible NAME, so the note would be read as part of the
  // name on every focus, and the "popped out" tag made the name read "Voice Keyerpopped
  // out" (no separator — the spans abut). Outside and described-by, they annotate the
  // control instead of renaming it.
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
        ref={btnRef}
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
            if (e.key === 'Escape') {
              setOpen(false)
              // …and focus goes back to the ⊞ button that opened it. Closing the popover
              // destroys the focused element, and the browser's fallback is <body> — a
              // keyboard operator would restart his tab traversal at the top of the
              // document, dozens of stops from the cockpit he was working. preventScroll
              // per the layout contract: the trigger is on screen already (the popover
              // hangs off it), so a reveal here could only jolt the shell.
              btnRef.current?.focus({ preventScroll: true })
            }
          }}
        >
          {items.map((it) => {
            // A note explains why this panel has NOTHING ON SCREEN right now. Once the
            // operator has unticked the entry, that question has a different answer —
            // "because you unticked it" — and the line stops being true of what he is
            // looking at: CW's Sent Echo sat unchecked at session start still describing
            // itself as empty until the first over, as though the station were the reason.
            // A consequence note ("hiding this stops…") is worse when removed: it warns
            // about an act already taken. Either way it is dropped while the panel is gone
            // and is back in the same render as the re-tick.
            const note = it.state === 'removed' ? undefined : it.note
            // Truthiness, not `!= null`: an empty note carries no id, so a blank string
            // cannot leave a checkbox pointing at an empty description.
            const whyId = note ? `${uid}-${it.id}-why` : undefined
            const tagId = it.state === 'popped' ? `${uid}-${it.id}-tag` : undefined
            const describedBy = [tagId, whyId].filter(Boolean).join(' ') || undefined
            return (
              <div key={it.id} className="panels-menu-item">
                {/* The label holds the box and its NAME, and nothing else. Anything more
                    it contained would be read as part of that name. */}
                <div className="panels-menu-row">
                  <label className="panels-menu-check">
                    <input
                      type="checkbox"
                      checked={it.state !== 'removed'}
                      aria-describedby={describedBy}
                      onChange={(e) => onToggle(it.id, e.target.checked)}
                    />
                    <span>{it.label}</span>
                  </label>
                  {tagId && (
                    <span className="panels-menu-tag" id={tagId}>
                      popped out
                    </span>
                  )}
                </div>
                {note && (
                  <span className="panels-menu-why" id={whyId}>
                    {note}
                  </span>
                )}
              </div>
            )
          })}
          {/* Above the button row, not inside it: .panels-menu-actions is a two-button flex
              row and a third child would sit beside them. Same class as an entry's line, so
              a consequence reads the same wherever it appears. */}
          {canUndo && undoNote && (
            <span className="panels-menu-why" id={`${uid}-undo-why`}>
              {undoNote}
            </span>
          )}
          <div className="panels-menu-actions">
            <button
              type="button"
              onClick={onUndo}
              disabled={!canUndo}
              aria-describedby={canUndo && undoNote ? `${uid}-undo-why` : undefined}
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
