// @vitest-environment jsdom
//
// The Rotator-Model picker: the baud it may impose, and the catalog it may offer.
//
// ⭐ THE FIFTH ROUND OF THE BAUD RULE, and it is the one that was never run. The rig picker
// beside this one cost four rounds of shipped bugs to get right, and wrote the lesson down in
// `SettingsPanel.rigpicker.test.tsx`: a baud table transcribed by hand is wrong at a rate that
// kills radios, so the table must rest on one fact the backend states outright —
// `serial_rate_min == serial_rate_max`. **None of that reached the ROTATOR picker.** It shipped
// one app-wide 9600 for all fourteen models, a tooltip asserting "GS-232 default 9600" to every
// one of them, and a `rotctld_args` that forces `-s 9600` over the backend's own rate. Measured
// against the Hamlib in the installer, five of the thirteen real-hardware entries declare a
// single, different rate — SPID Rot2Prog 600, SPID Rot1Prog 1200, Idiom Press Rotor-EZ /
// Hy-Gain DCU-1 / Green Heron RT-21 4800 — so they were shipped unable to talk at all. That is
// the field report this file exists to close: "one rotator model does not work".
//
// So the rows are DERIVED, by `scripts/gen-hamlib-rotator-speeds.mjs`, into
// `__fixtures__/hamlibRotatorSpeeds.json`, and everything below checks the table against that
// fixture. A row typed from a manual cannot pass. Nor can a picker label that claims an
// elevation axis the backend does not declare.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel, ROT_FIXED_BAUD, ROTATOR_MODELS, baudForRotator } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import rotCaps from './__fixtures__/hamlibRotatorSpeeds.json'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// THE BUDGET (2026-10-09). The slowest case here, "picking SPID Rot2Prog moves the baud to 600 — the field…", takes
// 0.54 s and 0.42 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list —
// same reasoning as the rig picker's suite.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

/** A station whose rotator sits on the app-wide 9600 default — the state the field report
 *  starts in, for every model in the picker. */
function stationWithRotator(rotatorModel = 0, rotatorBaud = 9600) {
  const radio = {
    id: 0,
    name: 'Radio 1',
    enabled: true,
    serialPort: 'COM3',
    baud: 38400,
    rigModel: 0,
    rigModelName: '',
    rigConn: 'serial',
    rigAddr: '',
    rigctldPort: 4532,
    rotctldPort: 4533,
    icomNativeCat: false,
    audioIn: 'in-0',
    audioOut: 'out-0',
    txLevel: 1,
    rxGain: 1,
    pttMethod: 'cat',
    rotatorModel,
    rotatorPort: 'COM7',
    rotatorBaud,
    rotatorHost: '',
    nativeScope: 'auto',
    bands: [],
  }
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

async function openRotator(model = 0, baud = 9600) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(stationWithRotator(model, baud)))
  render(
    <SettingsPanel
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Connection')
}

const rotSelect = () => screen.getByRole('combobox', { name: 'Rotator model' }) as HTMLSelectElement
const baudBox = () => screen.getByRole('spinbutton', { name: 'Rotator baud rate' }) as HTMLInputElement

/** Caps of every rotator the bundled Hamlib carries. The only admissible basis. */
const CAPS = new Map(rotCaps.rotators.map((r) => [r.model, r]))
const SERIAL = rotCaps.rotators.filter((r) => r.port === 'serial')

