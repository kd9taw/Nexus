// THE FSK SERIAL PORT BOX POINTS AT THE RIGHT PORT, AND SHOWS NO SAMPLE.
//
// Its empty box read "e.g. the FTDX10's USB Enhanced COM". An FT-991A operator on Linux read that
// grey text as a value already filled in that would not change, and it named the wrong port as
// well: on Yaesu's two-port USB radios (FT-991A, FTDX10) the Enhanced COM port is CAT, and the
// DTR/RTS lines that key FSK are on the Standard COM port, the second of the pair (/dev/ttyUSB1
// on Linux when the radio is the only USB serial device). An empty box means "use the CAT port",
// exactly as the PTT port box beside CAT says, so it says that; the hint names the Standard port.
import { describe, it, expect } from 'vitest'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { IT } from './it'
import { JA } from './ja'
import { RU } from './ru'
import { ZH } from './zh'
import { ZH_HANT } from './zh-hant'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = {
  en: EN, de: DE, es: ES, fr: FR, it: IT, ja: JA, ru: RU, zh: ZH, 'zh-hant': ZH_HANT,
}

describe('the FSK serial port box', () => {
  it.each(Object.entries(CATALOGS))('%s: an empty box says it uses the CAT port, as the PTT port box does', (_l, cat) => {
    expect(cat['settings.rtty.fskPort.placeholder']).toBe(cat['settings.rigControl.pttPort.placeholder'])
  })

  it.each(Object.entries(CATALOGS))('%s: the hint sends a two-port Yaesu to its Standard COM port', (_l, cat) => {
    const hint = String(cat['settings.rtty.fskPort.hint'])
    expect(hint).toContain('Standard COM')
    expect(hint).toContain('/dev/ttyUSB1')
  })
})
