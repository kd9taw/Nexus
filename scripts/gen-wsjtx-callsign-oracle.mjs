#!/usr/bin/env node
// Measure WSJT-X's callsign predicates — the REAL Qt regexes, with their REAL flags — over a
// deterministic corpus, and commit the measurements as a fixture.
//
// WHY THIS IS A GENERATOR AND NOT A HAND-WRITTEN TABLE. Nexus vendored WSJT-X's DSP and its
// 77-bit packer byte-identical, but `stdCall`, `genStdMsgs` and `genCQMsg` live in
// `widgets/mainwindow.cpp` — a Qt GUI class with no counterpart in our vendor tree. So
// `message.rs` RE-DERIVES by hand what upstream states as a regex, and that seam is where the
// FT-mode bugs of 2026-08-06 came from: `is_compound` ("contains a slash") stood in for
// `stdCall`, and F4CYH/P transmitted neither a grid nor a report. Everywhere we transcribed it
// was right first time; everywhere we re-derived it broke. This script is the instrument that
// tells those two apart, so nobody has to re-type a regex again.
//
// THE FLAGS ARE LOAD-BEARING, and that is why a hand transcription cannot be trusted:
//   * `ExtendedPatternSyntaxOption` is PCRE `/x` — pattern whitespace ignored, `#` a comment.
//     Drop it and `( /R | /P )?` demands literal spaces and the pattern matches nothing at all.
//   * `CaseInsensitiveOption` folds through UNICODE (Qt builds PCRE2 in UTF mode), so
//     `ſ9XYZ` (U+017F LONG S) and `K1ABC` spelled with U+212A KELVIN SIGN match upstream's
//     `stdCall`. An ASCII `to_ascii_uppercase` cannot reproduce that.
//   * Qt compiles PCRE2 WITHOUT `PCRE2_UCP`, so `\s` is ASCII-only — narrower than Rust's
//     `str::trim`, which is Unicode White_Space and eats NBSP and U+2028.
//   * PCRE2's default `$` also matches BEFORE a final newline, so `"W9XYZ\n"` satisfies
//     `^[A-Z0-9/]{3,11}$`. Masked inside `stdCall` (its `\s*` eats the newline first) and LIVE
//     in the two Radio.cpp regexes — a naive port is right where it does not matter and wrong
//     where it does.
// Those four are measured here rather than reasoned about. The fixture is the measurement.
//
// NO NETWORK AT TEST TIME. Fetch happens HERE, when a maintainer runs this; the committed
// fixture is what CI reads. The test job makes no network calls today and must not start: a
// gate that goes red because SourceForge is down is a gate people disable, and a third-party
// host must never be a silent rewrite path for a correctness oracle. Same shape as
// `scripts/gen-hamlib-serial-speeds.mjs` — script + committed fixture + deterministic re-run.
//
// NOTHING UPSTREAM IS COMMITTED. The harness this script compiles carries upstream text (the
// patterns and `Radio::base_callsign`), but it is built in a temp dir and thrown away; only
// MEASUREMENTS reach the tree. `libtempo/vendor/wsjtx/README.md` and NOTICE both state that no
// Qt/GUI code is included, and that stays true. The two patterns that do appear in the fixture
// are already quoted in `crates/tempo-core/src/message.rs`'s doc comments at HEAD, so the
// fixture adds no upstream expression the repo did not already carry.
//
// STALENESS IS DETECTED, NOT PREVENTED. The fixture records the upstream branch tip, the
// sha256 of each source file, and the extracted pattern text. This script FAILS LOUDLY if a
// pattern can no longer be found or if `is_77bit_nonstandard_callsign` is no longer the
// composition the fixture assumes — that is the whole staleness mechanism. Re-run after any
// WSJT-X release and diff: a verdict that moves is upstream changing its mind.
//
// Requires: Qt5Core dev headers (`pkg-config Qt5Core`), a C++17 g++, and network.
// Run:   node scripts/gen-wsjtx-callsign-oracle.mjs
// Out:   crates/tempo-core/tests/fixtures/wsjtx-callsign-oracle.json
// Read by: crates/tempo-core/tests/wsjtx_predicate_differential.rs  (Rust predicates)
//          ui/src/txMessages.oracle.test.ts                          (the TS duplicates)

import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const out = join(repo, 'crates/tempo-core/tests/fixtures/wsjtx-callsign-oracle.json')