describe('the basis rule: a row exists only where the backend says there is ONE rate', () => {
  const entries = [...ROT_FIXED_BAUD.entries()]

  it('the caps fixture is the bundled Hamlib and covers the whole rotator library', () => {
    // If this reads as a different build, every fact below is about a different Hamlib than
    // the operator's — regenerate with `node scripts/gen-hamlib-rotator-speeds.mjs`.
    expect(rotCaps.hamlib).toMatch(/^Hamlib 4\.7\.1 /)
    expect(rotCaps.rotators.length).toBeGreaterThan(50)
    expect(SERIAL.length).toBeGreaterThan(40)
  })

  it('⭐ every listed model declares serial_rate_min == serial_rate_max', () => {
    // The rule itself, unchanged from the rig side: a range — however narrow — is not evidence.
    // Hamlib does not enforce serial_rate_min/_max, and `-s` overrides whatever they say.
    for (const [model] of entries) {
      const c = CAPS.get(model)
      expect({ model, declared: c ? `${c.min}..${c.max}` : 'not in the rotator caps' }).toEqual({
        model,
        declared: c ? `${c.min}..${c.min}` : 'not in the rotator caps',
      })
    }
  })

  it('⭐ every listed rate IS the rate its backend declares', () => {
    for (const [model, rate] of entries) {
      expect({ model, rate }).toEqual({ model, rate: CAPS.get(model)?.min })
    }
  })

  it('⭐ every one-rate SERIAL rotator in the library HAS a row', () => {
    // The other direction — the FX-4 bug's rotator twin. Any fixed-rate backend added by a
    // Hamlib bump fails here until it is listed, whether or not the picker offers it: "Other
    // Hamlib model #…" lets an operator type any number.
    const fixed = SERIAL.filter((r) => r.min === r.max)
    expect(fixed.length).toBeGreaterThan(20)
    for (const r of fixed) {
      expect({ model: r.model, name: r.name, listed: ROT_FIXED_BAUD.has(r.model) }).toEqual({
        model: r.model,
        name: r.name,
        listed: true,
      })
    }
  })

  it('⭐ the five models the field report is about are listed at their real rates', () => {
    // Named rather than swept, because these are the rows whose absence shipped: an owner of
    // any of them set the app up exactly as the docs said and the rotator never answered.
    expect([...ROT_FIXED_BAUD.entries()].filter(([m]) => [901, 902, 401, 403, 405].includes(m))).toEqual([
      [401, 4800], // Idiom Press Rotor-EZ
      [403, 4800], // Hy-Gain DCU-1/DCU-1X
      [405, 4800], // Green Heron RT-21
      [901, 600], // SPID Rot2Prog
      [902, 1200], // SPID Rot1Prog
    ])
  })

  it('a backend with no serial port at all never gets a row', () => {
    // EA4TX ARS (1101/1102) is PARALLEL and declares 0..0. A "fixed rate" of zero is not a
    // rate, and imposing it would hand rotctld `-s 0`.
    for (const r of rotCaps.rotators.filter((x) => x.port !== 'serial')) {
      expect({ model: r.model, port: r.port, listed: ROT_FIXED_BAUD.has(r.model) }).toEqual({
        model: r.model,
        port: r.port,
        listed: false,
      })
    }
  })
})

describe('a listed rotator gets its one rate; a range model is never touched', () => {
  const entries = [...ROT_FIXED_BAUD.entries()]

  it('the app default is replaced, and the rate itself is left alone', () => {
    for (const [model, rate] of entries) {
      expect({ model, fromDefault: baudForRotator(model, 9600) }).toEqual({
        model,
        fromDefault: rate === 9600 ? null : rate,
      })
      expect({ model, already: baudForRotator(model, rate) }).toEqual({ model, already: null })
    }
  })

  it('…and so is every other rate, because on a one-rate controller none of them can work', () => {
    for (const [model, rate] of entries) {
      for (const other of [1200, 4800, 9600, 19200, 38400].filter((b) => b !== rate)) {
        expect({ model, other, imposed: baudForRotator(model, other) }).toEqual({
          model,
          other,
          imposed: rate,
        })
      }
    }
  })

  it('every RANGE model in the library is left exactly where the operator put it', () => {
    // The rig side's hard-won half. GS-232A is 150..9600, GS-232B 1200..9600, EasyComm
    // 9600..19200, SPID MD-01/02 600..460800 — all real menus, none of them a fact.
    const ranged = SERIAL.filter((r) => r.min !== r.max)
    expect(ranged.map((r) => r.model)).toEqual(expect.arrayContaining([601, 602, 603, 202, 204, 903]))
    for (const r of ranged) {
      expect({ model: r.model, listed: ROT_FIXED_BAUD.has(r.model) }).toEqual({
        model: r.model,
        listed: false,
      })
      for (const rate of [1200, 4800, 9600, 19200]) {
        expect(baudForRotator(r.model, rate)).toBeNull()
      }
    }
  })

  it('a model number typed by hand that is not a rotator at all is left alone', () => {
    for (const rate of [1200, 4800, 9600]) expect(baudForRotator(999999, rate)).toBeNull()
  })
})

