// "CI-V address" (#528): the address of a radio whose CI-V address was changed on its menu. The
// form refuses what the engine would not talk on, and the two read the text the same way: this
// reads the engine's own table of cases (`CIV_ADDR_CASES`, crates/tempo-app/src/settings.rs) and
// fails if `parseCivAddr` disagrees with any of them.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { checkRigForm, blocks, civAddrFieldShown, parseCivAddr, type RigFormFacts } from './rigFormChecks'

const RUST = fileURLToPath(new URL('../../crates/tempo-app/src/settings.rs', import.meta.url))

/** The engine's cases: [text, null | address | 'bad']. */
function rustCases(): [string, number | null | 'bad'][] {
  const src = readFileSync(RUST, 'utf8')
  const table = src.slice(src.indexOf('const CIV_ADDR_CASES'), src.indexOf('];', src.indexOf('const CIV_ADDR_CASES')))
  return [...table.matchAll(/\("([^"]*)", (Ok\(None\)|Ok\(Some\(0x([0-9A-Fa-f]+)\)\)|Err\(NotACivAddr\))\)/g)].map((m) => [
    // A Rust `\u{..}` escape is the character it names.
    m[1].replace(/\\u\{([0-9A-Fa-f]+)\}/g, (_, hex: string) => String.fromCodePoint(parseInt(hex, 16))),
    m[2] === 'Ok(None)' ? null : m[3] != null ? parseInt(m[3], 16) : 'bad',
  ])
}

const form = (over: Partial<RigFormFacts> = {}): RigFormFacts => ({
  serialPort: '/dev/cu.usbserial-A',
  rigConn: 'serial',
  pttMethod: 'cat',
  rigModel: 3073,
  ...over,
})

describe('the CI-V address', () => {
  it('is read as the engine reads it, case for case', () => {
    const cases = rustCases()
    expect(cases.length, 'control: the Rust table was actually parsed').toBeGreaterThan(10)
    expect(cases.some(([text]) => text.startsWith('\uFEFF')), 'control: an escape was decoded').toBe(true)
    for (const [text, want] of cases) expect(parseCivAddr(text), JSON.stringify(text)).toBe(want)
  })

  it('a bad address stops the Save; blank and a radio menu value do not', () => {
    for (const bad of ['E0', '148', '0x', 'zz']) {
      const checks = checkRigForm(form({ civAddr: bad }), ['/dev/cu.usbserial-A'], [])
      expect(blocks(checks), bad).toBe(true)
      expect(checks.some((c) => /CI-V address/.test(c.message)), bad).toBe(true)
    }
    for (const ok of ['', '94', '0x96', 'A2h']) {
      expect(blocks(checkRigForm(form({ civAddr: ok }), ['/dev/cu.usbserial-A'], [])), ok).toBe(false)
    }
  })

  it('is offered for a Hamlib Icom on a link that opens its CI-V, and its value blocks nothing elsewhere', () => {
    expect(civAddrFieldShown(3073, 'serial')).toBe(true)
    expect(civAddrFieldShown(3087, 'serial'), 'a Xiegu Hamlib drives as an Icom').toBe(true)
    expect(civAddrFieldShown(3073, 'network')).toBe(true)
    for (const conn of ['icomlan', 'omnirig', 'tci']) expect(civAddrFieldShown(3073, conn), conn).toBe(false)
    expect(civAddrFieldShown(1035, 'serial'), 'a Yaesu has no CI-V').toBe(false)
    expect(civAddrFieldShown(30001, 'serial'), 'an Icom marine radio is another backend').toBe(false)
    // A value left from an Icom set-up does not refuse the Save of the Yaesu the radio now is.
    expect(blocks(checkRigForm(form({ rigModel: 1035, civAddr: 'zz' }), ['/dev/cu.usbserial-A'], []))).toBe(false)
  })
})
