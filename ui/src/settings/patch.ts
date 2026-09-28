/** THE SETTINGS PATCH SEAM — how a surface saves the settings fields it owns, and only those.
 *
 * `set_settings` takes a WHOLE settings struct and makes it the truth for every field in it. It
 * cannot take a partial: a missing field deserialises to its default. So every writer sends a whole
 * struct, and what decides whether a save is correct is where the fields it does NOT own come from.
 *
 * They must come from the backend at save time, never from a copy the surface has been holding. A
 * view that read the settings when it opened and later saved `{ ...itsCopy, ...change }` wrote its
 * copy's value of every other field back over whatever had changed since, silently: the Contest
 * view's scoring chips turned simultaneous radios back on after "use one radio" in the launch
 * picker, handed the log back to the operator from before a seat swap, and sent the dial back to
 * where the rig was when the view opened — which the radio loop then commands, because a saved dial
 * that differs from the rig's is a retune.
 *
 * `patchSettings(edit)` reads the settings the backend holds NOW and saves them with `edit`'s fields
 * applied. `edit` is handed that read, so a change that depends on the current value (one id toggled
 * in a list, one key inside `macros`) is made against the live value, not against a copy.
 *
 * ⚠️ The read and the write are two calls. A change that lands between them — a window of
 * milliseconds — is still overwritten. Closing it needs the backend to apply the fields under its
 * own lock.
 */
import { getSettings, setSettings } from '../api'
import type { AppSnapshot, Settings } from '../types'

/** Save `edit`'s fields over the settings the backend holds now. */
export async function patchSettings(
  edit: (current: Settings) => Partial<Settings>,
): Promise<AppSnapshot> {
  const current = await getSettings()
  return setSettings({ ...current, ...edit(current) })
}
