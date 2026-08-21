// Which rig mode a section owns, and whether entering it re-homes the frequency.
//
// Pure, and extracted from App.tsx because the bug it fixes (#143) was invisible while the
// rule lived inline as three lines of ref bookkeeping inside a `useEffect`: nothing could
// exercise the SEQUENCE of views that breaks it without mounting the whole application.
//
// ── THE RULE, AND THE DISTINCTION IT TURNS ON ────────────────────────────────────────────
//
// Two different questions were being answered by one ref:
//
//   1. "Which rig mode did we last ASSERT?"  — every operating view asserts one, always,
//      with no same-value guard, because the guard drifts out of sync with the real rig.
//   2. "Which mode have we last re-HOMED the frequency for?" — only the views that own a
//      frequency do that: the FT cockpit goes to 14.074, CW to the CW watering hole, and so on.
//
// `chat` (Tempo) is where those two come apart. It maps to the `digital` rig mode — Tempo IS
// a digital mode — but it keeps its own band picker's frequency and must never yank the VFO.
// So it asserts without homing. Under the old code it still advanced the one ref, and that is
// the whole of #143:
//
//     CW  → asserts cw,      re-homes,     ref = cw
//     Tempo → asserts digital, does NOT home, ref = digital   ← the damage
//     FT  → asserts digital, sees ref == digital, "no change", DOES NOT RE-HOME
//
// and FT8 comes up on the CW frequency. ve3wej reported exactly this ("ft8 stays on last cw
// frequency, but switches to digu"), was told in discussion #66 that it was fixed, and hit it
// again on 1.7.0. It is intermittent for everyone else only because you have to pass through
// a digital-mode view that does not home — Tempo — on your way to the FT screen.
//
// So the guard now tracks question 2 ONLY, and advances only when a re-home actually happened.
// Asserting a mode without homing leaves it alone, which is what makes the later FT visit
// still count as a change.

/** The rig modes a section can own. */
export type RigMode = 'cw' | 'phone' | 'rtty' | 'keyboard' | 'digital'

/** ⚠️ AN EXPLICIT MAP, NOT A FALLTHROUGH — carried over from App.tsx with its history.
 *  It used to be `featureById(view)?.workspace || …`, so ANY view carrying a `workspace`
 *  silently commanded the rig into DATA. POTA/SOTA declares `workspace: 'dx'` for LAYOUT
 *  reasons and is not a mode at all — opening it flipped an FT-991A from USB to D-U with no
 *  operator action and no way to tell why (#80). Listing the views that OWN a rig mode makes
 *  that class unrepresentable: a new workspace view asserts NOTHING until someone adds it. */
export const RIG_MODE_BY_VIEW: Partial<Record<string, RigMode>> = {
  cw: 'cw',
  phone: 'phone',
  rtty: 'rtty',
  psk: 'keyboard', // the Keyboard Modes cockpit (PSK31) — one flat section
  operate: 'digital', // the FT8/FT4 cockpit
  chat: 'digital', // Tempo is a digital mode — but see below: it does NOT own a frequency
}

/** The views that OWN a frequency, and therefore re-home the dial on entry. `chat` is
 *  deliberately absent: Tempo asserts the digital rig mode but keeps its own band picker's
 *  frequency, and that gap is what #143 was made of. */
const VIEWS_THAT_HOME = new Set(['operate', 'cw', 'phone', 'rtty', 'psk'])

export interface RigModeTransition {
  /** The mode to assert, or undefined for a view that owns none (Map, Logbook, Settings…). */
  mode?: RigMode
  /** Re-home the dial to this mode's frequency. */
  followFreq: boolean
  /** What the caller's "last re-homed mode" guard should hold after this. */
  nextHomed: RigMode
}

/**
 * What entering `view` should do to the rig, given the mode we last re-homed for.
 *
 * A view with no rig mode returns `nextHomed` unchanged: glancing at the map mid-QSO must
 * not touch the VFO AND must not advance the guard, so a later Operate click still homes.
 */
export function rigModeTransition(view: string, lastHomed: RigMode): RigModeTransition {
  const mode = RIG_MODE_BY_VIEW[view]
  if (!mode) return { followFreq: false, nextHomed: lastHomed }
  const followFreq = mode !== lastHomed && VIEWS_THAT_HOME.has(view)
  // ONLY a real re-home advances the guard. This one line is the fix.
  return { mode, followFreq, nextHomed: followFreq ? mode : lastHomed }
}
