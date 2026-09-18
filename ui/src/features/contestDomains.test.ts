import { describe, expect, it } from 'vitest'
import {
  contestDomain,
  domainSuggestions,
  inDomain,
  resolveDomainValue,
} from './contestDomains'

// ---------------------------------------------------------------------------
// THE COUNTY TYPE-AHEAD — "Cook" → COOK.
//
// A QSO-party county exchange is four letters an operator has to know (JODA, SCLA, MCDN)
// and the NAME is the part they actually know, so the strip has to accept either. These
// are the rules that decides: what is offered, and what a typed name resolves to.
// Every code and name below is the Illinois QSO Party's own chart, mirrored in
// `ilqpQth.ts` and guarded against the rules seed by tempo-core.
// ---------------------------------------------------------------------------

const COUNTIES = ['il_counties']
const QTH = ['il_counties', 'il_mults'] // the ILQP exchange slot's two enum arms

describe('the ILQP domains reach the UI at all', () => {
  it('carries the sponsor’s whole county chart and its state list', () => {
    expect(contestDomain('il_counties')?.total).toBe(102)
    expect(contestDomain('il_mults')?.total).toBe(66)
    // The verdict path the strip already had, over the new tables.
    expect(inDomain('il_counties', 'cook')).toBe(true)
    expect(inDomain('il_counties', ' LEE ')).toBe(true)
    expect(inDomain('il_counties', 'ADAMS')).toBe(false)
    // …and Illinois itself is not one of the states a station can send.
    expect(inDomain('il_mults', 'IL')).toBe(false)
    expect(inDomain('il_mults', 'WI')).toBe(true)
  })
})

describe('what the type-ahead offers', () => {
  it('offers code matches first, then names that start with it, then names that contain it', () => {
    const codes = domainSuggestions(COUNTIES, 'CO').map((v) => v.code)
    // COLE and COOK are the two county CODES beginning CO, and they lead. Their names
    // (Coles, Cook) begin with it too, so the second pass adds nobody new; what follows
    // is the contains pass — Han-co-ck, Ma-co-n, Ma-co-upin, S-co-tt.
    expect(codes).toEqual(['COLE', 'COOK', 'HANC', 'MACN', 'MCPN', 'SCOT'])
    // A name-only match is found with no code in sight: no county code begins MA.
    expect(domainSuggestions(COUNTIES, 'MA').map((v) => v.code)).toEqual([
      'MACN',
      'MADN',
      'MARI',
      'MASN',
      'MCPN',
      'MSHL',
    ])
  })

  it('finds a county by its name', () => {
    expect(domainSuggestions(COUNTIES, 'Cook')).toEqual([{ code: 'COOK', name: 'Cook' }])
    expect(domainSuggestions(COUNTIES, 'jo dav')).toEqual([
      { code: 'JODA', name: 'JoDaviess' },
    ])
    // Punctuation and spacing are the operator's, not the chart's.
    expect(domainSuggestions(COUNTIES, 'st clair')).toEqual([
      { code: 'SCLA', name: 'St. Clair' },
    ])
  })

  it('searches every arm of a slot that has more than one', () => {
    expect(domainSuggestions(QTH, 'Wisconsin').map((v) => v.code)).toEqual(['WI'])
    expect(domainSuggestions(QTH, 'Ontario').map((v) => v.code)).toEqual(['ON'])
  })

  it('offers nothing for an empty box, an unknown domain, or no domain at all', () => {
    expect(domainSuggestions(COUNTIES, '')).toEqual([])
    expect(domainSuggestions(COUNTIES, '   ')).toEqual([])
    expect(domainSuggestions(['oh_counties'], 'Franklin')).toEqual([])
    expect(domainSuggestions(undefined, 'Cook')).toEqual([])
    expect(domainSuggestions([], 'Cook')).toEqual([])
  })

  it('is bounded, so a two-letter prefix cannot drop a list of 102 on the strip', () => {
    expect(domainSuggestions(COUNTIES, 'MA').length).toBeLessThanOrEqual(6)
    expect(domainSuggestions(COUNTIES, 'MA', 3).length).toBe(3)
    // POSITIVE CONTROL: without the cap there really are more than six.
    expect(domainSuggestions(COUNTIES, 'MA', 99).length).toBeGreaterThan(6)
  })
})

