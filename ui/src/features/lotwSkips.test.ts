// The LoTW upload toast for contacts the station HELD BACK because TQSL does not know their mode
// yet (a JTTY contact, written `MODE=MFSK SUBMODE=JTTY`): the station counts them (`heldMode`) and
// its connection log says why; this is the same sentence on the screen, in every catalog.
import { describe, it, expect, afterEach } from 'vitest'
import { EN, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { RU } from '../i18n/ru'
import { lotwSkipNote } from './lotwSkips'

afterEach(() => setLocale('en'))

describe('the LoTW toast for contacts held back from TQSL', () => {
  it('says, in English, exactly what the connection log says', () => {
    // `tempo_core::lotw_upload::held_sentence`, word for word, for one contact and for three.
    expect(lotwSkipNote({ dispatched: 4, outcome: 'pending', heldMode: 1 })).toBe(
      '1 JTTY contact was not sent: TQSL does not know the JTTY mode yet and would drop it, so it was held back. It stays marked as not uploaded to LoTW.',
    )
    expect(lotwSkipNote({ dispatched: 0, outcome: 'none', heldMode: 3 })).toBe(
      '3 JTTY contacts were not sent: TQSL does not know the JTTY mode yet and would drop them, so they were held back. They stay marked as not uploaded to LoTW.',
    )
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
  ] as const)('%s: its own sentence, with the count and the names it must keep', (lang, catalog) => {
    installCatalog(lang, catalog)
    setLocale(lang)
    const one = lotwSkipNote({ dispatched: 0, outcome: 'none', heldMode: 1 })
    const two = lotwSkipNote({ dispatched: 0, outcome: 'none', heldMode: 2 })
    for (const said of [one, two]) {
      expect(said, `${lang}: translated, not the English fallback`).not.toMatch(/was not sent|were not sent/)
      for (const name of ['JTTY', 'TQSL', 'LoTW']) expect(said, `${lang}: keeps ${name}`).toContain(name)
    }
    expect(one).toContain('1')
    expect(two).toContain('2')
  })

  it('rides after the changed-while-signing sentence, and is absent when nothing was held', () => {
    const both = lotwSkipNote({ dispatched: 5, outcome: 'pending', skippedEdited: 1, heldMode: 1 })
    expect(both).toMatch(/^1 QSO changed while TQSL was signing this upload/)
    expect(both).toMatch(/ 1 JTTY contact was not sent: /)
    expect(lotwSkipNote({ dispatched: 5, outcome: 'pending', heldMode: 0 })).toBeNull()
    // Control: the catalog entry the toast is built from is the one asserted above.
    expect(EN['logbook.lotw.held']).toBeTruthy()
  })
})

describe('the LoTW toast for contacts signed with the Station Location in Settings', () => {
  it('counts a tie between station profiles and a contact no profile fits, and why', () => {
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationTie: 1 })).toBe(
      '1 contact fits two of your station profiles, and they name different TQSL Station Locations, so it was signed with the Station Location in Settings ▸ Logging & Connectors.',
    )
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationUnmatched: 2 })).toBe(
      '2 contacts fit none of your station profiles by their call and grid, so they were signed with the Station Location in Settings ▸ Logging & Connectors.',
    )
    const all = lotwSkipNote({ dispatched: 9, outcome: 'pending', heldMode: 1, locationTie: 2, locationUnmatched: 3 })
    expect(all).toMatch(/^1 JTTY contact was not sent: .* 2 contacts fit two of your station profiles.* 3 contacts fit none of/)
  })

  it('says nothing when every contact was signed with its own profile’s location', () => {
    // Control: a report whose counts are zero, and one from a station older than the counts.
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationTie: 0, locationUnmatched: 0 })).toBeNull()
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending' })).toBeNull()
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: translated, with the count and the names it keeps', (lang, catalog) => {
    installCatalog(lang, catalog)
    setLocale(lang)
    for (const n of [1, 2, 5]) {
      const tie = lotwSkipNote({ dispatched: n, outcome: 'pending', locationTie: n })
      const none = lotwSkipNote({ dispatched: n, outcome: 'pending', locationUnmatched: n })
      for (const said of [tie, none]) {
        expect(said, `${lang}: not the English fallback`).not.toMatch(/station profiles/)
        expect(said).toContain(String(n))
        expect(said).toContain('Station Location')
      }
      expect(tie).toContain('TQSL')
    }
  })
})

