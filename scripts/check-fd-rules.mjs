#!/usr/bin/env node
// Validate a Field Day rules file (crates/tempo-core/src/fd_rules.seed.json → the published
// fd-rules.json) with the SAME structural checks the in-app loader runs
// (tempo_core::fd_rules::parse_spec — keep the two in step). This is the publish gate
// .github/workflows/fd-rules.yml runs before the rolling `fd-rules` Release: a seed edit
// that the app would refuse must never ship (the refusing-a-garbage-file discipline from
// the cty/fcc pipelines).
//
// Run:  node scripts/check-fd-rules.mjs [path]   (default: the in-repo seed)
// Exits 1 with a specific reason on any miss.

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const path = process.argv[2] || 'crates/tempo-core/src/fd_rules.seed.json'
const spec = JSON.parse(readFileSync(path, 'utf8'))

function fail(msg) {
  console.error(`fd-rules INVALID: ${msg}`)
  process.exit(1)
}

if (spec.schema !== 2) fail(`schema ${spec.schema} (the app reads schema 2)`)
if (!spec.generated) fail('empty `generated` stamp')

// §8(d)'s inversion, mirroring tempo_core::fd_rules::seed_events(): a candidate
// file must carry every ruleset the BUNDLED seed carries — a download may ADD a
// contest, never REMOVE one this build ships with. When we are validating the
// seed ITSELF that list is its own, which is why the seed path is compared
// before deciding whether to re-read it from disk.
const SEED_PATH = 'crates/tempo-core/src/fd_rules.seed.json'
const seedEvents = (
  resolve(path) === resolve(SEED_PATH) ? spec : JSON.parse(readFileSync(SEED_PATH, 'utf8'))
).rulesets.map((r) => r.event)
for (const want of seedEvents)
  if (!spec.rulesets.some((r) => r.event === want)) fail(`missing the \`${want}\` ruleset`)

// Domain ids the LOADER derives from the top-level section list; a file that
// declares one is refused. Mirrors fd_rules::RESERVED_DOMAIN_IDS.
const RESERVED_DOMAIN_IDS = ['arrl_sections', 'fd_sections']
// Lowercase snake, so a domain id is never confused with an exchange SLOT id
// (uppercase).
const isDomainId = (id) => typeof id === 'string' && /^[a-z][a-z0-9_]*$/.test(id)
// `''` is the explicit "no standard ADIF column this direction" marker.
const isAdifTag = (t) => t === '' || (typeof t === 'string' && /^[A-Z][A-Z0-9_]*$/.test(t))

