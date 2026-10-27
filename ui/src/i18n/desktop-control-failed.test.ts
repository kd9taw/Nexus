// A failed attenuator, preamp or monitor-level call on the desktop says which control Nexus could not
// set. It used to show the Remote page's sentence about station control permission, which a desktop
// operator holds by definition. Each sentence is in every catalog, keeps the rig's own plate as
// {{plate}}, names no station control, and the pt-BR kit's rows have the English the app shows.
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
const KEYS = ['phone.chain.failed', 'phone.monitor.failed'] as const

describe('the desktop’s own control failures', () => {
  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => KEYS.map((key) => [lang, key, cat] as const)))(
    '%s %s: is there, in its own words',
    (_lang, key, cat) => {
      const text = cat[key]
      expect(typeof text === 'string' && text.trim().length > 0, String(text)).toBe(true)
      expect(String(text).includes('{{plate}}'), String(text)).toBe(key === 'phone.chain.failed')
    },
  )

  it('the English says what failed, plainly', () => {
    expect([EN['phone.chain.failed'], EN['phone.monitor.failed']]).toEqual([
      'Nexus could not set {{plate}}.',
      'Nexus could not set the monitor level.',
    ])
  })

  it.each(KEYS)("the pt-BR kit's %s row has the English the app shows", (key) => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    expect(csv).toContain(`"${key}","${String(CATALOGS.en[key])}"`)
  })
})
