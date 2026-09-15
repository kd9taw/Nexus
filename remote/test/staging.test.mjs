// Provider calls below are intercepted. The deployment smoke check also runs
// against the actual compiled Worker and Static Assets in local workerd.
import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { randomBytes, randomUUID, createHash } from 'node:crypto'
import { mkdtemp, mkdir, readFile, readdir, writeFile, copyFile, cp, rm, stat, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { STAGING, TARGETS, deployable, identity, stagingConfig, target, verifyIdentity, requestBytes, requestJson } from '../scripts/staging-common.mjs'
import { cloudflare, workerDigest } from '../scripts/cloudflare-staging.mjs'
import { createArtifact, verifyArtifact, verifyLive, verifyLiveSettled, verifyPublicSource } from '../scripts/staging-artifact.mjs'
import { runtime } from './runtime.mjs'
import { uploadArtifact, compareSchema, redactedDiagnostic, runWrangler, schemaQuery, settleUpload, withSecretsFile, wranglerLogLevel } from '../scripts/deploy-staging.mjs'
import { trialGrant } from '../scripts/grant-trial.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const ids = { issuer: 'https://nexus-staging-test.us.auth0.com/', clientId: 'synthetic-client-id',
  audience: STAGING.audience, databaseId: randomUUID(), revision: createHash('sha1').update(randomBytes(16)).digest('hex') }
const template = JSON.parse(await readFile(join(root, 'remote/wrangler.jsonc'), 'utf8'))
let scratch, app, artifact
before(async () => {
  scratch = await mkdtemp(join(tmpdir(), 'nexus-staging-test-'))
  await mkdir(join(scratch, 'remote/dist'), { recursive: true })
  await mkdir(join(scratch, 'ui'), { recursive: true })
  await copyFile(join(root, 'remote/dist/index.js'), join(scratch, 'remote/dist/index.js'))
  await copyFile(join(root, 'remote/wrangler.jsonc'), join(scratch, 'remote/wrangler.jsonc'))
  await cp(join(root, 'remote/migrations'), join(scratch, 'remote/migrations'), { recursive: true })
  await cp(join(root, 'ui/dist-remote'), join(scratch, 'ui/dist-remote'), { recursive: true })
  await createArtifact(scratch, ids)
  artifact = await verifyArtifact(scratch, ids.revision)
  app = await runtime({ bindings: { PUBLIC_REMOTE_ORIGIN: STAGING.origin, AUTH0_ISSUER: ids.issuer,
    AUTH0_CLIENT_ID: ids.clientId, AUTH0_AUDIENCE: ids.audience, REMOTE_BUILD_REVISION: ids.revision } })
})
after(async () => { await app?.mf.dispose(); if (scratch) await rm(scratch, { recursive: true, force: true }) })

// The live verification checks the Worker, every browser asset, the security headers and both
// admission refusals - and touches the database not at all, because `ready` is a var comparison and
// both admission probes are refused before any query runs. So the migrate step's exit code was the
// only evidence the schema had landed. A schema that did not land is not a loud failure: it is
// endpoints refusing with serviceUnavailable the first time an operator uses them.
test('the schema check compares what the database reports against what the artifact ships', () => {
  const wrangler = names =>
    `\n [info] some wrangler chatter\n${JSON.stringify([{ results: names.map(name => ({ name })), success: true }])}\n`

  // Agreement, including a database that is AHEAD - a migration applied by an earlier deploy is
  // not a fault, only a missing one is.
  assert.deepEqual(
    compareSchema(wrangler(['0001_a.sql', '0002_b.sql', '0003_c.sql']), ['0001_a.sql', '0002_b.sql']),
    { applied: 3, shipped: 2 })

  // THE CASE THIS EXISTS FOR: migrate reported success, the newest migration is not there.
  assert.throws(() => compareSchema(wrangler(['0001_a.sql']), ['0001_a.sql', '0002_b.sql']),
    /missing migrations: 0002_b\.sql/)

  // Neither side may pass as agreement by being empty - the shape a lenient parser produces.
  assert.throws(() => compareSchema(wrangler([]), ['0001_a.sql']), /reports no applied migrations/)
  assert.throws(() => compareSchema(wrangler(['0001_a.sql']), []), /no migrations to verify against/)

  // Unreadable output must refuse rather than silently compare nothing.
  assert.throws(() => compareSchema('wrangler exploded', ['0001_a.sql']), /readable JSON/)
  assert.throws(() => compareSchema('[{"results":[{"nombre":"x"}]}]', ['0001_a.sql']),
    /reports no applied migrations/)

  // A bracketed line AFTER the JSON used to make every candidate unparseable, because each one was
  // cut at the last `]` in the buffer. Text on both sides is tolerated; agreement is still exact.
  const warned = `${wrangler(['0001_a.sql', '0002_b.sql'])}\n▲ [WARNING] a later notice [with brackets]\n`
  assert.deepEqual(compareSchema(warned, ['0001_a.sql', '0002_b.sql']), { applied: 2, shipped: 2 })
  assert.throws(() => compareSchema(warned, ['0001_a.sql', '0002_b.sql', '0003_c.sql']), /missing migrations: 0003_c\.sql/)
  // Only an array of D1 result sets is an answer: not a JSON array that merely contains one as text.
  assert.throws(() => compareSchema(JSON.stringify([JSON.stringify([{ results: [{ name: '0001_a.sql' }] }])]), ['0001_a.sql']), /readable JSON/)

  // The failure names what came back - tonight it said nothing at all - redacted, and never a secret.
  const token = randomBytes(32).toString('hex')
  assert.throws(() => compareSchema('', ['0001_a.sql'], { stderr: '', env: {} }),
    error => error.message === 'The schema query did not return readable JSON (stdout 0 bytes; stderr 0 bytes)')
  assert.throws(() => compareSchema(`Authentication failed\n  for ${token}`, ['0001_a.sql'], { stderr: 'boom', env: { CLOUDFLARE_API_TOKEN: token } }), error => {
    assert.match(error.message, /\(stdout \d+ bytes "Authentication failed for \[redacted\]"; stderr 4 bytes "boom"\)$/)
    assert.ok(!error.message.includes(token))
    return true
  })
})

