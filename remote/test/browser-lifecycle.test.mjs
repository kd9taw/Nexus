// Imported by the compiled-browser CI entry point; also runnable alone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { readdir, readFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import { fileURLToPath } from 'node:url'

const run = promisify(execFile)
test('browser timeout cleanup completes before the next test, with a failing control', { timeout: 120000 }, async () => {
  const fixture = fileURLToPath(new URL('./fixtures/browser-timeout-owner.mjs', import.meta.url))
  for (const omitHook of [true, false]) {
    const env = { ...process.env, NEXUS_TIMEOUT_WITHOUT_HOOK: omitHook ? '1' : '0' }
    // This is a separate runner. Inheriting the parent's child marker makes
    // Node skip its files as a recursive run and misleadingly exit zero.
    delete env.NODE_TEST_CONTEXT
    const child = await run(process.execPath, ['--test', '--test-reporter=tap', fixture], {
      env, timeout: 60000,
    }).then(value => ({ ...value, code: 0 }), error => error)
    assert.equal(child.code, 1, 'the child must report a test failure, not a launch or process timeout')
    const result = child.stdout + child.stderr
    assert.match(result, /# tests 2\b/)
    assert.match(result, /# cancelled 1\b/, 'the deadline must actually cancel the owning test')
    assert.match(result, /test timed out after 100ms/)
    assert.match(result, omitHook ? /# pass 0\b/ : /# pass 1\b/)
    assert.match(result, omitHook ? /# fail 1\b/ : /# fail 0\b/)
    assert.match(result, omitHook
      ? /not ok 2 - next case observes completed Chrome cleanup/
      : /\nok 2 - next case observes completed Chrome cleanup/)
  }
})

// A Worker that fails to start must not keep the runner alive. In staging run 34769726458 one
// case died at startup in 225 ms, every later case passed, and node --test then printed nothing
// for 55 minutes until the job timeout: no summary, a cancelled run instead of a red one.
test('a Worker that fails to start leaves no process behind and the runner exits red', { timeout: 120000 }, async () => {
  const fixture = fileURLToPath(new URL('./fixtures/runtime-startup-owner.mjs', import.meta.url))
  const marker = crypto.randomUUID()
  // Miniflare hands its environment to workerd, so the marker identifies every process it started.
  const env = { ...process.env, NEXUS_RUNTIME_STARTUP_MARKER: marker }
  delete env.NODE_TEST_CONTEXT
  const started = performance.now()
  const child = await run(process.execPath, ['--test', '--test-reporter=tap', fixture], { env, timeout: 30000 })
    .then(value => ({ ...value, code: 0 }), error => error)
  const seconds = Math.round((performance.now() - started) / 100) / 10
  assert.equal(child.killed ?? false, false, `the runner must exit by itself; it was killed after ${seconds} s`)
  assert.equal(child.code, 1, 'the child must report the startup failure as a failed test')
  const result = child.stdout + child.stderr
  assert.match(result, /# fail 1\b/)
  assert.match(result, /Address already in use/, 'the failure must be the forced bind failure, not another one')
  if (process.platform === 'linux') {
    const survivors = []
    for (const pid of (await readdir('/proc')).filter(name => /^\d+$/.test(name))) {
      try { if ((await readFile(`/proc/${pid}/environ`, 'utf8')).includes(marker)) survivors.push(pid) } catch {}
    }
    assert.deepEqual(survivors, [], 'no process started by the failed Worker may outlive its runner')
  }
  console.log(`Worker startup failure: runner exited red in ${seconds} s with no surviving process`)
})
