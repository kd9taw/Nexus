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
//
// TWO passes, in parse_spec's own order. The SHAPE pass is derived from the Rust
// `Deserialize` types (scripts/rust-serde-schema.mjs) because the halves check
// different things and always did: this file checks VALUES, serde checks PRESENCE and
// INTEGER WIDTH — so `role.selector` absent, `text.max_len` 300 (`u8`) and
// `number.min` -1 (`u32`) all exited 0 here and were refused by every shipped app.
// The VALUE pass below is this file's own, and every rule in it has a corpus fixture
// (crates/tempo-core/tests/fixtures/fd-rules-corpus) proving parse_spec refuses the
// same file for the same reason.

import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { checkAgainstSchema, schemaFromRust } from './rust-serde-schema.mjs'

const path = process.argv[2] || 'crates/tempo-core/src/fd_rules.seed.json'

function fail(msg) {
  console.error(`fd-rules INVALID: ${msg}`)
  process.exit(1)
}

let spec
try {
  spec = JSON.parse(readFileSync(path, 'utf8'))
} catch (e) {
  // parse_spec wraps serde's own parse error the same way, so a malformed file is a
  // clean refusal here rather than a stack trace.
  fail(`bad JSON: ${e.message}`)
}

// PASS 1 — the shape serde will accept, read off the Rust types. Anchored on this
// script's own location: the schema's source of truth must not depend on the cwd.
const RUST_SPEC_SRC = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../crates/tempo-core/src/fd_rules.rs',
)
const shapeError = checkAgainstSchema(
  spec,
  schemaFromRust(readFileSync(RUST_SPEC_SRC, 'utf8'), 'FileSpec'),
)
if (shapeError) fail(`bad JSON: ${shapeError}`)

