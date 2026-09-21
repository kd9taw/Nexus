// ⊘ — WHY THIS CONTROL IS DEAD, in text, beside the control it is about.
//
// ⭐ THE RULING IT EXISTS FOR (operator, 2026-09-20). A control the radio cannot drive used
// not to render at all, and the argument for that was real: *"a control that does nothing is
// worse than none."* It is overruled, because a control that VANISHES is indistinguishable
// from one that was never built — the operator who cannot find a manual notch, or an ATU
// button, learns nothing about whether his radio lacks it, whether his CAT backend lacks it,
// or whether Nexus never wrote it, and the third reading is the one he acts on. #95 was filed
// in exactly that confusion.
//
// The old argument survives as the CONSTRAINT this component enforces: the control must be
// visibly dead AND must say why, which is three things together — present, disabled, and a
// reason. Two of the three is a half-built control.
//
// ⚠️ ITS OWN MODULE, and with no catalog dependency, for one reason: the cockpits are on the
// i18n MIGRATED list and `CockpitHeader` is PARTIAL with its transmit-path words deliberately
// still in English. Both must be able to print the same mark, so the WORDS are the caller's
// and nothing here reaches for a key. That also keeps it out of the import cycle a shared
// component inside PhoneCockpit would have created with the header it renders.
//
// ⚠️ TEXT, NEVER COLOUR ALONE, and never the glyph alone either: `⊘` is aria-hidden
// decoration, and the word beside it is what a screen reader speaks and what a monochrome
// display shows. The long sentence is the tooltip — a mark that carried the whole explanation
// would not fit a control row.
export function Unavailable({ mark, title }: { mark: string; title: string }) {
  return (
    <span className="ph-unavail" role="note" title={title}>
      <span aria-hidden="true">⊘</span> {mark}
    </span>
  )
}
