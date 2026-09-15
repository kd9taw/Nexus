// THE PANE'S OWN ✕ — one button, rendered into every removable pane's own header.
//
// WHY IT EXISTS, and why the opposite decision is recorded in PanelsMenu.tsx. Removal was
// menu-only by design ("a persistent ✕ … would sit inches from a decode list the operator
// clicks all night"), and on 2026-09-15 the operator went looking for a way to close a panel
// on the FT8 screen and could not find one. The capability had shipped; the AFFORDANCE had
// not. So this is a second DOOR onto the existing mechanism, never a second mechanism: the
// click is `setPanelState(id, 'removed')`, the same call the tick makes, writing the same
// record to the same key. ⊞ Panels remains the way back — and its Undo still reaches this
// hide, because there is only one history.
//
// A PANE WITH NO VOCABULARY ENTRY GETS NO ✕. `panelHost.closeProps` returns `{}` for an id
// the layout does not list, so "not removable" stays unrepresentable rather than guarded —
// the same shape as CockpitPaneFrame drawing nothing when `onRemove` is omitted. Nothing here
// can reach a control on a cockpit's stop-line census: those live outside every pane and hold
// no ⊞ id (features/panelState.ts, THE STOP LINE).
//
// THE WARNING. THE PRACTICE half of that rule puts the consequence BEFORE the act, because a
// stop the operator did not ask for reads as a dropout. The menu prints the pane's note under
// its checkbox; this carries the SAME string — the cockpit's own `endsOnHide` copy, never a
// second wording — as the button's tooltip and, for a keyboard/screen-reader operator, as its
// accessible DESCRIPTION. Exactly one pane populates it today (Phone's voiceKeyer, whose
// unmount stops a message and discards a recording).
//
// KEYBOARD. A real <button> in the header: one ordinary tab stop, before the pane's content,
// so a pane whose body is a roving list (one Tab stop, arrows within) is unaffected — the ✕
// is not in the list and never joins its roving index.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The pane's NAME
// arrives from the cockpit and is interpolated as data; the consequence copy arrives from the
// cockpit too (the ⊞ notes are not migrated yet and move as one batch when they are).
import { useId } from 'react'
import { t } from '../../i18n'

export function PaneCloseButton({
  title,
  onRemove,
  hideNote,
}: {
  /** The pane's own name — the accessible name is "Hide <title>", so a screen reader says
   *  WHICH panel this closes rather than announcing a row of identical ✕ buttons. */
  title: string
  /** Hide this pane. Omitted ⇒ nothing renders (the pane is not removable). */
  onRemove?: () => void
  /** What this hide ENDS, in the cockpit's own words. Omitted for a hide that ends nothing —
   *  and a note the operator cannot act on teaches him to ignore the next one. */
  hideNote?: string
}) {
  // Stable across renders and unique per instance: several panes carry a ✕ on one screen and
  // each description must belong to its own button.
  const uid = useId()
  if (!onRemove) return null
  return (
    <>
      <button
        type="button"
        className="cockpit-popout"
        onClick={onRemove}
        aria-label={t('pane.hide.aria', { title })}
        // The description is what a screen reader reads on FOCUS — before the press. The
        // tooltip is the same string for a pointer operator, with the way back appended so
        // the ✕ never reads as destruction.
        aria-describedby={hideNote ? uid : undefined}
        title={hideNote ? `${hideNote} — ${t('pane.hide.title')}` : t('pane.hide.title')}
      >
        ✕
      </button>
      {/* Referenced by aria-describedby, so it is announced despite being hidden — the
          accessible description computation reads a referenced node whether or not it is
          rendered. Hidden rather than shown because a pane header is a control row, not a
          place for a sentence; the menu is where the sentence is visible. */}
      {hideNote && (
        <span id={uid} hidden>
          {hideNote}
        </span>
      )}
    </>
  )
}
