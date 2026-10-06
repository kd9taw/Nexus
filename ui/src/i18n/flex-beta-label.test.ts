// THE FLEX NATIVE TOGGLES ARE LABELLED BETA. Native DAX audio first (the operator, 2026-10-04: the label
// follows the earlier Beta ruling): its hint said "Beta, unverified on hardware" and the CHANGELOG called it
// native DAX audio (Beta), but the label still read "(early access)". Then the panadapter (the operator,
// 2026-10-05, "Say Beta, with hint and changelog": both native parts are Beta opt-in): its label, its hint
// and the SWR cutoff's note that names it. Each carries the marker the Flex native client carries, in every
// catalog, and the pt-BR kit's rows have the new English.
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
/** The Beta word inside each catalog's marker, as its hints write it. */
const BETA: Record<string, string> = { en: 'Beta', de: 'Beta', es: 'Beta', fr: 'Bêta', ja: 'ベータ' }
const LABELS = ['settings.rigControl.flexAudio.label', 'settings.rigControl.flexPan.label'] as const

describe('the Flex native toggles', () => {
  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => LABELS.map((key) => [lang, key, cat] as const)))(
    "%s %s: carries the Flex native client's marker",
    (_lang, key, cat) => {
      const client = String(cat['settings.rigControl.flexClient.label'])
      const label = String(cat[key])
      const marker = client.match(MARKER)?.[0]
      expect(marker, client).toBeTruthy()
      expect(label.endsWith(marker as string), `${label} / ${marker}`).toBe(true)
    },
  )

  it.each(Object.entries(CATALOGS))("%s: the panadapter's hint says Beta", (lang, cat) => {
    expect(String(cat['settings.rigControl.flexPan.hint'])).toContain(BETA[lang])
  })

  it.each(Object.entries(CATALOGS))("%s: the SWR cutoff's note names the panadapter by its label", (_lang, cat) => {
    const label = String(cat['settings.rigControl.flexPan.label'])
    expect(String(cat['settings.transmit.swrStop.noMeterStream'])).toContain(label)
  })

  it.each([...LABELS, 'settings.rigControl.flexPan.hint', 'settings.transmit.swrStop.noMeterStream'])(
    "the pt-BR kit's %s row has the English the app shows",
    (key) => {
      const csv = readFileSync(
        fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
        'utf8',
      )
      const english = String(CATALOGS.en[key]).replace(/"/g, '""')
      expect(csv).toContain(`"${key}","${english}"`)
    },
  )
})
