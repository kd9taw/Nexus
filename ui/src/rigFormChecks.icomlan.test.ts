// The Icom network connection, as the Settings form checks it.
//
// Its model list must be the SETTINGS' list, not a second opinion: the daemon decides whether it
// can serve a radio from `ICOM_LAN_RIGS` (crates/tempo-app/src/settings.rs), and a screen that
// offered the connection on a different set would offer it where nothing can connect, or hide it
// where it works. This reads the Rust source and fails if the two drift.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { blocks, checkRigForm, ICOM_LAN_MODELS, isIpv4, nativeCivBlockedReason } from './rigFormChecks'

const RUST = fileURLToPath(new URL('../../crates/tempo-app/src/settings.rs', import.meta.url))

describe('the Icom network model list', () => {
  it('matches ICOM_LAN_RIGS in the settings', () => {
    const src = readFileSync(RUST, 'utf8')
    const line = src.split('\n').find((l) => l.startsWith('pub const ICOM_LAN_RIGS'))
    expect(line, 'control: the Rust constant was found').toBeTruthy()
    const models = [...(line ?? '').slice(line!.indexOf('=')).matchAll(/\d+/g)]
      .map((m) => Number(m[0]))
      .sort((a, b) => a - b)
    expect(models.length, 'control: the Rust list was actually parsed').toBe(6)
    expect([...ICOM_LAN_MODELS].sort((a, b) => a - b)).toEqual(models)
  })
})

const lan = (over: Partial<Parameters<typeof checkRigForm>[0]> = {}) => ({
  serialPort: '',
  rigConn: 'icomlan',
  pttMethod: 'cat',
  rigModel: 3092,
  icomLanHost: '192.0.2.10',
  icomLanUser: 'test-user',
  ...over,
})

describe('the pre-save checks on the Icom network connection', () => {
  it('pass a complete IC-7760 with its password saved, and ask for no serial port', () => {
    expect(checkRigForm(lan(), [], [0], undefined, undefined, true)).toEqual([])
  })

  it('refuse a radio with no network server, and an address that is not IPv4', () => {
    const noLan = checkRigForm(lan({ rigModel: 3073 }), [], [0], undefined, undefined, true)
    expect(noLan.map((c) => c.level)).toEqual(['error'])
    expect(noLan[0].message).toMatch(/IC-7610, IC-9700, IC-705, IC-905, IC-7760 and IC-7300MK2/)
    for (const host of ['', 'radio.local', '192.0.2', '192.0.2.300']) {
      const c = checkRigForm(lan({ icomLanHost: host }), [], [0], undefined, undefined, true)
      expect(blocks(c), host).toBe(true)
    }
    // The example address reaches the message as data, filled in.
    const [bad] = checkRigForm(lan({ icomLanHost: 'radio.local' }), [], [0], undefined, undefined, true)
    expect(bad.message).toBe("Enter the radio's IPv4 address, for example 192.168.1.50.")
  })

  it('only warn about a missing network user or password: the operator may save first', () => {
    const c = checkRigForm(lan({ icomLanUser: '' }), [], [0], undefined, undefined, false)
    expect(c.map((x) => x.level)).toEqual(['warning', 'warning'])
    expect(blocks(c)).toBe(false)
    // Not known (still asking the keychain) says nothing.
    expect(checkRigForm(lan(), [], [0], undefined, undefined, null)).toEqual([])
  })

  it('reads an IPv4 address the way the protocol needs one', () => {
    expect(isIpv4('198.51.100.50')).toBe(true)
    expect(isIpv4(' 10.0.0.1 ')).toBe(true)
    expect(isIpv4('256.1.1.1')).toBe(false)
    expect(isIpv4('fe80::1')).toBe(false)
  })

  it('says the native CI-V toggle is always on there, rather than offering it', () => {
    expect(nativeCivBlockedReason(3081, 'icomlan')).toBe('icomlan')
    expect(nativeCivBlockedReason(3081, 'serial')).toBeNull() // the control: USB still offers it
  })
})