// Staging run 34797528705 - the FIRST run ever to reach verify-schema - failed "did not return readable
// JSON". Nothing was malformed: Wrangler ran with WRANGLER_LOG=error, and it prints `d1 execute --json`
// through its level-filtered logger, so the query succeeded and printed nothing. The parser tests
// above only ever saw synthetic text; this runs the pinned Wrangler against a local D1 built from the
// artifact's own migrations, with the exact query and log level the deploy uses.
test('verify-schema parses the real Wrangler output at the log level the deploy uses, and still refuses a database that is behind', async () => {
  const state = await mkdtemp(join(tmpdir(), 'nexus-schema-test-')), env = { PATH: process.env.PATH }
  const config = join(scratch, 'remote/staging-artifact/wrangler.jsonc'), local = ['--local', '--config', config, '--persist-to', state]
  const wrangler = (argv, logLevel = 'error') => runWrangler(argv, { cwd: state, env, logLevel })
  const query = () => wrangler([...schemaQuery(STAGING.database, config, '--local'), '--persist-to', state], wranglerLogLevel('verify-schema'))
  const shipped = Object.keys(artifact.manifest.files).filter(name => name.startsWith('migrations/')).map(name => name.slice('migrations/'.length)).sort()
  try {
    const applied = await wrangler(['d1', 'migrations', 'apply', STAGING.database, ...local])
    assert.equal(applied.code, 0, redactedDiagnostic(applied.output, env))

    // Tonight, reproduced: same query, WRANGLER_LOG=error - exit 0, and no output on either stream.
    const tonight = await wrangler([...schemaQuery(STAGING.database, config, '--local'), '--persist-to', state], 'error')
    assert.deepEqual({ code: tonight.code, stdout: tonight.stdout, stderr: tonight.stderr }, { code: 0, stdout: '', stderr: '' })
    assert.throws(() => compareSchema(tonight.stdout, shipped, { stderr: tonight.stderr, env }), /readable JSON \(stdout 0 bytes; stderr 0 bytes\)/)

    // The deploy's level for this mode prints the result, and the schema verifies.
    const fixed = await query()
    assert.equal(fixed.code, 0, redactedDiagnostic(fixed.output, env))
    assert.deepEqual(compareSchema(fixed.stdout, shipped, { stderr: fixed.stderr, env }), { applied: shipped.length, shipped: shipped.length })
    assert.ok(shipped.length >= 5, 'positive control: every shipped migration, 0005 included, was compared')

    // Positive control on the same real database: take the newest migration out of its ledger and the
    // check must refuse, naming it. A parser that returned "agreement" for anything would pass above.
    const newest = shipped.at(-1)
    const removed = await wrangler(['d1', 'execute', STAGING.database, ...local, '--command', `DELETE FROM d1_migrations WHERE name = '${newest}'`])
    assert.equal(removed.code, 0, redactedDiagnostic(removed.output, env))
    const behind = await query()
    assert.equal(behind.code, 0)
    assert.throws(() => compareSchema(behind.stdout, shipped, { stderr: behind.stderr, env }), new RegExp(`missing migrations: ${newest.replace('.', '\\.')}$`))
  } finally { await rm(state, { recursive: true, force: true }) }
})

// The deploy applies migrations BEFORE it uploads the Worker, so for a while the OLD Worker runs on
// the NEW schema. Every migration therefore has to be additive: a dropped, renamed or rebuilt column
// breaks the Worker that is still serving. The check reads each statement with its comments removed.
const additive = sql => sql.split('\n').filter(line => !line.trimStart().startsWith('--')).join('\n')
  .split(';').map(s => s.replace(/\s+/g, ' ').trim()).filter(Boolean)
  .filter(s => /\b(DROP|RENAME)\b/i.test(s) || !/^(PRAGMA \w+ ?= ?\w+|CREATE (TABLE|INDEX|UNIQUE INDEX) |ALTER TABLE \w+ ADD COLUMN |UPDATE \w+ SET )/i.test(s))
test('every D1 migration is additive, including the approval-lifetime column', async () => {
  // Positive control: the check really does refuse the shapes it exists for.
  assert.equal(additive('ALTER TABLE devices DROP COLUMN approved_at;').length, 1)
  assert.equal(additive('DROP TABLE devices;').length, 1)
  assert.equal(additive('ALTER TABLE devices RENAME COLUMN expires_at TO ends_at;').length, 1)
  assert.equal(additive('-- DROP TABLE devices; is only prose here\nALTER TABLE devices ADD COLUMN x INTEGER;').length, 0)
  const directory = join(root, 'remote/migrations')
  const names = (await readdir(directory)).filter(name => name.endsWith('.sql')).sort()
  assert.ok(names.length >= 7)
  for (const name of names) assert.deepEqual(additive(await readFile(join(directory, name), 'utf8')), [], name)
  // The lifetime batch adds exactly one nullable column: an approval given before it has no approval
  // time, and so is never renewed, which is what the Nexus that gave it expects.
  const lifetime = additive(await readFile(join(directory, '0007_device_lifetime.sql'), 'utf8'))
  assert.deepEqual(lifetime, [])
  const statements = (await readFile(join(directory, '0007_device_lifetime.sql'), 'utf8')).split('\n')
    .filter(line => !line.trimStart().startsWith('--')).join('\n').split(';').map(s => s.replace(/\s+/g, ' ').trim()).filter(Boolean)
  assert.deepEqual(statements, ['ALTER TABLE devices ADD COLUMN approved_at INTEGER'])
})

test('staging configuration requires exact service/database scope and public Auth0 tenant values', () => {
  const config = stagingConfig(template, ids)
  assert.equal(config.vars.REMOTE_BUILD_REVISION, ids.revision)
  assert.equal(config.d1_databases[0].database_id, ids.databaseId)
  assert.deepEqual(config.routes, [{ pattern: new URL(STAGING.origin).hostname, custom_domain: true }])
  for (const change of [
    { issuer: 'http://tenant.auth0.com/' }, { issuer: 'https://user:password@tenant.auth0.com/' },
    { issuer: 'https://tenant.auth0.com/?code=private' }, { issuer: 'https://tenant.auth0.com/extra' },
    { issuer: 'https://127.0.0.1/' }, { issuer: 'https://auth0.com.evil.example/' },
    { clientId: 'unconfigured' }, { clientId: 'line\nbreak' }, { audience: 'https://another.example/api' },
    { databaseId: '00000000-0000-0000-0000-000000000000' }, { databaseId: '../another-database' }, { revision: 'main' },
  ]) assert.throws(() => stagingConfig(template, { ...ids, ...change }))
  assert.throws(() => stagingConfig({ ...template, name: 'hamradiotools' }, ids), /dedicated staging/)
})