describe('the LoTW toast when signing from the ADIF, and when a later TQSL run never ran', () => {
  it('says the contacts were signed from the call and grid they carry, not a Station Location', () => {
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationTie: 1, locationFromAdif: true })).toBe(
      "1 contact fits two of your station profiles, and they name different TQSL Station Locations, so no profile's Station Location was used: it was signed from the call and grid it carries.",
    )
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationUnmatched: 2, locationFromAdif: true })).toBe(
      "2 contacts fit none of your station profiles by their call and grid, so no profile's Station Location was used: they were signed from the call and grid they carry.",
    )
    // Control: with a named Station Location, the Settings sentence as before.
    expect(lotwSkipNote({ dispatched: 3, outcome: 'pending', locationTie: 1, locationFromAdif: false })).toMatch(
      /signed with the Station Location in Settings/,
    )
  })

  it('counts the contacts a failed later run left unsent, with why, beside what went', () => {
    expect(
      lotwSkipNote({ dispatched: 2, outcome: 'pending', unsentAfterError: 3, unsentError: 'TQSL did not answer in time' }),
    ).toBe(
      '3 contacts were not sent (TQSL did not answer in time). The contacts signed before that went to LoTW. They stay marked as not uploaded to LoTW and go with the next upload.',
    )
    // Control: nothing left unsent says nothing.
    expect(lotwSkipNote({ dispatched: 2, outcome: 'pending', unsentAfterError: 0 })).toBeNull()
  })

  it('says a later run whose mark was not confirmed on disk went to LoTW, apart from what was not sent', () => {
    expect(
      lotwSkipNote({
        dispatched: 4,
        outcome: 'pending',
        unconfirmedAfterError: 2,
        unsentAfterError: 1,
        unsentError: 'database is locked',
      }),
    ).toBe(
      '2 contacts went to LoTW, but Nexus could not confirm their upload mark was saved to disk (database is locked). Any not saved go again with the next upload; LoTW ignores a contact it already has. ' +
        '1 contact was not sent (database is locked). The contacts signed before that went to LoTW. It stays marked as not uploaded to LoTW and goes with the next upload.',
    )
    // The last run: nothing left unsent, so no "not sent" sentence.
    expect(
      lotwSkipNote({ dispatched: 4, outcome: 'pending', unconfirmedAfterError: 2, unsentError: 'database is locked' }),
    ).not.toMatch(/not sent/)
    // Control: confirmed says nothing.
    expect(lotwSkipNote({ dispatched: 4, outcome: 'pending', unconfirmedAfterError: 0 })).toBeNull()
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: translated, with the count, the names and the error it keeps', (lang, catalog) => {
    installCatalog(lang, catalog)
    setLocale(lang)
    for (const n of [1, 2, 5]) {
      const tie = lotwSkipNote({ dispatched: n, outcome: 'pending', locationTie: n, locationFromAdif: true })
      const none = lotwSkipNote({ dispatched: n, outcome: 'pending', locationUnmatched: n, locationFromAdif: true })
      const left = lotwSkipNote({ dispatched: n, outcome: 'pending', unsentAfterError: n, unsentError: 'TQSL ERR' })
      const unsure = lotwSkipNote({ dispatched: n, outcome: 'pending', unconfirmedAfterError: n, unsentError: 'DISK ERR' })
      expect(unsure, `${lang}: not the English fallback`).not.toMatch(/could not confirm/)
      expect(unsure).toContain(String(n))
      expect(unsure).toContain('DISK ERR')
      expect(unsure).toContain('LoTW')
      for (const said of [tie, none, left]) {
        expect(said, `${lang}: not the English fallback`).not.toMatch(/station profiles|were not sent|was not sent/)
        expect(said).toContain(String(n))
      }
      for (const said of [tie, none]) expect(said).toContain('Station Location')
      expect(tie).toContain('TQSL')
      expect(left).toContain('TQSL ERR')
      expect(left).toContain('LoTW')
    }
  })
})
