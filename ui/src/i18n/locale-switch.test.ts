// @vitest-environment jsdom
//
// The switching machinery: persistence, the OS default, and the re-render notification.
//
// Each of these is a way for a language setting to LOOK broken rather than be broken, which is
// why they are pinned before a translation exists:
//   • no notify  → the screen keeps the old language until something unrelated re-renders, so
//                  half the interface changes and half does not;
//   • no persist → the choice survives until the app closes;
//   • no OS read → a German operator on a German Windows still starts in English, which fails
//                  the premise that a capability needing configuration is unfinished.

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { localeChoices } from './useLocale'
import {
  EN,
  getLocale,
  initLocale,
  installCatalog,
  LOCALE_EVENT,
  LOCALE_KEY,
  setLocale,
  t,
} from './index'

// A stand-in catalog. Deliberately NOT German — the guard below checks the real one when it
// exists, and a fixture that pretends to be German would be the thing everyone reads instead.
const ZZ = { 'reveal.notNow': 'zz-notnow' } as const

beforeEach(() => {
  localStorage.clear()
  installCatalog('zz', ZZ)
  setLocale('en')
  localStorage.clear() // the setLocale above persists; start each test from nothing
})
afterEach(() => {
  setLocale('en')
  localStorage.clear()
})

describe('switching', () => {
  it('applies, persists, and announces the change exactly once', () => {
    const seen: string[] = []
    const on = () => seen.push(getLocale())
    window.addEventListener(LOCALE_EVENT, on)
    try {
      setLocale('zz')
      expect(getLocale()).toBe('zz')
      expect(t('reveal.notNow')).toBe('zz-notnow')
      expect(localStorage.getItem(LOCALE_KEY)).toBe('zz')
      expect(seen, 'one event for one change').toEqual(['zz'])
      // Re-selecting the SAME language is not a change and must not re-announce one.
      setLocale('zz')
      expect(seen).toEqual(['zz'])
    } finally {
      window.removeEventListener(LOCALE_EVENT, on)
    }
  })

  it('ignores a language with no catalog rather than blanking the interface', () => {
    setLocale('xx-nope')
    expect(getLocale()).toBe('en')
    expect(t('reveal.notNow')).toBe(EN['reveal.notNow'])
  })

  it('survives a storage that refuses to write — the switch still applies', () => {
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError')
    })
    try {
      setLocale('zz')
      expect(getLocale(), 'the session keeps the language even when it cannot be saved').toBe('zz')
    } finally {
      spy.mockRestore()
    }
  })
})

describe('startup', () => {
  it('restores the stored choice', () => {
    localStorage.setItem(LOCALE_KEY, 'zz')
    initLocale()
    expect(getLocale()).toBe('zz')
  })

  it('falls back to the OS language, matching on LANGUAGE not region', () => {
    // de-AT must find a `de` catalog: shipping Austrian German separately is not the plan, and
    // falling to English because of a region tag would be the bug this exists to prevent.
    installCatalog('de', { 'reveal.notNow': 'de-notnow' })
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('de-AT')
    initLocale()
    expect(getLocale()).toBe('de')
    vi.restoreAllMocks()
  })

  it('stays English when the OS language has no catalog', () => {
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('fr-FR')
    initLocale()
    expect(getLocale()).toBe('en')
    vi.restoreAllMocks()
  })

  it('prefers a stored choice OVER the OS language', () => {
    localStorage.setItem(LOCALE_KEY, 'zz')
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('de-DE')
    installCatalog('de', { 'reveal.notNow': 'de-notnow' })
    initLocale()
    expect(getLocale(), 'an explicit choice outranks the machine').toBe('zz')
    vi.restoreAllMocks()
  })
})

// THE PICKER WAS INVISIBLE IN 1.7.1-test4, and this is the test that would have caught it.
//
// The choices were captured in a module-level `const LOCALE_CHOICES = availableLocales()`.
// That runs when the module is first IMPORTED — which happens through App → SettingsPanel,
// deep inside `import App from './App'`, long before main.tsx's callback installs the German
// catalog. So the array froze at ['en'], the picker's `length > 1` was false forever, and a
// finished, tested, shipped translation had no way to be selected. Every other test passed:
// they all install a catalog and then read, which is the one order the real app never uses.
//
// The property, stated so it cannot regress: the choices are read LIVE, and a catalog
// installed after this module was imported must appear in them.
describe('the picker can see a catalog installed after import', () => {
  it('lists a locale registered later — the real startup order', () => {
    // 'zz' is installed in beforeEach, i.e. AFTER this module was imported at suite load.
    expect(localeChoices()).toContain('zz')
    expect(localeChoices()[0], 'English stays first').toBe('en')
    // The control: a language nobody installed is not offered.
    expect(localeChoices()).not.toContain('qq')
  })
})
