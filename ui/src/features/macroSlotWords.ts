// THE WORDS FOR MACRO KEYS 13–16 — the four keys a dock of 16 shows past F12, which no keyboard key
// sends (`macroSets.ts`, `macroKeyCaption`). Where a key's F-key name would go — its tooltips, its
// editor, Settings ▸ CW ▸ Macros — they are named from the catalog instead, and a filled one's
// tooltip ends by saying a click sends it. Shared by the CW, RTTY and PSK wrappers, so the three say
// it the same way; F1–F12 are named by their key, as they always were.
import { t } from '../i18n'
import { macroKeyCaption, macroSlotNumber } from './macroSets'

/** How a key is named where its F-key's name would go: its F-key for F1–F12, "#13" (the catalog's
 *  word) for keys 13–16. */
export function macroSlotName(key: string): string {
  return macroKeyCaption(key) || t('macroSlot.name', { n: macroSlotNumber(key) })
}

/** A filled key's tooltip, `title`, ending for keys 13–16 with the line that says a click sends
 *  them and no keyboard key does. */
export function macroSlotTitle(key: string, title: string): string {
  return macroKeyCaption(key) ? title : `${title}\n${t('macroSlot.click.hint')}`
}