function provider({ database = null, domains = [], workers = [], denied = false, ambiguous = false, loseWrite = false, grantChanges = 1, extraEnv = {} } = {}) {
  const env = { CLOUDFLARE_ACCOUNT_ID: randomBytes(16).toString('hex'), CLOUDFLARE_API_TOKEN: randomBytes(32).toString('hex'), ...extraEnv }
  const calls = [], queries = [], privateText = randomBytes(20).toString('hex')
  let current = database
  const fetcher = async (input, options) => {
    const url = new URL(input)
    assert.equal(url.origin, 'https://api.cloudflare.com')
    assert.ok(url.pathname.startsWith(`/client/v4/accounts/${env.CLOUDFLARE_ACCOUNT_ID}/`))
    assert.ok(options.headers.authorization.endsWith(env.CLOUDFLARE_API_TOKEN))
    assert.equal(options.redirect, 'error')
    calls.push({ path: url.pathname.split('/').slice(5).join('/'), method: options.method })
    if (denied) return new Response(privateText, { status: 403 })
    let result
    if (url.pathname.endsWith('/query')) {
      assert.equal(options.method, 'POST')
      assert.ok(url.pathname.endsWith(`/d1/database/${current}/query`), 'queries go only to the inventoried staging database')
      const body = JSON.parse(options.body)
      queries.push(body)
      const expires = Date.now() + 365 * 86400000
      return Response.json({ success: true, result: [/^INSERT INTO trials/.test(body.sql)
        ? { success: true, meta: { changes: grantChanges }, results: [] }
        : { success: true, meta: { changes: 0 }, results: [{ enabled: 1, source: 'manual', expires_at: expires }] }] })
    }
    if (url.pathname.endsWith('/d1/database')) {
      if (options.method === 'POST') {
        assert.deepEqual(JSON.parse(options.body), { name: STAGING.name, primary_location_hint: 'enam', read_replication: { mode: 'disabled' } })
        current = randomUUID()
        if (loseWrite) throw new Error(privateText)
        result = { uuid: current, name: STAGING.name }
      } else {
        assert.equal(url.searchParams.get('name'), STAGING.name)
        result = current ? [{ uuid: current, name: STAGING.name }] : []
        if (ambiguous) result.push({ uuid: randomUUID(), name: STAGING.name }, { uuid: randomUUID(), name: STAGING.name })
      }
    } else if (url.pathname.endsWith('/workers/scripts')) result = workers
    else {
      assert.ok(url.pathname.endsWith('/workers/domains'))
      assert.equal(url.searchParams.get('hostname'), new URL(STAGING.origin).hostname)
      result = domains
    }
    return Response.json({ success: true, result })
  }
  return { api: cloudflare(env, fetcher), calls, queries, privateText }
}

// The workflow's grant-trial operation is how an operator with no wrangler login gets a trial
// granted, so it writes production-shaped data with a real token. It must run exactly the statement
// grant-trial.mjs prints (which service.test.mjs executes against D1), bind the account rather than
// splice it, write only the inventoried staging database, and prove the result.
test('a trial grant binds one account, writes only the staging database and reads the trial back', async () => {
  const p = provider({ database: ids.databaseId }), account = randomUUID()
  const result = await p.api.grantTrial(ids.databaseId, account, 365)
  assert.deepEqual({ trial: result.trial, source: result.source }, { trial: 'active', source: 'manual' })
  assert.ok(result.daysLeft >= 364 && !JSON.stringify(result).includes(account), 'the result reports the trial, never the account')
  assert.equal(p.queries.length, 2)
  const [write, read] = p.queries
  assert.match(write.sql, /^INSERT INTO trials/)
  assert.doesNotMatch(write.sql, /DELETE/i)
  assert.ok(!write.sql.includes(account), 'the account travels as a parameter, never spliced into SQL')
  assert.deepEqual(write.params, [account])
  assert.ok(write.sql.includes(`unixepoch() + ${365 * 86400}`))
  assert.deepEqual(read.params, [account])
  assert.deepEqual(p.calls.map(call => call.method), ['GET', 'POST', 'POST'], 'the database was confirmed by inventory before any write')
  // One source for the printed and executed forms: bind the account back in and they are identical.
  const grant = trialGrant(account, 365)
  assert.equal(grant.statement.replace('?', `'${account}'`), grant.printable)
})

test('a trial grant refuses bad input unsent, an unknown account, a foreign database and leaks no provider body', async () => {
  const p = provider({ database: ids.databaseId })
  for (const [account, days] of [['not-a-uuid', 14], [randomUUID(), 0], [randomUUID(), 366], [randomUUID(), 1.5],
    [randomUUID(), Number.NaN], [`${randomUUID()}' OR '1'='1`, 14], [randomUUID().toUpperCase(), 14]]) {
    await assert.rejects(p.api.grantTrial(ids.databaseId, account, days))
  }
  assert.equal(p.calls.length, 0, 'nothing was sent for invalid input')
  // Positive control for the loop above: the same provider accepts a valid grant.
  await p.api.grantTrial(ids.databaseId, randomUUID(), 14)
  assert.equal(p.queries.length, 2)

  const missing = provider({ database: ids.databaseId, grantChanges: 0 })
  await assert.rejects(missing.api.grantTrial(ids.databaseId, randomUUID(), 14), /does not exist in the staging database/)
  assert.equal(missing.queries.length, 1, 'no read-back after a grant that wrote nothing')

  const foreign = provider({ database: ids.databaseId })
  await assert.rejects(foreign.api.grantTrial(randomUUID(), randomUUID(), 14), /not the staging database/)
  assert.equal(foreign.queries.length, 0)

  const denied = provider({ database: ids.databaseId, denied: true })
  await assert.rejects(denied.api.grantTrial(ids.databaseId, randomUUID(), 14), error => !error.message.includes(denied.privateText))
})

