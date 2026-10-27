// THE TCI WORDS SAY WHAT WORKS, read off all five catalogs, the pt-BR kit and the guides.
//
// A program's own transmit audio, spots from programs, a Flex's DAX audio for programs and Nexus's
// spots on an SDR program's panadapter all work, so no string or guide may still call them not
// ready. CW through an SDR program's own keyer is still not driven, and the TCI connection's hint
// still says so: that sentence is the control that each catalog's "not ready" words are found at
// all. A program's own audio goes out only once it has asked for a voice mode, and WSJT-X and JTDX
// ask for a mode only when it differs from Nexus's, so the transmit switch's hint and the guide name
// the four voice modes and say what those two programs set to USB get. A click on a program's spot
// only tunes, so its tooltip never borrows the cluster spot's "click to work".
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

/** Each catalog's words for "not ready yet", as its TCI hints write them. */
const NOT_READY: Record<string, string> = {
  en: 'Not ready yet',
  de: 'Noch nicht fertig',
  es: 'Aún no está listo',
  fr: 'Pas encore prêt',
  ja: 'まだ対応していない',
}
/** Each catalog's word for a spot, lower case. */
const SPOT: Record<string, string> = { en: 'spot', de: 'spot', es: 'spot', fr: 'spot', ja: 'スポット' }

const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')
/** The sentences of `text`, each ending at its full stop. */
const sentences = (text: string) => text.split(/(?<=[.。])\s*/)
/** `text` with its line wraps undone, so a phrase a guide wraps still reads as one. */
const flat = (text: string) => text.replace(/\s+/g, ' ')

describe('the catalogs', () => {
  it.each(Object.entries(CATALOGS))('%s: no Share by TCI string calls anything not ready', (lang, cat) => {
    const stale = Object.entries(cat)
      .filter(([k, v]) => k.startsWith('settings.transmit.tciServer.') && String(v).includes(NOT_READY[lang]))
      .map(([k]) => k)
    expect(stale).toEqual([])
  })

  it.each(Object.entries(CATALOGS))(
    "%s: the TCI connection's not-ready sentence names CW alone, never the panadapter's spots",
    (lang, cat) => {
      const hint = String(cat['settings.rigControl.tci.hint'])
      const notReady = sentences(hint).filter((s) => s.includes(NOT_READY[lang]))
      // CONTROL: this catalog's "not ready" words are found, in the one sentence about CW.
      expect(notReady, hint).toHaveLength(1)
      expect(notReady[0]).toContain('CW')
      expect(notReady[0].toLowerCase()).not.toContain(SPOT[lang])
    },
  )

  it.each(Object.entries(CATALOGS))(
    '%s: the transmit switch names the voice modes, and what WSJT-X and JTDX set to USB get',
    (_lang, cat) => {
      const hint = String(cat['settings.transmit.tciServer.tx.hint'])
      for (const mode of ['USB', 'LSB', 'AM', 'FM']) expect(hint, mode).toMatch(new RegExp(`\\b${mode}\\b`))
      const programs = sentences(hint).filter((s) => s.includes('WSJT-X'))
      expect(programs, hint).toHaveLength(1)
      expect(programs[0]).toContain('JTDX')
      expect(programs[0]).toContain('USB')
    },
  )

  it.each(Object.entries(CATALOGS))("%s: a program's spot has its own tooltip, never the cluster's", (_lang, cat) => {
    for (const where of ['bandMap', 'bandStrip']) {
      const tune = String(cat[`${where}.spot.tci.title`])
      expect(tune, where).toContain('{{detail}}')
      expect(tune, where).not.toBe(String(cat[`${where}.spot.title`]))
      const from = String(cat[`${where}.spot.tci.from`])
      expect(from, where).toContain('TCI')
      expect(from, where).toContain('{{id}}')
    }
  })
})

describe('the pt-BR kit', () => {
  const csv = read('../../../translations/pt-BR/nexus-ptbr-translation.csv')
  const en = EN as Readonly<Record<string, unknown>>

  it.each([
    'settings.rigControl.tci.hint',
    'bandMap.spot.tci.title',
    'bandMap.spot.tci.from',
    'bandStrip.spot.tci.title',
    'bandStrip.spot.tci.from',
  ])('has a row for %s with the English as it now reads', (key) => {
    expect(typeof en[key], key).toBe('string')
    expect(csv).toContain(`,"${key}","${String(en[key]).replace(/"/g, '""')}"`)
  })

  it('has no row for the note English no longer has; CONTROL: its neighbour is still there', () => {
    expect(csv).not.toContain('"settings.transmit.tciServer.notYet"')
    expect(csv).toContain('"settings.transmit.tciServer.spots.hint"')
  })
})

describe('the guides', () => {
  const server = read('../../../docs/rigs/tci-server.md')
  const client = read('../../../docs/rigs/tci.md')
  const index = read('../../../docs/rigs/index.md')
  const reference = read('../../../docs/guide/settings-reference.md')

  it('Share by TCI calls nothing "not yet"', () => {
    expect(server.match(/^.*\bnot yet\b.*$/gim)).toBeNull()
  })

  it('the TCI connection calls only CW through the program\'s own keyer "not yet"', () => {
    // CONTROL: the one part still to come is found, so the search reads this guide at all.
    expect(client.match(/^.*\bnot yet\b.*$/gim)).toEqual([expect.stringContaining('| CW over TCI |')])
    expect(client).not.toMatch(/when that part arrives/)
  })

  it('the rig index and the settings reference call no TCI part not ready', () => {
    const tci = index.slice(index.indexOf('### TCI (SDR program)'), index.indexOf('See [Share by TCI]'))
    expect(tci).toContain('panadapter')
    expect(flat(tci)).not.toMatch(/not ready/i)
    expect(reference).not.toMatch(/not yet in effect/i)
  })

  it("Share by TCI says a program's audio needs a voice mode, and what WSJT-X and JTDX set to USB get", () => {
    const paragraph = server.split(/\n\s*\n/).map(flat).find((p) => p.includes('voice mode') && p.includes('JTDX'))
    expect(paragraph, 'the paragraph is there').toBeTruthy()
    expect(paragraph).toContain('USB, LSB, AM or FM')
    expect(paragraph).toContain('WSJT-X')
    expect(paragraph).toMatch(/already in USB/)
    expect(paragraph).toContain('refused')
  })
})
