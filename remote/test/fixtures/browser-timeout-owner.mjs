// Child process used by browser-lifecycle.test.mjs. Its first test MUST time out;
// the parent checks that the next case starts only after actual Chrome cleanup.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { setTimeout as sleep } from 'node:timers/promises'
import { chrome, cleanupAfterTest } from '../browser-runtime.mjs'

const browser = await chrome()
let stopped = false
test('intentional browser-owner timeout', { timeout: 100 }, async context => {
  // Omitting registration is the positive control for the original defect.
  const owner = process.env.NEXUS_TIMEOUT_WITHOUT_HOOK === '1' ? { after() {} } : context
  const stop = cleanupAfterTest(owner, async () => {
    await browser.stop()
    stopped = true
  })
  try { await sleep(500) } finally { await stop() }
})
test('next case observes completed Chrome cleanup', () => {
  assert.equal(stopped, true, 'the previous browser must stop before the next case')
})