const seenEvents = new Set()
for (const r of spec.rulesets) {
  const tag = `ruleset ${r.event}/${r.rules_year}`
  if (!['arrlfd', 'wfd'].includes(r.event)) fail(`${tag}: unknown event`)
  const key = `${r.event}/${r.rules_year}`
  if (seenEvents.has(key)) fail(`${tag}: duplicate event+year`)
  seenEvents.add(key)
  if (!r.contest_id) fail(`${tag}: empty contest_id`)
  // `scoring` is a BLOCK (schema 2): the model plus the two tables that were
  // always part of it. Required, never defaulted — mirrors ScoringSpec.
  const sc = r.scoring
  if (!sc || typeof sc !== 'object') fail(`${tag}: missing the \`scoring\` block`)
  if (!['powered_multiplier', 'objectives'].includes(sc.model))
    fail(`${tag}: unknown scoring model ${JSON.stringify(sc.model)}`)
  if (!sc.points_by_mode_class || typeof sc.points_by_mode_class !== 'object')
    fail(`${tag}: scoring misses points_by_mode_class`)
  for (const k of ['PH', 'CW', 'DIG'])
    if (!(k in sc.points_by_mode_class)) fail(`${tag}: points_by_mode_class misses ${k}`)
  if (!Array.isArray(sc.power_tiers) || !sc.power_tiers.length)
    fail(`${tag}: empty power_tiers`)
  for (let i = 1; i < sc.power_tiers.length; i++)
    if (sc.power_tiers[i - 1] >= sc.power_tiers[i]) fail(`${tag}: power_tiers not strictly ascending`)
  // The dupe key, as data (schema 2) rather than a shared const. Required, and
  // by_call must be true: a rule that does not key on the callsign is a mistake
  // far more often than a new contest shape, and the cost of being wrong is a
  // log full of contacts that should have been refused as dupes.
  const dupe = r.dupe
  if (!dupe || typeof dupe !== 'object') fail(`${tag}: missing the \`dupe\` block`)
  for (const k of ['by_call', 'by_band', 'by_mode_class'])
    if (typeof dupe[k] !== 'boolean') fail(`${tag}: dupe.${k} must be a boolean`)
  if (!dupe.by_call)
    fail(`${tag}: dupe.by_call is false (a dupe rule must key on the callsign)`)

  // File-declared domains. `arrl_sections` / `fd_sections` are DERIVED from the
  // top-level section list, so a file declaring one would be a second copy of a
  // list it cannot fully represent (a Domain value is a code/label pair; a
  // Section also carries a division). Mirrors DomainSpec's checks exactly.
  if (!Array.isArray(r.domains)) fail(`${tag}: missing the \`domains\` array`)
  const domainIds = new Set()
  for (const d of r.domains) {
    if (!isDomainId(d.id)) fail(`${tag}: domain id ${JSON.stringify(d.id)} is not ^[a-z][a-z0-9_]*$`)
    if (RESERVED_DOMAIN_IDS.includes(d.id))
      fail(`${tag}: domain id ${JSON.stringify(d.id)} is reserved (derived from the section list)`)
    if (domainIds.has(d.id)) fail(`${tag}: duplicate domain id ${JSON.stringify(d.id)}`)
    domainIds.add(d.id)
    if (!d.adif || !isAdifTag(d.adif.rcvd) || !isAdifTag(d.adif.sent))
      fail(`${tag}: domain ${d.id} has a malformed adif tag`)
    if (!Array.isArray(d.values) || !d.values.length) fail(`${tag}: domain ${d.id} has no values`)
    const codes = new Set()
    for (const v of d.values) {
      if (!v.code || v.code !== v.code.toUpperCase())
        fail(`${tag}: domain ${d.id} code ${JSON.stringify(v.code)} not uppercase`)
      if (codes.has(v.code)) fail(`${tag}: domain ${d.id} duplicate code ${JSON.stringify(v.code)}`)
      codes.add(v.code)
      if (!v.label) fail(`${tag}: domain ${d.id} code ${v.code} has no label`)
    }
  }

  const ids = new Set()
  for (const b of [...r.bonuses, ...(r.objectives || [])]) {
    if (!b.id) fail(`${tag}: empty bonus id`)
    if (ids.has(b.id)) fail(`${tag}: duplicate bonus id ${JSON.stringify(b.id)}`)
    ids.add(b.id)
  }
  for (const m of r.banned_modes)
    if (!m || m !== m.toUpperCase()) fail(`${tag}: banned mode ${JSON.stringify(m)} not uppercase`)
  if (r.enforcement !== 'warn')
    fail(`${tag}: enforcement ${JSON.stringify(r.enforcement)} (the app only warns)`)
  const w = r.window
  if (!(w.month >= 1 && w.month <= 12)) fail(`${tag}: window month ${w.month}`)
  if (w.weekend === 'nth_full') {
    if (!(w.n >= 1 && w.n <= 4)) fail(`${tag}: nth_full n=${w.n}`)
  } else if (w.weekend !== 'last_full') {
    fail(`${tag}: window weekend ${JSON.stringify(w.weekend)}`)
  }
  if (!(w.start_hour_utc >= 0 && w.start_hour_utc < 24))
    fail(`${tag}: window start_hour_utc ${w.start_hour_utc}`)
  if (!(w.duration_hours >= 1 && w.duration_hours <= 72))
    fail(`${tag}: window duration_hours ${w.duration_hours}`)
  for (const [y, o] of Object.entries(w.overrides || {})) {
    if (!/^\d{4}$/.test(y)) fail(`${tag}: override year ${JSON.stringify(y)}`)
    if (!(o.start_unix < o.end_unix)) fail(`${tag}: override ${y} start ≥ end`)
  }
}

// The section universe is pinned (71 US + 12 RAC): the app's TS mirror guard and the board
// layout both assume it — a grown/shrunk list must land with a code release, not a data push.
if (spec.sections.length !== 83) fail(`${spec.sections.length} sections (expected 83)`)
const codes = new Set()
for (const s of spec.sections) {
  if (!s.code || s.code !== s.code.toUpperCase())
    fail(`section code ${JSON.stringify(s.code)} not uppercase`)
  if (codes.has(s.code)) fail(`duplicate section code ${JSON.stringify(s.code)}`)
  codes.add(s.code)
  if (!s.name || !s.division) fail(`section ${s.code} misses name/division`)
}

console.error(
  `ok: schema ${spec.schema} · generated ${spec.generated} · ${spec.rulesets.length} rulesets ` +
    `(years ${spec.rulesets.map((r) => r.rules_year).join('/')}) · ${spec.sections.length} sections`,
)
