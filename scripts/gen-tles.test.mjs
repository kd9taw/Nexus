// Gates for the amateur-satellite catalog mirror (scripts/gen-tles.mjs).
//
// The mirror is unattended CI that publishes to every install: a derivation
// bug here silently deletes birds from the list operators star from, and a
// hole in a sanity gate publishes it. So the DERIVATION is tested as a pure
// function over committed fixtures (scripts/fixtures/tles/), and every gate
// gets a payload that must fail it.
//
// Run: node --test scripts/gen-tles.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, mkdtempSync, writeFileSync, existsSync, cpSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  assemble,
  deriveAmateurCatalog,
  FIXTURE_LIMITS,
  PROD_LIMITS,
  GateError,
  parse3le,
} from './gen-tles.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const FIX = join(HERE, 'fixtures', 'tles')
const SCRIPT = join(HERE, 'gen-tles.mjs')

/// 2026-08-01T00:00:00Z — one day past the fixture corpus' fetch, so the
/// freshness gates see it exactly as a client would have, forever.
const NOW = 1_785_542_400

const read = (f) => readFileSync(join(FIX, f), 'utf8')

const sources = () => ({
  satellites: read('satnogs-satellites.json'),
  transmitters: read('satnogs-transmitters.json'),
  satnogsTle: read('satnogs-tle.json'),
  ctAmateur: read('celestrak-amateur.tle'),
  ctSatnogs: read('celestrak-satnogs.tle'),
})

const build = (over = {}) =>
  assemble({ ...sources(), now: NOW, limits: FIXTURE_LIMITS, ...over })

const derive = () =>
  deriveAmateurCatalog(
    JSON.parse(read('satnogs-satellites.json')),
    JSON.parse(read('satnogs-transmitters.json')),
    // The FIXED clock, never the wall one: the re-entry window is a clock
    // decision, and a fixture whose population drifts with the calendar is a
    // test that fails on a date instead of on a change.
    NOW,
  )

const noradOf = (line1) => Number(line1.slice(2, 7))

// --- the amateur derivation ------------------------------------------------

test('SO-50 survives even though every one of its transmitters says service "Unknown"', () => {
  // THE trap the research measured: `service == "Amateur"` alone loses the
  // single most-worked FM bird. Only the band predicate keeps it.
  const { birds } = derive()
  const so50 = birds.find((b) => b.norad === 27607)
  assert.ok(so50, 'SO-50 (27607) must be in the amateur population')
  assert.equal(so50.serviceAmateur, false, 'SatNOGS never calls it Amateur')
  assert.equal(so50.bandAmateur, true, '145.025 / 436.795 are amateur allocations')
})

test('a non-amateur observable with an out-of-band downlink is not amateur', () => {
  const { birds } = derive()
  assert.ok(!birds.some((b) => b.norad === 25338), 'NOAA 15 (137 MHz APT) must drop')
})

test('a dead satellite is LISTED as dead, and never carries elements', () => {
  // IO-117 / GREENCUBE: transmitter.alive lags reality, satellite.status moved
  // first, so satellite status is authoritative for `active`. The bird still
  // gets a catalog row: an operator who starred it must find out that it died,
  // not find the row gone.
  const { birds } = derive()
  const gc = birds.find((b) => b.norad === 53106)
  assert.ok(gc, 'GREENCUBE stays in the catalog')
  assert.equal(gc.status, 'dead')
  assert.equal(gc.active, false, 'nothing to publish elements for')
})

test('a live satellite whose transmitters are all silent is listed, marked silent', () => {
  const { birds } = derive()
  const ao85 = birds.find((b) => b.norad === 40967)
  assert.ok(ao85, 'AO-85 is alive and in orbit')
  assert.equal(ao85.status, 'alive')
  assert.equal(ao85.amateur, false, 'every one of its transmitters is dead')
  assert.equal(ao85.active, false)
})

test('a bird still on the pad is listed as pre-launch', () => {
  const { birds } = derive()
  const pre = birds.find((b) => b.norad === 98777)
  assert.ok(pre, 'a future bird is worth knowing about')
  assert.equal(pre.status, 'future')
  assert.equal(pre.active, false, 'nothing flies yet')
})

test('a RECENT re-entry keeps its row; an old one is history, not a catalog', () => {
  const { birds } = derive()
  assert.ok(birds.some((b) => b.norad === 50988), 'TEVEL-1 decayed 5 months ago')
  assert.ok(
    birds.some((b) => b.norad === 50989),
    'TEVEL-2 has no decay date yet — its record was edited last month',
  )
  assert.ok(!birds.some((b) => b.norad === 50987), 'TEVEL-0 decayed in 2019')
  assert.ok(!birds.some((b) => b.norad == null), 'no NORAD, no bird')
})

