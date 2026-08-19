// The locale subscription hook.
//
// Lives HERE, not in ./index.ts, for the reason T.tsx gives for its own split: the runtime
// stays React-free and node-testable (the settings-registry guard imports it from a node
// test). A hook in index.ts would drag React into every one of those importers.

import { useEffect, useState } from 'react'
import { availableLocales, getLocale, LOCALE_EVENT } from './index'

/**
 * Re-render this component when the prose locale changes.
 *
 * The mirror of `useUnits`, and needed for the same reason: `t()` reads a module-level variable,
 * so React has no idea the answer changed. A component that renders translated prose and does
 * NOT subscribe keeps the old language until something unrelated re-renders it — which is worse
 * than not switching at all, because half the screen changes and half does not.
 *
 * Both events: `storage` reaches Nexus's POP-OUT windows (a band map, a detached panel), and the
 * same-window event reaches the window that made the change, which `storage` never does.
 */
export function useLocale(): string {
  const [locale, setLocaleState] = useState(getLocale)
  useEffect(() => {
    const on = () => setLocaleState(getLocale())
    window.addEventListener('storage', on)
    window.addEventListener(LOCALE_EVENT, on)
    return () => {
      window.removeEventListener('storage', on)
      window.removeEventListener(LOCALE_EVENT, on)
    }
  }, [])
  return locale
}

/**
 * The languages this build can offer, English first.
 *
 * A build with one catalog has ONE entry, and the picker that reads this renders nothing —
 * deliberately. A control that cannot change anything is worse than a missing one: it invites
 * a click and answers with silence.
 */
export const LOCALE_CHOICES: string[] = availableLocales()

/**
 * What each language calls ITSELF. A picker that offers "German" to a German operator is
 * asking them to find their language in a language they may not read; every shipping picker
 * worth copying — Windows', Firefox's, WSJT-X's — lists the native name.
 */
export const LOCALE_NATIVE_NAME: Record<string, string> = {
  en: 'English',
  de: 'Deutsch',
}
