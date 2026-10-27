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
import { STAGING, TARGETS, billingServices, deployable, identity, optionalVars, settingsFromEnv, stagingConfig, target, verifyIdentity, requestBytes, requestJson } from '../scripts/staging-common.mjs'
import { MODES, cloudflare, relayKeyCheck, workerDigest } from '../scripts/cloudflare-staging.mjs'
import { createArtifact, verifyArtifact, verifyLive, verifyLiveSettled, verifyPublicSource } from '../scripts/staging-artifact.mjs'
import { runtime } from './runtime.mjs'
import { uploadArtifact, compareSchema, redactedDiagnostic, runWrangler, schemaQuery, settleUpload, withSecretsFile, wranglerLogLevel } from '../scripts/deploy-staging.mjs'
import { trialGrant } from '../scripts/grant-trial.mjs'
import { productionGuard, protectedEnvironment } from '../scripts/production-guard.mjs'

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
  const statements = async name => (await readFile(join(directory, name), 'utf8')).split('\n')
    .filter(line => !line.trimStart().startsWith('--')).join('\n').split(';').map(s => s.replace(/\s+/g, ' ').trim()).filter(Boolean)
  assert.deepEqual(await statements('0007_device_lifetime.sql'), ['ALTER TABLE devices ADD COLUMN approved_at INTEGER'])
  // The device key (A5) adds two nullable columns and nothing else: a browser approved before it has
  // no key, and a pairing confirmed before it brought none.
  assert.ok(names.includes('0008_device_key.sql'))
  assert.deepEqual(await statements('0008_device_key.sql'),
    ['ALTER TABLE devices ADD COLUMN public_key TEXT', 'ALTER TABLE enrollments ADD COLUMN public_key TEXT'])
  // The station's own key (S3-M1) adds one nullable column and nothing else: a station from before it
  // has none until it is updated and connects.
  assert.ok(names.includes('0009_station_key.sql'))
  assert.deepEqual(await statements('0009_station_key.sql'), ['ALTER TABLE stations ADD COLUMN public_key TEXT'])
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