test('a placeholder NORAD resolves to its follow id and appears exactly once', () => {
  // Marina: SatNOGS carries 98293 with norad_follow_id 69920; Celestrak
  // catalogues her as 69920. Joining on norad_cat_id alone doubles the bird.
  const { birds } = derive()
  const marina = birds.filter((b) => b.norad === 69920 || b.satnogsNorad === 98293)
  assert.equal(marina.length, 1, 'one bird, not a phantom plus a real one')
  assert.equal(marina[0].norad, 69920, 'the catalogued id is the identity')
  assert.equal(marina[0].satnogsNorad, 98293, 'SatNOGS is still asked under the placeholder')
})

test('the transmitter join falls back to NORAD when a record carries no sat_id', () => {
  const { birds } = derive()
  const ao7 = birds.find((b) => b.norad === 7530)
  assert.ok(ao7, 'AO-7 joins by norad_cat_id alone')
  assert.equal(ao7.bandAmateur, true, '29.502 MHz is the 10 m satellite segment')
})

// --- the union -------------------------------------------------------------

test('the union publishes elements for exactly the ACTIVE amateur population', () => {
  const { manifest } = build()
  const cat = manifest.catalog
  assert.equal(cat.length, 18, 'eighteen amateur birds in the fixture')
  assert.equal(manifest.catalogCount, 18)
  const withEls = cat.filter((b) => b.src)
  assert.equal(withEls.length, 12, 'twelve of them are active AND have current elements')
  assert.equal(manifest.count, manifest.elements.length)
  assert.equal(manifest.count, 12)
  // Nothing outside the population rides along.
  for (const el of manifest.elements) {
    assert.ok(
      cat.some((b) => b.norad === noradOf(el.line1)),
      `${el.name} is not in the amateur catalog`,
    )
  }
  assert.ok(
    !manifest.elements.some((e) => noradOf(e.line1) === 25338),
    'NOAA 15 rode in on GROUP=satnogs and must be filtered out',
  )
  // The listed-but-not-drawn line: Celestrak carries AO-85's elements, and the
  // mirror deliberately does not publish them — a bird with nothing left to
  // work is a row that says "silent", never a track on the map.
  assert.ok(
    !manifest.elements.some((e) => noradOf(e.line1) === 40967),
    'AO-85 is in GROUP=amateur, but silent birds carry no elements',
  )
  const ao85 = cat.find((b) => b.norad === 40967)
  assert.equal(ao85.src, undefined, 'and its catalog row says it has none')
  assert.equal(ao85.amateur, false)
})

test('a bird that leaves the population keeps a row that says what happened', () => {
  // THE regression this whole catalog exists to prevent: a starred bird that
  // dies, re-enters or goes silent must never simply stop existing — the
  // operator would have no row to read and no ★ to click off.
  const { manifest } = build()
  const row = (n) => manifest.catalog.find((b) => b.norad === n)
  assert.deepEqual(
    { ...row(53106) },
    { norad: 53106, name: 'GREENCUBE', status: 'dead', amateur: true, decayed: false },
    'dead in orbit: status dead, its transmitter record still claims alive',
  )
  assert.equal(row(40967).status, 'alive')
  assert.equal(row(40967).amateur, false, 'alive but nothing left to work')
  assert.equal(row(50988).status, 're-entered')
  assert.equal(row(50988).decayed, true)
  assert.equal(row(98777).status, 'future')
  assert.ok(!row(50987), 'a 2019 re-entry is not a bird anyone is chasing')
  assert.ok(!row(25338), 'and a non-amateur observable never had a row to lose')
})

test('an object in GROUP=amateur that the derivation drops is named out loud', () => {
  // The one way this list can be SHORTER than the one Nexus used to ship.
  // Two of Celestrak's 97 have no amateur transmitter on record at SatNOGS;
  // dropping them may be right, dropping them silently is not.
  const sats = JSON.parse(read('satnogs-satellites.json')).filter((s) => s.norad_cat_id !== 40967)
  const { log } = build({ satellites: JSON.stringify(sats) })
  const line = log.find((l) => /GROUP=amateur carries/.test(l))
  assert.ok(line, 'the divergence is reported')
  assert.match(line, /40967/)
})

