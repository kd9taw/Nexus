// @vitest-environment jsdom
//
// WHAT CONNECT'S BOXES ARE CALLED (the operator's picks, 2026-09-30 and 2026-10-01). Three things were
// called "Conditions": the view-to-be, the headline box and the map's floating card. The operator
// named the headline box "Best band" and the card "Propagation" (display strings only), and the
// side-by-side renders then found the new name in the same tab row as "Best Band → Region", so that
// box is "Bands by region". Read through the registry's own getters, in every language that ships,
// so a translation that brings a clash back is caught the same way.
import { afterEach, describe, expect, it } from 'vitest'
import { PANES } from './panes'
import { installCatalog, setLocale, t } from '../../i18n'
import { DE } from '../../i18n/de'
import { ES } from '../../i18n/es'
import { FR } from '../../i18n/fr'
import { JA } from '../../i18n/ja'

const LOCALES = { en: null, de: DE, es: ES, fr: FR, ja: JA } as const
for (const [locale, catalog] of Object.entries(LOCALES)) if (catalog) installCatalog(locale, catalog)

afterEach(() => setLocale('en'))

const title = (id: string) => PANES.find((p) => p.id === id)?.title

describe("Connect's box names", () => {
  it('the headline box is "Best band", the region box "Bands by region", and the map’s card "Propagation"', () => {
    setLocale('en')
    expect(title('advisory')).toBe('Best band')
    expect(title('bestband')).toBe('Bands by region')
    expect(t('map.insights.title')).toBe('Propagation')
    expect(t('map.insights.pill'), 'the card folded to its pill').toBe('Propagation')
  })

  for (const locale of Object.keys(LOCALES)) {
    it(`${locale}: no two boxes share a name, and no box is named like the map’s card or the view`, () => {
      setLocale(locale)
      const names = PANES.map((p) => p.title.trim().toLocaleLowerCase(locale))
      expect(names.length, 'control: the registry is read').toBeGreaterThan(20)
      const twice = names.filter((n, i) => names.indexOf(n) !== i)
      expect(twice, 'a name two boxes share').toEqual([])
      for (const other of [t('map.insights.title'), t('map.insights.pill'), t('nav.connect.label')])
        expect(names, `a box named like "${other}"`).not.toContain(other.trim().toLocaleLowerCase(locale))
    })
  }
})