function uploadedProvider({ fault, denied = false, legacy = false, observabilityAbsent = false, tagged = false, billing = false, secrets = {}, secretText = 'synthetic|someone', values = {}, live = values } = {}) {
  const env = { CLOUDFLARE_ACCOUNT_ID: randomBytes(16).toString('hex'), CLOUDFLARE_API_TOKEN: randomBytes(32).toString('hex'), ...secrets }
  const writes = [], privateText = randomBytes(20).toString('hex')
  let tags = tagged ? [STAGING.tag] : null, attached = false
  // `values` are the optional settings the artifact carries; `live`, the ones the Worker reads back.
  const config = stagingConfig(template, { ...ids, ...values }), uploaded = stagingConfig(template, { ...ids, ...live })
  // The artifact of a row whose billing Worker is deployed: the one binding it adds.
  if (billing) config.services = billingServices({ billing: { ...STAGING.billing, deployed: true } })
  const settings = { compatibility_date: config.compatibility_date, observability: { enabled: false }, bindings: [
    ...Object.entries(uploaded.vars).map(([name, text]) => ({ name, type: 'plain_text', text })),
    { name: 'DB', type: 'd1', id: ids.databaseId }, { name: 'ASSETS', type: 'assets' },
    { name: 'STATIONS', type: 'durable_object_namespace', class_name: 'StationRoom' },
    // Every secret the config declares required must be present AND encrypted. A live Worker
    // carrying ADMIN_SUBJECT as a plain var, or not carrying it at all, is not the artifact.
    ...(uploaded.secrets?.required ?? []).map(name => ({ name, type: 'secret_text' })),
  ] }
  if (fault === 'secret-as-var') {
    const row = settings.bindings.find(binding => binding.name === 'ADMIN_SUBJECT')
    row.type = 'plain_text'; row.text = secretText
  }
  if (billing && fault !== 'billing-missing') settings.bindings.push({ name: 'BILLING', type: 'service',
    service: fault === 'billing-other' ? 'another-service' : STAGING.billing.service, environment: 'production',
    ...(fault === 'billing-default-export' ? {} : { entrypoint: fault === 'billing-other-entrypoint' ? 'AnotherEntrypoint' : 'BillingService' }) })
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
      // The fields the relay's no-logs rule reads, as an untouched Worker reports them.
      if (options.method === 'GET') return Response.json({ success: true, result: { observability: settings.observability,
        logpush: fault === 'logpush', tail_consumers: fault === 'tail' ? [{ service: 'another-service' }] : null } })
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
// The stream's relay key, synthetic: two required secrets a deploy must supply beside ADMIN_SUBJECT.
const relaySecrets = { REMOTE_TURN_KEY_ID: randomBytes(16).toString('hex'), REMOTE_TURN_KEY_TOKEN: randomBytes(32).toString('hex') }
const relayPresent = { REMOTE_TURN_KEY_ID_PRESENT: 'true', REMOTE_TURN_KEY_TOKEN_PRESENT: 'true' }
test('the deploy preflight requires every Worker secret from the environment, and refuses a foreign database or unowned Worker, before any write', async () => {
  const required = template.secrets?.required
  assert.deepEqual(required, ['ADMIN_SUBJECT', 'TURN_KEY_ID', 'TURN_KEY_TOKEN'], 'positive control: the template declares the secrets this guards')

  // Not firing: the environment supplies it, as the value or, for steps that must not hold it, as
  // presence only. The live Worker's current binding is reported, never required: the upload replaces it.
  for (const [secrets, fault, live] of [
    [{ REMOTE_ADMIN_SUBJECT: subject, ...relaySecrets }, undefined, 'secret_text'],
    [{ REMOTE_ADMIN_SUBJECT_PRESENT: 'true', ...relayPresent }, undefined, 'secret_text'],
    [{ REMOTE_ADMIN_SUBJECT: subject, ...relaySecrets }, 'secret-missing', 'absent'],
    [{ REMOTE_ADMIN_SUBJECT: subject, ...relaySecrets }, 'secret-as-var', 'plain_text'],
  ]) {
    const p = uploadedProvider({ tagged: true, fault, secrets })
    const result = await p.api.preflight(required, ids.databaseId)
    assert.deepEqual(result.liveSecretTypes, { ADMIN_SUBJECT: live, TURN_KEY_ID: 'secret_text', TURN_KEY_TOKEN: 'secret_text' })
    assert.deepEqual(result.secretsSupplied, required)
    const reported = JSON.stringify(result)
    assert.ok(!reported.includes(subject) && !reported.includes('synthetic|someone'), 'no secret value is ever reported')
    assert.ok(Object.values(relaySecrets).every(value => !reported.includes(value)), 'nor the relay key')
    assert.equal(p.writes.length, 0)
  }
  // A first deploy has no Worker to hold anything, and may proceed: the upload brings the secret.
  const fresh = provider({ database: ids.databaseId, extraEnv: { REMOTE_ADMIN_SUBJECT_PRESENT: 'true', ...relayPresent } })
  assert.deepEqual((await fresh.api.preflight(required, ids.databaseId)).liveSecretTypes,
    { ADMIN_SUBJECT: 'no Worker yet', TURN_KEY_ID: 'no Worker yet', TURN_KEY_TOKEN: 'no Worker yet' })
  assert.ok(fresh.calls.every(call => call.method === 'GET'))

  // Firing: not supplied, in each shape an unset or mangled GitHub secret takes. Refused before any request.
  for (const [secrets, missing] of [
    [{}, 'REMOTE_ADMIN_SUBJECT, REMOTE_TURN_KEY_ID, REMOTE_TURN_KEY_TOKEN'],
    ...[{ REMOTE_ADMIN_SUBJECT: '' }, { REMOTE_ADMIN_SUBJECT: `${subject} ` }, { REMOTE_ADMIN_SUBJECT: 'short' },
      { REMOTE_ADMIN_SUBJECT_PRESENT: 'false' }, { REMOTE_ADMIN_SUBJECT_PRESENT: 'TRUE' }].map(admin => [{ ...admin, ...relaySecrets }, 'REMOTE_ADMIN_SUBJECT']),
    // The relay key is as required as the administrator: a deploy without it stops here too.
    [{ REMOTE_ADMIN_SUBJECT: subject }, 'REMOTE_TURN_KEY_ID, REMOTE_TURN_KEY_TOKEN'],
    [{ REMOTE_ADMIN_SUBJECT: subject, REMOTE_TURN_KEY_ID: relaySecrets.REMOTE_TURN_KEY_ID }, 'REMOTE_TURN_KEY_TOKEN'],
    [{ REMOTE_ADMIN_SUBJECT_PRESENT: 'true', REMOTE_TURN_KEY_TOKEN_PRESENT: 'true', REMOTE_TURN_KEY_ID_PRESENT: 'false' }, 'REMOTE_TURN_KEY_ID'],
  ]) {
    const p = provider({ database: ids.databaseId, extraEnv: secrets })
    await assert.rejects(p.api.preflight(required, ids.databaseId), error => {
      assert.equal(error.message, `${missing} not set in this GitHub environment; every deploy applies required Worker secrets from it, so nothing was migrated or uploaded`)
      assert.ok(!error.message.includes(subject) && Object.values(relaySecrets).every(value => !error.message.includes(value)))
      return true
    })
    assert.equal(p.calls.length, 0, JSON.stringify(Object.keys(secrets)))
  }
  // Firing: the other preconditions, with the secrets supplied.
  const supplied = { REMOTE_ADMIN_SUBJECT: subject, ...relaySecrets }
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
      /REMOTE_ADMIN_SUBJECT, REMOTE_TURN_KEY_ID, REMOTE_TURN_KEY_TOKEN not set in this GitHub environment/)
    assert.equal(p.writes.length, 0, mode)
  }
})

