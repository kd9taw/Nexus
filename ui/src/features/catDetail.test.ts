// A launch Nexus refuses itself — Serial handshake = Hardware on a port where RTS keys the
// transmitter — reaches the CAT status and Test CAT as one English sentence from the backend. It
// is shown in the operator's language; every other detail passes through as sent. The sentence is
// one string in three places (the Rust constant, the English catalog, the Russian kit's English
// column), and this holds them together.
import { describe, it, expect, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { catDetailText } from './catDetail'
import { EN, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { RU } from '../i18n/ru'

const KEY = 'settings.rigControl.serialHandshake.rtsKeys'
const SENTENCE = EN[KEY]
const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')

afterEach(() => setLocale('en'))

describe('the hardware-handshake refusal in the CAT status', () => {
  it('is the exact sentence the backend sends', () => {
    const rust = read('../../../crates/tempo-audio/src/rigctld_proc.rs')
    const m = rust.match(/pub const HARDWARE_HANDSHAKE_RTS_KEYS: &str =\s*"([^"]*)";/)
    expect(m?.[1]).toBe(SENTENCE)
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: is translated, and keeps RTS as RTS', (lang, cat) => {
    const text = (cat as Record<string, unknown>)[KEY]
    expect(typeof text).toBe('string')
    expect(text).not.toBe(SENTENCE)
    expect(String(text)).toContain('RTS')
    installCatalog(lang, cat)
    setLocale(lang)
    expect(catDetailText(SENTENCE)).toBe(text)
  })

  it('keeps whatever leads it, and leaves every other detail as sent', () => {
    installCatalog('ru', RU)
    setLocale('ru')
    const lead = 'the CAT helper (rigctld) stopped and was restarted. '
    expect(catDetailText(lead + SENTENCE)).toBe(lead + String(RU[KEY]))
    const other = 'Could not launch rigctld (Hamlib): permission denied'
    expect(catDetailText(other)).toBe(other)
    setLocale('en')
    expect(catDetailText(SENTENCE)).toBe(SENTENCE)
  })

  it("the Russian kit's row has the English the app shows", () => {
    const csv = read('../../../translations/ru/nexus-ru-translation.csv')
    expect(csv).toContain(`"${KEY}","${SENTENCE.replace(/"/g, '""')}"`)
  })
})
