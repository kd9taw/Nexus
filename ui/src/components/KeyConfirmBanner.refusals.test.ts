// The station refuses some answers in the notice for saved keys (the address changed since the
// notice asked; the radio is no longer on the roster; the key was kept only because removing it
// failed). Each refusal reaches the operator in their language: the words the station sends are
// its own, mapped here to the catalog's. Any other refusal passes through as it came.
import { afterEach, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { answerRefusal, ICOM_PASSWORD_OF_NO_RADIO, KEY_ADDRESS_CHANGED, KEY_NOT_REMOVED } from './KeyConfirmBanner'
import { installCatalog, setLocale } from '../i18n'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { RU } from '../i18n/ru'

const REFUSALS = [
  ['KEY_ADDRESS_CHANGED', KEY_ADDRESS_CHANGED, 'keyConfirm.refused.addressChanged'],
  ['ICOM_PASSWORD_OF_NO_RADIO', ICOM_PASSWORD_OF_NO_RADIO, 'keyConfirm.refused.noRadio'],
  ['KEY_NOT_REMOVED', KEY_NOT_REMOVED, 'keyConfirm.refused.notRemoved'],
] as const

/** A `const NAME: &str = "…";` of the shell, its line continuations joined as Rust joins them. */
function rustConst(source: string, name: string): string | undefined {
  const found = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(source)
  return found?.[1].replace(/\\\n\s*/g, '')
}

afterEach(() => setLocale('en'))

describe("the notice's refused answers", () => {
  // The suite runs from ui/.
  const shell = readFileSync(resolve(process.cwd(), '../src-tauri/src/lib.rs'), 'utf8')

  it.each(REFUSALS)("%s is the station's words, word for word", (name, text, key) => {
    expect(rustConst(shell, name)).toBe(text)
    expect(EN[key]).toBe(text)
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: has each in its own words', (_lang, cat) => {
    for (const [, , key] of REFUSALS) {
      const text = (cat as Readonly<Record<string, unknown>>)[key]
      expect(typeof text === 'string' && text.trim().length > 0 && text !== EN[key], `${key}: ${String(text)}`).toBe(
        true,
      )
    }
  })

  it("is shown in the operator's language", () => {
    installCatalog('de', DE)
    setLocale('de')
    for (const [, text, key] of REFUSALS) {
      expect(answerRefusal(text)).toBe(DE[key])
      expect(answerRefusal(new Error(text))).toBe(DE[key])
    }
  })

  it('CONTROL: any other refusal passes through as it came', () => {
    installCatalog('de', DE)
    setLocale('de')
    expect(answerRefusal("couldn't remove it from the system keychain: locked")).toBe(
      "couldn't remove it from the system keychain: locked",
    )
  })
})