describe('the picker itself', () => {
  it('every curated entry is a model the bundled Hamlib actually carries', () => {
    // Issue #34's rotator twin: a model number can be real in rotlist.h and have no backend in
    // the library that ships. Pick it and rotctld never starts.
    for (const r of ROTATOR_MODELS) {
      expect({ model: r.model, known: CAPS.has(r.model) }).toEqual({ model: r.model, known: true })
    }
  })

  it('⭐ no curated entry needs a port type Nexus cannot offer', () => {
    // THE EA4TX BUG. "EA4TX ARS (az)" was model 1102 — Hamlib's PARALLEL-PORT backend — offered
    // with a serial-port box and a baud. It could not work as presented, and the brand name
    // steered ARS-USB owners (whose box speaks GS-232 over USB) away from the entry that does.
    //
    // The box is rotctld's `-r`, which takes a serial device or, for a backend that talks over
    // the network, its address. So one UDP entry can work as presented: PstRotator (model 3),
    // whose address the operator types there. The Dummy needs no port at all.
    const offered: Record<number, string> = { 1: 'none', 3: 'udp' }
    for (const r of ROTATOR_MODELS) {
      const caps = CAPS.get(r.model)
      expect({ model: r.model, label: r.label, port: caps?.port }).toEqual({
        model: r.model,
        label: r.label,
        port: offered[r.model] ?? 'serial',
      })
    }
  })

  it('the only curated entries that are not serial are PstRotator and the Dummy', () => {
    // Named, so the allowance above cannot quietly admit a parallel or another network backend.
    expect(ROTATOR_MODELS.filter((r) => CAPS.get(r.model)?.port !== 'serial').map((r) => [r.model, CAPS.get(r.model)?.port])).toEqual([
      [3, 'udp'],
      [1, 'none'],
    ])
  })

  it('⭐ an "(az)" or "(az/el)" label is what the BACKEND declares, not what the box says', () => {
    // The az/el half of the same defect: only the azimuth-only variants of two manufacturers
    // were offered, so an az/el owner who picked the entry with his brand on it had elevation
    // refused for the whole pass. A label may only claim what `rot_type` states.
    for (const r of ROTATOR_MODELS) {
      const axes = CAPS.get(r.model)?.axes
      const claimed = /\(az\/el[^)]*\)/.test(r.label) ? 'azel' : /\(az[^/)]*\)/.test(r.label) ? 'az' : null
      if (claimed !== null) {
        expect({ model: r.model, label: r.label, claimed }).toEqual({
          model: r.model,
          label: r.label,
          claimed: axes,
        })
      }
    }
  })

  it('both manufacturers whose az-only entry stranded az/el owners now have an az/el entry', () => {
    const listed = new Set(ROTATOR_MODELS.map((r) => r.model))
    expect({ prosistelCombiTrack: listed.has(1703), spidMd01: listed.has(903) }).toEqual({
      prosistelCombiTrack: true,
      spidMd01: true,
    })
    expect(listed.has(1102)).toBe(false) // the parallel-port EA4TX entry is gone
  })

  it('picking SPID Rot2Prog moves the baud to 600 — the field report, directly', async () => {
    await openRotator(0, 9600)
    fireEvent.change(rotSelect(), { target: { value: '901' } })
    expect(baudBox().value).toBe('600')
  })

  it.each([
    [902, '1200'],
    [401, '4800'],
    [403, '4800'],
    [405, '4800'],
  ])('picking model %i lands on %s baud', async (model, want) => {
    await openRotator(0, 9600)
    fireEvent.change(rotSelect(), { target: { value: String(model) } })
    expect(baudBox().value).toBe(want)
  })

  it('picking a GS-232 leaves the operator’s own rate alone — it is a range, not a fact', async () => {
    await openRotator(0, 4800)
    fireEvent.change(rotSelect(), { target: { value: '601' } })
    expect(baudBox().value).toBe('4800')
  })

  it('re-picking the same rotator is idempotent', async () => {
    await openRotator(0, 9600)
    for (let i = 0; i < 3; i++) fireEvent.change(rotSelect(), { target: { value: '901' } })
    expect(baudBox().value).toBe('600')
  })

  it('a saved rotator whose baud cannot work says so, in words that name the rate', async () => {
    // The rescue path for everyone already stranded: their setting is not rewritten behind
    // their back, but they are told the number and why.
    await openRotator(901, 9600)
    expect(screen.getByText(/runs at 600 baud, not 9,600/i)).not.toBeNull()
  })

  it('…and says nothing alarming once it is right', async () => {
    await openRotator(901, 600)
    expect(screen.queryByText(/runs at 600 baud, not/i)).toBeNull()
    expect(screen.getByText(/runs at 600 baud/i)).not.toBeNull()
  })

  it('the model and its port are in the ROTATOR section, where the chip points', async () => {
    // The "Rotator not answering" chip deep-links to `rotator`, and that section used to hold
    // the pointing manners alone — no model, no port. Landing an operator one screen away from
    // the two fields their fault is nearly always in is the whole defect.
    await openRotator(901, 600)
    const section = document.getElementById('settings-rotator')
    expect(section).not.toBeNull()
    expect(section?.contains(rotSelect())).toBe(true)
    expect(section?.contains(baudBox())).toBe(true)
    expect(section?.contains(screen.getByRole('textbox', { name: 'Rotator serial port' }))).toBe(true)
  })

  it('a saved model that is not curated shows its NUMBER, not an empty box', async () => {
    // 903, 404, 607 and 406 are all real rotators an operator can be on. The "Other" box is
    // local state seeded only when they pick Other by hand, so re-opening Settings rendered a
    // blank field over a perfectly good saved model.
    await openRotator(2801, 9600) // Sky-Watcher — real, and deliberately not in the curated list
    const other = screen.getByRole('spinbutton', { name: 'Hamlib rotator model number' })
    expect((other as HTMLInputElement).value).toBe('2801')
  })
})

