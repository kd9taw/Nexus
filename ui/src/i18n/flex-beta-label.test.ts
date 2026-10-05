// THE FLEX NATIVE DAX AUDIO TOGGLE IS LABELLED BETA (the operator, 2026-10-04: the label follows the
// earlier Beta ruling). Its hint says "Beta, unverified on hardware" and the CHANGELOG calls it native
// DAX audio (Beta), but the label still read "(early access)". It now carries the marker the Flex native
// client it needs carries, in every catalog, and the pt-BR kit's row has the new English. The
// panadapter's label keeps "(early access)": neither its hint nor its CHANGELOG entries call it Beta.
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

/** A label's closing marker in parentheses, either width: "(Beta)", "（ベータ）". */
const MARKER = /[(（][^()（）]*[)）]$/

describe('the Flex native DAX audio label', () => {
  it.each(Object.entries(CATALOGS))('%s: carries the Flex native client\'s marker', (_, cat) => {
    const client = String(cat['settings.rigControl.flexClient.label'])
    const audio = String(cat['settings.rigControl.flexAudio.label'])
    const marker = client.match(MARKER)?.[0]
    expect(marker, client).toBeTruthy()
    expect(audio.endsWith(marker as string), `${audio} / ${marker}`).toBe(true)
  })

  it("the pt-BR kit's row has the English the app shows", () => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    const english = String(CATALOGS.en['settings.rigControl.flexAudio.label'])
    expect(csv).toContain(`"settings.rigControl.flexAudio.label","${english}"`)
  })
})
