// @vitest-environment jsdom
// A settings restore while the radio is keyed is refused by the station (the restore's hold can
// wait on the system keychain, and the radio loop needs that hold to end the over). The refusal
// reaches the operator in their language: the words the station sends are the engine's own,
// mapped here to the catalog's. Any other refusal passes through as it came.
import { afterEach, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { importSettingsBundle } from './api'
import { RESTORE_ON_AIR } from './features/restoreWords'
import { installCatalog, setLocale } from './i18n'
import { EN } from './i18n/en'
import { DE } from './i18n/de'
import { ES } from './i18n/es'
import { FR } from './i18n/fr'
import { JA } from './i18n/ja'
import { RU } from './i18n/ru'

const KEY = 'settings.backup.restore.onAir'

function refusing(answer: string) {
  window.__TAURI_INTERNALS__ = {
    invoke: (async () => {
      throw answer
    }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'],
  }
}

afterEach(() => {
  delete window.__TAURI_INTERNALS__
  setLocale('en')
})

describe('a restore refused while the radio is keyed', () => {
  it("says the engine's reason, word for word", () => {
    // The suite runs from ui/.
    const engine = readFileSync(resolve(process.cwd(), '../crates/tempo-app/src/engine.rs'), 'utf8')
    expect(engine).toContain(`pub const RESTORE_ON_AIR: &str = "${RESTORE_ON_AIR}";`)
    expect(EN[KEY]).toBe(RESTORE_ON_AIR)
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: has it in its own words', (_lang, cat) => {
    const text = (cat as Readonly<Record<string, unknown>>)[KEY]
    expect(typeof text === 'string' && text.trim().length > 0 && text !== EN[KEY], String(text)).toBe(true)
  })

  it("is shown in the operator's language", async () => {
    installCatalog('de', DE)
    setLocale('de')
    refusing(RESTORE_ON_AIR)
    await expect(importSettingsBundle('{}')).rejects.toBe(DE[KEY])
  })

  it('CONTROL: any other refusal passes through as it came', async () => {
    installCatalog('de', DE)
    setLocale('de')
    refusing('That file is not a Nexus settings backup.')
    await expect(importSettingsBundle('{}')).rejects.toBe('That file is not a Nexus settings backup.')
  })
})