describe('the Yaesu G-5500 by its own name', () => {
  // The operator's request: "You need to add Yaesu G5500 az/el rotator". Hamlib has no G-5500
  // model, because the rotator has no computer port: it is driven through a GS-232B or GS-232A
  // interface, or a GS-232 clone board, and the entry that works is that interface's model. So the
  // name goes ON those entries. The setting stores one number, and a <select> shows the FIRST
  // option carrying the saved value, so a second "G-5500" entry beside "GS-232B" would be shown in
  // its place after a save, naming something the operator did not pick.
  const labelOf = (model: number) => ROTATOR_MODELS.find((r) => r.model === model)?.label ?? ''

  it('no two entries save the same model number', () => {
    const models = ROTATOR_MODELS.map((r) => r.model)
    expect(models.filter((m, i) => models.indexOf(m) !== i)).toEqual([])
  })

  it('a G-5500 owner finds it by name: on the GS-232B entry first, then the GS-232A one', () => {
    expect(ROTATOR_MODELS.filter((r) => r.label.includes('G-5500')).map((r) => r.model)).toEqual([
      603, 601,
    ])
    // The name leads, and the interface follows it, so a closed <select> too narrow for the
    // whole label still shows both.
    expect(labelOf(603)).toMatch(/^Yaesu G-5500 \/ G-5500DC — GS-232B interface \(az\/el\)/)
    expect(labelOf(601)).toMatch(/^Yaesu G-5500 \/ G-5500DC — GS-232A interface \(az\/el\)/)
    // Both stay the interface's entry for everything else that runs on it.
    expect(labelOf(603)).toMatch(/any GS-232B/)
    expect(labelOf(603)).toMatch(/ERC V4 in its recommended mode/)
    expect(labelOf(601)).toMatch(/any GS-232A/)
  })

  it('a GS-232 clone board points at the generic entry', () => {
    expect(labelOf(602)).toMatch(/^GS-232 \(generic, az\/el\) — GS-232 clone boards/)
    expect(labelOf(602)).toMatch(/EA4TX ARS-USB, LVB, ST2/)
  })

  it.each([
    [603, 'GS-232B'],
    [601, 'GS-232A'],
  ])('after a save of model %i the list still names the G-5500 and its %s', async (model, iface) => {
    await openRotator(model, 9600)
    const shown = rotSelect().selectedOptions[0]?.textContent ?? ''
    expect(rotSelect().value).toBe(String(model))
    expect(shown).toContain('Yaesu G-5500 / G-5500DC')
    expect(shown).toContain(iface)
  })

  it('picking it saves the Hamlib model number and shows what was picked', async () => {
    await openRotator(0, 9600)
    fireEvent.change(rotSelect(), { target: { value: '603' } })
    expect(rotSelect().value).toBe('603')
    expect(rotSelect().selectedOptions[0]?.textContent).toContain('GS-232B interface')
  })

  it('picking it leaves the baud alone: GS-232B is a 1200–9600 range, not a fact', async () => {
    for (const rate of [4800, 9600, 1200]) {
      cleanup()
      await openRotator(0, rate)
      fireEvent.change(rotSelect(), { target: { value: '603' } })
      expect(baudBox().value).toBe(String(rate))
    }
    expect(CAPS.get(603)).toMatchObject({ min: 1200, max: 9600, axes: 'azel', minEl: 0, maxEl: 180 })
  })
})