const SF_GIT = 'https://git.code.sf.net/p/wsjt/wsjtx'
const RAW = (p) => `https://sourceforge.net/p/wsjt/wsjtx/ci/master/tree/${p}?format=raw`
const SOURCES = { 'widgets/mainwindow.cpp': null, 'Radio.cpp': null }

// ---------------------------------------------------------------- fetch + provenance

async function fetchText(url) {
  const r = await fetch(url, { redirect: 'follow' })
  if (!r.ok) throw new Error(`GET ${url} → ${r.status}`)
  return await r.text()
}

// `git ls-remote` is the only reproducible way to name the revision: SourceForge's raw-file
// endpoint serves the branch tip without saying which commit that is.
const revision = execFileSync('git', ['ls-remote', SF_GIT, 'master'], { encoding: 'utf8' })
  .trim()
  .split(/\s+/)[0]
if (!/^[0-9a-f]{40}$/.test(revision)) throw new Error(`bad revision from ls-remote: ${revision}`)

const sha256 = {}
for (const path of Object.keys(SOURCES)) {
  const text = await fetchText(RAW(path))
  SOURCES[path] = text
  sha256[path] = createHash('sha256').update(text, 'utf8').digest('hex')
  process.stderr.write(`fetched ${path} (${text.length} bytes, sha256 ${sha256[path].slice(0, 12)}…)\n`)
}

// ---------------------------------------------------------------- extract, never re-type

/** Pull one `QRegularExpression <name> {R"(<pattern>)"<rest>};` out of a source file. */
function extractRe(file, name) {
  const src = SOURCES[file]
  const re = new RegExp(
    String.raw`QRegularExpression\s+${name}\s*\{\s*R"\(([\s\S]*?)\)"([^}]*)\}`,
    'm',
  )
  const m = src.match(re)
  if (!m) {
    throw new Error(
      `${file}: could not find QRegularExpression ${name}. Upstream moved or renamed it — ` +
        `re-read the source before touching this script; the extraction failing IS the alarm.`,
    )
  }
  return { pattern: m[1], flags: m[2].replace(/^\s*,\s*/, '').trim() }
}

const stdCallRe = extractRe('widgets/mainwindow.cpp', 'standard_call_re')
const alphabetRe = extractRe('Radio.cpp', 'callsign_alphabet_re')
const strictRe = extractRe('Radio.cpp', 'strict_standard_callsign_re')

// The flags are the part a future reader drops. Assert the two we depend on are still there.
for (const f of ['CaseInsensitiveOption', 'ExtendedPatternSyntaxOption']) {
  if (!stdCallRe.flags.includes(f)) {
    throw new Error(`stdCall no longer sets ${f} — the fixture's whole premise changed.`)
  }
}
if (alphabetRe.flags || strictRe.flags) {
  throw new Error('a Radio.cpp regex gained flags — re-measure before trusting this fixture.')
}

// `is_77bit_nonstandard_callsign` is a COMPOSITION of the two Radio.cpp regexes, not a regex of
// its own. Verify the composition rather than transcribe it, so a change upstream is a hard
// failure here instead of a wrong column in the fixture.
const nonstdBody = SOURCES['Radio.cpp'].match(
  /bool\s+is_77bit_nonstandard_callsign\s*\([^)]*\)\s*\{([\s\S]*?)\}/,
)
if (!nonstdBody) throw new Error('Radio.cpp: is_77bit_nonstandard_callsign not found')
const normalised = nonstdBody[1].replace(/\s+/g, ' ').trim()
const EXPECTED_NONSTD =
  'return callsign.contains (callsign_alphabet_re) && !callsign.contains (strict_standard_callsign_re);'
if (normalised !== EXPECTED_NONSTD) {
  throw new Error(
    `Radio.cpp: is_77bit_nonstandard_callsign changed shape.\n  got:      ${normalised}\n` +
      `  expected: ${EXPECTED_NONSTD}\nRe-derive the fixture columns before regenerating.`,
  )
}

// `base_callsign` is a function, not a pattern — lift it whole so the harness runs upstream's
// own code. It is compiled in a temp dir and never written into the tree.
const baseFn = SOURCES['Radio.cpp'].match(
  /QString\s+base_callsign\s*\(QString\s+callsign\)\s*\{[\s\S]*?\n {2}\}/,
)
if (!baseFn) throw new Error('Radio.cpp: base_callsign not found')

// ---------------------------------------------------------------- corpus

const corpus = []
const seen = new Set()
const add = (s) => {
  if (!seen.has(s)) {
    seen.add(s)
    corpus.push(s)
  }
}

