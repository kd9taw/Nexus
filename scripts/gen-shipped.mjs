#!/usr/bin/env node
// Emit docs/shipped.json — the machine-readable list of what Nexus SHIPS, for consumers
// outside this repo that make claims about it.
//
// WHY THIS EXISTS. The website's features page is hand-written copy in a different repo, and
// it is where "what does Nexus do" is actually answered for anyone who has not installed it.
// It went four releases without PSK31 and two without FT2 — two shipped modes absent from the
// one page whose whole job is listing them — and nothing anywhere could notice, because the
// guard that checks documentation coverage lives here and cannot see that repo, while that
// repo has no idea what this one ships. The same blind spot put a shipped macOS build behind
// four platform lists that said Windows, Linux and Raspberry Pi.
//
// So the truth is published as data. This repo generates it from the lists the app actually
// uses; the site syncs it with the manual and fails its own build when its copy omits
// something. Neither side gets to hold a stale opinion about the other.
//
// SOURCES, and each is the file the app itself reads — never a list retyped here:
//   tiers    ui/src/types.ts        the `Tier` union: the slot-synchronous modes
//   sections ui/src/features/registry.ts  `kind: 'section'`: the cockpits and screens
//   platforms .github/workflows/release.yml  the artifacts a release actually publishes
//
// `--check` verifies the committed file matches what the sources say and exits non-zero if
// not, so it can run in CI. That matters more than it sounds: the last generator whose only
// trigger was somebody remembering had thrown on every invocation for three releases and
// three releases shipped over it.
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const read = (p) => readFileSync(root + p, 'utf8')

/** The `Tier` union, in declaration order — the FT/Tempo modes. */
function tiers() {
  const src = read('ui/src/types.ts')
  const m = src.match(/export type Tier =([^\n]*(?:\n\s*\|[^\n]*)*)/)
  if (!m) throw new Error('could not find the Tier union in ui/src/types.ts')
  const found = [...m[1].matchAll(/'([A-Za-z0-9]+)'/g)].map((x) => x[1])
  if (found.length < 5) throw new Error(`Tier union parsed as only ${found.length} entries`)
  return found
}

/** Resolve a `labelKey` against the English catalog — the source the getters read. */
let EN = null
function catalogLabel(key) {
  EN ??= read('ui/src/i18n/en.ts')
  const m = EN.match(new RegExp(`'${key.replace(/\./g, '\\.')}':\\s*'([^']+)'`))
  return m?.[1] ?? null
}

/** Feature-registry sections — the cockpits and screens. */
function sections() {
  const src = read('ui/src/features/registry.ts')
  // Anchor on `kind: 'section'` and walk BACK to the id that owns it. Splitting the file into
  // entries first is what a previous version did, and it silently found 8 of 22 because the
  // entries are not all formatted alike — the exact failure this whole file exists to stop,
  // committed inside the tool meant to stop it. The tripwire below is not decoration.
  const out = []
  for (const m of src.matchAll(/kind: 'section'/g)) {
    const before = src.slice(0, m.index)
    const idMatches = [...before.matchAll(/id: '([A-Za-z0-9-]+)'/g)]
    const idHit = idMatches[idMatches.length - 1]
    if (!idHit) continue
    const id = idHit[1]
    // The label lives BETWEEN this entry's id and its kind. Bounding the search that way is
    // the whole correctness argument: an unbounded backwards search returns the FIRST label
    // in the file for every entry, which is exactly what the first version did — it stamped
    // all 21 sections 'CW' and produced a file that looked entirely plausible, and the
    // website check then passed PSK by looking for the word "CW". A label read from a getter
    // (`get label() { return t(...) }`) has no literal to find, and falls back to the id
    // rather than to a neighbour's name.
    const between = src.slice(idHit.index, m.index)
    // Two forms, and both must resolve. A MODE keeps its name as a literal (`label: 'PSK'`)
    // because a translated mode name names nothing; everything else carries a `labelKey` into
    // the catalog, because its label is prose. Falling back to the id for the second form
    // would put 'awards' where 'Awards' belongs — close enough to look right in the file and
    // wrong in every consumer that searches for it.
    const literal = between.match(/label: '([^']+)'/)?.[1]
    const key = between.match(/labelKey: '([^']+)'/)?.[1]
    const label = literal ?? (key ? catalogLabel(key) : null) ?? id
    out.push({ id, label })
  }
  if (out.length < 15) throw new Error(`registry parsed as only ${out.length} sections`)
  return out
}

/** The platforms a release publishes an installer for. */
function platforms() {
  const wf = read('.github/workflows/release.yml')
  const have = []
  if (/windows:/.test(wf)) have.push('Windows')
  if (/macos:/.test(wf)) have.push('macOS')
  if (/linux-x86:/.test(wf)) have.push('Linux')
  if (/pi:/.test(wf) || /Raspberry Pi/.test(wf)) have.push('Raspberry Pi')
  if (have.length < 3) throw new Error(`release.yml parsed as only ${have.length} platforms`)
  return have
}

const manifest = {
  // A note to whoever opens this file wondering whether to hand-edit it.
  _: 'GENERATED by scripts/gen-shipped.mjs from the lists the app reads. Do not hand-edit.',
  tiers: tiers(),
  sections: sections(),
  platforms: platforms(),
}

const path = 'docs/shipped.json'
const text = JSON.stringify(manifest, null, 2) + '\n'

if (process.argv.includes('--check')) {
  let current = ''
  try {
    current = read(path)
  } catch {
    console.error(`${path} is missing — run: node scripts/gen-shipped.mjs`)
    process.exit(1)
  }
  if (current !== text) {
    console.error(`${path} is out of date — run: node scripts/gen-shipped.mjs`)
    process.exit(1)
  }
  console.log(`${path} is current (${manifest.tiers.length} tiers, ${manifest.sections.length} sections, ${manifest.platforms.length} platforms)`)
} else {
  writeFileSync(root + path, text)
  console.log(`wrote ${path}: ${manifest.tiers.length} tiers, ${manifest.sections.length} sections, ${manifest.platforms.length} platforms`)
}
