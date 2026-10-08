// THE CONNECTION HINT SENDS EACH ICOM LAN BRIDGE TO ITS OWN SETUP. wfview serves rigctld, so Nexus reaches it with
// Network and Rig Model NET rigctl. RS-BA1 serves no rigctld: it gives the PC a virtual COM port, so its users choose
// Serial, their Icom's own model and that COM port. The hint sent both to NET rigctl ("run wfview (or RS-BA1) … NET
// rigctl"), and an RS-BA1 user who followed it found nothing listening and no reason why.
//
// Read off all five catalogs and the pt-BR kit's row: the clause naming RS-BA1 never names NET rigctl, and names the
// catalog's own word for the Serial choice and a COM port; the clause naming wfview still names NET rigctl.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = { en: EN, de: DE, es: ES, fr: FR, ja: JA }
const HINT = 'settings.rigControl.conn.hint'

/** The clauses of `text` that name `who`: its sentences, and the halves of a sentence a semicolon joins. */
const naming = (text: string, who: string) => text.split(/[.;。；]/).filter((c) => c.includes(who))

describe('the Connection hint', () => {
  it.each(Object.entries(CATALOGS))('%s: RS-BA1 goes to Serial and its COM port, never to NET rigctl', (_lang, cat) => {
    const hint = String(cat[HINT])
    // The word the Connection picker shows for Serial in this language: "Serial (USB / COM port)" → "Serial".
    const serial = String(cat['settings.rigControl.conn.serial']).split(/[\s（(]/)[0]
    const rsba1 = naming(hint, 'RS-BA1')
    expect(rsba1.length, hint).toBeGreaterThan(0)
    for (const clause of rsba1) {
      expect(clause, 'RS-BA1 serves no rigctld').not.toContain('NET rigctl')
      expect(clause, 'the Serial choice').toContain(serial)
      expect(clause, 'the COM port it makes').toContain('COM')
    }
  })

  it.each(Object.entries(CATALOGS))('%s: wfview still goes to NET rigctl', (_lang, cat) => {
    const wfview = naming(String(cat[HINT]), 'wfview')
    expect(wfview.length).toBeGreaterThan(0)
    expect(wfview.some((clause) => clause.includes('NET rigctl'))).toBe(true)
  })

  it("the pt-BR kit's row has the English as it now reads", () => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    expect(csv).toContain(`"${HINT}","${String(EN[HINT])}"`)
  })
})