// GitHub never hands a secret back, so a swapped or mistyped relay key would show only on the first
// stream that needs the relay. The preflight mints one credential with it, lasting a minute, before
// anything is migrated or uploaded, and refuses the deploy unless Cloudflare answers 201 with a TURN
// server. It reports the HTTP status and nothing else (2026-10-03).
test('the deploy preflight proves the relay key by minting one short credential, and refuses without printing a value', async () => {
  const minted = randomBytes(12).toString('hex'), body = randomBytes(12).toString('hex')
  const served = { iceServers: [{ urls: 'stun:stun.cloudflare.com:3478' },
    { urls: ['turn:turn.cloudflare.com:3478?transport=udp', 'turns:turn.cloudflare.com:443?transport=tcp'], username: `u${minted}`, credential: `c${minted}` }] }
  const relay = (status, answer) => {
    const calls = []
    const fetcher = async (input, options) => {
      calls.push({ url: String(input), options })
      return new Response(typeof answer === 'string' ? answer : JSON.stringify(answer), { status })
    }
    return { calls, fetcher }
  }
  // Not firing: Cloudflare mints it. One request, to the key's own endpoint, with its token, for a minute.
  const good = relay(201, served)
  assert.deepEqual(await relayKeyCheck(relaySecrets, good.fetcher), { status: 201 })
  assert.equal(good.calls.length, 1)
  assert.equal(good.calls[0].url, `https://rtc.live.cloudflare.com/v1/turn/keys/${relaySecrets.REMOTE_TURN_KEY_ID}/credentials/generate-ice-servers`)
  assert.equal(good.calls[0].options.method, 'POST')
  assert.equal(good.calls[0].options.headers.authorization, `Bearer ${relaySecrets.REMOTE_TURN_KEY_TOKEN}`)
  assert.deepEqual(JSON.parse(good.calls[0].options.body), { ttl: 3600 })
  assert.equal(good.calls[0].options.redirect, 'error')

  // Firing: every other answer, with its status and no value in the message.
  for (const [status, answer, said] of [
    [401, { errors: [{ code: 10000, message: body }] }, /HTTP 401/],
    [403, body, /HTTP 403/],
    [404, { errors: [{ code: 10007, message: body }] }, /HTTP 404/],
    [200, served, /HTTP 200/],
    [201, { iceServers: [{ urls: 'stun:stun.cloudflare.com:3478' }] }, /201 without a TURN server/],
    [201, { iceServers: [{ urls: 'turn:turn.cloudflare.com:3478', username: '', credential: `c${minted}` }] }, /201 without a TURN server/],
    [201, { iceServers: { urls: 'turn:turn.cloudflare.com:3478', username: `u${minted}`, credential: `c${minted}` } }, /201 without a TURN server/],
    [201, body, /201 without a TURN server/],
  ]) {
    const bad = relay(status, answer)
    await assert.rejects(relayKeyCheck(relaySecrets, bad.fetcher), error => {
      assert.match(error.message, said)
      assert.match(error.message, /nothing was migrated or uploaded$/)
      for (const value of [...Object.values(relaySecrets), minted, body]) assert.ok(!error.message.includes(value), `${status}: a value was printed`)
      return true
    })
    assert.equal(bad.calls.length, 1)
  }
  // Firing before any request: a step holding the key's presence alone cannot mint, and neither can a
  // key id that would not stay one path segment.
  for (const env of [relayPresent, { ...relaySecrets, REMOTE_TURN_KEY_ID: `${relaySecrets.REMOTE_TURN_KEY_ID}/../x` }]) {
    const unsent = relay(201, served)
    await assert.rejects(relayKeyCheck(env, unsent.fetcher), /REMOTE_TURN_KEY_ID is not/)
    assert.equal(unsent.calls.length, 0)
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

  // Each row names its billing Worker, and neither binds it yet.
  assert.deepEqual(STAGING.billing, { service: 'nexus-billing-staging', deployed: false })
  assert.deepEqual(production.billing, { service: 'nexus-billing', deployed: false })
  assert.ok(Object.isFrozen(STAGING.billing) && Object.isFrozen(production.billing))
})

test('the billing Worker is bound only once its row says it is deployed, and the upload check demands exactly that binding', async () => {
  assert.equal('services' in stagingConfig(template, ids), false, 'no binding while the billing Worker does not exist')
  // The binding names the billing Worker's `BillingService` entrypoint: its RPC methods, never its public webhook route.
  assert.deepEqual(billingServices({ billing: { service: 'nexus-billing', deployed: true } }),
    [{ binding: 'BILLING', service: 'nexus-billing', entrypoint: 'BillingService' }])
  assert.deepEqual(billingServices({ billing: { service: 'nexus-billing', deployed: false } }), [])

  const p = uploadedProvider({ billing: true })
  assert.deepEqual(p.config.services, [{ binding: 'BILLING', service: 'nexus-billing-staging', entrypoint: 'BillingService' }])
  await p.api.recover(p.config, artifact.manifest.files['worker.js'])
  assert.equal(p.writes.length, 1)
  // Bound to the Worker's default export instead (no entrypoint), or another entrypoint: not the artifact.
  for (const fault of ['billing-missing', 'billing-other', 'billing-default-export', 'billing-other-entrypoint']) {
    const bad = uploadedProvider({ billing: true, fault })
    await assert.rejects(bad.api.recover(bad.config, artifact.manifest.files['worker.js']),
      fault === 'billing-missing' ? /binding inventory/ : /billing binding differs/, fault)
    assert.equal(bad.writes.length, 0, fault)
  }
})

// The Worker settings a deploy writes only when the GitHub environment sets them. Before this, the
// configuration held five fixed vars, so the Native client id, the trial settings and the move overlap
// never reached a deployed Worker (staging answered nativeClientId: null). Synthetic values only.
const nativeClientId = 'synthetic-native-client-id'
const movedTo = TARGETS.production.origin
const optional = { native: { nativeClientId }, trialIdentity: { trialIdentityRequired: '1' },
  move: { movedTo, moveStage: 'announce' }, trialKey: { secrets: ['TRIAL_KEY'] }, beta: { betaFree: '1' },
  ownerNotice: { secrets: ['OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM'] } }
optional.all = { ...Object.assign({}, ...Object.values(optional)), secrets: Object.values(optional).flatMap(values => values.secrets ?? []) }
const optionalSecretNames = ['TRIAL_KEY', 'OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM']
test('each optional Worker setting is written only when set, and refused by value', () => {
  const plain = stagingConfig(template, ids)
  assert.deepEqual(Object.keys(plain.vars), ['PUBLIC_REMOTE_ORIGIN', 'AUTH0_ISSUER', 'AUTH0_CLIENT_ID', 'AUTH0_AUDIENCE', 'REMOTE_BUILD_REVISION'])
  assert.deepEqual(plain.secrets, template.secrets, 'nothing set: the configuration is what it was')
  // An unset GitHub variable arrives as an empty string, and means unset.
  assert.deepEqual(stagingConfig(template, { ...ids, nativeClientId: '', trialIdentityRequired: '', movedTo: '', moveStage: '', betaFree: '', secrets: [] }), plain)

  const all = stagingConfig(template, { ...ids, ...optional.all })
  assert.deepEqual(all.vars, { ...plain.vars, AUTH0_NATIVE_CLIENT_ID: nativeClientId, TRIAL_IDENTITY_REQUIRED: '1',
    REMOTE_MOVED_TO: movedTo, REMOTE_MOVE_STAGE: 'announce', REMOTE_BETA_FREE: '1' })
  assert.deepEqual(all.secrets.required, [...template.secrets.required, ...optionalSecretNames])
  assert.deepEqual(stagingConfig(template, { ...ids, ...optional.ownerNotice }).secrets.required,
    [...template.secrets.required, 'OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM'], 'the owner notice pair, without the trial key')
  assert.deepEqual(template.secrets.required, ['ADMIN_SUBJECT', 'TURN_KEY_ID', 'TURN_KEY_TOKEN'], 'the template is not changed')
  for (const moveStage of ['announce', 'pairing-closed', 'closed'])
    assert.equal(stagingConfig(template, { ...ids, movedTo, moveStage }).vars.REMOTE_MOVE_STAGE, moveStage)

  for (const [change, message] of [
    // The Native id is the SPA's: authority.ts refuses that at run time, so it is refused before a deploy.
    [{ nativeClientId: ids.clientId }, /REMOTE_AUTH0_NATIVE_CLIENT_ID must differ from the SPA client ID/],
    ...['unconfigured', 'short', 'line\nbreak', 'has space id', 'a'.repeat(129), 42].map(nativeClientId =>
      [{ nativeClientId }, /REMOTE_AUTH0_NATIVE_CLIENT_ID is not a public Auth0 Native client ID/]),
    ...['0', 'true', 'yes', ' 1', 1].map(trialIdentityRequired => [{ trialIdentityRequired }, /REMOTE_TRIAL_IDENTITY_REQUIRED must be 1 or unset/]),
    [{ movedTo }, /set together/], [{ moveStage: 'announce' }, /set together/],
    ...[STAGING.origin, `${movedTo}/`, 'https://elsewhere.example', 'http://remote.hamradiotools.io'].map(other =>
      [{ movedTo: other, moveStage: 'announce' }, /REMOTE_MOVED_TO must be the origin of another Remote environment/]),
    ...['open', 'Announce', 'closed '].map(moveStage => [{ movedTo, moveStage }, /REMOTE_MOVE_STAGE must be one of announce, pairing-closed, closed/]),
    ...['0', 'true', 'yes', ' 1', 1].map(betaFree => [{ betaFree }, /REMOTE_BETA_FREE must be 1 or unset/]),
    [{ secrets: ['STRIPE_KEY'] }, /Unknown optional Worker secret/], [{ secrets: 'TRIAL_KEY' }, /Unknown optional Worker secret/],
    // The notice endpoint is a test override, never a deployed secret.
    [{ secrets: ['OWNER_NOTICE_ENDPOINT'] }, /Unknown optional Worker secret/],
  ]) assert.throws(() => stagingConfig(template, { ...ids, ...change }), message, JSON.stringify(change))
  // Free during the beta is staging's alone: production refuses it, whatever its environment holds.
  assert.throws(() => optionalVars({ betaFree: '1' }, TARGETS.production), /REMOTE_BETA_FREE is for staging only/)
  assert.deepEqual(optionalVars({ betaFree: '' }, TARGETS.production), {}, 'unset is nothing, on production too')
  assert.deepEqual(optionalVars({ betaFree: '1' }, STAGING), { REMOTE_BETA_FREE: '1' }, 'positive control')
})

test('the environment names reach the configuration, and an optional secret by its presence only', () => {
  const env = { REMOTE_TARGET: 'staging', REMOTE_AUTH0_ISSUER: ids.issuer, REMOTE_AUTH0_CLIENT_ID: ids.clientId,
    REMOTE_AUTH0_NATIVE_CLIENT_ID: nativeClientId, REMOTE_TRIAL_IDENTITY_REQUIRED: '1', REMOTE_MOVED_TO: movedTo,
    REMOTE_MOVE_STAGE: 'closed', REMOTE_BETA_FREE: '1', REMOTE_TRIAL_KEY_PRESENT: 'true' }
  const settings = settingsFromEnv(env)
  assert.deepEqual(settings, { issuer: ids.issuer, clientId: ids.clientId, audience: STAGING.audience, nativeClientId,
    trialIdentityRequired: '1', movedTo, moveStage: 'closed', betaFree: '1', secrets: ['TRIAL_KEY'] })
  for (const present of [{ REMOTE_TRIAL_KEY_PRESENT: 'false' }, { REMOTE_TRIAL_KEY_PRESENT: '' }, { REMOTE_TRIAL_KEY_PRESENT: undefined }])
    assert.deepEqual(settingsFromEnv({ ...env, ...present }).secrets, [], JSON.stringify(present))
  // The owner notice pair is detected the same way, each on its own flag.
  assert.deepEqual(settingsFromEnv({ ...env, REMOTE_OWNER_NOTICE_API_KEY_PRESENT: 'true', REMOTE_OWNER_NOTICE_FROM_PRESENT: 'true' }).secrets,
    optionalSecretNames)
  assert.deepEqual(settingsFromEnv({ ...env, REMOTE_TRIAL_KEY_PRESENT: 'false', REMOTE_OWNER_NOTICE_FROM_PRESENT: 'true' }).secrets, ['OWNER_NOTICE_FROM'])
  // An unknown name's flag is ignored: only the listed optional secrets are ever declared.
  assert.deepEqual(settingsFromEnv({ ...env, REMOTE_TRIAL_KEY_PRESENT: 'false', REMOTE_OWNER_NOTICE_ENDPOINT_PRESENT: 'true' }).secrets, [])
  // The values themselves, when a step holds them, mark presence and never reach the configuration.
  const apiKey = `re_${randomBytes(24).toString('hex')}`, from = 'Nexus Remote <noreply@remote.example.invalid>'
  const valued = settingsFromEnv({ ...env, REMOTE_TRIAL_KEY_PRESENT: 'false', REMOTE_OWNER_NOTICE_API_KEY: apiKey, REMOTE_OWNER_NOTICE_FROM: from })
  assert.deepEqual(valued.secrets, ['OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM'])
  const written = JSON.stringify(stagingConfig(template, { ...ids, ...valued }))
  assert.ok(!written.includes(apiKey) && !written.includes(from) && !written.includes('noreply@'), 'names only, never a value')
  assert.throws(() => settingsFromEnv({ ...env, REMOTE_AUTH0_NATIVE_CLIENT_ID: ids.clientId }), /must differ/)
})

test('the post-upload binding count includes each optional setting exactly when the artifact carries it', async () => {
  for (const [name, values] of Object.entries({ none: {}, ...optional })) {
    // Not firing: the Worker reads back what the artifact declared, with or without the setting.
    const good = uploadedProvider({ values })
    await good.api.recover(good.config, artifact.manifest.files['worker.js'])
    assert.equal(good.writes.length, 1, name)
    if (name === 'none') continue
    // Firing: the artifact carries it and the Worker does not, and the reverse: a setting left over on
    // the Worker that this deploy did not write is an extra binding.
    for (const [declared, held] of [[values, {}], [{}, values]]) {
      const bad = uploadedProvider({ values: declared, live: held })
      await assert.rejects(bad.api.recover(bad.config, artifact.manifest.files['worker.js']), /binding inventory differs/, name)
      assert.equal(bad.writes.length, 0, name)
    }
  }
  // Same count, different value: refused by the value comparison, not the count.
  const moved = uploadedProvider({ values: optional.move, live: { movedTo, moveStage: 'closed' } })
  await assert.rejects(moved.api.recover(moved.config, artifact.manifest.files['worker.js']), /public configuration differs/)
})

test('a supplied optional secret is demanded, applied through the private file, counted, and never printed', async () => {
  const trialKey = randomBytes(32).toString('hex')
  const supplied = { REMOTE_ADMIN_SUBJECT: subject, ...relaySecrets }
  const required = [...template.secrets.required, 'TRIAL_KEY']
  // The preflight demands it once the artifact declares it.
  const missing = provider({ database: ids.databaseId, extraEnv: supplied })
  await assert.rejects(missing.api.preflight(required, ids.databaseId), /^Error: REMOTE_TRIAL_KEY not set in this GitHub environment/)
  assert.equal(missing.calls.length, 0)
  const present = uploadedProvider({ tagged: true, values: optional.trialKey, secrets: { ...supplied, REMOTE_TRIAL_KEY: trialKey } })
  const result = await present.api.preflight(required, ids.databaseId)
  assert.equal(result.liveSecretTypes.TRIAL_KEY, 'secret_text')
  assert.ok(!JSON.stringify(result).includes(trialKey))

  // A key the Worker already holds that this deploy would not apply survives the upload, and would then
  // fail the count with the Worker live. Refused before any write, by name.
  const stranded = uploadedProvider({ tagged: true, live: optional.trialKey, secrets: supplied })
  await assert.rejects(stranded.api.preflight(template.secrets.required, ids.databaseId), error => {
    assert.equal(error.message, 'The Worker holds TRIAL_KEY but REMOTE_TRIAL_KEY is not set in this GitHub environment; '
      + 'set it to the same value or remove it from the Worker, so nothing was migrated or uploaded')
    return true
  })
  assert.equal(stranded.writes.length, 0)
  // Positive control: the same Worker, with the artifact applying the key, passes.
  const applied = uploadedProvider({ tagged: true, live: optional.trialKey, values: optional.trialKey, secrets: { ...supplied, REMOTE_TRIAL_KEY_PRESENT: 'true' } })
  await applied.api.preflight(required, ids.databaseId)

  // The upload's private file carries it beside the required ones.
  const env = { ...supplied, REMOTE_TRIAL_KEY: trialKey }
  await withSecretsFile(env, required, async file => {
    assert.deepEqual(Object.keys(JSON.parse(await readFile(file, 'utf8'))), required)
    assert.equal(JSON.parse(await readFile(file, 'utf8')).TRIAL_KEY, trialKey)
  })
  // The owner notice pair rides the same file; a From address with a display name is a usable value.
  const notice = { REMOTE_OWNER_NOTICE_API_KEY: `re_${randomBytes(24).toString('hex')}`, REMOTE_OWNER_NOTICE_FROM: 'Nexus Remote <noreply@remote.example.invalid>' }
  const withNotice = [...required, 'OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM']
  await withSecretsFile({ ...env, ...notice }, withNotice, async file => {
    const written = JSON.parse(await readFile(file, 'utf8'))
    assert.deepEqual(Object.keys(written), withNotice)
    assert.equal(written.OWNER_NOTICE_API_KEY, notice.REMOTE_OWNER_NOTICE_API_KEY)
    assert.equal(written.OWNER_NOTICE_FROM, notice.REMOTE_OWNER_NOTICE_FROM)
  })
  await assert.rejects(withSecretsFile({ ...env, REMOTE_OWNER_NOTICE_API_KEY: notice.REMOTE_OWNER_NOTICE_API_KEY }, withNotice, async () => assert.fail('no upload')),
    error => error.message === 'REMOTE_OWNER_NOTICE_FROM is not set in this GitHub environment as a usable secret')
  await assert.rejects(withSecretsFile({ ...supplied, REMOTE_TRIAL_KEY: 'short' }, required, async () => assert.fail('no upload')),
    error => error.message === 'REMOTE_TRIAL_KEY is not set in this GitHub environment as a usable secret')
  // Absent from the report and the error of a failed check.
  const logs = [], live = () => verifyLive(artifact, served)
  const wrong = uploadedProvider({ live: optional.trialKey, secrets: env })
  await assert.rejects(settleUpload(wrong.api, wrong.config, artifact.manifest, live, line => logs.push(line)), error => {
    assert.match(error.message, /binding inventory differs/)
    return !error.message.includes(trialKey)
  })
  assert.ok(logs.every(line => !line.includes(trialKey)))
})

test('an artifact carrying optional settings verifies, passes the real Wrangler dry run, and the live check reads the Native id back', async () => {
  const other = await mkdtemp(join(tmpdir(), 'nexus-staging-test-'))
  try {
    await cp(scratch, other, { recursive: true, filter: source => !source.includes('staging-artifact') })
    await createArtifact(other, { ...ids, ...optional.all })
    const carried = await verifyArtifact(other, ids.revision)
    assert.equal(carried.config.vars.AUTH0_NATIVE_CLIENT_ID, nativeClientId)
    assert.deepEqual(carried.config.secrets.required, [...template.secrets.required, ...optionalSecretNames])
    // The pinned Wrangler accepts the declared optional secret through the same synthetic file.
    await uploadArtifact(other, 'dry-run', { PATH: process.env.PATH, GITHUB_SHA: ids.revision })
    // The live Worker in this test has no Native id, so an artifact that declares one does not match it.
    await assert.rejects(verifyLive(carried, served), /does not match the selected source and identity/)
  } finally { await rm(other, { recursive: true, force: true }) }
  // Positive control: the plain artifact matches the same Worker, whose config answers null.
  assert.equal((await (await served(`${STAGING.origin}/api/remote/config`)).json()).nativeClientId, null)
  await verifyLive(artifact, served)
})

test('both workflows hand the optional settings to the steps that build, check and upload a deploy, and the key value to the upload alone', async () => {
  for (const file of ['remote-staging.yml', 'remote-production.yml']) {
    const text = await readFile(join(root, '.github/workflows', file), 'utf8')
    const step = name => { const at = text.indexOf(`- name: ${name}`); assert.ok(at > 0, `${file}: ${name}`); return text.slice(at, text.indexOf('\n\n', at)) }
    const vars = ['REMOTE_AUTH0_NATIVE_CLIENT_ID', 'REMOTE_TRIAL_IDENTITY_REQUIRED', 'REMOTE_MOVED_TO', 'REMOTE_MOVE_STAGE']
      .map(name => `${name}: \${{ vars.${name} }}`)
    for (const secret of optionalSecretNames.map(name => `REMOTE_${name}`)) {
      const present = `${secret}_PRESENT: \${{ secrets.${secret} != '' }}`
      for (const name of ['Package the tested bytes', 'Recover ownership']) {
        for (const line of [...vars, present]) assert.ok(step(name).includes(line), `${file}: ${name} lacks ${line}`)
      }
      for (const name of ['Check deploy preconditions', 'Apply ']) assert.ok(step(name).includes(present), `${file}: ${name} lacks ${present}`)
      assert.ok(step('Upload the verified Worker').includes(`${secret}: \${{ secrets.${secret} }}`), `${file}: ${secret}`)
      assert.equal(text.split(`secrets.${secret} }}`).length - 1, 1, `${file}: ${secret}'s value reaches the upload step only`)
    }
    assert.ok(step('Check public identity configuration').includes(vars[0]), `${file}: the identity check sees the Native id`)
    // Free during the beta reaches staging's two configuration steps, and production's workflow never names it.
    const beta = 'REMOTE_BETA_FREE: ${{ vars.REMOTE_BETA_FREE }}'
    if (file === 'remote-staging.yml') for (const name of ['Package the tested bytes', 'Recover ownership']) assert.ok(step(name).includes(beta), `${file}: ${name}`)
    else assert.ok(!text.includes('REMOTE_BETA_FREE'), file)
  }
})

// What GitHub returns for remote-production once the operator has built it as the runbook says.
const protectedProduction = () => ({
  environment: { name: 'remote-production', can_admins_bypass: false,
    protection_rules: [{ type: 'required_reviewers', prevent_self_review: false, reviewers: [{ type: 'User', reviewer: { login: 'kd9taw' } }] },
      { type: 'branch_policy' }],
    deployment_branch_policy: { protected_branches: false, custom_branch_policies: true } },
  policies: { total_count: 2, branch_policies: [{ name: 'main', type: 'branch' }, { name: 'v*', type: 'tag' }] },
})
test('the production workflow guard refuses the placeholder row before any request, and an unprotected environment', async () => {
  let sent = 0
  const fetcher = async () => { sent++; return Response.json({}) }
  const env = { REMOTE_TARGET: 'production', GITHUB_REPOSITORY: 'kd9taw/Nexus', GITHUB_TOKEN: 'synthetic-token' }
  await assert.rejects(productionGuard(env, fetcher), /placeholder/)
  await assert.rejects(productionGuard({ ...env, REMOTE_TARGET: 'staging' }, fetcher), /must target the production row/)
  await assert.rejects(productionGuard({ ...env, REMOTE_TARGET: undefined }, fetcher), /must target the production row/)
  assert.equal(sent, 0)

  const good = protectedProduction()
  assert.deepEqual(protectedEnvironment(good.environment, good.policies),
    { environment: 'remote-production', reviewer: 'kd9taw', adminBypass: false, deployFrom: ['branch:main', 'tag:v*'] })
  const faults = {
    'another environment': d => { d.environment.name = 'production' },
    'admins bypass': d => { d.environment.can_admins_bypass = true },
    'bypass unreported': d => { delete d.environment.can_admins_bypass },
    'no reviewer': d => { d.environment.protection_rules = [{ type: 'branch_policy' }] },
    'a team reviewer only': d => { d.environment.protection_rules[0].reviewers = [{ type: 'Team', reviewer: { slug: 'kd9taw' } }] },
    'another reviewer': d => { d.environment.protection_rules[0].reviewers[0].reviewer.login = 'someone' },
    'any branch': d => { d.environment.deployment_branch_policy = null },
    'protected branches': d => { d.environment.deployment_branch_policy = { protected_branches: true, custom_branch_policies: false } },
    'an extra branch': d => { d.policies.branch_policies.push({ name: 'dev', type: 'branch' }) },
    'main missing': d => { d.policies.branch_policies.shift() },
    'tag rule as a branch': d => { d.policies.branch_policies[1].type = 'branch' },
  }
  for (const [name, mutate] of Object.entries(faults)) {
    const doc = protectedProduction(); mutate(doc)
    assert.throws(() => protectedEnvironment(doc.environment, doc.policies), /remote-production/, name)
  }
})

test('the production workflow is the staging one aimed at production, behind the guard, never the shared environment', async () => {
  const staging = await readFile(join(root, '.github/workflows/remote-staging.yml'), 'utf8')
  const production = await readFile(join(root, '.github/workflows/remote-production.yml'), 'utf8')
  const job = production.slice(production.indexOf('\n  production:\n'))
  assert.ok(job.length < production.length && production.includes('\n  guard:\n'))
  assert.match(job, /\n    needs: guard\n/)
  assert.match(job, /\n    environment: remote-production\n/)
  assert.doesNotMatch(production, /environment: production\b/)
  assert.equal(production.match(/environment:/g).length, 1, 'only the guarded job names an environment')
  assert.match(production, /\n  group: nexus-remote-production\n/)
  assert.equal(production.match(/REMOTE_TARGET: production/g).length, 2)
  assert.doesNotMatch(production, /REMOTE_TARGET: staging|grant[-_]/)
  // The guard holds no secret; it reads the environment's protection with the run's own token.
  const guard = production.slice(production.indexOf('\n  guard:\n'), production.indexOf('\n  production:\n'))
  assert.doesNotMatch(guard, /secrets\.|environment:/)
  assert.match(guard, /run: node remote\/scripts\/production-guard\.mjs/)
  const options = text => text.match(/options:\n((?:          - .+\n)+)/)[1].trim().split(/\n\s*- /).map(o => o.replace(/^- /, ''))
  assert.deepEqual(options(production), ['inspect', 'provision', 'deploy', 'recover'])
  assert.match(production, /inspect\|provision\|deploy\|recover\) ;;/)
  // The same scripts and gates as staging, operation for operation.
  const runs = text => [...text.matchAll(/node remote\/scripts\/[\w.-]+\.mjs [\w-]+|scripts\/gates --job .*/g)].map(m => m[0]).sort()
  assert.deepEqual(runs(production), runs(staging).filter(r => !r.includes('grant-trial')))
})