test('the run reports the birds that left since the last publish, by name', () => {
  // A count ratchet cannot see a curation regression that swaps birds. The
  // previous manifest names them, so the job log names them too.
  const { log } = build({
    prevCatalog: [
      { norad: 25544, name: 'ISS (ZARYA)' },
      { norad: 99999, name: 'GONE BIRD' },
    ],
  })
  const line = log.find((l) => /^departed/.test(l))
  assert.ok(line, 'a departure is reported')
  assert.match(line, /GONE BIRD \(99999\)/)
  assert.ok(!/ISS/.test(line), 'a bird that is still listed has not departed')
})

test('a known-amateur bird with no elements stays in the catalog, sourceless', () => {
  // The honesty line: it must not silently vanish — the client needs to be
  // able to say "no current elements" rather than pretend the bird is gone.
  const { manifest, log } = build()
  const quiet = manifest.catalog.find((b) => b.norad === 61999)
  assert.ok(quiet, 'the bird stays in the catalog')
  assert.equal(quiet.src, undefined, 'with no element source')
  assert.equal(quiet.status, 'alive')
  assert.ok(log.some((l) => /uncovered/.test(l)), 'the run reports what it could not cover')
})

test('overlapping groups draw a bird once, and the newest epoch wins', () => {
  const { manifest } = build()
  const iss = manifest.elements.filter((e) => noradOf(e.line1) === 25544)
  assert.equal(iss.length, 1, 'ISS is in both Celestrak groups and the SatNOGS TLEs')
  assert.equal(iss[0].line1.slice(18, 32), '26212.89378683', 'the freshest set wins')
})

test('a placeholder bird publishes the catalogued element, not the placeholder one', () => {
  // Both sources have Marina: SatNOGS under 98293 (older), Celestrak under
  // 69920 (fresher). Publishing both would draw her twice under two ids.
  const { manifest } = build()
  const marina = manifest.elements.filter((e) => /MARINA/i.test(e.name))
  assert.equal(marina.length, 1)
  assert.equal(noradOf(marina[0].line1), 69920)
  assert.ok(!manifest.elements.some((e) => noradOf(e.line1) === 98293))
})

test('every catalog entry with elements agrees with its element line', () => {
  // The identity invariant the client depends on: it keys birds by the NORAD
  // in the element line, so a catalog row that disagrees is a status stamped
  // on the wrong bird.
  const { manifest } = build()
  const byNorad = new Map(manifest.elements.map((e) => [noradOf(e.line1), e]))
  for (const b of manifest.catalog) {
    if (!b.src) continue
    assert.ok(byNorad.has(b.norad), `catalog ${b.norad} claims elements it has not got`)
  }
  assert.equal(byNorad.size, manifest.catalog.filter((b) => b.src).length)
})

test('a bird only SatNOGS has elements for is still published', () => {
  const { manifest } = build()
  const lapan = manifest.catalog.find((b) => b.norad === 40931)
  assert.equal(lapan.src, 'satnogs-tle', 'Celestrak 404s IO-86; SatNOGS carries it')
})

test('the manifest reports per-source provenance and counts', () => {
  const { manifest } = build()
  assert.equal(manifest.schema, 2)
  assert.deepEqual(manifest.sources, {
    'celestrak-amateur': 9,
    'celestrak-satnogs': 1,
    'satnogs-tle': 2,
  })
  const summed = Object.values(manifest.sources).reduce((a, b) => a + b, 0)
  assert.equal(summed, manifest.count)
})

test('per-bird status fields ride the catalog', () => {
  const { manifest } = build()
  const iss = manifest.catalog.find((b) => b.norad === 25544)
  assert.deepEqual(iss, {
    norad: 25544,
    name: 'ISS (ZARYA)',
    status: 'alive',
    amateur: true,
    decayed: false,
    src: 'celestrak-amateur',
  })
})

// --- the gates -------------------------------------------------------------

/// Run a build that must be REFUSED, and hand the refusal back so the caller
/// can pin which gate caught it (a payload rejected for the wrong reason is a
/// gate that isn't doing its job).
const refuse = (fn) => {
  try {
    fn()
  } catch (e) {
    assert.ok(e instanceof GateError, `expected a gate refusal, got ${e}`)
    return e
  }
  assert.fail('expected a gate refusal, the payload was accepted')
}

const rejects = (over, re) => assert.match(refuse(() => build(over)).message, re)

