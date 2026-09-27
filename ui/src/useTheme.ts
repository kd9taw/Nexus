import { useCallback, useEffect, useState } from 'react'

/** The theme the page PAINTS: what `data-theme` carries and what every component renders
 *  against. Always light or dark, so no selector and no theme-keyed cache ever sees 'system'. */
export type Theme = 'light' | 'dark'
/** What the operator PICKED, and what `tempo-theme` stores: a theme, or 'system' — follow the
 *  computer's own light/dark setting. */
export type ThemeChoice = Theme | 'system'

const STORAGE_KEY = 'tempo-theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

/** The computer's light/dark setting as a theme; Dark when the platform cannot say, which is
 *  also the app's default. index.html's preseed carries the first-paint copy of this. */
export function systemTheme(): Theme {
  if (typeof window.matchMedia !== 'function') return 'dark'
  return window.matchMedia(DARK_QUERY).matches ? 'dark' : 'light'
}

/** The computer's light/dark setting, followed live while `active` — the listener exists only
 *  while something is actually following it. */
export function useSystemTheme(active: boolean): Theme {
  const [os, setOs] = useState<Theme>(systemTheme)
  useEffect(() => {
    if (!active || typeof window.matchMedia !== 'function') return
    const mq = window.matchMedia(DARK_QUERY)
    const follow = () => setOs(mq.matches ? 'dark' : 'light')
    follow()
    mq.addEventListener('change', follow)
    return () => mq.removeEventListener('change', follow)
  }, [active])
  return os
}

function readInitial(): ThemeChoice {
  const saved = localStorage.getItem(STORAGE_KEY)
  if (saved === 'light' || saved === 'dark' || saved === 'system') return saved
  // the amber theme was removed — migrate any saved value to dark so it doesn't recur
  if (saved === 'amber') {
    localStorage.setItem(STORAGE_KEY, 'dark')
    return 'dark'
  }
  // default to dark (shack), matching the index.html default — also when the computer prefers
  // light: System is a choice, not the default (operator, 2026-09-26)
  return 'dark'
}

/** [the painted theme, set the choice, the choice itself (for the Settings chips)]. */
export function useTheme(): [Theme, (t: ThemeChoice) => void, ThemeChoice] {
  const [choice, setChoice] = useState<ThemeChoice>(readInitial)
  const os = useSystemTheme(choice === 'system')
  const theme: Theme = choice === 'system' ? os : choice

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])
  // The CHOICE is stored, never the resolved theme — or System would become a fixed theme
  // the next time the app starts.
  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, choice)
  }, [choice])

  const setTheme = useCallback((t: ThemeChoice) => setChoice(t), [])
  return [theme, setTheme, choice]
}
