// Pre-save rig checks. Each case here is a mistake that saves silently today and then presents as
// broken hardware — the symptom always appears far from the cause, which is why they are worth
// catching at the one moment the operator is looking at the setting.
import { describe, it, expect } from 'vitest'
import { checkRigForm, blocks, type RigFormFacts } from './rigFormChecks'

const PORTS = ['/dev/cu.usbserial-A', '/dev/tty.usbserial-A', 'COM5']

// A stand-in for `getPortlessRigModels()`: Hamlib's low range plus two software-CAT profiles.
// The real list comes from Rust, where `rigmodels.rs` pins it against the predicate it mirrors.
const PORTLESS = [0, 1, 2, 3, 4, 2054, 23005]

const form = (over: Partial<RigFormFacts> = {}): RigFormFacts => ({
  serialPort: '/dev/cu.usbserial-A',
  rigConn: 'serial',
  pttMethod: 'cat',
  rigModel: 1049,
  ...over,
})

describe('checkRigForm', () => {
  it('passes a correct setup silently', () => {
    expect(checkRigForm(form(), PORTS, PORTLESS)).toEqual([])
  })

  it('ignores a network rig entirely — it has no serial port to be wrong about', () => {
    const checks = checkRigForm(form({ rigConn: 'network', serialPort: '' }), PORTS, PORTLESS)
    expect(checks).toEqual([])
  })

  it('blocks a rig model with no port — CAT cannot work without one', () => {
    const checks = checkRigForm(form({ serialPort: '' }), PORTS, PORTLESS)
    expect(blocks(checks)).toBe(true)
    expect(checks[0].message).toMatch(/no serial port/i)
  })

  it('allows no port when there is no rig model either — that is VOX, not a mistake', () => {
    const checks = checkRigForm(
      form({ serialPort: '', rigModel: 0, pttMethod: 'vox' }),
      PORTS,
      PORTLESS,
    )
    expect(checks).toEqual([])
  })

  // THE GATE. A whole class of models is served over TCP or a virtual COM pair by a program on
  // this machine, and is configured with NO port on purpose. Ungated, the check above called
  // every one of them an error and refused the save.
  it.each([
    [4, 'FLRig'],
    [2054, 'Thetis'],
    [23005, 'SmartSDR'],
  ])('allows model %i (%s) with no port — it is served by software, not a cable', (model) => {
    const checks = checkRigForm(
      form({ serialPort: '', rigModel: model, pttMethod: 'rts' }),
      PORTS,
      PORTLESS,
    )
    expect(checks).toEqual([])
  })

  // Positive control for the gate: the same call with a REAL rig must still block, or the two
  // cases above would pass for the wrong reason (a check that stopped firing at all).
  it('still blocks a real rig with no port, so the gate is a gate and not an off switch', () => {
    const checks = checkRigForm(form({ serialPort: '', rigModel: 1049 }), PORTS, PORTLESS)
    expect(blocks(checks)).toBe(true)
  })

  it('does not block when the portless rule could not be read', () => {
    // Empty list = the backend could not answer. Blocking then would make an unreadable rule the
    // reason an operator cannot save a configuration that is fine.
    const checks = checkRigForm(form({ serialPort: '', rigModel: 1049 }), PORTS, [])
    expect(blocks(checks)).toBe(false)
  })

  it('survives a non-array where the rule should be, rather than throwing mid-save', () => {
    // This runs inside the save handler. A throw here aborts the save with no message — which is
    // the silent no-op this whole file exists to prevent. It happened: under the panel's test
    // mocks the fetch resolved `null`, and three unrelated save tests died on `.includes`.
    const checks = checkRigForm(
      form({ serialPort: '', rigModel: 1049 }),
      PORTS,
      null as unknown as number[],
    )
    expect(blocks(checks)).toBe(false)
  })

  it('blocks a /dev/tty.* port, which hangs on carrier instead of failing', () => {
    const checks = checkRigForm(form({ serialPort: '/dev/tty.usbserial-A' }), PORTS, PORTLESS)
    expect(blocks(checks)).toBe(true)
    expect(checks.some((c) => /dial-in device/.test(c.message))).toBe(true)
  })

  it('blocks PTT over CAT with no rig model — nothing to send the keying command to', () => {
    const checks = checkRigForm(form({ rigModel: 0, pttMethod: 'cat' }), PORTS, PORTLESS)
    expect(blocks(checks)).toBe(true)
    expect(checks.some((c) => /PTT is set to CAT/.test(c.message))).toBe(true)
  })

  it('warns, but does NOT block, when the port is simply not plugged in right now', () => {
    const checks = checkRigForm(form({ serialPort: 'COM9' }), PORTS, PORTLESS)
    expect(blocks(checks)).toBe(false)
    expect(checks[0].level).toBe('warning')
    expect(checks[0].message).toMatch(/not connected right now/)
  })

  // Port collisions belong to the backend (`settings::serial_port_conflicts`), which App.tsx
  // already surfaces as `radioConfigWarning`. This file must not grow a second, unqualified
  // opinion on them: the earlier version here ignored `enabled`, `rig_model` and `rig_conn`,
  // compared case-sensitively, and BLOCKED — so it refused to save a station that shares one
  // cable between two rigs on purpose.
  it('says nothing about two radios sharing a port — that rule lives in the backend', () => {
    const checks = checkRigForm(form({ serialPort: 'COM5' }), PORTS, PORTLESS)
    expect(checks.some((c) => /already uses|cannot share/i.test(c.message))).toBe(false)
  })
})
