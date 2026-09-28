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
 * that differs from the rig's is a retune. The CW cockpit's macro-set switch did the same from the
 * copy it reads when the CW view opens, and the Settings form's own Save did it for every field the
 * operator had not touched.
 *
 * `patchSettings(edit)` reads the settings the backend holds NOW and saves them with `edit`'s fields
 * applied. `edit` is handed that read, so a change that depends on the current value (one id toggled
 * in a list, one key inside `macros`) is made against the live value, not against a copy.
 *
 * `changedSince(baseline, form)` is the Settings form's side of it: the fields the form changed
 * since it loaded or last saved, which is what its Save sends. It compares what the form would send
 * with what the form loaded, so every field the form writes counts — an edit, and anything the form
 * or its save rewrites.
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

/** Fields that describe ONE thing: a form that changed any of them saves all of them. The tune is
 *  the one another writer moves constantly (the rig's knob), and a dial typed in Settings saved
 *  beside the band the knob has since moved to would put that dial on the wrong band. */
const TOGETHER: readonly (keyof Settings)[][] = [['dialMhz', 'band', 'sideband']]

/** The fields of `form` that differ from `baseline`: what a form changed since `baseline` was read.
 *  Compared as JSON because that is what `set_settings` receives — two values that serialise alike
 *  send the same thing. With no baseline every field counts as changed, which is the whole form. */
export function changedSince(baseline: Settings | null, form: Settings): Partial<Settings> {
  if (!baseline) return form
  const was = baseline as unknown as Record<string, unknown>
  const now = form as unknown as Record<string, unknown>
  const changed: Record<string, unknown> = {}
  for (const key of new Set([...Object.keys(was), ...Object.keys(now)])) {
    if (JSON.stringify(now[key]) !== JSON.stringify(was[key])) changed[key] = now[key]
  }
  for (const unit of TOGETHER) {
    if (unit.some((key) => key in changed)) for (const key of unit) changed[key] = now[key]
  }
  return changed as Partial<Settings>
}
