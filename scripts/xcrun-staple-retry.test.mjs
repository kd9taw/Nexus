// Tests for scripts/xcrun-staple-retry.sh, the `xcrun` release.yml puts in front of the macOS
// bundler so that stapling the notarization ticket is retried and its failure is seen.
//
// A fake xcrun stands in for Apple's: it records every call and fails `stapler staple` (or
// `stapler validate`) until a set attempt, which is how a ticket Apple has accepted but not yet
// published looks to the stapler. Each refusal is paired with a run that passes.
//
// Run: node --test scripts/xcrun-staple-retry.test.mjs

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'xcrun-staple-retry.sh');

// The fake: staple succeeds from attempt STAPLE_OK_AT on (0 = never), validate from VALIDATE_OK_AT.
const FAKE = `#!/usr/bin/env bash
echo "$*" >> "$FAKE_DIR/calls"
if [ "$1 $2" = "stapler staple" ]; then
  n=$(( $(cat "$FAKE_DIR/staples" 2>/dev/null || echo 0) + 1 )); echo $n > "$FAKE_DIR/staples"
  if [ "$STAPLE_OK_AT" -gt 0 ] && [ $n -ge "$STAPLE_OK_AT" ]; then echo "The staple and validate action worked!"; exit 0; fi
  echo "verbose request dump"; echo "{ records: [] }"; echo "CloudKit query failed due to \\"Record not found\\"."; echo "The staple and validate action failed! Error 65."; exit 65
fi
if [ "$1 $2" = "stapler validate" ]; then
  n=$(( $(cat "$FAKE_DIR/validates" 2>/dev/null || echo 0) + 1 )); echo $n > "$FAKE_DIR/validates"
  if [ $n -ge "\${VALIDATE_OK_AT:-1}" ]; then echo "The validate action worked!"; exit 0; fi
  echo "$3 does not have a ticket stapled to it."; exit 65
fi
echo "passed through: $*"; exit "\${OTHER_EXIT:-0}"
`;

function run(args, env = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'xcrun-staple-'));
  const fake = path.join(dir, 'xcrun');
  fs.writeFileSync(fake, FAKE, { mode: 0o755 });
  const log = path.join(dir, 'staple.log');
  const r = spawnSync('bash', [SCRIPT, ...args], {
    encoding: 'utf8',
    env: {
      PATH: process.env.PATH,
      FAKE_DIR: dir,
      NEXUS_REAL_XCRUN: fake,
      NEXUS_STAPLE_LOG: log,
      NEXUS_STAPLE_WAIT: '0',
      STAPLE_OK_AT: '1',
      ...env,
    },
  });
  const read = (f) => (fs.existsSync(path.join(dir, f)) ? fs.readFileSync(path.join(dir, f), 'utf8') : '');
  return { status: r.status, stdout: r.stdout, stderr: r.stderr, calls: read('calls').trim().split('\n').filter(Boolean), log: read('staple.log') };
}

const STAPLE = ['stapler', 'staple', '-v', 'Nexus.app'];

test('every other xcrun call goes straight through, arguments and exit status unchanged', () => {
  const r = run(['notarytool', 'submit', 'Nexus.zip', '--wait'], { OTHER_EXIT: '3' });
  assert.equal(r.status, 3);
  assert.deepEqual(r.calls, ['notarytool submit Nexus.zip --wait']);
  assert.match(r.stdout, /passed through: notarytool submit Nexus\.zip --wait/);
  assert.equal(r.log, '');
});

test('a ticket stapled and validated on the first attempt is logged once and exits 0', () => {
  const r = run(STAPLE);
  assert.equal(r.status, 0);
  assert.deepEqual(r.calls, ['stapler staple -v Nexus.app', 'stapler validate Nexus.app']);
  assert.equal(r.log, 'stapled Nexus.app (attempt 1 of 10)\n');
});

test('a ticket not published yet is retried until it staples', () => {
  const r = run(STAPLE, { STAPLE_OK_AT: '3' });
  assert.equal(r.status, 0);
  assert.equal(r.calls.filter((c) => c.startsWith('stapler staple')).length, 3);
  assert.match(r.log, /staple attempt 1 of 10 failed for Nexus\.app: .*Record not found/);
  assert.match(r.log, /staple attempt 2 of 10 failed/);
  assert.match(r.log, /^stapled Nexus\.app \(attempt 3 of 10\)$/m);
  // The stapler's verbose dump stays out of the log; only its last lines are kept.
  assert.doesNotMatch(r.log, /verbose request dump/);
});

test('a staple that never succeeds gives up after the bounded number of attempts, exit 65, no "stapled" line', () => {
  const r = run(STAPLE, { STAPLE_OK_AT: '0', NEXUS_STAPLE_TRIES: '4' });
  assert.equal(r.status, 65);
  assert.equal(r.calls.filter((c) => c.startsWith('stapler staple')).length, 4);
  assert.match(r.log, /no ticket stapled to Nexus\.app after 4 attempts/);
  assert.doesNotMatch(r.log, /^stapled /m);
  // No wait after the last attempt.
  assert.equal((r.log.match(/trying again/g) || []).length, 3);
});

test('a staple that reports success but does not validate counts as a failure', () => {
  const r = run(STAPLE, { VALIDATE_OK_AT: '2' });
  assert.equal(r.status, 0);
  assert.match(r.log, /staple attempt 1 of 10 failed for Nexus\.app: .*does not have a ticket stapled/);
  assert.match(r.log, /^stapled Nexus\.app \(attempt 2 of 10\)$/m);
});

test('the default bound is 10 attempts, 30 seconds apart', () => {
  const text = fs.readFileSync(SCRIPT, 'utf8');
  assert.match(text, /tries=\$\{NEXUS_STAPLE_TRIES:-10\}/);
  assert.match(text, /wait=\$\{NEXUS_STAPLE_WAIT:-30\}/);
});