test('the ISS canary refuses a set that lost it', () => {
  const ct = read('celestrak-amateur.tle')
    .split('\n')
    .filter((l) => !/25544/.test(l) && !/^ISS/.test(l))
    .join('\n')
  const sn = JSON.parse(read('satnogs-tle.json')).filter((r) => r.norad_cat_id !== 25544)
  rejects(
    { ctAmateur: ct, ctSatnogs: read('celestrak-satnogs.tle').replace(/^ISS[\s\S]*$/m, ''), satnogsTle: JSON.stringify(sn) },
    /25544/,
  )
})

test('the worked-bird canary tolerates one loss and refuses two', () => {
  // A gate that hard-requires every well-known bird stalls the mirror the day
  // one of them re-enters; a gate that requires none cannot see the amateur
  // derivation breaking. The rule is "most of them, still here".
  const drop = (norads) => {
    const sats = JSON.parse(read('satnogs-satellites.json')).map((s) =>
      norads.includes(s.norad_cat_id) ? { ...s, status: 're-entered' } : s,
    )
    return { satellites: JSON.stringify(sats) }
  }
  assert.doesNotThrow(() => build(drop([43678])))
  rejects(drop([43678, 39444]), /worked-bird canar/i)
})

test('the count ratchet refuses a silent mass shrink, and the override says so out loud', () => {
  rejects({ prevCount: 1000 }, /floor/)
  const { manifest, log } = build({ prevCount: 1000, ratchetOverride: true })
  assert.equal(manifest.count, 12)
  assert.ok(
    log.some((l) => /RATCHET OVERRIDE/.test(l) && /1000/.test(l)),
    'the override must name the baseline it waived',
  )
})

test('the ratchet is waived only by the override, never by a missing baseline', () => {
  assert.doesNotThrow(() => build({ prevCount: 0 }))
  rejects({ prevCount: 1000, ratchetOverride: false }, /floor/)
})

test('a broken amateur join is refused rather than published', () => {
  // At PRODUCTION limits, over a synthetic catalog big enough for the ratio
  // gates to have an opinion (the committed fixture is 13 birds by design).
  const at = (over) => refuse(() => assemble({ ...sources(), now: NOW, limits: PROD_LIMITS, ...over }))
  const catalog = (n, amateurOf) => {
    const sats = []
    const tx = []
    for (let i = 0; i < n; i++) {
      const norad = 700000 + i
      sats.push({ sat_id: 'S' + i, norad_cat_id: norad, name: 'SAT ' + i, status: 'alive' })
      const a = amateurOf(i)
      if (!a) continue
      tx.push({
        uuid: 'T' + i,
        description: 'x',
        alive: true,
        service: a.service ? 'Amateur' : 'Unknown',
        downlink_low: a.band ? 435_000_000 : 137_620_000,
        norad_cat_id: norad,
        sat_id: 'S' + i,
      })
    }
    return { satellites: JSON.stringify(sats), transmitters: JSON.stringify(tx) }
  }

  // The population vanished — the predicate broke outright.
  assert.match(at({ transmitters: '[]' }).message, /population 0/)
  // The predicate stopped discriminating. 700 of 800 is inside the population
  // ceiling, so only the SHARE gate can see it — a broken predicate does not
  // have to overflow to be wrong.
  assert.match(
    at(catalog(800, (i) => (i < 700 ? { service: true, band: true } : null))).message,
    /share .* outside/,
  )
  // The `service` field emptied upstream. The band clause is a strict superset
  // of the service clause, so the population size and its share are UNMOVED —
  // only the per-clause census can see this one.
  assert.match(
    at(catalog(1000, (i) => (i < 300 ? { service: false, band: true } : null))).message,
    /service=="Amateur"/,
  )
  // `downlink_low` changed shape (units, nesting, a string): the service
  // clause still finds birds, the band clause goes dark.
  assert.match(
    at(catalog(1000, (i) => (i < 300 ? { service: true, band: false } : null))).message,
    /amateur band/,
  )
})

test("Celestrak's in-cycle refusal is a 200 with prose, and must not read as data", () => {
  const inCycle =
    'GP data has not updated since your last successful download of GROUP=amateur at 2026-08-01 18:17:46 UTC.\n'
  rejects({ ctAmateur: inCycle }, /has not updated/)
  rejects({ ctSatnogs: 'No GP data found\n' }, /No GP data found/)
})

