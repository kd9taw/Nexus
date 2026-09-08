// Provider calls below are intercepted. The deployment smoke check also runs
// against the actual compiled Worker and Static Assets in local workerd.
import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { randomBytes, randomUUID, createHash } from 'node:crypto'
import { mkdtemp, mkdir, readFile, writeFile, copyFile, cp, rm, stat, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { STAGING, stagingConfig, verifyIdentity, requestBytes } from '../scripts/staging-common.mjs'
import { cloudflare } from '../scripts/cloudflare-staging.mjs'
import { createArtifact, verifyArtifact, verifyLive, verifyPublicSource } from '../scripts/staging-artifact.mjs'
import { runtime } from './runtime.mjs'
import { uploadArtifact } from '../scripts/deploy-staging.mjs'

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

function provider({ database = null, domains = [], workers = [], denied = false, ambiguous = false, loseWrite = false } = {}) {
  const env = { CLOUDFLARE_ACCOUNT_ID: randomBytes(16).toString('hex'), CLOUDFLARE_API_TOKEN: randomBytes(32).toString('hex') }
  const calls = [], privateText = randomBytes(20).toString('hex')
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
  return { api: cloudflare(env, fetcher), calls, privateText }
}

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

test('the actual pinned Wrangler accepts the packaged upload without credentials or changing its bytes', async () => {
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
