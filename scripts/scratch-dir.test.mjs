// Tests for scripts/scratch-dir.mjs: a scratch folder is gone however its process ends.
//
// Each case runs a child that makes two folders (one with a file, one with a nested tree), prints
// their paths, and then ends one way. The parent sees both folders exist before the child ends,
// so "gone afterwards" cannot pass by their never having been made, and a control child that
// makes its folder with plain mkdtempSync is shown to leave it behind.
//
// Run: node --test scripts/scratch-dir.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync, mkdirSync, readdirSync } from 'node:fs'
import { constants } from 'node:os'
import { join } from 'node:path'

import { scratchDir } from './scratch-dir.mjs'

const HELPER = new URL('./scratch-dir.mjs', import.meta.url).href
const box = scratchDir('scratch-dir-test-')

const CHILD = `
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { scratchDir } from ${JSON.stringify(HELPER)}
const how = process.argv[1]
const make = how === 'unhelped' ? (p) => mkdtempSync(join(tmpdir(), p)) : scratchDir
const a = make('one-'), b = make('two-')
writeFileSync(join(a, 'file'), 'x')
mkdirSync(join(b, 'nested', 'deep'), { recursive: true })
console.log(JSON.stringify([a, b]))
// The ending ones wait for the parent's go, or their own exit could remove the folders before the
// parent has seen them exist.
if (how === 'end' || how === 'exit1' || how === 'throw') await new Promise((resolve) => process.stdin.once('data', resolve))
if (how === 'throw') throw new Error('planted')
if (how === 'exit1') process.exit(1)
if (how === 'wait' || how === 'unhelped') setInterval(() => {}, 1000)
`

// Runs the child in its own empty TMPDIR; once its folders exist it is sent `signal`, or told to go on.
async function run(how, signal) {
  const tmp = join(box, `${how}-${signal ?? 'none'}`)
  mkdirSync(tmp)
  const child = spawn(process.execPath, ['--input-type=module', '-e', CHILD, how], {
    env: { ...process.env, TMPDIR: tmp },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  let out = ''
  child.stdout.on('data', (c) => (out += c))
  const closed = once(child, 'close')
  while (!out.includes('\n')) await once(child.stdout, 'data')
  const dirs = JSON.parse(out.split('\n')[0])
  assert.ok(dirs.every((d) => existsSync(d)), `${how}: the folders must exist while the child runs`)
  if (signal) child.kill(signal)
  else child.stdin.end('go\n')
  const [code] = await closed
  return { code, left: readdirSync(tmp) }
}

test('the folders are removed when the process ends normally, exits 1, or throws', async () => {
  for (const [how, code] of [['end', 0], ['exit1', 1], ['throw', 1]]) {
    const r = await run(how)
    assert.equal(r.code, code, `${how}: the exit code is the process's own`)
    assert.deepEqual(r.left, [], `${how}: left behind`)
  }
})

test('the folders are removed on SIGINT, SIGTERM and SIGHUP, and the exit code names the signal', async () => {
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const r = await run('wait', signal)
    assert.equal(r.code, 128 + constants.signals[signal], `${signal}: exit code`)
    assert.deepEqual(r.left, [], `${signal}: left behind`)
  }
})

test('control: a folder made without the helper is left behind by SIGTERM, so the check can see a leak', async () => {
  const r = await run('unhelped', 'SIGTERM')
  assert.equal(r.left.length, 2)
})
