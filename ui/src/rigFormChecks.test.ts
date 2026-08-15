// Pre-save rig checks. Each case here is a mistake that saves silently today and then presents as
// broken hardware — the symptom always appears far from the cause, which is why they are worth
// catching at the one moment the operator is looking at the setting.
import { describe, it, expect } from 'vitest'
import { checkRigForm, blocks, type RigFormFacts } from './rigFormChecks'

const PORTS = ['/dev/cu.usbserial-A', '/dev/tty.usbserial-A', 'COM5']

const form = (over: Partial<RigFormFacts> = {}): RigFormFacts => ({
  serialPort: '/dev/cu.usbserial-A',
  rigConn: 'serial',
  pttMethod: 'cat',
  rigModel: 1049,
  radios: [{ id: 0, name: 'FT-710', serialPort: '/dev/cu.usbserial-A' }],
  activeRadio: 0,
  ...over,
})

describe('checkRigForm', () => {
  it('passes a correct setup silently', () => {
    expect(checkRigForm(form(), PORTS, 0)).toEqual([])
  })

  it('ignores a network rig entirely — it has no serial port to be wrong about', () => {
    const checks = checkRigForm(form({ rigConn: 'network', serialPort: '' }), PORTS, 0)
    expect(checks).toEqual([])
  })

  it('blocks a rig model with no port — CAT cannot work without one', () => {
    const checks = checkRigForm(form({ serialPort: '' }), PORTS, 0)
    expect(blocks(checks)).toBe(true)
    expect(checks[0].message).toMatch(/no serial port/i)
  })

  it('allows no port when there is no rig model either — that is VOX, not a mistake', () => {
    const checks = checkRigForm(
      form({ serialPort: '', rigModel: 0, pttMethod: 'vox' }),
      PORTS,
      0,
    )
    expect(checks).toEqual([])
  })

  it('blocks a /dev/tty.* port, which hangs on carrier instead of failing', () => {
    const checks = checkRigForm(form({ serialPort: '/dev/tty.usbserial-A' }), PORTS, 0)
    expect(blocks(checks)).toBe(true)
    expect(checks.some((c) => /dial-in device/.test(c.message))).toBe(true)
  })

  it('blocks two radios sharing one CAT port', () => {
    const checks = checkRigForm(
      form({
        radios: [
          { id: 0, name: 'FT-710', serialPort: '/dev/cu.usbserial-A' },
          { id: 1, name: 'FTX-1', serialPort: '/dev/cu.usbserial-A' },
        ],
      }),
      PORTS,
      1, // editing the FTX-1: the clash is with the OTHER radio
    )
    expect(blocks(checks)).toBe(true)
    expect(checks.some((c) => /FT-710 already uses/.test(c.message))).toBe(true)
  })

  it('does not accuse a radio of clashing with itself', () => {
    // The radio being edited already holds this port — that is the normal case, not a clash.
    expect(checkRigForm(form(), PORTS, 0)).toEqual([])
  })

  it('blocks PTT over CAT with no rig model — nothing to send the keying command to', () => {
    const checks = checkRigForm(form({ rigModel: 0, pttMethod: 'cat' }), PORTS, 0)
    expect(blocks(checks)).toBe(true)
    expect(checks.some((c) => /PTT is set to CAT/.test(c.message))).toBe(true)
  })

  it('warns, but does NOT block, when the port is simply not plugged in right now', () => {
    const checks = checkRigForm(form({ serialPort: 'COM9' }), PORTS, 0)
    expect(blocks(checks)).toBe(false)
    expect(checks[0].level).toBe('warning')
    expect(checks[0].message).toMatch(/not connected right now/)
  })
})
