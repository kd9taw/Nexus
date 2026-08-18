// Platform detection for operator-facing text and modifier chords.
//
// The gesture/chord POLICY is platform-independent on purpose — Ctrl and Cmd are both
// accepted everywhere they gate an action (cheap, harmless, and it means a mac operator's
// ⌘ muscle memory and a Windows habit both work on either box). Only the LABELS branch:
// advertising "Ctrl" to a mac operator points them at the OS right-click gesture
// (Ctrl+click) or Mission Control's Spaces chords (Ctrl+digit). Module-level so every
// label site agrees; the same check SettingsPanel has used since the serial-hint work.

/** True on macOS — Tauri's WKWebView reports "Macintosh" in the user agent. */
export const IS_MAC = navigator.userAgent.includes('Mac')

/** A digit/letter chord label in the platform's own vocabulary: `modChord('1')` is
 * "⌘1" on a Mac and "Ctrl+1" elsewhere. */
export function modChord(key: string | number): string {
  return IS_MAC ? `⌘${key}` : `Ctrl+${key}`
}

/** The bare primary-modifier label for gesture hints ("⌘ = both" / "Ctrl = both"). */
export const MOD_LABEL = IS_MAC ? '⌘' : 'Ctrl'

/** Where F-key bindings are advertised, the one fact a default Mac keyboard hides:
 * F1–F12 are media keys until Fn is held or the system setting flips. Appended to
 * tooltips on the Mac only — everywhere else it is noise. */
export const FN_KEY_HINT =
  'On Mac keyboards F-keys need Fn held — or enable "Use F1, F2, etc. keys as standard function keys" in System Settings ▸ Keyboard'