describe('what a typed value resolves to', () => {
  it('turns a county name into the code that goes on the air', () => {
    expect(resolveDomainValue(COUNTIES, 'Cook')).toBe('COOK')
    expect(resolveDomainValue(COUNTIES, '  cook  ')).toBe('COOK')
    expect(resolveDomainValue(COUNTIES, 'St. Clair')).toBe('SCLA')
    expect(resolveDomainValue(COUNTIES, 'Jo Daviess')).toBe('JODA')
    // ⚠️ A PREFIX does NOT resolve, however unambiguous it looks — nothing else starts
    // "Kanka", and it still refuses. The rule is deliberately blunt because the case it
    // protects is ARRL's section list (see the state-name test below): a prefix rule
    // turns "New York" into one of the four sections that cover New York. What replaces
    // the convenience is the list, which offers KANK while the operator types.
    expect(resolveDomainValue(COUNTIES, 'Kanka')).toBeUndefined()
    expect(domainSuggestions(COUNTIES, 'Kanka').map((v) => v.code)).toEqual(['KANK'])
  })

  it('leaves a code alone', () => {
    expect(resolveDomainValue(COUNTIES, 'JODA')).toBe('JODA')
    expect(resolveDomainValue(COUNTIES, 'lee')).toBe('LEE')
  })

  // ⭐ THE CASE THAT MUST NEVER RESOLVE. Several ARRL sections cover one state — New York
  // is NLI, ENY, NNY and WNY; California is ten — and a resolver that picks one is worse
  // than one that refuses, because the operator sees a plausible code and logs it.
  it('never turns a state name into one of the several sections that cover it', () => {
    const FD = ['fd_sections']
    expect(resolveDomainValue(FD, 'Wisconsin')).toBe('WI') // one section, one answer
    // …however it is typed: the fold is the same one every other value gets.
    expect(resolveDomainValue(FD, 'wisconsin ')).toBe('WI')
    expect(resolveDomainValue(FD, '  WISCONSIN')).toBe('WI')
    expect(resolveDomainValue(FD, 'New York')).toBeUndefined()
    expect(resolveDomainValue(FD, 'California')).toBeUndefined()
    // A state that is not a section name at all resolves to nothing rather than to the
    // section whose name happens to begin with it.
    expect(resolveDomainValue(FD, 'Texas')).toBeUndefined()
    expect(resolveDomainValue(FD, 'Massachusetts')).toBeUndefined()
    // …and in every one of those cases the operator is not left guessing: the list
    // OFFERS the sections that cover the state, which is the hint.
    expect(domainSuggestions(FD, 'New York', 99).map((v) => v.code).sort()).toEqual([
      'ENY',
      'NLI',
      'NNY',
      'WNY',
    ])
    expect(domainSuggestions(FD, 'Texas', 99).map((v) => v.code).sort()).toEqual([
      'NTX',
      'STX',
      'WTX',
    ])
    expect(domainSuggestions(FD, 'Massachusetts', 99).map((v) => v.code).sort()).toEqual([
      'EMA',
      'WMA',
    ])
    // ⚠️ AND THE HONEST LIMIT, recorded rather than assumed: California's ten sections are
    // named East Bay, Los Angeles, Orange, Pacific, Sacramento Valley, San Diego, San
    // Francisco, San Joaquin Valley, Santa Barbara and Santa Clara Valley — not one of
    // them contains the word "California", so an operator who types the state gets no
    // suggestion either. The list can only offer what the data NAMES; the section table
    // carries no state field, and inventing a state→section map would be this build's
    // data wearing the ARRL's name.
    expect(domainSuggestions(FD, 'California', 99)).toEqual([])
  })

  it('refuses to guess between several, which is the whole safety of it', () => {
    // Macon, Macoupin, Madison, Marion, Marshall, Mason, Massac…
    expect(resolveDomainValue(COUNTIES, 'Ma')).toBeUndefined()
    expect(resolveDomainValue(COUNTIES, 'M')).toBeUndefined()
    // Nothing at all matches: the box keeps what was typed and the verdict refuses it.
    expect(resolveDomainValue(COUNTIES, 'Nowhere')).toBeUndefined()
    expect(resolveDomainValue(COUNTIES, '')).toBeUndefined()
    expect(resolveDomainValue(undefined, 'Cook')).toBeUndefined()
  })

  it('resolves across both arms of the ILQP exchange slot', () => {
    expect(resolveDomainValue(QTH, 'Cook')).toBe('COOK')
    expect(resolveDomainValue(QTH, 'Wisconsin')).toBe('WI')
    expect(resolveDomainValue(QTH, 'Ontario')).toBe('ON')
    // A country a DX station sends is in neither arm — it stays exactly as typed.
    expect(resolveDomainValue(QTH, 'Germany')).toBeUndefined()
  })
})
