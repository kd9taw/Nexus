// The LoTW upload toast for contacts the station HELD BACK because TQSL does not know their mode
// yet (a JTTY contact, written `MODE=MFSK SUBMODE=JTTY`): the station counts them (`heldMode`) and
// its connection log says why; this is the same sentence on the screen, in every catalog.
import { describe, it, expect, afterEach } from 'vitest'
import { EN, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
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