test('inspection makes only reads and does not misrepresent write permissions', async () => {
  const p = provider({ database: ids.databaseId, workers: [{ id: STAGING.name, tags: [STAGING.tag] }] }), result = await p.api.inspect()
  assert.equal(result.databaseId, ids.databaseId)
  assert.equal(p.calls.length, 3)
  assert.equal(result.workerExists, true)
  assert.ok(p.calls.every(call => call.method === 'GET'))
  assert.match(result.writeAccess, /not established/)
})

test('provisioning creates once, verifies inventory and reuses the exact database on rerun', async () => {
  const p = provider(), first = await p.api.provision(), second = await p.api.provision()
  assert.equal(first.created, true)
  assert.equal(second.created, false)
  assert.equal(first.databaseId, second.databaseId)
  assert.equal(p.calls.filter(call => call.method === 'POST').length, 1)
})

test('denied/ambiguous reads and hostname collisions refuse writes without logging provider bodies', async () => {
  for (const options of [{ denied: true }, { ambiguous: true }, { workers: [{ id: STAGING.name, tags: [] }] },
    { domains: [{ hostname: new URL(STAGING.origin).hostname, service: 'unrelated-service' }] }]) {
    const p = provider(options)
    await assert.rejects(p.api.provision(), error => { assert.ok(!error.message.includes(p.privateText)); return true })
    assert.ok(p.calls.every(call => call.method === 'GET'))
  }
  const p = provider({ loseWrite: true })
  await assert.rejects(p.api.provision(), /before a complete response/)
  assert.equal(p.calls.filter(call => call.method === 'POST').length, 1, 'an ambiguous write is not automatically repeated')
})

function uploadedProvider({ fault, denied = false, legacy = false, observabilityAbsent = false, tagged = false, secrets = {}, secretText = 'synthetic|someone' } = {}) {
  const env = { CLOUDFLARE_ACCOUNT_ID: randomBytes(16).toString('hex'), CLOUDFLARE_API_TOKEN: randomBytes(32).toString('hex'), ...secrets }
  const writes = [], privateText = randomBytes(20).toString('hex')
  let tags = tagged ? [STAGING.tag] : null, attached = false
  const config = stagingConfig(template, ids)
  const settings = { compatibility_date: config.compatibility_date, observability: { enabled: false }, bindings: [
    ...Object.entries(config.vars).map(([name, text]) => ({ name, type: 'plain_text', text })),
    { name: 'DB', type: 'd1', id: ids.databaseId }, { name: 'ASSETS', type: 'assets' },
    { name: 'STATIONS', type: 'durable_object_namespace', class_name: 'StationRoom' },
    // Every secret the config declares required must be present AND encrypted. A live Worker
    // carrying ADMIN_SUBJECT as a plain var, or not carrying it at all, is not the artifact.
    ...(config.secrets?.required ?? []).map(name => ({ name, type: 'secret_text' })),
  ] }
  if (fault === 'secret-as-var') {
    const row = settings.bindings.find(binding => binding.name === 'ADMIN_SUBJECT')
    row.type = 'plain_text'; row.text = secretText
  }
  if (fault === 'secret-missing') settings.bindings = settings.bindings.filter(b => b.name !== 'ADMIN_SUBJECT')
  if (fault === 'identity') settings.bindings.find(row => row.name === 'AUTH0_ISSUER').text = 'https://other.auth0.com/'
  if (fault === 'revision') settings.bindings.find(row => row.name === 'REMOTE_BUILD_REVISION').text = '0'.repeat(40)
  if (fault === 'database') settings.bindings.find(row => row.name === 'DB').id = randomUUID()
  if (fault === 'namespace') settings.bindings.find(row => row.name === 'STATIONS').script_name = 'another-service'
  if (fault === 'extra-binding') settings.bindings.push({ name: privateText, type: 'secret_text' })
  if (fault === 'observability') settings.observability.enabled = true
  if (fault === 'logs') settings.observability.logs = { enabled: true }
  if (observabilityAbsent) delete settings.observability
  const additional = Object.fromEntries(['assets/index.html', 'assets/remote-licenses.txt', 'migrations/0001_observation.sql']
    .map(name => [name, artifact.manifest.files[name]]))
  const fetcher = async (input, options) => {
    const url = new URL(input)
    assert.equal(url.origin, 'https://api.cloudflare.com')
    assert.equal(options.headers.authorization, `Bearer ${env.CLOUDFLARE_API_TOKEN}`)
    if (denied) return Response.json({ success: false, errors: [{ code: 10000, message: privateText }] }, { status: 403 })
    const path = url.pathname.slice(`/client/v4/accounts/${env.CLOUDFLARE_ACCOUNT_ID}`.length)
    if (options.method && options.method !== 'GET') writes.push({ path, method: options.method })
    let result
    if (path === '/d1/database') result = [{ uuid: ids.databaseId, name: STAGING.name }]
    else if (path === '/workers/scripts') result = [{ id: STAGING.name, tags }]
    else if (path === '/workers/domains') {
      if (options.method === 'PUT') {
        assert.deepEqual(JSON.parse(options.body), { hostname: new URL(STAGING.origin).hostname, service: STAGING.name, zone_name: STAGING.zone })
        attached = true
      }
      result = attached || fault === 'domain' ? [{ hostname: new URL(STAGING.origin).hostname,
        service: fault === 'domain' ? 'another-service' : STAGING.name }] : []
    } else if (path === `/workers/scripts/${STAGING.name}/settings`) result = settings
    else if (path === `/workers/scripts/${STAGING.name}/content/v2`) {
      const form = new FormData()
      form.set('worker.js', new File([fault === 'code' ? privateText : await readFile(join(root, 'remote/dist/index.js'))], 'worker.js', { type: 'application/javascript' }))
      if (legacy) for (const name of Object.keys(additional)) form.set(name,
        new File([fault === 'legacy-module' ? privateText : await readFile(join(scratch, 'remote/staging-artifact', name))], name))
      if (fault === 'extra-module') form.set('extra.js', new File([privateText], 'extra.js'))
      return new Response(form, { headers: { 'cf-entrypoint': 'worker.js' } })
    } else if (path === `/workers/scripts/${STAGING.name}/script-settings`) {
      if (options.method === 'GET') return Response.json({ success: true, result: { observability: settings.observability } })
      assert.equal(options.method, 'PATCH')
      const body = JSON.parse(options.body)
      assert.deepEqual(body, { tags: [STAGING.tag] })
      tags = body.tags; result = { tags }
    } else assert.fail('Unexpected provider request')
    return Response.json({ success: true, result })
  }
  return { api: cloudflare(env, fetcher), config, writes, privateText, additional, env, fetcher }
}