// 1. EXHAUSTIVE over a discriminating alphabet, lengths 1..6 — the NORMALISED domain, which is
//    where the transcription claim is made and therefore where the budget belongs. Six is
//    exactly where the grammar tops out (part1 ≤ 2 + part2 ≤ 4 = AA9AAA), so this is a complete
//    sweep of standard-callsign shape space. The alphabet has no redundant members: one generic
//    uppercase letter (A), both suffix letters (R, P — the only two the alternation names), one
//    digit (9), and the slash. 5^1..5^6 = 19,530 strings.
const ALPHA = ['A', 'R', 'P', '9', '/']
const sweep = []
const odometer = (n) => {
  const idx = new Array(n).fill(0)
  for (;;) {
    const s = idx.map((i) => ALPHA[i]).join('')
    sweep.push(s)
    add(s)
    let k = n - 1
    while (k >= 0 && ++idx[k] === ALPHA.length) idx[k--] = 0
    if (k < 0) return
  }
}
for (let n = 1; n <= 6; n++) odometer(n)

// 1b. The SAME shapes pushed off the normalised domain, so the normalisation claim
//     (`nexus(x) == upstream(normalise(x))`) has something to check and its normalised form is
//     guaranteed present above. Lowercase exercises Qt's caseless folding against Rust's
//     `to_ascii_uppercase`; the paddings exercise Qt's non-UCP ASCII-only `\s` against Rust's
//     Unicode `str::trim` — the NBSP and U+2028 rows are the ones that part company.
const PAD = [' ', '\t', '\n', '\r\n', '\v', '\f', ' ', ' ', '　']
for (const s of sweep) {
  if (s.length > 4) continue
  add(s.toLowerCase())
  add(' ' + s.toLowerCase() + ' ')
  // The full padding cross-product only up to length 3 (155 shapes): padding is a property of
  // the string's ENDS, so a longer body adds no class the shorter one did not already exercise.
  if (s.length > 3) continue
  for (const w of PAD) {
    add(s + w)
    add(w + s)
    add(w + s + w)
  }
}

// 2. LENGTH 6..9 — past the exhaustive ceiling, where a standard call actually tops out
//    (AA9AAA = 6, plus "/P" = 8). Every letter/digit body up to 6, crossed with every suffix
//    shape that matters: none, the two real ones in both cases, a near-miss letter, a bare
//    slash, and a doubled suffix.
const bodies = []
;(function grow(prefix) {
  if (prefix.length) bodies.push(prefix)
  if (prefix.length === 6) return
  for (const c of ['A', '9']) grow(prefix + c)
})('')
for (const b of bodies) for (const s of ['', '/P', '/R', '/p', '/r', '/A', '/', '/PP']) add(b + s)

// 3. A slash at every position of the canonical shapes — prefix-portable, suffix-portable and
//    the malformed middles that `packed_suffix`'s `index(...).ge.4` rule turns on.
for (const b of ['A9A', 'AA9AA', 'AA9AAA', '9A9AA', 'A9AAA']) {
  for (let i = 0; i <= b.length; i++) add(b.slice(0, i) + '/' + b.slice(i))
}

// 4. Real-world callsign forms — the classes the 2026-08-06 bugs live in, plus the affixes
//    operators actually send. Each in upper, lower, /P, /R and space-padded.
const REAL = [
  'W9XYZ', 'KD9TAW', 'F4CYH', 'PJ4/K1ABC', 'YW18FIFA', '9A1A', '2E0AAA', '3DA0RS', 'KH8/W1AW',
  'VP2E/AA9A', 'W1AW/4', 'W9XYZ/QRP', 'W1AW/PORTABLE', 'AA1A/QRPP', 'K1ABC/PJ4', 'W1/P',
  'AB1CDE', 'A1', '1A', '<W9XYZ>', '<...>', 'B4/P', 'G/W1AW', 'W1AW/MM', 'W1AW/AM', 'GM/W1AW',
  'ZS6/G0ABC', 'VK9/W1AW', 'W1AW/VK9', 'KD9TAW/LH', 'N0CALL/LGT', 'OH/DL1ABC', 'DL1ABC/OH',
  'HB0/DL1ABC', 'JW/LA1ABC', 'W4/W1AW', 'W1AW/W4', 'CQ', 'QRZ', 'DE', '73', 'RR73',
]
for (const c of REAL) {
  add(c)
  add(c.toLowerCase())
  add(c + '/P')
  add(c + '/R')
  add(' ' + c + ' ')
}

