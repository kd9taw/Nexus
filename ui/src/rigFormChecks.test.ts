// Pre-save rig checks. Each case here is a mistake that saves silently today and then presents as
// broken hardware — the symptom always appears far from the cause, which is why they are worth
// catching at the one moment the operator is looking at the setting.
import { describe, it, expect } from 'vitest'
import { checkRigForm, blocks, type RigFormFacts } from './rigFormChecks'

const PORTS = ['/dev/cu.usbserial-A', '/dev/tty.usbserial-A', 'COM5']

// The topology rows the backend sends on a platform that can prove them. Measured shape (a
// two-radio station, macOS, 2026-08-13): both rigs use the same CP2105 bridge chip, so every row
// carries the byte-identical product label and only the topology tells them apart. `hub` is the
// USB device each thing is INSIDE — rig A's CAT bridge and codec share 0x2400000, rig B's share
// 0x2100000, and the LG monitor's serial interface is inside neither.
const INFOS = [
  {
    name: '/dev/cu.usbserial-A',
    label: 'CP2105',
    interfaceIndex: 0,
    siblingPorts: 2,
    pairedAudio: 'USB Audio Device',
  },
  {
    name: '/dev/tty.usbserial-A',
    label: 'CP2105',
    interfaceIndex: 0,
    siblingPorts: 2,
    pairedAudio: 'USB Audio Device',
  },
  {
    name: '/dev/cu.usbserial-B',
    label: 'CP2105',
    interfaceIndex: 1,
    siblingPorts: 2,
    pairedAudio: 'USB Audio Device',
  },
  // The trap, and it is a real device on the desk this was measured on: an LG monitor's control
  // port is the ONLY interface its USB device has, and it is numbered 2. `interfaceIndex > 0`
  // alone would tell the operator to pick "port 1" of a device that has no port 1.
  { name: '/dev/cu.usbmodem601', label: 'LG Monitor Controls', interfaceIndex: 2, siblingPorts: 1 },
  // And a real port with no topology at all — nothing may be said about it either.
  { name: 'COM5', label: '', interfaceIndex: null, pairedAudio: null },
]

const AUDIO = {
  input: [
    { name: 'USB Audio Device', label: 'USB Audio Device', usbHub: 0x2400000 },
    { name: 'USB Audio Device #2', label: 'USB Audio Device', usbHub: 0x2100000 },
    // A built-in card, on no USB device at all — the case that must stay silent.
    { name: 'MacBook Pro Microphone', label: 'MacBook Pro Microphone', usbHub: null },
  ],
  output: [
    { name: 'USB Audio Device', label: 'USB Audio Device', usbHub: 0x2400000 },
    { name: 'USB Audio Device #2', label: 'USB Audio Device', usbHub: 0x2100000 },
  ],
}

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

// ─── The two checks that read USB topology ───────────────────────────────────────────────────
//
// Both are DIAGNOSTIC and both are warnings. The cases below therefore assert two things every
// time: that the finding is made, and that it does not BLOCK. A topology reading that turns out
// to be wrong on somebody's station must cost them a sentence, never their configuration.
describe('checkRigForm — USB topology (additive, warnings only)', () => {
  it('warns on the silent half of a dual bridge — the port that makes a working rig look dead', () => {
    const checks = checkRigForm(
      form({ serialPort: '/dev/cu.usbserial-B' }),
      [...PORTS, '/dev/cu.usbserial-B'],
      PORTLESS,
      INFOS,
    )
    expect(checks.some((c) => /is port 2 of this device/.test(c.message))).toBe(true)
    expect(blocks(checks)).toBe(false)
  })

  // Measured false positive (this desk, 2026-08-18): the LG monitor's control port is interface 2
  // of a device that has exactly one interface. Advice to pick a different port is only meaningful
  // when a different port exists — otherwise it tells the operator the only thing they CAN pick is
  // wrong, which is worse than saying nothing.
  it('says nothing about a lone interface that happens to be numbered 2', () => {
    const checks = checkRigForm(
      form({ serialPort: '/dev/cu.usbmodem601' }),
      [...PORTS, '/dev/cu.usbmodem601'],
      PORTLESS,
      INFOS,
    )
    expect(checks.some((c) => /of this device/.test(c.message))).toBe(false)
  })

  it('says nothing about interface 0 — the half that DOES carry CAT', () => {
    const checks = checkRigForm(form(), PORTS, PORTLESS, INFOS)
    expect(checks.some((c) => /of this device/.test(c.message))).toBe(false)
  })

  // The regression this exists for: audio devices are stored BY NAME, two rigs with the same codec
  // chip share a name, and the " #2" that separates them comes from enumeration order — so moving
  // one rig to another USB socket silently repoints every saved profile at the OTHER radio.
  it('warns when the chosen codec is inside the OTHER radio', () => {
    const checks = checkRigForm(
      form({ audioIn: 'USB Audio Device #2' }), // rig B's codec, on rig A's CAT port
      PORTS,
      PORTLESS,
      INFOS,
      AUDIO,
    )
    expect(
      checks.some((c) => /is not inside the radio on \/dev\/cu\.usbserial-A/.test(c.message)),
    ).toBe(true)
    expect(blocks(checks)).toBe(false)
  })

  it('says nothing when the codec IS inside the radio on that port', () => {
    const checks = checkRigForm(
      form({ audioIn: 'USB Audio Device', audioOut: 'USB Audio Device' }),
      PORTS,
      PORTLESS,
      INFOS,
      AUDIO,
    )
    expect(checks.some((c) => /is not inside the radio/.test(c.message))).toBe(false)
  })

  // A separate interface box or an analogue card is a legitimate station, and its device sits on no
  // USB rig at all. Unknown must read as "nothing proven", never as "wrong".
  it('says nothing about a built-in sound card, which is on no rig', () => {
    const checks = checkRigForm(
      form({ audioIn: 'MacBook Pro Microphone' }),
      PORTS,
      PORTLESS,
      INFOS,
      AUDIO,
    )
    expect(checks.some((c) => /is not inside the radio/.test(c.message))).toBe(false)
  })

  // THE DEGRADE PATH, and it is the whole reason these are optional parameters: every platform
  // that cannot read USB topology — which today is every platform except macOS — passes nothing,
  // and must get exactly the checks that existed before topology did.
  it('a caller with no topology gets precisely the checks it got before', () => {
    const f = form({ serialPort: '/dev/cu.usbserial-B', audioIn: 'USB Audio Device #2' })
    const without = checkRigForm(f, [...PORTS, '/dev/cu.usbserial-B'], PORTLESS)
    expect(without.some((c) => /of this device|inside the radio/.test(c.message))).toBe(false)
    // …and the same call WITH topology differs only by adding findings, never by removing one.
    const with_ = checkRigForm(f, [...PORTS, '/dev/cu.usbserial-B'], PORTLESS, INFOS, AUDIO)
    for (const c of without) expect(with_.map((x) => x.message)).toContain(c.message)
    expect(with_.length).toBeGreaterThan(without.length)
  })

  // Empty arrays are what a Mac reports when the IO registry answers nothing, and they must be
  // indistinguishable from a platform that has no topology source at all.
  it('empty topology is the same as no topology', () => {
    const f = form({ serialPort: '/dev/cu.usbserial-B', audioIn: 'USB Audio Device #2' })
    const empty = checkRigForm(f, [...PORTS, '/dev/cu.usbserial-B'], PORTLESS, [], {
      input: [],
      output: [],
    })
    expect(empty.some((c) => /of this device|inside the radio/.test(c.message))).toBe(false)
  })
})
