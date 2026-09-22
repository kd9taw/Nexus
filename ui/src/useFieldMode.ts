import { useCallback, useEffect, useState } from 'react'

// THE CONTRAST AXIS — two inputs, one attribute.
//
// `data-contrast='high'` on <html> selects the high-contrast token blocks in styles.css
// (`[data-theme='…'][data-contrast='high']`, both themes, spec'd by styles-field-contrast.ts:
// every ink ≥4.5:1 on --panel under open-shade glare, computed from the cascade winner).
// TWO things ask for it, and they are different kinds of thing:
//
//   · FIELD MODE (field report, 2026-08-09) — a SITUATION: "I am operating outdoors." One
//     boolean with two effects, this attribute and a larger auto-fit in useScale (see
//     `fieldFitScale`: it shrinks the natural footprint, so the same window fits less content
//     at a bigger size). The top bar's Field chip and the Settings row are both this boolean.
//
//   · HIGH CONTRAST (#215) — a PREFERENCE: "this screen is hard for me to read." Contrast
//     only. It never touches the zoom.
//
// ⭐ WHY THE SECOND ONE EXISTS, given the first already lit the same tokens. #215 asked for
// "larger fonts AND higher-contrast theme". The size half had a dedicated control the whole
// time — Settings ▸ Workspace ▸ UI scale, an eleven-step ladder plus a cap — so an operator
// who wanted bigger type could have exactly the size they wanted. The contrast half had no
// control at all: the only route to these tokens was field mode, which in auto scale mode
// ALSO moves the zoom. There was no way to say "same size, more contrast", and that is the
// sentence the reporter was trying to say. The asymmetry is the whole feature: both inputs
// reach the attribute, only field mode reaches `useScale`.
//
// ⭐ DERIVED, NOT OWNED — and this module is the ONLY writer of the attribute. Two hooks each
// setting and removing `data-contrast` would race on unmount and on first paint, and the
// value is a disjunction, not a last-writer-wins. So the two booleans live in one hook and
// the attribute is computed: `field || highContrast`. Neither input writes the other's key,
// which is what makes leaving the field restore the operator's own standing preference
// instead of a snapshot someone has to remember to take.
//
// GLOBAL, not per-surface: both are facts about the station and the person at it, the same
// class as the theme — a pop-out follows them (DetachedPanel wires this hook like useTheme).
// NOT in the durable store: cosmetic, same classification as nexus-density — losing either on
// a reinstall costs one tap, and a backup restoring "outdoors" onto an indoor session would
// be wrong more often than right.
//
// The first-paint copy of this derivation lives in index.html's preseed and is held to this
// one by index-preseed.test.ts — both halves must seed or every launch flashes the indoor
// look, and only field mode's half moves the zoom seed.

export const FIELD_STORAGE_KEY = 'nexus-field-mode'
export const CONTRAST_STORAGE_KEY = 'nexus-high-contrast'

// ⚠️ EVERY localStorage CALL BELOW NAMES ITS KEY CONST DIRECTLY, and that is load-bearing
// rather than style. storage-scope.test.ts scans the tree for `localStorage.getItem(X)` and
// resolves X only when it is a string literal or a same-file `const` — a key handed to a
// shared `read(key)`/`persist(key, on)` helper arrives as a PARAMETER and the scan cannot see
// it at all. An invisible key is not reported unclassified, so the shared/per-surface
// classification silently stops being enforced for it: folding these four calls into two
// helpers took `nexus-field-mode`'s own coverage away with it, and the suite stayed green.
// Two keys do not need an abstraction anyway; keep the calls here literal.

function readField(): boolean {
  try {
    return localStorage.getItem(FIELD_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

function readContrast(): boolean {
  try {
    return localStorage.getItem(CONTRAST_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export interface ContrastPrefs {
  /** Operating outdoors: high contrast AND a larger auto-fit (useScale takes this one). */
  fieldMode: boolean
  setFieldMode: (on: boolean) => void
  /** The standing readability preference: high contrast only, at whatever size is set. */
  highContrast: boolean
  setHighContrast: (on: boolean) => void
}

export function useContrastPrefs(): ContrastPrefs {
  const [fieldMode, setFieldState] = useState<boolean>(readField)
  const [highContrast, setHighState] = useState<boolean>(readContrast)

  // One writer, one derivation. Either input is enough; both is the same attribute.
  useEffect(() => {
    if (fieldMode || highContrast) {
      document.documentElement.setAttribute('data-contrast', 'high')
    } else {
      document.documentElement.removeAttribute('data-contrast')
    }
  }, [fieldMode, highContrast])

  // Separate effects so setting one preference cannot rewrite the other's key.
  useEffect(() => {
    try {
      localStorage.setItem(FIELD_STORAGE_KEY, fieldMode ? '1' : '0')
    } catch {
      /* storage unavailable — the attribute still applies for this session */
    }
  }, [fieldMode])
  useEffect(() => {
    try {
      localStorage.setItem(CONTRAST_STORAGE_KEY, highContrast ? '1' : '0')
    } catch {
      /* storage unavailable — the attribute still applies for this session */
    }
  }, [highContrast])

  const setFieldMode = useCallback((on: boolean) => setFieldState(on), [])
  const setHighContrast = useCallback((on: boolean) => setHighState(on), [])
  return { fieldMode, setFieldMode, highContrast, setHighContrast }
}
