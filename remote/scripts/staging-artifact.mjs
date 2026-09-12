// Package the already-tested Worker and browser without rebuilding at upload.
// The receipt identifies the public source and hashes every deployable file.
import { readFile, writeFile, readdir, mkdir, copyFile, lstat } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { setTimeout as delay } from 'node:timers/promises'
import { STAGING, revision, stagingConfig, identityFromEnv, verifyIdentity, requireValue, requestJson, requestBytes } from './staging-common.mjs'

const repository = fileURLToPath(new URL('../../', import.meta.url))
const hash = bytes => createHash('sha256').update(bytes).digest('hex')
const sourceUrl = sha => `https://github.com/kd9taw/Nexus/tree/${sha}`
const artifactDirectory = root => join(root, 'remote/staging-artifact')
async function files(directory, prefix = '') {
  const result = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    requireValue(/^[A-Za-z0-9_.-]+$/.test(entry.name) && !entry.name.startsWith('.'), 'Unexpected artifact filename')
    const name = `${prefix}${entry.name}`
    if (entry.isDirectory()) result.push(...await files(join(directory, entry.name), `${name}/`))
    else { requireValue(entry.isFile(), 'Artifact symlinks and special files are forbidden'); result.push(name) }
  }
  return result.sort()
}

function bundledConfig(template, values) {
  const config = stagingConfig(template, values)
  delete config.$schema
  config.main = './worker.js'
  config.no_bundle = true
  config.assets.directory = './assets'
  config.d1_databases[0].migrations_dir = './migrations'
  return config
}

