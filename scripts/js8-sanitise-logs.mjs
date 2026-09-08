#!/usr/bin/env node
// scripts/js8-sanitise-logs.mjs — cut the two JS8 golden fixtures out of a JS8Call profile.
//
// WHAT COMES OUT (and nothing else):
//   crates/js8/tests/fixtures/js8call_frames.txt   — every DECODE line of ALL.TXT as
//       `<letter> <snr> <dt> <freq_hz> <sixbit12> <i3> <rendered text, trailing spaces kept>`
//       with a `# +N` line whenever the decode timestamp advances (N = seconds since the
//       previous group, clamped non-negative; the first group is `# +0`). Relative seconds are
//       all the reassembly oracle needs for JS8Call's 60 s force-close / 90 s drop clock; no
//       date survives.
//   crates/js8/tests/fixtures/js8call_directed.txt — every DIRECTED.TXT line as
//       `<offset_hz>\t<snr>\t<text>` with the U+2662 marker and trailing whitespace removed.
//
// WHAT NEVER COMES OUT: the profile path, the Windows or Linux user name, dates, dial
// frequencies, `Transmitting` lines (they carry the profile's own call and dial), band-change
// lines. Spec G5 (approved): callsigns are public on-air data and stay; the path is not.
//
// WHY A SCRIPT AND NOT A HAND CUT: the fixture is regenerated whenever the operator's log grows
// (every regeneration re-pins the sha256 in SHA256SUMS), and the leak check must run every
// time with its positive control — a hand cut has neither property. Same shape as
// scripts/gen-wsjtx-callsign-oracle.mjs: run by a maintainer, output committed, never in CI.
//
// CLOCK CLAMP: a real multi-speed log interleaves A/B decodes, so a later-logged decode can
// carry an EARLIER timestamp (measured: 109 backwards steps in a 1333-line log). The clock line
// is therefore Math.max(0, delta) — a non-decreasing cadence clock the reassembly oracle can
// replay and the Rust reader can parse as u64. `parseAllTxt` keeps the true (signed) delta so
// the clamp is visible and tested; only `renderFrames` clamps.
//
// Run:  node scripts/js8-sanitise-logs.mjs "/mnt/c/Users/<win-user>/AppData/Local/JS8Call"
// Then: (cd crates/js8/tests/fixtures && sha256sum js8call_frames.txt js8call_directed.txt)
//       and replace/append those two lines in crates/js8/tests/fixtures/SHA256SUMS.
// Test: node --test scripts/js8-sanitise-logs.test.mjs
import { readFileSync, writeFileSync } from 'node:fs'
import { userInfo } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// decodedtext.cpp DecodedText::string(): "%1:%2:%3%4 %5 %6 %7  %8         %9   " —
// hh:mm:ss, snr right-aligned in 3 (so a negative snr abuts the time), dt %4.1f, freq %4d,
// the submode letter, TWO spaces, the 12-char frame, NINE spaces, the i3 digit, then the
// rendered text after a run of spaces. The date is prepended by the ALL.TXT writer.
const DECODE_RE = /^(\d{4})-(\d\d)-(\d\d) (\d\d):(\d\d):(\d\d)\s*(-?\d+)\s+(-?\d+\.\d)\s+(\d+) ([ABCEI])  (\S{12})         (\d)    (.*)$/

/** Every decode line of an ALL.TXT, in file order, with `t` = seconds since the first one. */
export function parseAllTxt(text) {
  const rows = []
  let t0 = null
  for (const raw of text.split('\n')) {
    const line = raw.replace(/\r$/, '')
    const m = DECODE_RE.exec(line)
    if (!m) continue
    const [, Y, Mo, D, h, mi, s, snr, dt, freq, letter, sixbit, i3, rendered] = m
    const epoch = Date.UTC(+Y, +Mo - 1, +D, +h, +mi, +s) / 1000
    if (t0 === null) t0 = epoch
    rows.push({ t: epoch - t0, letter, snr: +snr, dt, freq: +freq, sixbit, i3: +i3, text: rendered })
  }
  return rows
}