// Both staging runs on 2026-09-13 learned about ADMIN_SUBJECT only AFTER writing: 34789356296 applied
// migrations and then failed at upload, 34792345723 uploaded and then failed its secret check with
// the Worker live. Every deploy now applies the secret from its GitHub environment, and the preflight
// refuses a run whose environment does not supply it, before any request and before any write.
const subject = 'google-oauth2|104857600000000000042'
test('the deploy preflight requires every Worker secret from the environment, and refuses a foreign database or unowned Worker, before any write', async () => {
  const required = template.secrets?.required
  assert.deepEqual(required, ['ADMIN_SUBJECT'], 'positive control: the template declares the secret this guards')

  // Not firing: the environment supplies it, as the value or, for steps that must not hold it, as
  // presence only. The live Worker's current binding is reported, never required: the upload replaces it.
  for (const [secrets, fault, live] of [
    [{ REMOTE_ADMIN_SUBJECT: subject }, undefined, 'secret_text'],
    [{ REMOTE_ADMIN_SUBJECT_PRESENT: 'true' }, undefined, 'secret_text'],
    [{ REMOTE_ADMIN_SUBJECT: subject }, 'secret-missing', 'absent'],
    [{ REMOTE_ADMIN_SUBJECT: subject }, 'secret-as-var', 'plain_text'],
  ]) {
    const p = uploadedProvider({ tagged: true, fault, secrets })
    const result = await p.api.preflight(required, ids.databaseId)
    assert.deepEqual(result.liveSecretTypes, { ADMIN_SUBJECT: live })
    assert.deepEqual(result.secretsSupplied, ['ADMIN_SUBJECT'])
    const reported = JSON.stringify(result)
    assert.ok(!reported.includes(subject) && !reported.includes('synthetic|someone'), 'no secret value is ever reported')
    assert.equal(p.writes.length, 0)
  }
  // A first deploy has no Worker to hold anything, and may proceed: the upload brings the secret.
  const fresh = provider({ database: ids.databaseId, extraEnv: { REMOTE_ADMIN_SUBJECT_PRESENT: 'true' } })
  assert.deepEqual((await fresh.api.preflight(required, ids.databaseId)).liveSecretTypes, { ADMIN_SUBJECT: 'no Worker yet' })
  assert.ok(fresh.calls.every(call => call.method === 'GET'))

  // Firing: not supplied, in each shape an unset or mangled GitHub secret takes. Refused before any request.
  for (const secrets of [{}, { REMOTE_ADMIN_SUBJECT: '' }, { REMOTE_ADMIN_SUBJECT: `${subject} ` }, { REMOTE_ADMIN_SUBJECT: 'short' },
    { REMOTE_ADMIN_SUBJECT_PRESENT: 'false' }, { REMOTE_ADMIN_SUBJECT_PRESENT: 'TRUE' }]) {
    const p = provider({ database: ids.databaseId, extraEnv: secrets })
    await assert.rejects(p.api.preflight(required, ids.databaseId), error => {
      assert.match(error.message, /^REMOTE_ADMIN_SUBJECT not set in this GitHub environment; .*nothing was migrated or uploaded$/)
      assert.ok(!error.message.includes(subject))
      return true
    })
    assert.equal(p.calls.length, 0, JSON.stringify(Object.keys(secrets)))
  }
  // Firing: the other preconditions, with the secret supplied.
  const supplied = { REMOTE_ADMIN_SUBJECT: subject }
  for (const [options, database, expected] of [
    [{ tagged: true, secrets: supplied }, randomUUID(), /artifact database does not match/],
    [{ tagged: false, secrets: supplied }, ids.databaseId, /unrecognized deployment/],
  ]) {
    const p = uploadedProvider(options)
    await assert.rejects(p.api.preflight(required, database), expected)
    assert.equal(p.writes.length, 0)
  }
  await assert.rejects(provider({ extraEnv: supplied }).api.preflight(required), /database is absent/)
})

test('migrate and deploy each run the preflight themselves, so neither writes without the secret the upload applies', async () => {
  for (const mode of ['migrate', 'deploy']) {
    const p = uploadedProvider({ tagged: true })
    // Refused before Wrangler starts: past the preflight this would spawn Wrangler, which fails differently.
    await assert.rejects(uploadArtifact(scratch, mode, { ...p.env, PATH: process.env.PATH, GITHUB_SHA: ids.revision }, p.fetcher),
      /REMOTE_ADMIN_SUBJECT not set in this GitHub environment/)
    assert.equal(p.writes.length, 0, mode)
  }
})

test('the upload hands wrangler its secrets in a private file removed afterwards, and never prints a value', async () => {
  const env = { REMOTE_ADMIN_SUBJECT: subject }
  let path
  const result = await withSecretsFile(env, ['ADMIN_SUBJECT'], async file => {
    path = file
    assert.equal((await stat(file)).mode & 0o777, 0o600)
    assert.ok(!file.startsWith(root), 'never inside the repository')
    assert.deepEqual(JSON.parse(await readFile(file, 'utf8')), { ADMIN_SUBJECT: subject })
    return 'uploaded'
  })
  assert.equal(result, 'uploaded')
  await assert.rejects(stat(path), { code: 'ENOENT' }, 'removed once the upload returns')
  await assert.rejects(withSecretsFile(env, ['ADMIN_SUBJECT'], async file => { path = file; throw new Error('upload failed') }), /upload failed/)
  await assert.rejects(stat(path), { code: 'ENOENT' }, 'removed when the upload throws too')
  // Refused by variable name, before any file exists or any upload runs.
  for (const value of [undefined, '', ' ', 'short', `${subject}\n`, ` ${subject}`]) {
    await assert.rejects(withSecretsFile({ REMOTE_ADMIN_SUBJECT: value }, ['ADMIN_SUBJECT'], async () => assert.fail('no upload without the secret')),
      error => error.message === 'REMOTE_ADMIN_SUBJECT is not set in this GitHub environment as a usable secret')
  }
  assert.equal(await withSecretsFile({}, [], async file => file), null, 'nothing required: no file and no flag')

  // Redacted from Wrangler's diagnostics like the token. Positive control: the subject has no run long
  // enough for the token-shaped pattern, so only the REMOTE_* pass can remove it.
  assert.ok(redactedDiagnostic(`upload failed for ${subject}`, {}).includes(subject), 'positive control: the pattern alone misses it')
  assert.ok(!redactedDiagnostic(`upload failed for ${subject}`, env).includes(subject))
  assert.ok(redactedDiagnostic('workers_dev_subdomain_not_configured', { ...env, REMOTE_ADMIN_SUBJECT_PRESENT: 'true' })
    .includes('workers_dev_subdomain_not_configured'), 'error slugs stay readable')
})

