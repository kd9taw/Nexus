#!/usr/bin/env node
// Generate the Hamlib rotator caps table — serial rate, port type and axes — for every rotator
// the BUNDLED Hamlib carries.
//
// WHY THIS IS A SCRIPT AND NOT A HAND-WRITTEN TABLE. The rig picker learned this over four
// rounds of shipped bugs (see `scripts/gen-hamlib-serial-speeds.mjs` and
// `SettingsPanel.rigpicker.test.tsx`) and the ROTATOR picker never did: it shipped ONE app-wide
// 9600 for all fourteen curated models, forced onto the daemon as `-s 9600`, which OVERRIDES the
// backend's own rate. Five of the thirteen real-hardware entries declare a single rate that is
// not 9600 — SPID Rot2Prog 600, Rot1Prog 1200, Rotor-EZ / DCU-1 / RT-21 4800 — so a quarter of
// the picker could not talk to its controller at all. That is the field report ("one rotator
// model does not work") and the 2026-08-18 rotor review's headline finding.
//
// ⚠️ WHY THIS ONE PARSES THE DLL WHILE ITS RIG TWIN PARSES TEXT. `rigctl --dump-caps` prints
// caps without opening the port; `rotctl --dump-caps` does NOT — rotctl calls `rot_open` FIRST
// and exits 2 when it fails, so on a box with no rotator the caps are unreachable (measured:
// `rotctl.exe -m 901 --dump-caps` → "serial_open: serial port \\.\COM1 does not exist / IO
// error"; a regular file as `-r` opens and then fails GetCommState; `rotctld` exits on the same
// failure, so there is no daemon to ask over TCP either). The one remaining source of
// `serial_rate_min` is the `struct rot_caps` table inside the shipped `libhamlib-4.dll`, so this
// reads it there — and then proves it read the right bytes, three ways (below).
//
// THE THREE CROSS-CHECKS, and none of them is optional — an offset walk that lands one field
// wrong yields plausible numbers, which is the failure this whole exercise exists to prevent:
//   1. `rotctl.exe -l` gives model number, manufacturer, model name and backend VERSION. A
//      candidate struct must match ALL FOUR (three of them via pointers resolved through the
//      PE headers). This is what separates rot_caps from rig_caps and amp_caps, which also
//      carry a `Dummy`/`Hamlib` model 1.
//   2. `rotctl.exe -m <n> -L` prints the INITIALISED `serial_speed`, which `rot_init` copies
//      from `caps->serial_rate_max`. Every serial backend's parsed max must equal it. That is
//      an independent measurement of a field 56 bytes into the struct we located by other
//      means — if the walk were off, this could not agree 44 times out of 44.
//   3. Exactly ONE candidate may survive per model. A second hit means the anchor is ambiguous
//      and the run fails rather than picking one.
// A run that cannot satisfy all three throws. `min` is then read from the same struct, four
// bytes before a `max` that three sources agree on.
//
// Output (deterministic — no timestamp, so regenerating an unchanged tree is a no-op diff):
//   ui/src/components/__fixtures__/hamlibRotatorSpeeds.json
//     { hamlib, rotators: [{ model, mfg, name, version, port, axes, min, max,
//                            minAz, maxAz, minEl, maxEl }, …] }
//
// `axes` is DERIVED, and deliberately claims nothing it cannot prove: `rot_type` states az /
// el / az+el outright for most backends, and where it says ROT_TYPE_OTHER the only fact left is
// a zero-width elevation range (no elevation axis at all → 'az'). Anything else is 'other', and
// the picker prints no axis for it rather than guessing.
//
// Run:  node scripts/gen-hamlib-rotator-speeds.mjs [path/to/rotctl] [path/to/libhamlib-4.dll]
//   Defaults are the bundled Windows binaries, which run under WSL interop on the dev box.
//   Re-run and diff after any Hamlib bump: a row that moves is a rotator whose rate rule
//   just changed, and `SettingsPanel.rotpicker.test.tsx` fails until the table follows.

import { readFileSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const rotctl = process.argv[2] ?? join(repo, 'src-tauri/resources/hamlib/rotctl.exe')
const dllPath = process.argv[3] ?? join(repo, 'src-tauri/resources/hamlib/libhamlib-4.dll')
const out = join(repo, 'ui/src/components/__fixtures__/hamlibRotatorSpeeds.json')

// ---- the PE image: image base + section table, so a stored pointer can be followed ----------
const dll = readFileSync(dllPath)
const peAt = dll.readUInt32LE(0x3c)
if (dll.toString('latin1', peAt, peAt + 4) !== 'PE\0\0') throw new Error(`${dllPath} is not a PE image`)
const coff = peAt + 4
const nSections = dll.readUInt16LE(coff + 2)
const optSize = dll.readUInt16LE(coff + 16)
const opt = coff + 20
if (dll.readUInt16LE(opt) !== 0x20b) throw new Error('expected a PE32+ (64-bit) libhamlib')
const imageBase = dll.readBigUInt64LE(opt + 24)
const sections = []
for (let i = 0, p = opt + optSize; i < nSections; i++, p += 40) {
  sections.push({
    vaddr: dll.readUInt32LE(p + 12),
    vsize: dll.readUInt32LE(p + 8),
    raddr: dll.readUInt32LE(p + 20),
    rsize: dll.readUInt32LE(p + 16),
  })
}
/** File offset holding the byte at relative virtual address `rva`, or null if unmapped. */
function fileOffset(rva) {
  for (const s of sections) {
    if (rva >= s.vaddr && rva < s.vaddr + Math.max(s.vsize, s.rsize)) {
      const off = s.raddr + (rva - s.vaddr)
      return off < dll.length ? off : null
    }
  }
  return null
}
/** The NUL-terminated ASCII string a stored 64-bit pointer points at, or null. */
function stringAt(va) {
  if (va < imageBase || va > imageBase + (1n << 32n)) return null
  const off = fileOffset(Number(va - imageBase))
  if (off == null) return null
  const end = dll.indexOf(0, off)
  if (end < 0 || end - off > 96) return null
  const s = dll.toString('latin1', off, end)
  return s.length > 0 && /^[\x20-\x7e]+$/.test(s) ? s : null
}

// ---- what the daemon itself says: the model list, and the initialised conf per model ---------
function rotctlRun(args) {
  try {
    return execFileSync(rotctl, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
  } catch (e) {
    return e.stdout ?? ''
  }
}
const hamlib = (rotctlRun(['-V']).match(/^rotctl\(d\),\s*(.+)$/m)?.[1] ?? '').trim()
if (!hamlib.startsWith('Hamlib ')) throw new Error(`could not read a version banner from ${rotctl}`)

// ` <model>  <mfg>  <model name>  <version>  <status>  ROT_MODEL_x` — two-space columns.
const listing = rotctlRun(['-l'])
const catalog = [...listing.matchAll(/^\s*(\d+)\s+(.+?)\s{2,}(.+?)\s{2,}(\S+)\s+(\S+)\s+(ROT_MODEL_\S+)\s*$/gm)]
  .map((m) => ({ model: Number(m[1]), mfg: m[2].trim(), name: m[3].trim(), version: m[4].trim() }))
if (catalog.length < 40) throw new Error(`only ${catalog.length} rotators parsed from \`rotctl -l\` — check the pattern`)

/** `rotctl -m <n> -L`: the conf values `rot_init` seeded from caps. Absent params read as null. */
function showConf(model) {
  const dump = rotctlRun(['-m', String(model), '-L'])
  const num = (key) => {
    const m = dump.match(new RegExp(`^${key}: "[^"]*"\\s*\\n\\s*Default: [-\\d.]+, Value: (-?[\\d.]+)`, 'm'))
    return m ? Number(m[1]) : null
  }
  return {
    speed: num('serial_speed'),
    minAz: num('min_az'), maxAz: num('max_az'), minEl: num('min_el'), maxEl: num('max_el'),
  }
}

// ---- rot_caps, offset by offset (hamlib/rotator.h; stable since 1.2) -------------------------
// 0 rot_model(i32) · 8 model_name* · 16 mfg_name* · 24 version* · 32 copyright* · 40 status(i32)
// 44 rot_type(i32) · 48 port_type(i32) · 52 serial_rate_min(i32) · 56 serial_rate_max(i32)
const PORT_TYPES = { 0: 'none', 1: 'serial', 2: 'network', 3: 'device', 8: 'parallel', 9: 'usb', 10: 'udp', 11: 'cm108', 12: 'gpio', 13: 'gpion' }
const ROT_FLAG_AZIMUTH = 1 << 1
const ROT_FLAG_ELEVATION = 1 << 2

const rotators = []
for (const { model, mfg, name, version } of catalog) {
  const conf = showConf(model)
  const needle = Buffer.alloc(4)
  needle.writeUInt32LE(model)
  const hits = []
  for (let i = dll.indexOf(needle); i >= 0; i = dll.indexOf(needle, i + 1)) {
    if (i % 8 !== 0 || i + 60 > dll.length) continue // every rot_caps is 8-byte aligned
    if (stringAt(dll.readBigUInt64LE(i + 8)) !== name) continue
    if (stringAt(dll.readBigUInt64LE(i + 16)) !== mfg) continue
    if (stringAt(dll.readBigUInt64LE(i + 24)) !== version) continue // splits rot_caps from rig_caps/amp_caps
    hits.push({
      rotType: dll.readInt32LE(i + 44),
      port: dll.readInt32LE(i + 48),
      min: dll.readInt32LE(i + 52),
      max: dll.readInt32LE(i + 56),
    })
  }
  if (hits.length !== 1) throw new Error(`model ${model} (${mfg} ${name}): ${hits.length} candidate rot_caps — the anchor is not unique`)
  const [h] = hits
  const port = PORT_TYPES[h.port] ?? `port-${h.port}`
  // CROSS-CHECK 2. `-L`'s serial_speed IS caps.serial_rate_max (rot_init copies it). A serial
  // backend must agree; a non-serial one must print no serial_speed at all.
  if (port === 'serial' ? conf.speed !== h.max : conf.speed !== null) {
    throw new Error(`model ${model}: rotctl -L says serial_speed=${conf.speed}, the DLL says ${port} ${h.min}..${h.max}`)
  }
  const azel = h.rotType & (ROT_FLAG_AZIMUTH | ROT_FLAG_ELEVATION)
  const axes =
    azel === (ROT_FLAG_AZIMUTH | ROT_FLAG_ELEVATION) ? 'azel'
    : azel === ROT_FLAG_AZIMUTH ? 'az'
    : azel === ROT_FLAG_ELEVATION ? 'el'
    // ROT_TYPE_OTHER states nothing. A zero-width elevation range still does: there is no
    // elevation axis. Anything else stays unclaimed.
    : conf.maxEl != null && conf.maxEl === conf.minEl ? 'az'
    : 'other'
  rotators.push({
    model, mfg, name, version, port, axes,
    min: h.min, max: h.max,
    minAz: conf.minAz, maxAz: conf.maxAz, minEl: conf.minEl, maxEl: conf.maxEl,
  })
}

writeFileSync(out, JSON.stringify({ hamlib, rotators }, null, 2) + '\n')
const serial = rotators.filter((r) => r.port === 'serial')
const fixed = serial.filter((r) => r.min === r.max)
console.log(`${out}: ${rotators.length} rotators, ${serial.length} serial, ${fixed.length} with min == max (${hamlib})`)
console.log(`  one-rate and NOT 9600: ${fixed.filter((r) => r.min !== 9600).map((r) => `${r.model}=${r.min}`).join(' ')}`)