/** The frames fixture text. Trailing spaces in `text` are wire truth and are preserved. */
export function renderFrames(rows) {
  const out = [
    '# JS8 golden frames — decode lines of a JS8Call-improved 2.4.0 ALL.TXT, sanitised by scripts/js8-sanitise-logs.mjs (no date, dial, path or user).',
    '# Columns: <letter> <snr> <dt> <freq_hz> <sixbit12> <i3> <rendered text exactly as JS8Call displayed it — trailing spaces are significant>',
    '# A `# +N` line advances the clock by N seconds (relative to the previous group; the first is `# +0`; a backwards step clamps to 0).',
  ]
  let prev = null
  for (const r of rows) {
    if (prev === null || r.t !== prev) {
      out.push(`# +${prev === null ? 0 : Math.max(0, r.t - prev)}`)
      prev = r.t
    }
    out.push(`${r.letter} ${r.snr} ${r.dt} ${r.freq} ${r.sixbit} ${r.i3} ${r.text}`)
  }
  return out.join('\n') + '\n'
}

/** DIRECTED.TXT rows: `date time\tdial\toffset\tsnr\ttext ♢ ` → { freq, snr, text }. */
export function parseDirectedTxt(text) {
  const rows = []
  for (const raw of text.split('\n')) {
    const line = raw.replace(/\r$/, '')
    if (!line) continue
    const cols = line.split('\t')
    if (cols.length < 5) continue
    const body = cols.slice(4).join('\t').replace(/\u2662/g, '').replace(/\s+$/, '')
    rows.push({ freq: +cols[2], snr: +cols[3], text: body })
  }
  return rows
}

export function renderDirected(rows) {
  return rows.map((r) => `${r.freq}\t${r.snr}\t${r.text}`).join('\n') + '\n'
}

/**
 * Returns a description of the first leak found in `text`, or null. `names` are the user
 * names that must not appear (case-insensitive); the profile-path words are always banned.
 */
export function leaks(text, names) {
  const banned = [...names.filter((n) => n && n.length >= 3), 'AppData', 'Users/', 'Users\\', '/mnt/c/', 'C:\\']
  const lower = text.toLowerCase()
  for (const b of banned) {
    const at = lower.indexOf(b.toLowerCase())
    if (at >= 0) return `leak: "${b}" at byte ${at}: ${JSON.stringify(text.slice(Math.max(0, at - 20), at + b.length + 20))}`
  }
  return null
}

function main() {
  const profile = process.argv[2]
  if (!profile) {
    console.error('usage: node scripts/js8-sanitise-logs.mjs <JS8Call profile dir containing ALL.TXT and DIRECTED.TXT>')
    process.exit(2)
  }
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const outDir = join(repo, 'crates/js8/tests/fixtures')
  // The Windows profile user is the path segment after `Users`; the Linux user is ours.
  const segs = resolve(profile).split(/[\\/]/)
  const winUser = segs[segs.findIndex((s) => s.toLowerCase() === 'users') + 1] || ''
  const names = [winUser, userInfo().username]

  const frames = renderFrames(parseAllTxt(readFileSync(join(profile, 'ALL.TXT'), 'latin1')))
  const directed = renderDirected(parseDirectedTxt(readFileSync(join(profile, 'DIRECTED.TXT'), 'utf8')))

  // POSITIVE CONTROL first: the check must be able to fire, or its silence means nothing.
  const planted = `A 5 0.1 773 iXNuhbCBNNQl 0 ${winUser || 'nouser'} ${userInfo().username}`
  if (leaks(planted, names) === null) throw new Error('leak check positive control FAILED to fire')
  for (const [name, body] of [['js8call_frames.txt', frames], ['js8call_directed.txt', directed]]) {
    const hit = leaks(body, names)
    if (hit) throw new Error(`${name}: ${hit}`)
    writeFileSync(join(outDir, name), body)
  }
  const nFrames = frames.split('\n').filter((l) => l && !l.startsWith('#')).length
  const nDirected = directed.split('\n').filter(Boolean).length
  const byLetter = {}
  for (const l of frames.split('\n')) if (l && !l.startsWith('#')) byLetter[l[0]] = (byLetter[l[0]] || 0) + 1
  console.error(`wrote ${basename(outDir)}/js8call_frames.txt (${nFrames} frames: ${JSON.stringify(byLetter)}) and js8call_directed.txt (${nDirected} lines); leak check clean (positive control fired)`)
  console.error('next: (cd crates/js8/tests/fixtures && sha256sum js8call_frames.txt js8call_directed.txt) → SHA256SUMS')
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