test('a corrupt line drops its bird alone; wholesale corruption refuses the payload', () => {
  // Break the checksum on line 1 of named birds. AO-10 (14129) is carried by
  // Celestrak alone, so losing it really costs a bird — a bird the other two
  // legs also have would just be re-covered.
  const corrupt = (text, norads) =>
    text
      .split('\n')
      .map((l) =>
        /^1 /.test(l) && l.length === 69 && norads.includes(Number(l.slice(2, 7)))
          ? l.slice(0, 68) + (l[68] === '9' ? '8' : '9')
          : l,
      )
      .join('\n')
  const ct = read('celestrak-amateur.tle')
  const { manifest, log } = build({ ctAmateur: corrupt(ct, [14129]) })
  assert.equal(manifest.count, 11, 'the corrupt bird drops, the rest publish')
  assert.ok(!manifest.catalog.find((b) => b.norad === 14129).src, 'and says it has no elements')
  assert.ok(log.some((l) => /rejected/.test(l)))
  rejects(
    { ctAmateur: corrupt(ct, [14129, 25544, 27607, 44909, 43017, 7530, 43678, 39444]) },
    /integrity/,
  )
})

test('a structurally surprising feed is a fault, not a short set', () => {
  rejects({ ctAmateur: read('celestrak-amateur.tle').split('\n').slice(0, 5).join('\n') }, /multiple of 3/)
  rejects({ satnogsTle: '{"detail":"throttled"}' }, /not a JSON array/)
  rejects({ satellites: 'null' }, /not a JSON array/)
})

test('freshness is median-shaped, and never publishes what the client will refuse', () => {
  // The client twin (propagation::live::tle::validate_tles) refuses a set
  // unless half its birds are under 3 d old. Publishing one it will refuse
  // ages every install's elements for nothing.
  rejects({ now: NOW + 40 * 86_400 }, /median/)
  rejects({ now: NOW + 5 * 86_400 }, /under 3 d/)
})

test('one nameless upstream record costs its NAME, never the whole run', () => {
  // SatNOGS records can carry an empty `tle0`. Serializing that as a 3LE
  // triple writes a BLANK name line, the re-parse drops it, and the whole
  // run died on "line count is not a multiple of 3" — one upstream typo
  // halting the mirror, blaming a truncated feed for our own gap.
  const sn = JSON.parse(read('satnogs-tle.json')).map((r) =>
    r.norad_cat_id === 40931 ? { ...r, tle0: '' } : r,
  )
  const { manifest } = build({ satnogsTle: JSON.stringify(sn) })
  const lapan = manifest.elements.find((e) => noradOf(e.line1) === 40931)
  assert.ok(lapan, 'the bird is still published')
  assert.equal(lapan.name, 'LAPAN-A2', "the catalog's own name stands in")
  assert.equal(manifest.catalog.find((b) => b.norad === 40931).name, 'LAPAN-A2')
  assert.ok(
    manifest.elements.every((e) => e.name.trim() !== ''),
    'no element may be published nameless — the 3LE form has nowhere to put it',
  )
})

test('the published text and JSON round-trip to the same elements', () => {
  const { manifest, threeLe } = build()
  assert.deepEqual(parse3le(threeLe, 'roundtrip').elements.map((e) => ({
    name: e.name,
    line1: e.line1,
    line2: e.line2,
  })), manifest.elements)
})

// --- end to end ------------------------------------------------------------

test('the script publishes from fixtures, and publishes nothing when they degrade', () => {
  const run = (dir, out, env = {}) =>
    execFileSync(process.execPath, [SCRIPT, out], {
      env: { ...process.env, TLES_FIXTURE_DIR: dir, TLES_NOW: String(NOW), ...env },
      encoding: 'utf8',
      stdio: 'pipe',
    })

  const good = mkdtempSync(join(tmpdir(), 'tles-ok-'))
  run(FIX, good)
  const manifest = JSON.parse(readFileSync(join(good, 'tles.json'), 'utf8'))
  assert.equal(manifest.count, 12)
  assert.equal(manifest.catalog.length, 18)
  assert.ok(existsSync(join(good, 'tles.txt')), 'the 3LE import path keeps its file')

  const bad = mkdtempSync(join(tmpdir(), 'tles-bad-'))
  const badFix = mkdtempSync(join(tmpdir(), 'tles-fix-'))
  cpSync(FIX, badFix, { recursive: true })
  writeFileSync(join(badFix, 'satnogs-transmitters.json'), '[]')
  assert.throws(() => run(badFix, bad), /Command failed|status 1/)
  assert.ok(!existsSync(join(bad, 'tles.json')), 'a failed gate publishes NOTHING')
})
