// ---------------------------------------------------------------------------
// WHAT AN EXCHANGE SLOT IS CALLED ON SCREEN.
//
// A slot id (`CLASS`, `ZN`, `QTH`) is the rules file's INVARIANT TOKEN — never translated.
// Its caption is PROSE and comes from the catalog, for the slots whose id is not a word an
// operator reads. Everything else shows its id, which is a token and correctly untranslated,
// rather than an invented English word. The entry strip and the contest log table read the
// same table, so a box and its column cannot be called two different things.
// ---------------------------------------------------------------------------

import { t, type MessageKey } from '../i18n'

/** The catalog keys for one slot's caption and tooltip. Field Day's two slots keep the
 *  captions and tooltips they shipped with. */
export const SLOT_TEXT: Record<string, { labelKey: MessageKey; titleKey: MessageKey }> = {
  CLASS: { labelKey: 'logEntry.fd.class.label', titleKey: 'logEntry.fd.class.title' },
  SECTION: { labelKey: 'logEntry.fd.section.label', titleKey: 'logEntry.fd.section.title' },
  // CQ WW's zone slot. `RST` and `QTH` are tokens hams read as they are, and keep their ids.
  ZN: { labelKey: 'logEntry.fd.zone.label', titleKey: 'logEntry.fd.zone.title' },
}

/** The operator-facing caption for a slot. */
export function slotCaption(key: string): string {
  const m = SLOT_TEXT[key]
  return m ? t(m.labelKey) : key
}

/** The tooltip for a slot's box, when the catalog has one. */
export function slotTitle(key: string): string | undefined {
  const m = SLOT_TEXT[key]
  return m ? t(m.titleKey) : undefined
}
