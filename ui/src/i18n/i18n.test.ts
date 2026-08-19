// Guards for the i18n runtime.
//
// The three properties that actually matter, and each is tested through the PUBLIC path with a
// synthetic locale rather than by poking internals:
//
//   1. A missing / blank translation resolves to the ENGLISH source string. Never the key,
//      never an empty string. This is the one mechanical protection against shipping machine
//      translation, and it is worth more than every other line in the module.
//   2. Interpolation cannot emit a locale-formatted number (see i18n.invariant.test.ts for the
//      positive control), and it leaves single-brace macro tokens alone.
//   3. Markup markers can only produce elements the CALL SITE supplied — a catalog string
//      cannot introduce one.

import { afterEach, describe, expect, it } from 'vitest'
import {
  EN,
  getLocale,
  installCatalog,
  interpolate,
  parseRich,
  rawMessage,
  selectPluralForm,
  setLocale,
  t,
  type MessageKey,
} from './index'

afterEach(() => setLocale('en'))

describe('English is the floor', () => {
  it('falls back to the English string when the locale has no entry', () => {
    installCatalog('zz', { 'reveal.enable': 'Einschalten' })
    setLocale('zz')
    expect(t('reveal.enable')).toBe('Einschalten') // control: the overlay IS consulted
    expect(t('reveal.notNow')).toBe(EN['reveal.notNow'])
  })

  it('treats a blank translation as missing — a half-finished catalog shows English', () => {
    installCatalog('zz', { 'reveal.enable': '   ' })
    setLocale('zz')
    expect(t('reveal.enable')).toBe(EN['reveal.enable'])
  })

  it('never resolves a key to the key itself', () => {
    installCatalog('zz', {})
    setLocale('zz')
    const keys = Object.keys(EN) as MessageKey[]
    const echoed = keys.filter((k) => t(k) === k)
    expect(echoed, 'these resolved to their own key instead of English').toEqual([])
  })

  it('ignores an unknown locale rather than blanking the interface', () => {
    setLocale('not-installed')
    expect(getLocale()).toBe('en')
    expect(t('reveal.enable')).toBe(EN['reveal.enable'])
  })
})

describe('interpolation', () => {
  it('substitutes {{name}}', () => {
    expect(t('settings.search.matched', { term: 'sound card' })).toBe('matched “sound card”')
  })

  it('leaves single-brace macro tokens alone — {NAME} is a CW macro, not a placeholder', () => {
    const hint = t('settings.station.opName.hint')
    expect(hint).toContain('{NAME}')
    expect(t('settings.station.opState.hint')).toContain('{MYSTATE}')
  })

  it('leaves an unsupplied placeholder visible rather than blanking the sentence', () => {
    expect(interpolate('hi {{who}}', {})).toBe('hi {{who}}')
  })

  it('does not let a value be re-scanned for placeholders', () => {
    expect(interpolate('{{a}}', { a: '{{b}}', b: 'no' })).toBe('{{b}}')
  })
})

describe('plurals', () => {
  const forms = { one: 'one thing', other: '{{count}} things' }

  it('selects by CLDR category, not by n === 1', () => {
    expect(selectPluralForm('en', 1, forms)).toBe('one thing')
    expect(selectPluralForm('en', 0, forms)).toBe('{{count}} things')
    // Polish: 2..4 is `few`, which English does not have. Missing category → `other`.
    expect(selectPluralForm('pl', 3, forms)).toBe('{{count}} things')
    expect(selectPluralForm('pl', 3, { ...forms, few: 'kilka' })).toBe('kilka')
  })

  it('resolves a plural entry end to end through t()', () => {
    installCatalog('zz', {
      'reveal.enable': { one: 'ein Ding', other: '{{count}} Dinge' },
    })
    setLocale('zz')
    expect(t('reveal.enable', { count: 1 })).toBe('ein Ding')
    expect(t('reveal.enable', { count: 4 })).toBe('4 Dinge')
  })

  it('falls back to English when a locale drops the plural entry', () => {
    installCatalog('zz', {})
    setLocale('zz')
    expect(t('reveal.enable', { count: 4 })).toBe(EN['reveal.enable'])
  })
})

describe('rich-text markers', () => {
  it('splits only the markers the call site declared', () => {
    expect(parseRich('a <b>bee</b> c', ['b'])).toEqual(['a ', { tag: 'b', children: ['bee'] }, ' c'])
    expect(parseRich('a <b>bee</b> c', [])).toEqual(['a <b>bee</b> c'])
  })

  it('cannot be made to produce an element the call site did not supply', () => {
    // The shape a translation-injection attempt would take.
    expect(parseRich('<script>x</script>', ['b'])).toEqual(['<script>x</script>'])
    expect(parseRich('<b onclick="x">y</b>', ['b'])).toEqual(['<b onclick="x">y</b>'])
  })

  it('nests', () => {
    expect(parseRich('<b>x<b>y</b>z</b>', ['b'])).toEqual([
      { tag: 'b', children: ['x', { tag: 'b', children: ['y'] }, 'z'] },
    ])
  })

  it('degrades an unclosed marker to text instead of swallowing the sentence', () => {
    expect(parseRich('start <b>rest of it', ['b'])).toEqual(['start <b>rest of it'])
  })

  it('parses markers BEFORE values are substituted, so a value cannot become markup', () => {
    const nodes = parseRich(rawMessage('reveal.prompt'), ['b'])
    const leaves = nodes.flatMap((n) => (typeof n === 'string' ? [n] : n.children))
    expect(leaves).toContain('{{achievement}}')
    expect(interpolate('{{achievement}}', { achievement: '<b>hi</b>' })).toBe('<b>hi</b>')
  })
})

describe('the catalog itself', () => {
  it('uses only well-formed keys', () => {
    const bad = Object.keys(EN).filter((k) => !/^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*)+$/.test(k))
    expect(bad, 'keys are dot-separated lowerCamel segments — see en.ts').toEqual([])
  })

  it('closes every markup marker it opens', () => {
    const unbalanced = Object.entries(EN)
      .filter(([, v]) => typeof v === 'string')
      .filter(([, v]) => {
        const s = v as string
        const opens = [...s.matchAll(/<([a-zA-Z][a-zA-Z0-9]*)>/g)].map((m) => m[1])
        return opens.some((tag) => !s.includes(`</${tag}>`))
      })
      .map(([k]) => k)
    expect(unbalanced).toEqual([])
  })
})
