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

const path = process.argv[2] || 'crates/tempo-core/src/fd_rules.seed.json'
const spec = JSON.parse(readFileSync(path, 'utf8'))

function fail(msg) {
  console.error(`fd-rules INVALID: ${msg}`)
  process.exit(1)
}

if (spec.schema !== 1) fail(`schema ${spec.schema} (the app reads schema 1)`)
if (!spec.generated) fail('empty `generated` stamp')
for (const want of ['arrlfd', 'wfd'])
  if (!spec.rulesets.some((r) => r.event === want)) fail(`missing the \`${want}\` ruleset`)

const seenEvents = new Set()
for (const r of spec.rulesets) {
  const tag = `ruleset ${r.event}/${r.rules_year}`
  if (!['arrlfd', 'wfd'].includes(r.event)) fail(`${tag}: unknown event`)
  const key = `${r.event}/${r.rules_year}`
  if (seenEvents.has(key)) fail(`${tag}: duplicate event+year`)
  seenEvents.add(key)
  if (!r.contest_id) fail(`${tag}: empty contest_id`)
  if (!['powered_multiplier', 'objectives'].includes(r.scoring))
    fail(`${tag}: unknown scoring model ${JSON.stringify(r.scoring)}`)
  for (const k of ['PH', 'CW', 'DIG'])
    if (!(k in r.points_by_mode_class)) fail(`${tag}: points_by_mode_class misses ${k}`)
  if (!r.power_tiers.length) fail(`${tag}: empty power_tiers`)
  for (let i = 1; i < r.power_tiers.length; i++)
    if (r.power_tiers[i - 1] >= r.power_tiers[i]) fail(`${tag}: power_tiers not strictly ascending`)
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