test('a secret the upload applied must read back encrypted; one that arrived as plain text fails, and its value is never logged', async () => {
  const logs = [], live = () => verifyLive(artifact, served)
  const good = uploadedProvider()
  const report = await settleUpload(good.api, good.config, artifact.manifest, live, line => logs.push(line))
  assert.equal(report.uploadCheck.passed, true, 'positive control: ADMIN_SUBJECT read back as secret_text')
  const bad = uploadedProvider({ fault: 'secret-as-var', secretText: subject })
  await assert.rejects(settleUpload(bad.api, bad.config, artifact.manifest, live, line => logs.push(line)), error => {
    assert.match(error.message, /A declared secret is missing or was uploaded as plain text/)
    assert.ok(!error.message.includes(subject))
    return true
  })
  assert.equal(logs.length, 2)
  assert.ok(logs.every(line => !line.includes(subject)))
})

test('a failed post-upload check still runs the live verification, and the deploy reports both', async () => {
  const live = () => verifyLive(artifact, served)
  const stale = () => verifyLive(artifact, async (input, options) => new URL(input).pathname.endsWith('/config')
    ? Response.json({ ...await (await served(input, options)).json(), revision: 'stale' })
    : served(input, options))

  // Not firing: both pass, and only then is the Worker tagged and its domain attached.
  const good = uploadedProvider(), logs = []
  const report = await settleUpload(good.api, good.config, artifact.manifest, live, line => logs.push(line))
  assert.equal(report.uploadCheck.passed, true)
  assert.equal(report.liveVerification.passed, true)
  assert.equal(report.liveVerification.revision, ids.revision)
  assert.deepEqual(good.writes.map(write => write.method), ['PATCH', 'PUT'])
  assert.equal(logs.length, 1)
  assert.deepEqual(JSON.parse(logs[0]).uploadCheck, { passed: true })

  // THE CASE THIS EXISTS FOR (run 34792345723): the Worker is live, its upload check fails, and the
  // live verification must run anyway and be reported beside the failure.
  const bad = uploadedProvider({ fault: 'secret-as-var' })
  let verifications = 0
  await assert.rejects(settleUpload(bad.api, bad.config, artifact.manifest, () => { verifications++; return live() }, () => {}), error => {
    assert.match(error.message, /post-upload check failed \(A declared secret is missing or was uploaded as plain text\); live verification passed/)
    assert.ok(!error.message.includes('synthetic|someone'))
    return true
  })
  assert.equal(verifications, 1)
  assert.equal(bad.writes.length, 0, 'an upload that failed its check is neither tagged nor given the domain')

  // The upload check passes and the live Worker is not the artifact: still a failed deploy, named.
  const drift = uploadedProvider()
  await assert.rejects(settleUpload(drift.api, drift.config, artifact.manifest, stale, () => {}),
    /post-upload check passed; live verification failed \(The live Worker does not match/)

  // Both fail: both named.
  const both = uploadedProvider({ fault: 'secret-missing' })
  await assert.rejects(settleUpload(both.api, both.config, artifact.manifest, stale, () => {}),
    /post-upload check failed \(.+\); live verification failed \(/)
})

test('the live check is retried while an upload propagates, and its last failure is the one reported', async () => {
  let first = true
  const propagating = async (input, options) => {
    if (first && new URL(input).pathname.endsWith('/config')) { first = false; return Response.json({ ready: false }) }
    return served(input, options)
  }
  const logs = []
  const result = await verifyLiveSettled(artifact, propagating, STAGING, { attempts: 3, wait: 0, log: line => logs.push(line) })
  assert.equal(result.revision, ids.revision)
  assert.deepEqual(logs, ['The complete staging artifact is not verified yet (attempt 1/3).'])
  let reads = 0
  await assert.rejects(verifyLiveSettled(artifact, async () => { reads++; return Response.json({ ready: false }) }, STAGING,
    { attempts: 2, wait: 0, log: () => {} }), /does not match the selected source/)
  assert.equal(reads, 2)
})

test("the environment table: staging resolves to today's exact names, and production is a placeholder nothing can deploy", async () => {
  // Byte for byte the names every staging run used before the two-environment tooling.
  assert.deepEqual({ name: STAGING.name, database: STAGING.database, origin: STAGING.origin, zone: STAGING.zone,
    audience: STAGING.audience, tag: STAGING.tag }, {
    name: 'nexus-remote-staging', database: 'nexus-remote-staging', origin: 'https://remote-staging.hamradiotools.io',
    zone: 'hamradiotools.io', audience: 'https://remote-staging.hamradiotools.io/api', tag: 'nexus-remote-observation-staging' })
  assert.equal(target({}), STAGING, 'unset means staging, as every existing run meant')
  assert.equal(target({ REMOTE_TARGET: 'staging' }), STAGING)
  for (const value of ['', 'prod', 'Production', 'toString', '__proto__']) {
    assert.throws(() => target({ REMOTE_TARGET: value }), /staging or production/, value)
  }
  const config = stagingConfig(template, ids)
  assert.equal(config.name, 'nexus-remote-staging')
  assert.equal(config.d1_databases[0].database_name, 'nexus-remote-staging')
  assert.equal(config.vars.PUBLIC_REMOTE_ORIGIN, 'https://remote-staging.hamradiotools.io')
  assert.equal(config.vars.AUTH0_AUDIENCE, 'https://remote-staging.hamradiotools.io/api')
  assert.deepEqual(config.routes, [{ pattern: 'remote-staging.hamradiotools.io', custom_domain: true }])
  assert.equal(deployable(STAGING), STAGING)

  // Production resolves, so it can be read and reviewed, and is refused everywhere a script could act.
  const production = target({ REMOTE_TARGET: 'production' })
  assert.equal(production, TARGETS.production)
  assert.ok(production.placeholder)
  const productionIds = { ...ids, issuer: production.issuer.exact, audience: production.audience }
  assert.throws(() => stagingConfig(template, productionIds, production), /placeholder/)
  let sent = 0
  assert.throws(() => cloudflare({ CLOUDFLARE_ACCOUNT_ID: 'a'.repeat(32), CLOUDFLARE_API_TOKEN: 'synthetic' },
    async () => { sent++; return Response.json({}) }, production), /placeholder/)
  await assert.rejects(createArtifact(scratch, productionIds, production), /placeholder/)
  await verifyArtifact(scratch, ids.revision)
  assert.throws(() => deployable({ ...production, placeholder: undefined }), /Unknown Remote deployment target/,
    'a copy with its placeholder deleted is not the table')
  assert.equal(sent, 0)

  // The issuer rules: production pins one exact sign-in domain; staging keeps the tenant's own domain.
  assert.equal(identity(productionIds, production).issuer, 'https://login.hamradiotools.io/')
  assert.throws(() => identity({ ...productionIds, issuer: ids.issuer }, production), /issuer must be exactly/)
  assert.throws(() => identity({ ...ids, issuer: production.issuer.exact }), /Auth0 tenant HTTPS domain/)
  assert.throws(() => identity({ ...ids, audience: production.audience }), /staging API audience/)
  assert.equal(identity(ids).issuer, ids.issuer)
})


test('an untagged upload is recovered only from exact artifact bytes and bindings, without replacing code', async () => {
  const p = uploadedProvider()
  await assert.rejects(p.api.inspect(), /unrecognized deployment/)
  assert.equal(p.writes.length, 0)
  const result = await p.api.recover(p.config, artifact.manifest.files['worker.js'])
  assert.equal(result.revision, ids.revision)
  assert.equal((await p.api.inspect()).workerExists, true)
  await p.api.recover(p.config, artifact.manifest.files['worker.js'])
  assert.deepEqual(p.writes, [{ path: `/workers/scripts/${STAGING.name}/script-settings`, method: 'PATCH' }])
  await p.api.attachDomain()
  await p.api.attachDomain()
  assert.deepEqual(p.writes[1], { path: '/workers/domains', method: 'PUT' })
  assert.equal(p.writes.length, 2, 'domain attachment is confirmed and reused')
})

test('mismatched upload bytes, identities, resources, runtime and domains refuse recovery before any write', async () => {
  for (const fault of ['identity', 'revision', 'database', 'namespace', 'extra-binding', 'secret-as-var', 'secret-missing', 'observability', 'logs', 'code', 'extra-module', 'domain']) {
    const p = uploadedProvider({ fault })
    await assert.rejects(p.api.recover(p.config, artifact.manifest.files['worker.js']), error => {
      assert.ok(!error.message.includes(p.privateText)); return true
    })
    assert.equal(p.writes.length, 0, fault)
  }
  const p = uploadedProvider({ denied: true })
  await assert.rejects(p.api.recover(p.config, artifact.manifest.files['worker.js']), /HTTP 403; Cloudflare codes 10000/)
  assert.equal(p.writes.length, 0)
})

test('legacy text modules require every reviewed hash, and absent observability follows Wrangler defaults', async () => {
  const p = uploadedProvider({ legacy: true, observabilityAbsent: true })
  await assert.rejects(p.api.recover(p.config, artifact.manifest.files['worker.js']), /module inventory/)
  assert.equal(p.writes.length, 0)
  await p.api.recover(p.config, artifact.manifest.files['worker.js'], p.additional)
  assert.equal(p.writes.length, 1)
  const bad = uploadedProvider({ legacy: true, fault: 'legacy-module' })
  await assert.rejects(bad.api.recover(bad.config, artifact.manifest.files['worker.js'], bad.additional), /additional module bytes/)
  assert.equal(bad.writes.length, 0)
  await assert.rejects(bad.api.recover(bad.config, artifact.manifest.files['worker.js'], { 'unknown.txt': 'a'.repeat(64) }), /recovery module inventory/)
  assert.equal(bad.writes.length, 0)
})

test('provider diagnostics expose numeric error codes only, and Worker content framing is checked', async () => {
  const privateText = randomBytes(20).toString('hex')
  await assert.rejects(requestJson('https://example.invalid', {}, async () => Response.json({ errors: [
    { code: 10000, message: privateText }, { code: privateText }, { code: -1 },
  ] }, { status: 403 }), 'Provider'), error => error.message === 'Provider failed (HTTP 403; Cloudflare codes 10000)')
  const bytes = Buffer.from('export default {}'), headers = new Headers({ 'content-type': 'application/javascript' })
  assert.equal(await workerDigest({ status: 200, headers, bytes }), createHash('sha256').update(bytes).digest('hex'))
  const form = new FormData()
  form.set('../../staging-artifact/worker.js', bytes.toString('utf8'))
  const response = new Response(form, { headers: { 'cf-entrypoint': '../../staging-artifact/worker.js' } })
  const multipart = { status: 200, headers: response.headers, bytes: Buffer.from(await response.arrayBuffer()) }
  assert.equal(await workerDigest(multipart), createHash('sha256').update(bytes).digest('hex'))
  multipart.headers.set('cf-entrypoint', 'other.js')
  await assert.rejects(workerDigest(multipart), /module inventory/)
  await assert.rejects(workerDigest({ status: 200, headers: new Headers({ 'content-type': 'text/html' }), bytes }), /media type/)
  await assert.rejects(workerDigest({ status: 200, headers: new Headers({ 'content-type': 'multipart/form-data; boundary=missing' }), bytes }), /framing/)
})

test('public discovery rejects issuer/JWKS substitution and missing PKCE without making a login claim', async () => {
  const doc = { issuer: ids.issuer, authorization_endpoint: `${ids.issuer}authorize`, token_endpoint: `${ids.issuer}oauth/token`,
    jwks_uri: `${ids.issuer}.well-known/jwks.json`, response_types_supported: ['code'],
    id_token_signing_alg_values_supported: ['RS256'], code_challenge_methods_supported: ['S256'] }
  for (const change of [{}, { issuer: 'https://other.auth0.com/' }, { jwks_uri: 'https://other.auth0.com/keys' }, { code_challenge_methods_supported: [] }]) {
    let reads = 0
    const fetcher = async input => {
      reads++
      assert.ok(String(input).startsWith(ids.issuer))
      return Response.json(reads === 1 ? { ...doc, ...change } : { keys: [{ kty: 'RSA', use: 'sig', kid: 'synthetic', n: 'synthetic', e: 'AQAB' }] })
    }
    if (Object.keys(change).length) { await assert.rejects(verifyIdentity(ids, fetcher)); assert.equal(reads, 1) }
    else { const result = await verifyIdentity(ids, fetcher); assert.equal(reads, 2); assert.match(result.applicationSettings, /require.*real sign-in/) }
  }
})

test('source verification is anonymous and checks the exact full commit, including a negative control', async () => {
  const fetcher = async (url, options) => {
    assert.ok(String(url).endsWith(`/git/commits/${ids.revision}`))
    assert.equal(options.headers.authorization, undefined)
    return Response.json({ sha: ids.revision })
  }
  assert.ok((await verifyPublicSource(ids.revision, fetcher)).endsWith(ids.revision))
  await assert.rejects(verifyPublicSource(ids.revision, async () => Response.json({ sha: 'wrong' })), /exact source/)
  await assert.rejects(verifyPublicSource(ids.revision, async () => new Response('', { status: 404 })), /HTTP 404/)
})

test('artifact verification rejects changed bytes, unlisted files, symlinks and overwrite', async () => {
  const directory = join(scratch, 'remote/staging-artifact'), path = join(directory, 'worker.js'), original = await readFile(path)
  await writeFile(path, Buffer.concat([original, Buffer.from('\n// tampered')]))
  await assert.rejects(verifyArtifact(scratch, ids.revision), /bytes changed/)
  await writeFile(path, original)
  const extra = join(directory, 'assets/unlisted.js')
  await writeFile(extra, 'unlisted')
  await assert.rejects(verifyArtifact(scratch, ids.revision), /inventory changed/)
  await rm(extra)
  await symlink(path, extra)
  await assert.rejects(verifyArtifact(scratch, ids.revision), /symlinks/)
  await rm(extra)
  await assert.rejects(createArtifact(scratch, ids), { code: 'EEXIST' })
  await verifyArtifact(scratch, ids.revision)
})

test('the hosted Nexus artifact carries its reviewed map assets and bundled dependency notices', async () => {
  const directory = join(scratch, 'remote/staging-artifact/assets')
  const files = Object.keys(artifact.manifest.files)
  for (const stem of ['earth-night', 'earth-relief', 'cqzones']) {
    assert.ok(files.some(name => name.startsWith(`assets/assets/${stem}-`)), 'existing Nexus map assets must travel in the artifact')
  }
  const notices = await readFile(join(directory, 'remote-licenses.txt'), 'utf8')
  for (const name of ['react 18.3.1', '@radix-ui/react-dialog', 'three ', 'react-globe.gl', 'Copyright (c) 2024 HB9HIL', 'Copyright (c) 2022 WorkOS']) {
    assert.ok(notices.includes(name), `bundled notice must cover ${name}`)
  }
})

test('the configuration CLI refuses overwrites and produces a private file from public IDs only', async () => {
  const directory = join(scratch, 'remote/scripts')
  await mkdir(directory)
  for (const name of ['staging-common.mjs', 'staging-config.mjs']) await copyFile(join(root, 'remote/scripts', name), join(directory, name))
  const args = [join(directory, 'staging-config.mjs'), '--issuer', ids.issuer, '--client-id', ids.clientId,
    '--audience', ids.audience, '--database-id', ids.databaseId, '--revision', ids.revision]
  const first = spawnSync(process.execPath, args, { encoding: 'utf8' })
  assert.equal(first.status, 0, first.stderr)
  const configPath = join(scratch, 'remote/wrangler.staging.jsonc')
  assert.equal((await stat(configPath)).mode & 0o777, 0o600)
  const second = spawnSync(process.execPath, args, { encoding: 'utf8' })
  assert.equal(second.status, 1)
  assert.match(second.stderr, /already exists/)
})

// The dry-run passes the deploy's exact --secrets-file flag and file shape, holding synthetic values, so
// this also proves the pinned Wrangler accepts them offline: a malformed secrets file exits 1.
test('the actual pinned Wrangler emits only the verified Worker module without credentials or changing artifact bytes', async () => {
  await uploadArtifact(scratch, 'dry-run', { PATH: process.env.PATH, GITHUB_SHA: ids.revision })
  await verifyArtifact(scratch, ids.revision)
})

const served = (input, options) => app.mf.dispatchFetch(input, options)
test('live verification exercises the real compiled Worker, asset bytes and authentication refusals', async () => {
  const result = await verifyLive(artifact, served)
  assert.equal(result.revision, ids.revision)
  assert.ok(result.assetsChecked >= 3)
})

test('a wrong Worker revision, stale asset, missing security header or open admission fails the same live check', async () => {
  for (const fault of ['revision', 'asset', 'headers', 'admission']) {
    const corrupted = async (input, options) => {
      const path = new URL(input).pathname
      if (fault === 'admission' && path.endsWith('/session')) return Response.json({})
      const actual = await served(input, options)
      if (fault === 'revision' && path.endsWith('/config')) return Response.json({ ...await actual.json(), revision: 'stale' })
      if (fault === 'asset' && path.endsWith('.js')) return new Response('stale bytes', { status: 200 })
      if (fault === 'headers' && path === '/') {
        const headers = new Headers(actual.headers); headers.delete('content-security-policy')
        return new Response(actual.body, { status: actual.status, headers })
      }
      return actual
    }
    await assert.rejects(verifyLive(artifact, corrupted))
  }
})

test('incomplete or oversized provider responses fail without reflecting their contents', async () => {
  const privateText = randomBytes(20).toString('hex')
  await assert.rejects(requestBytes('https://example.invalid', {}, async () => { throw new Error(privateText) }), error => !error.message.includes(privateText))
  await assert.rejects(requestBytes('https://example.invalid', {}, async () => new Response(Buffer.alloc(8 * 1024 * 1024 + 1))), /complete response/)
})
