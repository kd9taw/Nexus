// WIRE CONTRACT. The application version negotiated from a station's request headers must be
// bit-for-bit what it was before the ladder was collapsed into one table, for every combination of
// headers - not only the ones a current desktop sends. So the nested ternary that used to live in
// remote/src/index.ts (and the `[1..17]` literals from remote/src/room.ts) are transcribed here
// verbatim as the ORACLE, and every case below is checked against both the oracle and, where the
// answer is nameable, a hand-written expectation. Two independent oracles: a transcription slip
// that happened to match a derivation slip would still have to match the table too.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { APPLICATION_EXTENSIONS, APPLICATION_VERSION, knownApplicationVersion, negotiatedApplicationVersion }
  from '../src/application-version.ts'

// --- the code as it stood before this commit, unchanged except `request.headers` -> `headers` ---
const legacyNegotiated = headers => (
  headers.get('x-nexus-application-query-version') === '1' && headers.get('x-nexus-application-stream-version') === '2'
    ? headers.get('x-nexus-application-recall-version') === '1'
      ? headers.get('x-nexus-application-keyboard-version') === '1'
        ? headers.get('x-nexus-application-insights-version') === '1'
          ? headers.get('x-nexus-application-dxpeditions-version') === '1'
            ? headers.get('x-nexus-application-memories-version') === '1'
              ? headers.get('x-nexus-application-ota-version') === '1'
                ? headers.get('x-nexus-application-field-day-version') === '1'
                  ? headers.get('x-nexus-application-js8-version') === '1' ? headers.get('x-nexus-application-station-modes-version') === '1' ? headers.get('x-nexus-application-navigation-version') === '1' ? headers.get('x-nexus-application-configuration-version') === '1' ? headers.get('x-nexus-application-lookups-version') === '1' ? headers.get('x-nexus-application-alerts-version') === '1' ? headers.get('x-nexus-application-rotator-version') === '1' ? 17 : 16 : 15 : 14 : 13 : 12 : 11 : 10 : 9 : 8 : 7 : 6
          : 5
        : 4
      : 3
    : headers.get('x-nexus-application-stream-version') === '2' ? 2
    : ['1', '2'].includes(headers.get('x-nexus-application-version') ?? '') ? Number(headers.get('x-nexus-application-version')) : 0)
const legacyKnown = version => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17].includes(version)

// The ladder as a station sends it, in order. Spelled out rather than read from the table so a
// reordering or a renamed header is a test failure and not a silently redefined wire.
const LADDER = [
  ['x-nexus-application-stream-version', '2'],
  ['x-nexus-application-query-version', '1'],
  ['x-nexus-application-recall-version', '1'],
  ['x-nexus-application-keyboard-version', '1'],
  ['x-nexus-application-insights-version', '1'],
  ['x-nexus-application-dxpeditions-version', '1'],
  ['x-nexus-application-memories-version', '1'],
  ['x-nexus-application-ota-version', '1'],
  ['x-nexus-application-field-day-version', '1'],
  ['x-nexus-application-js8-version', '1'],
  ['x-nexus-application-station-modes-version', '1'],
  ['x-nexus-application-navigation-version', '1'],
  ['x-nexus-application-configuration-version', '1'],
  ['x-nexus-application-lookups-version', '1'],
  ['x-nexus-application-alerts-version', '1'],
  ['x-nexus-application-rotator-version', '1'],
]
const headersFrom = entries => new Headers(entries.filter(([, value]) => value !== undefined))
const agree = (entries, expected, label) => {
  const request = headersFrom(entries)
  assert.equal(legacyNegotiated(request), negotiatedApplicationVersion(request), `oracle disagrees: ${label}`)
  if (expected !== undefined) assert.equal(negotiatedApplicationVersion(request), expected, label)
}

test('the ladder table is the one the desktop sends, in order, and ends where /config says', () => {
  assert.deepEqual(APPLICATION_EXTENSIONS.map(row => [...row]), LADDER)
  assert.equal(APPLICATION_VERSION, 17)
  assert.equal(APPLICATION_VERSION, LADDER.length + 1)
})