describe('PstRotatorAz by name (Hamlib model 3)', () => {
  // The operator (#362): "Add model 3 to the picker". YO3DMU's PstRotatorAz takes commands over
  // UDP, port 12000 by default, and Hamlib's PstRotator backend (model 3) speaks that protocol, so
  // the entry saves 3 and its port box takes an address. The bundled Hamlib 4.7.1 writes the
  // bearing with `%f.2`, a typo for `%.2f`, so 123.4° goes out as 123.400002.2: the hint says so.
  const labelOf = (model: number) => ROTATOR_MODELS.find((r) => r.model === model)?.label ?? ''
  const portBox = () => screen.getByRole('textbox', { name: 'Rotator serial port' }) as HTMLInputElement

  it('is offered as Hamlib model 3, by name', () => {
    expect(ROTATOR_MODELS.filter((r) => /PstRotator/.test(r.label)).map((r) => r.model)).toEqual([3])
    expect(labelOf(3)).toBe('PstRotatorAz / PstRotator (UDP)')
    expect(CAPS.get(3)).toMatchObject({ mfg: 'YO3DMU', name: 'PstRotator', port: 'udp' })
  })

  it('picking it saves model 3, and the list shows what was picked', async () => {
    await openRotator(0, 9600)
    fireEvent.change(rotSelect(), { target: { value: '3' } })
    expect(rotSelect().value).toBe('3')
    expect(rotSelect().selectedOptions[0]?.textContent).toContain('PstRotatorAz')
  })

  it('after a save of model 3 the list still names PstRotatorAz, not "Other"', async () => {
    await openRotator(3, 9600)
    expect(rotSelect().value).toBe('3')
    expect(rotSelect().selectedOptions[0]?.textContent).toContain('PstRotatorAz')
    expect(screen.queryByRole('spinbutton', { name: 'Hamlib rotator model number' })).toBeNull()
  })

  it('its port box asks for the address, and its hint says where to switch UDP on and what the backend sends', async () => {
    await openRotator(3, 9600)
    expect(portBox().placeholder).toBe('127.0.0.1:12000')
    const hint = screen.getByText(/^PstRotatorAz: enter 127\.0\.0\.1:12000 as the port/)
    expect(hint.textContent).toMatch(/turn on UDP Control in its Setup/)
    expect(hint.textContent).toMatch(/123\.400002\.2 for 123\.4°/)
    // In place of the baud hint, which would tell a UDP program to match a line rate.
    expect(screen.queryByText(/Match the rate your controller is set to/)).toBeNull()
  })

  it('every other model keeps its own port example and baud hint', async () => {
    await openRotator(603, 9600)
    expect(portBox().placeholder).not.toBe('127.0.0.1:12000')
    expect(screen.queryByText(/^PstRotatorAz: enter/)).toBeNull()
    expect(screen.getByText(/Match the rate your controller is set to/)).not.toBeNull()
  })
})

describe('the rotator guide lists the picker as it is', () => {
  // docs/rigs/rotators.md's "Curated rotator models" table says it is what the dropdown offers, and
  // it drifted twice: the ERC's DCU-1-mode label (2026-08-29) and the G-5500's entries (2026-09-29)
  // each changed the picker and left the guide naming entries it no longer had.
  const guide = readFileSync(resolve(process.cwd(), '..', 'docs', 'rigs', 'rotators.md'), 'utf8')
  const table = guide.split('## Curated rotator models')[1]?.split('\n## ')[0] ?? ''
  const rows = [...table.matchAll(/^\| (.+?) \| (\d+) \|$/gm)].map((m) => ({ label: m[1], model: Number(m[2]) }))

  it('finds the table (a renamed heading must not empty this check)', () => {
    expect(rows.length).toBeGreaterThan(10)
  })

  it("names every entry exactly as the picker does, in the picker's order", () => {
    expect(rows).toEqual(ROTATOR_MODELS.map((r) => ({ label: r.label, model: r.model })))
  })
})
