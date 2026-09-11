// Imported by the compiled-browser CI entry point; also runnable alone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
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