test('every prefix of the ladder negotiates its own rung and nothing above it', () => {
  for (let sent = 0; sent <= LADDER.length; sent++) {
    const rungs = LADDER.slice(0, sent)
    // With the legacy header, as a current desktop sends it: a sent prefix of length n is n + 1,
    // and an empty prefix falls back to the legacy header's own number.
    agree([['x-nexus-application-version', '1'], ...rungs], sent + 1, `prefix ${sent} with legacy 1`)
    // Without it: identical above the base, 0 below it.
    agree(rungs, sent === 0 ? 0 : sent + 1, `prefix ${sent}, no legacy header`)
  }
})

test('one missing extension costs every rung from there up', () => {
  for (let dropped = 0; dropped < LADDER.length; dropped++) {
    const entries = [['x-nexus-application-version', '1'], ...LADDER.filter((_, index) => index !== dropped)]
    agree(entries, dropped + 1, `missing ${LADDER[dropped][0]}`)
  }
})

test('one extension at the wrong value reads as absent, not as a different version', () => {
  for (let wrong = 0; wrong < LADDER.length; wrong++) {
    for (const value of ['0', '1', '2', '3', '', 'x']) {
      if (value === LADDER[wrong][1]) continue
      const entries = [['x-nexus-application-version', '1'],
        ...LADDER.map(([header, pinned], index) => [header, index === wrong ? value : pinned])]
      agree(entries, wrong + 1, `${LADDER[wrong][0]}=${JSON.stringify(value)}`)
    }
  }
})

test('the legacy header is the only value read as a number', () => {
  for (const [value, expected] of [[undefined, 0], ['', 0], ['0', 0], ['1', 1], ['2', 2], ['3', 0], ['17', 0], ['x', 0]]) {
    agree([['x-nexus-application-version', value]], expected, `legacy ${JSON.stringify(value)} alone`)
    // The stream capability alone still advertises 2, whatever the legacy header says.
    agree([['x-nexus-application-version', value], LADDER[0]], 2, `legacy ${JSON.stringify(value)} + stream`)
    // ...and a full ladder overrides it entirely.
    agree([['x-nexus-application-version', value], ...LADDER], 17, `legacy ${JSON.stringify(value)} + full ladder`)
  }
})

test('the whole 3^16 header space agrees with the old ternary on a fixed pseudo-random sample', () => {
  // Deterministic (mulberry32): a failure is reproducible, and a rerun cannot flake green.
  let seed = 0x9e3779b9
  const random = () => {
    seed = (seed + 0x6d2b79f5) >>> 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
  const legacyValues = [undefined, '', '1', '2', '3']
  for (let round = 0; round < 8000; round++) {
    const entries = [['x-nexus-application-version', legacyValues[Math.floor(random() * legacyValues.length)]]]
    for (const [header, pinned] of LADDER) {
      const state = Math.floor(random() * 3)
      if (state === 0) entries.push([header, pinned])
      else if (state === 1) entries.push([header, '9'])
    }
    agree(entries, undefined, `round ${round}: ${JSON.stringify(entries)}`)
  }
})

test('a version carried back from an attachment is known exactly when the old literal said so', () => {
  for (const version of [-1, -0, 0, 0.5, 1, 1.5, 2, 9, 16, 17, 17.0001, 18, 99, NaN, Infinity, -Infinity]) {
    assert.equal(knownApplicationVersion(version), legacyKnown(version), `knownApplicationVersion(${version})`)
  }
})

test('the equivalence check can fail', () => {
  // Positive control. Without this, every assertion above would also pass against a derivation
  // that ignored its input entirely - agreement is only evidence if disagreement is detectable.
  const full = headersFrom(LADDER), partial = headersFrom(LADDER.slice(0, 3))
  assert.notEqual(negotiatedApplicationVersion(full), negotiatedApplicationVersion(partial))
  assert.equal(legacyNegotiated(full), 17)
  assert.equal(legacyNegotiated(partial), 4)
})