test('every Cloudflare operation a Remote workflow runs is one the script implements', async () => {
  const modes = text => [...text.matchAll(/node remote\/scripts\/cloudflare-staging\.mjs ([\w-]+)/g)].map(m => m[1])
  for (const file of ['remote-staging.yml', 'remote-production.yml']) {
    const used = modes(await readFile(join(root, '.github/workflows', file), 'utf8'))
    assert.ok(used.length >= 5, `${file}: positive control, its operations are found`)
    assert.deepEqual(used.filter(mode => !MODES.includes(mode)), [], `${file} runs an operation the script refuses`)
  }
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
  for (const fault of ['identity', 'revision', 'database', 'namespace', 'extra-binding', 'secret-as-var', 'secret-missing', 'observability', 'logs', 'logpush', 'tail', 'code', 'extra-module', 'domain']) {
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

test('source verification waits out an anonymous rate limit and still demands the public commit', async () => {
  const waits = [], wait = async ms => { waits.push(ms) }
  // A private repository and a missing commit both answer 404 to an anonymous caller: never retried.
  let calls = 0
  await assert.rejects(verifyPublicSource(ids.revision, async () => { calls++; return new Response('', { status: 404 }) }, wait), /HTTP 404/)
  assert.equal(calls, 1)
  assert.deepEqual(waits, [])
  // A 403 with the limit spent waits until the reset GitHub names, then succeeds anonymously.
  const reset = String(Math.floor(Date.now() / 1000) + 30)
  const answers = [new Response('', { status: 403, headers: { 'x-ratelimit-remaining': '0', 'x-ratelimit-reset': reset } }),
    new Response('', { status: 429, headers: { 'retry-after': '7' } }), Response.json({ sha: ids.revision })]
  const urls = []
  const limited = async (url, options) => {
    assert.equal(options.headers.authorization, undefined)
    urls.push(String(url))
    return answers.shift()
  }
  assert.equal(await verifyPublicSource(ids.revision, limited, wait), `https://github.com/kd9taw/Nexus/tree/${ids.revision}`)
  assert.equal(urls.length, 3)
  assert.ok(urls.every(url => url === `https://api.github.com/repos/kd9taw/Nexus/git/commits/${ids.revision}`))
  assert.equal(waits.length, 2)
  assert.ok(waits[0] > 25000 && waits[0] <= 32000, `waited ${waits[0]} ms for the reset`)
  assert.equal(waits[1], 7000)
  // A retried answer is checked like the first: the wrong commit still fails.
  waits.length = 0
  const wrong = [new Response('', { status: 403 }), Response.json({ sha: 'wrong' })]
  await assert.rejects(verifyPublicSource(ids.revision, async () => wrong.shift(), wait), /exact source/)
  assert.deepEqual(waits, [2000])
  // The limit never lifting fails the run after five tries, each wait capped at five minutes.
  waits.length = 0
  calls = 0
  const far = String(Math.floor(Date.now() / 1000) + 3600)
  await assert.rejects(verifyPublicSource(ids.revision, async () => { calls++
    return new Response('', { status: 403, headers: { 'x-ratelimit-remaining': '0', 'x-ratelimit-reset': far } }) }, wait), /HTTP 403/)
  assert.equal(calls, 5)
  assert.deepEqual(waits, [300000, 300000, 300000, 300000])
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

test('the hosted Nexus artifact carries its bundled dependency notices', async () => {
  const directory = join(scratch, 'remote/staging-artifact/assets')
  const notices = await readFile(join(directory, 'remote-licenses.txt'), 'utf8')
  for (const name of ['react 18.3.1', '@radix-ui/react-dialog', 'three ', 'react-globe.gl', 'Copyright (c) 2024 HB9HIL']) {
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

test('a wrong Worker revision, stale asset, missing security header, page over plain HTTP, open admission or an open relay route fails the same live check', async () => {
  for (const fault of ['revision', 'asset', 'headers', 'hsts', 'plain', 'admission', 'relay-admission']) {
    const corrupted = async (input, options) => {
      const { pathname: path, protocol } = new URL(input)
      if (fault === 'admission' && path.endsWith('/session')) return Response.json({})
      if (fault === 'plain' && protocol === 'http:') return new Response('<!doctype html>', { status: 200 })
      // The stream's relay route hands out a credential, so it is checked for the same refusals.
      if (fault === 'relay-admission' && path.endsWith('/turn')) return Response.json({})
      const actual = await served(input, options)
      if (fault === 'hsts' && path === '/' && protocol === 'https:') {
        const headers = new Headers(actual.headers); headers.delete('strict-transport-security')
        return new Response(actual.body, { status: actual.status, headers })
      }
      if (fault === 'revision' && path.endsWith('/config')) return Response.json({ ...await actual.json(), revision: 'stale' })
      if (fault === 'asset' && path.endsWith('.js')) return new Response('stale bytes', { status: 200 })
      if (fault === 'headers' && path === '/') {
        const headers = new Headers(actual.headers); headers.delete('content-security-policy')
        return new Response(actual.body, { status: actual.status, headers })
      }
      return actual
    }
    await assert.rejects(verifyLive(artifact, corrupted), { hsts: /missing its security headers/, plain: /answers plain HTTP/ }[fault])
  }
})

test('incomplete or oversized provider responses fail without reflecting their contents', async () => {
  const privateText = randomBytes(20).toString('hex')
  await assert.rejects(requestBytes('https://example.invalid', {}, async () => { throw new Error(privateText) }), error => !error.message.includes(privateText))
  await assert.rejects(requestBytes('https://example.invalid', {}, async () => new Response(Buffer.alloc(8 * 1024 * 1024 + 1))), /complete response/)
})
