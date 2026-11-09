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
import { ZH } from '../i18n/zh'
import { ZH_HANT } from '../i18n/zh-hant'

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

describe('the unconfirmed RTS/DTR holds refusal, in the CAT status', () => {
  const HOLDS_KEY = 'settings.rigControl.hamlibCannotConfirmHolds'
  const HOLDS = EN[HOLDS_KEY]

  it('is the exact sentence the backend sends', () => {
    const rust = read('../../../crates/tempo-audio/src/rigctld_proc.rs')
    const m = rust.match(/pub const HAMLIB_CANNOT_CONFIRM_HOLDS: &str =\s*"([^"]*)";/)
    expect(m?.[1]).toBe(HOLDS)
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
    ['zh', ZH],
    ['zh-hant', ZH_HANT],
  ] as const)('%s: renders translated, keeps Hamlib, RTS and DTR, and what leads it', (lang, cat) => {
    const text = String((cat as Record<string, unknown>)[HOLDS_KEY])
    expect(text).not.toBe(HOLDS)
    for (const word of ['Hamlib', 'RTS', 'DTR', '4.6']) expect(text).toContain(word)
    installCatalog(lang, cat)
    setLocale(lang)
    expect(catDetailText(HOLDS)).toBe(text)
    const lead = 'the CAT helper (rigctld) stopped and was restarted. '
    expect(catDetailText(lead + HOLDS)).toBe(lead + text)
  })

  it.each([
    ['ru', '../../../translations/ru/nexus-ru-translation.csv'],
    ['zh', '../../../translations/zh/nexus-zh-translation.csv'],
    ['zh-hant', '../../../translations/zh-hant/nexus-zh-hant-translation.csv'],
  ])("the %s kit's row has the English the app shows", (_, file) => {
    expect(read(file)).toContain(`"${HOLDS_KEY}","${HOLDS.replace(/"/g, '""')}"`)
  })
})

describe('NET rigctl given a serial port, in the CAT status', () => {
  const NET_KEY = 'settings.rigControl.netRigctl.notConnected'
  const sent = (port: string) => EN[NET_KEY].split('{{port}}').join(port)

  it('is the exact sentence the backend sends', () => {
    const rust = read('../../../crates/tempo-audio/src/rigctld_proc.rs')
    const m = rust.match(/pub const NET_RIGCTL_ON_A_SERIAL_PORT: &str =\s*"([^"]*)";/)
    expect(m?.[1]).toBe(EN[NET_KEY])
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
  ] as const)('%s: is translated, keeps what surrounds it, and carries the port across', (lang, cat) => {
    const text = String((cat as Record<string, unknown>)[NET_KEY])
    expect(text).not.toBe(EN[NET_KEY])
    installCatalog(lang, cat)
    setLocale(lang)
    const shown = catDetailText(`the port was reopened. ${sent('COM4')} Hamlib said: COM4 No such file`)
    expect(shown).toBe(
      `the port was reopened. ${text.split('{{port}}').join('COM4')} Hamlib said: COM4 No such file`,
    )
  })

  it('takes a Unix device path too, and leaves a different sentence alone', () => {
    installCatalog('ru', RU)
    setLocale('ru')
    expect(catDetailText(sent('/dev/ttyUSB0'))).toContain('/dev/ttyUSB0')
    expect(catDetailText(sent('/dev/ttyUSB0'))).not.toContain('Rig Model is NET rigctl')
    expect(catDetailText('rigctld is not reachable on 127.0.0.1:1.')).toBe('rigctld is not reachable on 127.0.0.1:1.')
  })
})

describe('transmit turned off by a silent CAT link, in the CAT status', () => {
  const OFF_KEY = 'settings.rigControl.catSilentTxOff'

  it('is the exact sentence the backend sends', () => {
    const rust = read('../../../crates/tempo-app/src/engine.rs')
    const m = rust.match(/pub const CAT_SILENT_TX_OFF: &str =\s*"([^"]*)";/)
    expect(m?.[1]).toBe(EN[OFF_KEY])
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
    ['zh', ZH],
    ['zh-hant', ZH_HANT],
  ] as const)('%s: is translated, names TX On as it is on the button, and keeps what follows it', (lang, cat) => {
    const text = String((cat as Record<string, unknown>)[OFF_KEY])
    expect(text).not.toBe(EN[OFF_KEY])
    expect(text).toContain('TX On')
    installCatalog(lang, cat)
    setLocale(lang)
    const rest = '14.0740 MHz CAT link unreachable (1/3)'
    expect(catDetailText(`${EN[OFF_KEY]} ${rest}`)).toBe(`${text} ${rest}`)
  })
})

describe('transmit still off once CAT is back, in the CAT status', () => {
  const KEY = 'settings.rigControl.catBackTxOff'

  it('is the exact sentence the backend sends', () => {
    const rust = read('../../../crates/tempo-app/src/engine.rs')
    const m = rust.match(/pub const CAT_BACK_TX_OFF: &str =\s*"([^"]*)";/)
    expect(m?.[1]).toBe(EN[KEY])
  })

  it.each([
    ['de', DE],
    ['es', ES],
    ['fr', FR],
    ['ja', JA],
    ['ru', RU],
    ['zh', ZH],
    ['zh-hant', ZH_HANT],
  ] as const)('%s: is translated and names TX On as it is on the button', (lang, cat) => {
    const text = String((cat as Record<string, unknown>)[KEY])
    expect(text).not.toBe(EN[KEY])
    expect(text).toContain('TX On')
    installCatalog(lang, cat)
    setLocale(lang)
    expect(catDetailText(`${EN[KEY]} 14.074 MHz`)).toBe(`${text} 14.074 MHz`)
  })
})

describe('the NET rigctl sentence and a transmit-off sentence in one status', () => {
  const NET_KEY = 'settings.rigControl.netRigctl.notConnected'
  const sent = (port: string) => EN[NET_KEY].split('{{port}}').join(port)

  it.each([
    ['settings.rigControl.catSilentTxOff'],
    ['settings.rigControl.catBackTxOff'],
  ] as const)('both are translated: %s', (offKey) => {
    installCatalog('ru', RU)
    setLocale('ru')
    const net = String(RU[NET_KEY]).split('{{port}}').join('COM4')
    expect(catDetailText(`${sent('COM4')} ${EN[offKey]}`)).toBe(`${net} ${String(RU[offKey])}`)
  })
})