// 5. ADVERSARIAL — where the FLAG semantics show, and the only place Nexus and upstream are
//    expected to disagree. ASCII whitespace (which Qt's non-UCP `\s` and Rust's `trim` both
//    accept), the Unicode whitespace only Rust accepts, the trailing newline PCRE2's `$` lets
//    through, and the Unicode case folds only Qt accepts.
const WS = ['\n', '\r\n', '\t', ' ', '\r', '\v', '\f', ' ', ' ', ' ', '　']
for (const c of ['W9XYZ', 'F4CYH/P', '9A1A', 'PJ4/K1ABC']) {
  for (const w of WS) {
    add(c + w)
    add(w + c)
    add(w + c + w)
  }
}
for (const c of ['ſ9XYZ', 'K1ABC', 'W9XYK', 'İ1ABC', 'W9XYİ', 'ᏦABC', 'Ω9ABC']) add(c)
add('')
add(' ')
add('\n')

process.stderr.write(`corpus: ${corpus.length} inputs\n`)

// A curated subset for `base_callsign`. Storing it for all 20k inputs would double the fixture
// for no coverage: `base_callsign` only does anything when a '/' is present, and the divergence
// class Nexus deliberately owns is compound calls, which the real-world list enumerates.
//
// NORMALISED FORMS ONLY — uppercase, trimmed, unbracketed. `base_call` additionally strips an
// i3=4 `<...>` wrapper and trims, which upstream does not do because upstream never sees a
// bracketed token; comparing those would measure Nexus's own input handling against a function
// that was never asked the question, and bury the real finding (the SPLIT RULE differs) under a
// hundred rows of noise.
const baseSeen = new Set()
const baseSubset = []
const addBase = (s) => {
  if (!baseSeen.has(s)) {
    baseSeen.add(s)
    baseSubset.push(s)
  }
}
for (const c of REAL) {
  if (c.startsWith('<')) continue // an i3=4 hashed token is not a callsign; upstream never sees one
  addBase(c)
  // …and no doubled suffix on an already-compound call. `PJ4/K1ABC/P` is not a form anyone
  // sends, and forty synthetic rows of it would bury the two real findings under noise.
  if (!c.includes('/')) {
    addBase(c + '/P')
    addBase(c + '/R')
  }
}
for (const b of ['A9A', 'AA9AA', 'AA9AAA', '9A9AA', 'A9AAA']) {
  for (let i = 0; i <= b.length; i++) addBase(b.slice(0, i) + '/' + b.slice(i))
}
for (const s of baseSubset) add(s) // every base case is also a predicate case
process.stderr.write(`base_callsign subset: ${baseSubset.length} inputs\n`)

// ---------------------------------------------------------------- run the Qt oracle

