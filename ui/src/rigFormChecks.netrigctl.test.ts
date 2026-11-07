// NET rigctl (Hamlib model 2) reaches a radio through a rigctld over TCP and opens no serial port.
// Given a COM port, it looks the name up as a machine on the network and fails with
// "COM4 No such file or directory". Settings says so when the two are picked, as a warning, because
// the same setting works while another program's rigctld answers on rigctld TCP Port.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { blocks, checkRigForm, looksLikeSerialPort, netRigctlSerialPort, type RigFormFacts } from './rigFormChecks'

const table = JSON.parse(
  readFileSync(
    fileURLToPath(new URL('../../crates/tempo-app/tests/fixtures/serial_port_names.json', import.meta.url)),
    'utf8',
  ),
) as { ports: string[]; hosts: string[] }

const form = (o: Partial<RigFormFacts>): RigFormFacts => ({
  serialPort: '',
  rigConn: 'serial',
  pttMethod: 'cat',
  rigModel: 2,
  ...o,
})
const PORTLESS = [0, 1, 2, 4]

describe('NET rigctl given a serial port', () => {
  it.each(table.ports)('reads %j as a serial port, as the backend does', (name) => {
    expect(looksLikeSerialPort(name)).toBe(true)
  })
  it.each(table.hosts)('reads %j as a host, as the backend does', (name) => {
    expect(looksLikeSerialPort(name)).toBe(false)
  })

  it('warns on the COM port, on either field, and the save still goes through', () => {
    for (const f of [form({ serialPort: 'COM4' }), form({ rigConn: 'network', rigAddr: 'COM4' })]) {
      const checks = checkRigForm(f, ['COM4'], PORTLESS)
      expect(checks.filter((c) => c.message.includes('COM4'))).toHaveLength(1)
      expect(checks[0].level).toBe('warning')
      expect(blocks(checks)).toBe(false)
    }
  })

  it('says nothing about a valid address, an odd host, or a real radio on its port', () => {
    for (const f of [
      form({ rigConn: 'network', rigAddr: '127.0.0.1:4532' }),
      form({ rigConn: 'network', rigAddr: 'COM4:4532' }),
      form({ rigConn: 'network', rigAddr: 'com4.local' }),
      form({ rigConn: 'network', rigAddr: 'shack-pi:4532', serialPort: 'COM4' }),
      form({}),
      form({ rigModel: 1001, serialPort: 'COM4' }),
      form({ rigConn: 'omnirig', serialPort: 'COM4' }),
      form({ rigConn: 'tci', serialPort: 'COM4' }),
    ]) {
      expect(netRigctlSerialPort(f)).toBeNull()
      expect(checkRigForm(f, ['COM4'], PORTLESS).map((c) => c.message).join()).not.toMatch(/NET rigctl/)
    }
  })
})
