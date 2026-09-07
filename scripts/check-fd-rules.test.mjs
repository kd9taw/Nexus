#!/usr/bin/env node
// The node half of the fd-rules validator-parity gate. Walks the SAME corpus
// crates/tempo-core/tests/fd_rules_corpus.rs walks and asserts the same verdict
// and the same reason substring, so a rule that lands in one validator and not
// the other goes red HERE instead of at publish time.
//
// check-fd-rules.mjs is the gate .github/workflows/fd-rules.yml runs before the
// rolling `fd-rules` Release; parse_spec is what the shipped app enforces. They
// are two independent implementations of one contract and nothing kept them in
// step until this file.
//
// Run:  node --test scripts/check-fd-rules.test.mjs      (from the repo root)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { join } from 'node:path'

const ROOT = 'crates/tempo-core/tests/fixtures/fd-rules-corpus'

/** Run the real validator as a subprocess: exit 0 = accept, 1 = refuse. */
const run = (p) => {
  try {
    execFileSync('node', ['scripts/check-fd-rules.mjs', p], {
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return { code: 0, err: '' }
  } catch (e) {
    return { code: e.status, err: String(e.stderr) }
  }
}

/** The fixtures in one arm — and never an empty list, which would pass vacuously. */
const files = (kind) => {
  const f = readdirSync(join(ROOT, kind))
    .filter((n) => n.endsWith('.json'))
    .sort()
  assert.ok(f.length > 0, `${kind} corpus is empty — the walk proves nothing`)
  return f
}

test('every accept fixture validates', () => {
  for (const n of files('accept')) {
    const { code, err } = run(join(ROOT, 'accept', n))
    assert.equal(code, 0, `${n} must validate: ${err}`)
  }
})

test('every refuse fixture is refused with the expected reason', () => {
  for (const n of files('refuse')) {
    const want = readFileSync(join(ROOT, 'refuse', n.replace('.json', '.expect')), 'utf8').trim()
    assert.ok(want, `${n}: empty .expect matches anything`)
    const { code, err } = run(join(ROOT, 'refuse', n))
    assert.equal(code, 1, `${n} must be refused`)
    assert.ok(
      err.includes(want),
      `${n}: ${JSON.stringify(err)} does not carry ${JSON.stringify(want)}`,
    )
  }
})