// PASS 2 — the value rules, in parse_spec's order.
if (spec.schema !== 2) fail(`schema ${spec.schema} (this build reads schema 2)`)
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
const isDomainId = (id) => /^[a-z][a-z0-9_]*$/.test(id)
// `''` is the explicit "no standard ADIF column this direction" marker.
const isAdifTag = (t) => t === '' || /^[A-Z][A-Z0-9_]*$/.test(t)

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
  if (!['powered_multiplier', 'objectives'].includes(sc.model))
    fail(`${tag}: unknown scoring model ${JSON.stringify(sc.model)}`)
  for (const k of ['PH', 'CW', 'DIG'])
    if (!(k in sc.points_by_mode_class)) fail(`${tag}: points_by_mode_class misses ${k}`)
  if (!sc.power_tiers.length) fail(`${tag}: empty power_tiers`)
  for (let i = 1; i < sc.power_tiers.length; i++)
    if (sc.power_tiers[i - 1] >= sc.power_tiers[i]) fail(`${tag}: power_tiers not strictly ascending`)
  // The dupe key, as data (schema 2) rather than a shared const. Required, and
  // by_call must be true: a rule that does not key on the callsign is a mistake
  // far more often than a new contest shape, and the cost of being wrong is a
  // log full of contacts that should have been refused as dupes.
  if (!r.dupe.by_call)
    fail(`${tag}: dupe.by_call is false (a dupe rule must key on the callsign)`)

  // File-declared domains. `arrl_sections` / `fd_sections` are DERIVED from the
  // top-level section list, so a file declaring one would be a second copy of a
  // list it cannot fully represent (a Domain value is a code/label pair; a
  // Section also carries a division). Mirrors DomainSpec's checks exactly.
  const domainIds = new Set()
  for (const d of r.domains) {
    if (!isDomainId(d.id)) fail(`${tag}: domain id ${JSON.stringify(d.id)} is not ^[a-z][a-z0-9_]*$`)
    if (RESERVED_DOMAIN_IDS.includes(d.id))
      fail(`${tag}: domain id ${JSON.stringify(d.id)} is reserved (derived from the section list)`)
    if (domainIds.has(d.id)) fail(`${tag}: duplicate domain id ${JSON.stringify(d.id)}`)
    domainIds.add(d.id)
    if (!isAdifTag(d.adif.rcvd) || !isAdifTag(d.adif.sent))
      fail(`${tag}: domain ${d.id} has a malformed adif tag`)
    if (!d.values.length) fail(`${tag}: domain ${d.id} has no values`)
    const codes = new Set()
    for (const v of d.values) {
      if (!v.code || v.code !== v.code.toUpperCase())
        fail(`${tag}: domain ${d.id} code ${JSON.stringify(v.code)} not uppercase`)
      if (codes.has(v.code)) fail(`${tag}: domain ${d.id} duplicate code ${JSON.stringify(v.code)}`)
      codes.add(v.code)
      if (!v.label) fail(`${tag}: domain ${d.id} code ${v.code} has no label`)
    }
  }

  // The exchange block (§2.5). Mirrors ExchangeBlockSpec + check_kind exactly:
  // every rule here is a rules bug that must be a REFUSAL rather than a runtime
  // lookup miss on the air.
  const x = r.exchange
  if (!x.name) fail(`${tag}: exchange has no name`)
  const resolvesDomain = (id) =>
    RESERVED_DOMAIN_IDS.includes(id) || r.domains.some((d) => d.id === id)

  const checkKind = (key, k) => {
    switch (k.type) {
      case 'rst':
        if (!(k.digits === 2 || k.digits === 3))
          fail(`${tag}: ${key} rst digits ${k.digits} (expected 2 or 3)`)
        break
      case 'serial':
        if (k.scope === 'per_band')
          fail(
            `${tag}: ${key} serial scope per_band is not supported ` +
              `(this build allocates one series per contest)`,
          )
        else if (k.scope !== 'per_contest')
          fail(`${tag}: ${key} unknown serial scope ${JSON.stringify(k.scope)}`)
        break
      case 'enum':
        if (!resolvesDomain(k.domain))
          fail(`${tag}: ${key} names unknown domain ${JSON.stringify(k.domain)}`)
        break
      case 'pattern':
        // Anchored both ends or it is not the pattern it claims.
        if (k.re === '' || !k.re.startsWith('^') || !k.re.endsWith('$'))
          fail(`${tag}: ${key} pattern ${JSON.stringify(k.re)} is not ^…$-anchored`)
        break
      case 'number':
        if (!(k.min <= k.max)) fail(`${tag}: ${key} number min ${k.min} > max ${k.max}`)
        break
      case 'grid':
        if (!(k.chars === 4 || k.chars === 6))
          fail(`${tag}: ${key} grid chars ${k.chars} (expected 4 or 6)`)
        break
      case 'text':
        if (k.max_len === 0) fail(`${tag}: ${key} text max_len 0`)
        break
      case 'call':
        break
      case 'one_of':
        // One arm is not a choice; zero is not a slot.
        if (k.of.length < 2) fail(`${tag}: ${key} one_of needs at least 2 arms`)
        for (const arm of k.of) checkKind(key, arm)
        break
      default:
        // `check_kind` has no such arm: its `match` is exhaustive over KindSpec,
        // and pass 1 refuses a `type` that is not one of the variants.
        throw new Error(`unreachable: pass 1 admitted kind ${JSON.stringify(k.type)}`)
    }
  }

  const slots = new Set()
  for (const f of x.fields) {
    if (!f.key || f.key !== f.key.toUpperCase())
      fail(`${tag}: exchange slot ${JSON.stringify(f.key)} not uppercase`)
    if (slots.has(f.key)) fail(`${tag}: duplicate exchange slot ${JSON.stringify(f.key)}`)
    slots.add(f.key)
    // §2.1.1's "BOTH adif halves must be WRITTEN" is AdifTagsSpec's two required
    // `String`s, so an absent `sent` is pass 1's `missing field \`sent\``; `''`
    // stays the explicit "no standard column this direction" marker.
    if (!isAdifTag(f.adif.rcvd) || !isAdifTag(f.adif.sent))
      fail(`${tag}: slot ${f.key} has a malformed adif tag`)
    checkKind(f.key, f.kind)
  }
  if (!x.roles.length) fail(`${tag}: exchange has no roles`)
  const roleIds = new Set()
  for (const role of x.roles) {
    if (roleIds.has(role.id)) fail(`${tag}: duplicate role id ${JSON.stringify(role.id)}`)
    roleIds.add(role.id)
    for (const key of [...role.sends, ...role.receives, ...role.constant_sent])
      if (!slots.has(key))
        fail(`${tag}: role ${JSON.stringify(role.id)} names undeclared slot ${JSON.stringify(key)}`)
    for (const key of role.constant_sent)
      if (!role.sends.includes(key))
        fail(
          `${tag}: role ${JSON.stringify(role.id)} constant_sent ` +
            `${JSON.stringify(key)} is not in sends`,
        )
    // Five is the layout budget at the 1024 px supported floor.
    if (role.receives.length > 5)
      fail(
        `${tag}: role ${JSON.stringify(role.id)} receives ${role.receives.length} fields (max 5)`,
      )
    if (role.selector.type === 'always' && x.roles.length !== 1)
      fail(
        `${tag}: role ${JSON.stringify(role.id)} selector \`always\` must be the only role ` +
          `(a role after it could never be reached)`,
      )
    if (role.selector.type === 'my_location_in' && !role.selector.locations.length)
      fail(`${tag}: role ${JSON.stringify(role.id)} my_location_in is empty`)
    if (role.selector.type === 'my_category_is' && !role.selector.category)
      fail(`${tag}: role ${JSON.stringify(role.id)} my_category_is is empty`)
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
    fail(
      `${tag}: enforcement ${JSON.stringify(r.enforcement)} ` +
        `(this build only warns — never removes or disables)`,
    )
  const w = r.window
  if (!(w.month >= 1 && w.month <= 12)) fail(`${tag}: window month ${w.month}`)
  // ONE message for both halves, as parse_spec's single `match` arm gives: a
  // `nth_full` with a bad `n` and an unknown weekend are the same refusal. `n` is
  // `#[serde(default)]`, so an absent one is 0 here exactly as it is there.
  const n = w.n ?? 0
  if (!(w.weekend === 'last_full' || (w.weekend === 'nth_full' && n >= 1 && n <= 4)))
    fail(`${tag}: window weekend ${JSON.stringify(w.weekend)} n=${n}`)
  if (w.start_hour_utc >= 24) fail(`${tag}: window start_hour_utc ${w.start_hour_utc}`)
  if (!(w.duration_hours >= 1 && w.duration_hours <= 72))
    fail(`${tag}: window duration_hours ${w.duration_hours}`)
  for (const [y, o] of Object.entries(w.overrides ?? {})) {
    // `y.parse::<u16>()` — digits only, and inside u16. NOT a 4-digit shape: a
    // node-only narrowing would block a publish for a reason the app does not hold.
    if (!/^\d+$/.test(y) || Number(y) > 65535)
      fail(`${tag}: override year ${JSON.stringify(y)}`)
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