const work = mkdtempSync(join(tmpdir(), 'wsjtx-oracle-'))
try {
  const harness = `// GENERATED by scripts/gen-wsjtx-callsign-oracle.mjs — temp build, never committed.
// Patterns and base_callsign lifted verbatim from WSJT-X master ${revision}.
#include <QCoreApplication>
#include <QRegularExpression>
#include <QString>
#include <cstdio>
#include <string>
#include <iostream>

static QRegularExpression standard_call_re {R"(${stdCallRe.pattern})", ${stdCallRe.flags}};
static QRegularExpression callsign_alphabet_re {R"(${alphabetRe.pattern})"};
static QRegularExpression strict_standard_callsign_re {R"(${strictRe.pattern})"};

namespace Radio {
${baseFn[0]}
}

int main(int argc, char** argv) {
  QCoreApplication app(argc, argv);
  std::string line, out;
  auto hx = [](char c) -> int { return c <= '9' ? c - '0' : (c | 32) - 'a' + 10; };
  while (std::getline(std::cin, line)) {
    std::string raw;
    for (size_t i = 0; i + 1 < line.size(); i += 2) raw += (char)((hx(line[i]) << 4) | hx(line[i + 1]));
    QString w = QString::fromUtf8(raw.c_str(), (int)raw.size());
    bool s = standard_call_re.match(w).hasMatch();
    bool a = w.contains(callsign_alphabet_re);
    bool t = w.contains(strict_standard_callsign_re);
    QByteArray b = Radio::base_callsign(w).toUtf8();
    out += (s ? '1' : '0');
    out += (a && !t ? '1' : '0');
    out += ' ';
    static const char* H = "0123456789abcdef";
    for (int i = 0; i < b.size(); ++i) { out += H[(unsigned char)b[i] >> 4]; out += H[(unsigned char)b[i] & 15]; }
    out += '\\n';
    if (out.size() > (1u << 16)) { fwrite(out.data(), 1, out.size(), stdout); out.clear(); }
  }
  fwrite(out.data(), 1, out.size(), stdout);
  return 0;
}
`
  writeFileSync(join(work, 'oracle.cpp'), harness)
  const qtFlags = execFileSync('pkg-config', ['--cflags', '--libs', 'Qt5Core'], { encoding: 'utf8' })
    .trim()
    .split(/\s+/)
  execFileSync('g++', ['-std=c++17', '-O2', '-fPIC', '-o', join(work, 'oracle'), join(work, 'oracle.cpp'), ...qtFlags], {
    stdio: ['ignore', 'inherit', 'inherit'],
  })

  const runOracle = (inputs) => {
    const hex = inputs.map((s) => Buffer.from(s, 'utf8').toString('hex')).join('\n') + '\n'
    const raw = execFileSync(join(work, 'oracle'), [], { input: hex, encoding: 'utf8', maxBuffer: 1 << 28 })
    const lines = raw.split('\n').filter((l) => l.length)
    if (lines.length !== inputs.length) throw new Error(`oracle returned ${lines.length} rows for ${inputs.length} inputs`)
    return lines
  }

  const rows = runOracle(corpus)
  const verdicts = rows.map((l) => l.split(' ')[0]).join('')
  const baseRows = runOracle(baseSubset)
  const bases = baseRows.map((l) => Buffer.from(l.split(' ')[1] ?? '', 'hex').toString('utf8'))

  const fixture = {
    _: [
      'GENERATED — do not hand-edit. Regenerate: node scripts/gen-wsjtx-callsign-oracle.mjs',
      'Measurements of the REAL WSJT-X Qt regexes, with their real flags, over a deterministic corpus.',
      'Read by crates/tempo-core/tests/wsjtx_predicate_differential.rs and ui/src/txMessages.oracle.test.ts.',
    ],
    provenance: {
      upstream: SF_GIT,
      revision,
      sha256,
      // No timestamp, deliberately: regenerating an unchanged tree must be a no-op diff, or
      // nobody re-runs this and staleness stops being detectable. `revision` and `sha256` are
      // what actually say when the measurement was taken. The Qt version stays because PCRE2's
      // behaviour is genuinely part of what was measured — that one moving IS worth seeing.
      qt: execFileSync('pkg-config', ['--modversion', 'Qt5Core'], { encoding: 'utf8' }).trim(),
      extracted: {
        stdCall: { file: 'widgets/mainwindow.cpp', symbol: 'MainWindow::stdCall / standard_call_re' },
        nonstandard77: { file: 'Radio.cpp', symbol: 'Radio::is_77bit_nonstandard_callsign' },
        baseCallsign: { file: 'Radio.cpp', symbol: 'Radio::base_callsign' },
      },
    },
    // The pattern TEXT and the FLAGS, so a future reader can see what was measured and diff it
    // against a later upstream without re-running anything.
    patterns: {
      standard_call_re: { pattern: stdCallRe.pattern, flags: stdCallRe.flags },
      callsign_alphabet_re: { pattern: alphabetRe.pattern, flags: alphabetRe.flags || null },
      strict_standard_callsign_re: { pattern: strictRe.pattern, flags: strictRe.flags || null },
      is_77bit_nonstandard_callsign: EXPECTED_NONSTD,
    },
    // `verdicts[2*i]` = stdCall(inputs[i]); `verdicts[2*i+1]` = is_77bit_nonstandard_callsign.
    // Two chars per input rather than an object per input: 20k objects is a 900 KB fixture and
    // an unreadable diff; this is 250 KB and a moved character is a moved verdict.
    columns: 'stdCall,nonstandard77',
    inputs: corpus,
    verdicts,
    // Curated subset — see the header. `baseInputs[i]` → upstream `Radio::base_callsign`.
    baseInputs: baseSubset,
    baseCallsign: bases,
  }
  writeFileSync(out, JSON.stringify(fixture, null, 1) + '\n')
  process.stderr.write(`wrote ${out} (${readFileSync(out).length} bytes)\n`)
} finally {
  rmSync(work, { recursive: true, force: true })
}
