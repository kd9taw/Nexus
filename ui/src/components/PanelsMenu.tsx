// The ⊞ Panels control — the ONLY way to RESTORE a cockpit panel, and one of two ways to
// remove one.
//
// ⚠️ THE "MENU-ONLY" RULING WAS OVERTURNED 2026-09-15 and the reasoning is kept because it
// is still half right. What stood here: "a persistent ✕ on every panel header was
// considered and rejected: it would sit inches from a decode list the operator clicks all
// night, with no confirm, and hover-reveal would break the always-on accessibility
// posture." What happened: the operator went looking for a way to close a panel on the FT8
// screen and could not find one. A capability nobody can find is not a capability, and
// that outranks the misclick it was protecting against. Every removable pane now carries a
// ✕ in its own header (components/panes/PaneCloseButton) — ALWAYS VISIBLE, never
// hover-revealed, so the accessibility half of the old objection is honoured rather than
// traded away; and the misclick half is answered by this menu, which is still one click
// from Undo and from Reset, and is still where a panel comes BACK from. The ✕ makes the
// same `setPanelState(id, 'removed')` call the tick makes — one mechanism, two doors, one
// undo history.
//
// Only the panels the CURRENT layout renders are listed. Every cockpit keeps at least one
// control that STOPS a transmission outside every pane this menu can reach — Stop TX and PTT
// everywhere they exist, Tune for its own carrier, the dock aborts, and the TX-enable latch in
// RTTY and SSTV, where a disarm really does abort the over (it does not in Operate; see
// features/panelState, THE STOP LINE, for the per-cockpit census). None of them has an entry
// here by construction. An entry can cost you a sender, and
// it can cost you a stop control a PANE happens to host (Phone's voice keyer holds a ■ Stop,
// RTTY's decode pane holds the Auto toggle) — what it can never cost you is the way to shut a
// transmission up, because that lives where no tick reaches. Senders ARE listed, six of them:
// Operate's Tx messages, its two decode panes and two rosters, Phone's voice keyer. That is
// the rule holding, not a hole in it.
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
// not need one: it applies the empty record, which puts every pane at its default — docked,
// except the few a cockpit ships hidden (Phone's Spots and Needed), and a pane may ship hidden
// only if its hide ends nothing (`defaultRemoved`, features/panelState). So reset can hide a
// pane, but never one whose hide would need saying.
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
//
// THE DASHBOARD RAIL'S ROW. Where App publishes a rail switch (components/dashRailSwitch — for the
// operating cockpit on screen), the menu draws a "Dashboard rail" row above the entries: the rail
// beside this cockpit, on or off, remembered for this section. It is not one of the cockpit's panes, so
// it is not counted as hidden, and Undo / Reset never touch it. A window too small for the rail keeps
// the row operable and says why nothing appears — the note explains, it never refuses. The rail's OWN
// menu (its four slots) says `offersRail={false}`: its ✕ is the rail's off switch there.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The ENTRIES are not
// its words: each cockpit names its own panels and supplies the `label` and the `note`, so
// those move with the cockpit. What is here is the menu itself — the ⊞ button, the popover,
// the popped-out tag, the dashboard rail's row, and Undo / Reset. No vocabulary ID passes
// through this file as prose, so nothing here can rename one.
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { t } from '../i18n'
import type { PanelState } from '../features/panelState'
import { useDashRailSwitch } from './dashRailSwitch'

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
  /** This entry is UNTICKED IN THE STOCK LAYOUT (the cockpit ships the pane hidden — Phone's
   *  Spots and Needed). While it stays unticked the button does not count it as hidden: "⊞
   *  Panels · 2 hidden", lit, on a screen the operator never touched would be a change on update
   *  and a false statement about what the operator did. Ticked, it is shown and there is nothing
   *  to count. */
  shipsHidden?: boolean
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
   *  Reset needs no twin of this: it applies the empty record, so every pane goes to its
   *  default, and the only panes whose default is hidden are ones whose hide ends nothing. */
  undoNote?: string
  /** Back to the stock layout: every panel at its default (docked, or hidden for a pane the
   *  cockpit ships hidden). */
  onReset: () => void
  /** A view's own controls above the entries — Connect's layout presets, Operate's rail side
   *  (layout L5). The view owns their words and their effect; this menu only gives them the place
   *  the operator already looks. Neither reaches a transmitter, so no stop-line census changes. */
  lead?: ReactNode
  /** False for the dashboard rail's OWN menu, which never offers the rail's switch. Default true: a
   *  cockpit's menu draws the row wherever App publishes one. */
  offersRail?: boolean
}

