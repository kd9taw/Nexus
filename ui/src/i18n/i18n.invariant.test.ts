// THE INVARIANT-TOKEN GUARD — the one i18n failure that can put a station off frequency.
//
// A decimal comma reaching a frequency, a log field or a CAT string is an operating fault. The
// rule is written out in `i18n/index.ts`; this file holds it to a POSITIVE CONTROL, because a
// "no comma found" result proves nothing unless the same check can be shown to find one.
//
// Every assertion below therefore comes in pairs: first a de-DE formatter is shown to really
// produce the hazardous form (so the check can fail), then our path is shown not to.

import { describe, expect, it } from 'vitest'
import { EN, interpolate, invariantNumber, t } from './index'
import { STATION_EXAMPLES } from '../components/SettingsStation'

// A locale with a comma decimal separator AND dot grouping — the shape that makes a frequency
// unreadable and a parse silently wrong.
const HAZARD = 'de-DE'

describe('the control fires — a locale-aware formatter really does produce a decimal comma', () => {
  it('de-DE renders 14.074 with a comma and 115200 with grouping', () => {
    expect((14.074).toLocaleString(HAZARD)).toContain(',')
    expect((115200).toLocaleString(HAZARD)).toContain('.')
    expect(new Intl.NumberFormat(HAZARD).format(14.074)).toContain(',')
  })
})

describe('numbers that pass through i18n stay invariant', () => {
  it('invariantNumber never groups and never moves the decimal point', () => {
    expect(invariantNumber(14.074)).toBe('14.074')
    expect(invariantNumber(115200)).toBe('115200')
    expect(invariantNumber(-14)).toBe('-14')
    expect(invariantNumber(0.5)).toBe('0.5')
  })

  it('interpolating a number cannot produce a localised one, whatever the locale', () => {
    // The runtime has no locale-formatting path at all, so this holds by construction; the
    // control above is what makes the assertion meaningful.
    expect(interpolate('{{f}} MHz', { f: 14.074 })).toBe('14.074 MHz')
    expect(interpolate('{{f}} MHz', { f: 14.074 })).not.toContain(',')
    expect(interpolate('{{n}} baud', { n: 115200 })).toBe('115200 baud')
  })

  it('a string value is inserted verbatim — a callsign or a grid is never touched', () => {
    expect(t('settings.search.matched', { term: 'EN52xa' })).toContain('EN52xa')
    expect(t('settings.search.matched', { term: 'KD9TAW/P' })).toContain('KD9TAW/P')
  })
})

describe('technical tokens never enter the catalog', () => {
  const strings = Object.entries(EN).filter(([, v]) => typeof v === 'string') as [string, string][]

  it('no message embeds a literal frequency', () => {
    // A frequency in prose is a frequency nobody can update and a translator may reformat.
    // Frequencies reach the interface as DATA, through an invariant formatter.
    const control = /(?<![\d.])\d{1,3}\.\d{3}(?![\d])/
    expect(control.test('the dial is 14.074 MHz'), 'the detector must find one').toBe(true)
    const offenders = strings.filter(([, v]) => control.test(v)).map(([k]) => k)
    expect(offenders, 'move the number out of the string and interpolate it').toEqual([])
  })

  it('no message is one of the invariant example values', () => {
    const tokens = Object.values(STATION_EXAMPLES)
    expect(tokens.length, 'the example table is empty — this check proves nothing').toBeGreaterThan(0)
    const leaked = strings.filter(([, v]) => tokens.some((tok) => v === tok)).map(([k]) => k)
    expect(
      leaked,
      'a callsign / grid / section-code example became translatable prose — it is a wire format',
    ).toEqual([])
  })

  it('the example values really are the ones the panel shows', () => {
    // Ties the table above to the surface, so deleting a placeholder does not silently make
    // the previous check vacuous.
    expect(STATION_EXAMPLES.callsign).toBe('KD9TAW')
    expect(STATION_EXAMPLES.grid).toBe('EN52xa')
    expect(STATION_EXAMPLES.state).toBe('WI')
  })
})

describe('what this guard does NOT prove', () => {
  it('is written down rather than assumed', () => {
    // Read this list before trusting a green run:
    //
    //  • It covers the i18n path only. The tree still has ~47 `toLocaleString()` call sites
    //    outside it (counted 2026-08-18), including the Rig & CAT baud picker, which renders
    //    115200 as "115.200" on a German install. They are display-only today; none is checked
    //    here, and migrating them is phase-2 work.
    //  • It cannot see a number formatted correctly and then PARSED with `Number()` against a
    //    comma-decimal input. Persistence and parsing are outside this module by design.
    //  • The frequency detector is a shape match on the catalog, not a semantic one: `599`,
    //    `20m` and `S9` would pass it. Those are safe to keep in prose today because they do
    //    not carry a decimal separator, which is the hazard being guarded.
    expect(true).toBe(true)
  })
})