export async function createArtifact(root, values) {
  const sha = revision(values.revision), destination = artifactDirectory(root)
  const template = JSON.parse(await readFile(join(root, 'remote/wrangler.jsonc'), 'utf8'))
  const config = bundledConfig(template, values)
  // Refuse overwrite: otherwise an old, unlisted asset can ride a new deploy.
  await mkdir(destination)
  await mkdir(join(destination, 'assets'))
  await mkdir(join(destination, 'migrations'))
  await copyFile(join(root, 'remote/dist/index.js'), join(destination, 'worker.js'))
  const browser = join(root, 'ui/dist-remote'), assets = await files(browser)
  requireValue(assets.includes('index.html') && assets.includes('remote-licenses.txt')
    && assets.some(name => /^assets\/.+\.js$/.test(name)), 'The compiled browser and license asset must exist')
  for (const name of assets) {
    const nexusMapAsset = /^assets\/(earth-night|earth-relief)-[A-Za-z0-9_-]+\.webp$/.test(name)
      || /^assets\/cqzones-[A-Za-z0-9_-]+\.geojson$/.test(name)
    requireValue(name === 'index.html' || name === 'remote-licenses.txt' || /^assets\/[A-Za-z0-9_.-]+\.(js|css)$/.test(name) || nexusMapAsset,
      'Unexpected browser asset; review it before adding it to the upload')
    const target = join(destination, 'assets', name)
    await mkdir(resolve(target, '..'), { recursive: true })
    await copyFile(join(browser, name), target)
  }
  const migrations = await files(join(root, 'remote/migrations'))
  requireValue(migrations.length > 0 && migrations.every(name => /^\d{4}_[a-z0-9_]+\.sql$/.test(name)), 'Unexpected D1 migration inventory')
  for (const name of migrations) await copyFile(join(root, 'remote/migrations', name), join(destination, 'migrations', name))
  await writeFile(join(destination, 'wrangler.jsonc'), JSON.stringify(config, null, 2) + '\n', { flag: 'wx', mode: 0o600 })
  const entries = {}
  for (const name of await files(destination)) entries[name] = hash(await readFile(join(destination, name)))
  const manifest = { format: 1, revision: sha, source: sourceUrl(sha), origin: STAGING.origin, files: entries }
  await writeFile(join(destination, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx', mode: 0o600 })
  await verifyArtifact(root, sha)
  return manifest
}

export async function verifyArtifact(root, expectedRevision) {
  const directory = artifactDirectory(root)
  requireValue((await lstat(directory)).isDirectory(), 'The artifact must be a real directory')
  const manifest = JSON.parse(await readFile(join(directory, 'manifest.json'), 'utf8'))
  requireValue(manifest.format === 1 && manifest.revision === revision(expectedRevision)
    && manifest.source === sourceUrl(expectedRevision) && manifest.origin === STAGING.origin
    && manifest.files && typeof manifest.files === 'object' && !Array.isArray(manifest.files), 'Artifact receipt does not match the selected source')
  const actual = (await files(directory)).filter(name => name !== 'manifest.json')
  requireValue(JSON.stringify(actual) === JSON.stringify(Object.keys(manifest.files).sort()), 'Artifact file inventory changed')
  for (const name of actual) requireValue(hash(await readFile(join(directory, name))) === manifest.files[name], 'Artifact bytes changed after packaging')
  const config = JSON.parse(await readFile(join(directory, 'wrangler.jsonc'), 'utf8'))
  const template = JSON.parse(await readFile(join(root, 'remote/wrangler.jsonc'), 'utf8'))
  const expected = bundledConfig(template, { issuer: config.vars?.AUTH0_ISSUER, clientId: config.vars?.AUTH0_CLIENT_ID,
    audience: config.vars?.AUTH0_AUDIENCE, databaseId: config.d1_databases?.[0]?.database_id, revision: expectedRevision })
  requireValue(JSON.stringify(config) === JSON.stringify(expected), 'Artifact configuration exceeds the staging scope')
  requireValue(actual.includes('worker.js') && actual.includes('assets/index.html') && actual.includes('assets/remote-licenses.txt'), 'Artifact is incomplete')
  return { manifest, config }
}

export async function verifyPublicSource(sha, fetcher = fetch) {
  sha = revision(sha)
  const commit = await requestJson(`https://api.github.com/repos/kd9taw/Nexus/git/commits/${sha}`,
    { headers: { accept: 'application/vnd.github+json', 'user-agent': 'nexus-remote-staging' } }, fetcher, 'Public corresponding source')
  requireValue(commit.sha === sha, 'The exact source commit is not publicly available')
  return sourceUrl(sha)
}

export async function verifyLive({ manifest, config }, fetcher = fetch) {
  const origin = STAGING.origin, options = { headers: { 'cache-control': 'no-cache' } }
  const active = await requestJson(`${origin}/api/remote/config`, options, fetcher, 'Deployed Worker identity')
  requireValue(active.ready === true && active.revision === manifest.revision
    && active.issuer === config.vars.AUTH0_ISSUER && active.clientId === config.vars.AUTH0_CLIENT_ID
    && active.audience === config.vars.AUTH0_AUDIENCE, 'The live Worker does not match the selected source and identity configuration')
  let count = 0
  for (const [name, expected] of Object.entries(manifest.files).filter(([name]) => name.startsWith('assets/'))) {
    const path = name === 'assets/index.html' ? '/' : `/${name.slice('assets/'.length)}`
    const result = await requestBytes(`${origin}${path}`, options, fetcher, 'Deployed browser asset')
    requireValue(result.status === 200 && hash(result.bytes) === expected, 'The live browser asset does not match the tested artifact')
    if (path === '/') {
      const csp = result.headers.get('content-security-policy') ?? ''
      requireValue(csp.includes("frame-ancestors 'none'") && csp.includes(new URL(config.vars.AUTH0_ISSUER).origin)
        && result.headers.get('cache-control') === 'no-store'
        && result.headers.get('referrer-policy') === 'no-referrer', 'The live document is missing its security headers')
    }
    count++
  }
  requireValue(count >= 3, 'No complete browser artifact was checked')
  for (const [originHeader, status, error] of [[origin, 401, 'signInRequired'], ['https://other.invalid', 403, 'originDenied']]) {
    const response = await requestBytes(`${origin}/api/remote/session`, {
      method: 'POST', headers: { origin: originHeader, 'content-type': 'application/json' }, body: '{}',
    }, fetcher, 'Unauthenticated admission check')
    let value
    try { value = JSON.parse(response.bytes.toString('utf8')) } catch { throw new Error('Admission check returned invalid JSON') }
    requireValue(response.status === status && value.error === error, 'The live service did not enforce authentication and Origin')
  }
  return { revision: manifest.revision, source: manifest.source, assetsChecked: count, admission: 'anonymous and foreign Origin refused' }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const mode = process.argv[2]
    requireValue(process.argv.length === 3 && ['identity', 'source', 'prepare', 'check', 'verify'].includes(mode), 'Use identity, source, prepare, check or verify')
    if (mode === 'identity') console.log(JSON.stringify(await verifyIdentity(identityFromEnv())))
    else if (mode === 'source') console.log(await verifyPublicSource(process.env.GITHUB_SHA))
    else if (mode === 'prepare') {
      const result = await createArtifact(repository, { ...identityFromEnv(), databaseId: process.env.REMOTE_D1_DATABASE_ID, revision: process.env.GITHUB_SHA })
      console.log(`Packaged ${Object.keys(result.files).length} files from ${result.revision}`)
    } else {
      const artifact = await verifyArtifact(repository, process.env.GITHUB_SHA)
      if (mode === 'check') console.log('Artifact scope, source and hashes passed')
      else for (let attempt = 1; attempt <= 6; attempt++) {
        try { console.log(JSON.stringify(await verifyLive(artifact))); break }
        catch (error) {
          if (attempt === 6) throw error
          console.log(`The complete staging artifact is not verified yet (attempt ${attempt}/6).`)
          await delay(5000)
        }
      }
    }
  } catch (error) { console.error(error.code === 'EEXIST' ? 'The staging artifact already exists; use a fresh build directory.' : error.message); process.exitCode = 1 }
}