export function PanelsMenu({ items, onToggle, onUndo, canUndo, undoNote, onReset, lead, offersRail = true }: Props) {
  const [open, setOpen] = useState(false)
  const published = useDashRailSwitch()
  const rail = offersRail ? published : null
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

  // INSIDE THE WINDOW. The popover hangs from the button's right edge (`right: 0`), so where the
  // button sits near the left of a narrow window it hung off the window's left edge: at 700×800 at
  // 175 % only one of RTTY's nine entries could be reached (measured). And it opened DOWNWARD at the
  // sheet's 0.7 × the window, whatever room was left below the button, so where the button sits low
  // (a cockpit header at 150–175 %) the box ran past the window and its last entries could not be
  // reached by its own scroll: CW at 1280×720 at 175 %, 617–1121 px on a 720 px window (measured).
  // `fitMenu` runs once it is laid out, on every render while open, and whenever anything scrolls or
  // the window resizes (the button moves with its shell's scroll): see its own comment.
  const popRef = useRef<HTMLDivElement>(null)
  const menuSide = useRef<MenuSide | null>(null)
  useLayoutEffect(() => {
    if (!open) menuSide.current = null
    else if (popRef.current && btnRef.current) fitMenu(popRef.current, btnRef.current, menuSide)
  })
  useEffect(() => {
    if (!open) return
    const refit = (e: Event) => {
      const pop = popRef.current
      // The menu's own scroll moves nothing of the box.
      if (pop && btnRef.current && e.target !== pop) fitMenu(pop, btnRef.current, menuSide)
    }
    document.addEventListener('scroll', refit, true)
    window.addEventListener('resize', refit)
    return () => {
      document.removeEventListener('scroll', refit, true)
      window.removeEventListener('resize', refit)
    }
  }, [open])

  // Hidden by the OPERATOR — an entry unticked in the stock layout is not (see `shipsHidden`).
  const hidden = items.filter((i) => i.state === 'removed' && !i.shipsHidden).length
  return (
    <div className="panels-menu" ref={rootRef}>
      <button
        type="button"
        ref={btnRef}
        className={`panels-menu-btn${open || hidden > 0 ? ' active' : ''}`}
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title={t('panels.button.title')}
      >
        {hidden > 0 ? t('panels.button.hidden', { count: hidden }) : t('panels.button')}
      </button>
      {open && (
        <div
          ref={popRef}
          className="panels-menu-pop"
          role="group"
          aria-label={t('panels.popover.aria')}
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
          {lead}
          {rail && (
            <div className="panels-menu-item panels-menu-rail">
              <div className="panels-menu-row">
                <label className="panels-menu-check">
                  <input
                    type="checkbox"
                    checked={rail.on}
                    aria-describedby={rail.on && !rail.fits ? `${uid}-rail-why` : undefined}
                    onChange={(e) => rail.set(e.target.checked)}
                  />
                  <span>{t('dashRail.name')}</span>
                </label>
              </div>
              {rail.on && !rail.fits && (
                <span className="panels-menu-why" id={`${uid}-rail-why`}>
                  {rail.folds ? t('dashRail.menu.folded') : t('dashRail.menu.small')}
                </span>
              )}
            </div>
          )}
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
                      {t('panels.tag.popped')}
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
              title={t('panels.undo.title')}
            >
              {t('panels.undo')}
            </button>
            <button type="button" onClick={onReset} title={t('panels.reset.title')}>
              {t('panels.reset')}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

/** The dock's stop surfaces, which the menu never stands on: the TX dock or, folded, its keep row (cockpit-panes.css),
 *  and SSTV's bar. Only while parked (sticky): an observer's dock scrolls with the screen and bounds nothing. */
const PARKED_DOCK = '.cockpit-txdock, .cockpit-txdock-keep, .sstv-tx-bar'
/** THE stop button of every cockpit: the TX strip's Stop TX, and Operate's in its QSO strip. */
const STOP_BUTTON = '.op-btn.stop'
/** The least room worth opening into, in the menu's em. A menu of 26 px showed less than one entry (measured). Under
 *  this on both sides it takes the whole span between the scrollport's top and the dock's, over its own button. */
const MENU_FLOOR_EM = 6

type MenuSide = 'up' | 'down'

/** FIT THE OPEN ⊞ POPOVER INTO THE ROOM IT HAS, AND NEVER OVER THE STOP BUTTON. The operator must never lose a stop
 *  to a menu: clamped into the window it stood over Stop TX (Phone at 1024×768 at 175 %, by Tab + Enter: 100 % of
 *  Phone's only stop button, measured).
 *  · ACROSS, the rule that holds it: the menu's x range never takes in the stop button's, at any scroll, because x
 *    does not change with scroll. Anchored if that is clear. Otherwise shifted past the button on whichever side
 *    fits; if neither fits, narrowed (past the sheet's min-width) into the larger gap. Inside the scrollport always, 4 px
 *    (× the zoom) from the edge it was shifted to, by a `translate`, so nothing else moves. The sheet caps its width.
 *  · UP AND DOWN it is bounded only by the window, the ancestors that clip it (the cockpit shell scrolls) and the top
 *    of the parked dock. It may stand over the rest of the TX strip; the stop's column is clear. Where its height
 *    fits below the button it opens down as it always has; otherwise toward the side with more room (`data-up` opens
 *    it upward, styles.css) with that room as its `max-height`, so every entry is reachable by its own scroll.
 *    A `translate` keeps it in that range when the button is not: under the dock (a keyboard opens it from there) or
 *    scrolled out of the scrollport.
 *  · THE SIDE IS LATCHED AT OPEN (`side`): refits resize it, so it never jumps over the button under the pointer as
 *    the shell scrolls. It changes only when its side has less than MENU_FLOOR_EM left and the other has that.
 *  · WITH LESS THAN THAT ON BOTH SIDES it takes the whole span, over its own button: a click outside closes it.
 *  Rendered px over layout px is the UI zoom: the lengths are applied inside the zoomed tree, so they are divided by
 *  it. Reset and re-measured each time, keeping the operator's place in the menu's scroll. */
function fitMenu(pop: HTMLElement, btn: HTMLElement, side: { current: MenuSide | null }): void {
  const kept = pop.scrollTop
  pop.style.translate = ''
  pop.style.maxHeight = ''
  pop.style.minWidth = ''
  pop.style.width = ''
  pop.removeAttribute('data-up')
  let r = pop.getBoundingClientRect()
  if (r.width === 0 || pop.offsetWidth === 0) return
  const zoom = r.width / pop.offsetWidth
  const edge = 4 * zoom
  let top = 0
  let bottom = window.innerHeight
  let left = 0
  let right = window.innerWidth
  let clip: HTMLElement | null = null
  for (let e = pop.parentElement; e; e = e.parentElement) {
    if (getComputedStyle(e).overflowY === 'visible') continue
    const q = e.getBoundingClientRect()
    top = Math.max(top, q.top)
    bottom = Math.min(bottom, q.bottom)
    // Its CLIENT box across, inside its scrollbar: past that the menu gives the shell a horizontal scrollbar, the
    // scrollport loses its height, and the parked keep row rises under the menu (PSK at 900×600 at 175 %, measured).
    if (e.clientWidth > 0 && e.offsetWidth > 0) {
      const z = q.width / e.offsetWidth
      left = Math.max(left, q.left + e.clientLeft * z)
      right = Math.min(right, q.left + (e.clientLeft + e.clientWidth) * z)
    }
    clip ??= e
  }
  const cockpit: Element | null = pop.closest('main') ?? clip

  // ACROSS. Every stop button's column, as one.
  const stops = cockpit ? Array.from(cockpit.querySelectorAll<HTMLElement>(STOP_BUTTON)).map((el) => el.getBoundingClientRect()).filter((q) => q.width > 0) : []
  let dx = r.left < left + edge ? left + edge - r.left : r.right > right - edge ? Math.max(left + edge - r.left, right - edge - r.right) : 0
  if (stops.length > 0) {
    const stopL = Math.min(...stops.map((q) => q.left))
    const stopR = Math.max(...stops.map((q) => q.right))
    if (r.left + dx < stopR + edge && r.right + dx > stopL - edge) {
      const gapR = right - edge - stopR - edge
      const gapL = stopL - edge - (left + edge)
      if (gapR < r.width && gapL < r.width) {
        pop.style.minWidth = '0px'
        pop.style.width = `${Math.max(gapR, gapL) / zoom}px`
        r = pop.getBoundingClientRect()
      }
      dx = gapR >= r.width ? stopR + edge - r.left : stopL - edge - r.right
    }
  }

  // UP AND DOWN.
  for (const el of cockpit ? Array.from(cockpit.querySelectorAll<HTMLElement>(PARKED_DOCK)) : []) {
    const q = el.getBoundingClientRect()
    if (q.height > 0 && getComputedStyle(el).position === 'sticky') bottom = Math.min(bottom, q.top)
  }
  const b = btn.getBoundingClientRect()
  const gap = Math.max(0, r.top - b.bottom)
  const downFrom = b.bottom + gap
  const upFrom = b.top - gap
  const anchorTop = Math.max(downFrom, top + edge)
  const anchorBottom = Math.min(upFrom, bottom - edge)
  const below = bottom - edge - anchorTop
  const above = anchorBottom - (top + edge)
  const em = (parseFloat(getComputedStyle(pop).fontSize) || 16) * zoom
  const floor = Math.min(r.height, MENU_FLOOR_EM * em)
  const roomOf = (s: MenuSide) => (s === 'up' ? above : below)
  let s: MenuSide = side.current ?? (r.height <= below ? 'down' : above > below ? 'up' : 'down')
  let span = false
  if (roomOf(s) < floor) {
    const other: MenuSide = s === 'up' ? 'down' : 'up'
    if (roomOf(other) >= floor) s = other
    else {
      s = 'up'
      span = true
    }
  }
  side.current = s
  let dy: number
  if (s === 'down') {
    dy = anchorTop - downFrom
    if (r.height > below) pop.style.maxHeight = `${Math.max(0, below) / zoom}px`
  } else {
    pop.setAttribute('data-up', '')
    const room = span ? bottom - top - 2 * edge : above
    const h = Math.max(0, Math.min(r.height, room))
    if (r.height > room) pop.style.maxHeight = `${h / zoom}px`
    dy = (span ? Math.max(top + edge + h, Math.min(upFrom, bottom - edge)) : anchorBottom) - upFrom
  }
  if (dx !== 0 || dy !== 0) pop.style.translate = `${dx / zoom}px ${dy / zoom}px`
  if (pop.scrollTop !== kept) pop.scrollTop = kept
}
