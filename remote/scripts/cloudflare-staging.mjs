// Fixed-resource Cloudflare administration. Never prints provider response bodies,
// account identifiers, token values, other services or user/organization details.
import { appendFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'
import { STAGING, databaseId, revision, identityFromEnv, stagingConfig, requireValue, requestJson, requestBytes } from './staging-common.mjs'

export async function workerDigest(response) {
  requireValue(response.status === 200, `Worker content read failed (HTTP ${response.status})`)
  let bytes = response.bytes
  const type = response.headers.get('content-type') ?? ''
  if (type.startsWith('multipart/form-data;')) {
    let parts
    try { parts = [...(await new Response(bytes, { headers: response.headers }).formData()).entries()] }
    catch { throw new Error('Worker content has invalid multipart framing') }
    const inventory = { moduleCount: parts.length, entrypointPresent: response.headers.has('cf-entrypoint'),
      entrypointMatches: parts.length === 1 && response.headers.get('cf-entrypoint') === parts[0][0] }
    requireValue(parts.length === 1 && (!inventory.entrypointPresent || inventory.entrypointMatches),
      `Worker module inventory does not match the bundled artifact: ${JSON.stringify(inventory)}`)
    // Cloudflare can return a text form part without a filename, as also handled
    // by Wrangler's Worker downloader. The entrypoint name can be relative to
    // Wrangler's disposable config; either representation must hash exactly.
    bytes = typeof parts[0][1] === 'string' ? Buffer.from(parts[0][1], 'utf8') : Buffer.from(await parts[0][1].arrayBuffer())
  } else requireValue(/^(application|text)\/javascript(?:;|$)/.test(type), 'Worker content has an unexpected media type')
  return createHash('sha256').update(bytes).digest('hex')
}

export function cloudflare(env = process.env, fetcher = fetch) {
  requireValue(/^[0-9a-f]{32}$/.test(env.CLOUDFLARE_ACCOUNT_ID ?? ''), 'CLOUDFLARE_ACCOUNT_ID is missing or invalid')
  requireValue(typeof env.CLOUDFLARE_API_TOKEN === 'string' && env.CLOUDFLARE_API_TOKEN.length > 0
    && !/\s/.test(env.CLOUDFLARE_API_TOKEN), 'CLOUDFLARE_API_TOKEN is missing or invalid')
  const base = `https://api.cloudflare.com/client/v4/accounts/${env.CLOUDFLARE_ACCOUNT_ID}`
  async function api(path, label, value, method = value === undefined ? 'GET' : 'POST') {
    const response = await requestJson(`${base}${path}`, {
      method,
      headers: { authorization: `Bearer ${env.CLOUDFLARE_API_TOKEN}`, 'content-type': 'application/json' },
      ...(value === undefined ? {} : { body: JSON.stringify(value) }),
    }, fetcher, label)
    requireValue(response.success === true, `${label} was refused by Cloudflare`)
    return response
  }
  async function databases() {
    const matches = []
    for (let page = 1; page <= 100; page++) {
      const response = await api(`/d1/database?name=${STAGING.name}&per_page=100&page=${page}`, 'D1 inventory')
      requireValue(Array.isArray(response.result), 'D1 inventory returned an unexpected shape')
      matches.push(...response.result.filter(row => row.name === STAGING.name))
      if (response.result.length < 100) {
        requireValue(matches.length <= 1, 'Ambiguous staging database; inspect the account before continuing')
        return matches[0] ? databaseId(matches[0].uuid) : null
      }
    }
    throw new Error('D1 inventory exceeded the page limit; no database was selected')
  }
  async function inventory() {
    const id = await databases()
    const workers = await api('/workers/scripts', 'Worker inventory')
    requireValue(Array.isArray(workers.result) && workers.result.every(row => typeof row.id === 'string'),
      'Worker inventory returned an unexpected shape')
    const named = workers.result.filter(row => row.id === STAGING.name)
    requireValue(named.length <= 1, 'Ambiguous staging Worker; review it before replacement')
    const domains = await api(`/workers/domains?hostname=${new URL(STAGING.origin).hostname}`, 'Worker domain inventory')
    requireValue(Array.isArray(domains.result), 'Worker domain inventory returned an unexpected shape')
    const matches = domains.result.filter(row => row.hostname === new URL(STAGING.origin).hostname)
    requireValue(matches.length <= 1 && matches.every(row => row.service === STAGING.name),
      'The staging hostname is assigned to another service; no takeover is allowed')
    return { worker: named[0], result: { service: STAGING.name, databaseId: id, workerExists: named.length === 1, domainAttached: matches.length === 1,
      readAccess: 'D1 and Workers', writeAccess: 'not established by read-only inspection' } }
  }
  async function inspect() {
    const { worker, result } = await inventory()
    requireValue(!worker || worker.tags?.includes(STAGING.tag),
      'The staging Worker name belongs to an unrecognized deployment; review it before replacement')
    return result
  }
  async function confirmUpload(config, expectedHash) {
    requireValue(config.name === STAGING.name && /^[0-9a-f]{64}$/.test(expectedHash), 'A staging artifact and Worker SHA-256 are required')
    revision(config.vars?.REMOTE_BUILD_REVISION)
    const { worker, result } = await inventory()
    requireValue(worker && result.databaseId === config.d1_databases[0].database_id, 'The uploaded Worker and artifact database must exist in the staging account')
    const settings = (await api(`/workers/scripts/${STAGING.name}/settings`, 'Worker configuration read')).result
    requireValue(Array.isArray(settings?.bindings) && settings.bindings.length === Object.keys(config.vars).length + 3,
      'Uploaded Worker binding inventory differs from the artifact')
    const bindings = new Map(settings.bindings.map(binding => [binding.name, binding]))
    requireValue(bindings.size === settings.bindings.length && Object.entries(config.vars).every(([name, text]) =>
      bindings.get(name)?.type === 'plain_text' && bindings.get(name).text === text), 'Uploaded Worker public configuration differs from the artifact')
    requireValue(bindings.get('DB')?.type === 'd1' && bindings.get('DB').id === result.databaseId
      && bindings.get('ASSETS')?.type === 'assets' && bindings.get('STATIONS')?.type === 'durable_object_namespace'
      && bindings.get('STATIONS').class_name === 'StationRoom'
      && (!bindings.get('STATIONS').script_name || bindings.get('STATIONS').script_name === STAGING.name),
    'Uploaded Worker service bindings differ from the artifact')
    const runtime = { dateMatches: settings.compatibility_date === config.compatibility_date,
      dateTimestampMatches: settings.compatibility_date === `${config.compatibility_date}T00:00:00Z`,
      observabilityPresent: Object.hasOwn(settings, 'observability'), observabilityNull: settings.observability === null,
      observabilityDisabled: settings.observability?.enabled === false, observabilityEnabled: settings.observability?.enabled === true }
    const content = await requestBytes(`${base}/workers/scripts/${STAGING.name}/content/v2`, {
      headers: { authorization: `Bearer ${env.CLOUDFLARE_API_TOKEN}` },
    }, fetcher, 'Worker content read')
    let digest
    try { digest = await workerDigest(content) }
    catch (error) { throw new Error(`${error.message}; runtime comparison: ${JSON.stringify(runtime)}`) }
    requireValue(digest === expectedHash, 'Uploaded Worker bytes differ from the artifact')
    requireValue(runtime.dateMatches && runtime.observabilityDisabled,
      `Uploaded Worker runtime configuration differs from the artifact: ${JSON.stringify(runtime)}`)
    return { worker, result }
  }
  async function markUpload(config, expectedHash) {
    const { worker } = await confirmUpload(config, expectedHash)
    requireValue(worker.tags === undefined || Array.isArray(worker.tags) && worker.tags.every(tag => typeof tag === 'string'),
      'Worker tags have an unexpected shape')
    if (!worker.tags?.includes(STAGING.tag)) await api(`/workers/scripts/${STAGING.name}/script-settings`,
      'Staging Worker ownership tag', { tags: [...(worker.tags ?? []), STAGING.tag] }, 'PATCH')
    return inspect()
  }
  async function attachDomain() {
    const current = await inspect()
    requireValue(current.workerExists, 'The staging Worker must exist before domain attachment')
    if (!current.domainAttached) await api('/workers/domains', 'Staging custom domain attachment', {
      hostname: new URL(STAGING.origin).hostname, service: STAGING.name, zone_name: STAGING.zone,
    }, 'PUT')
    const confirmed = await inspect()
    requireValue(confirmed.domainAttached, 'Cloudflare did not confirm the staging custom domain')
    return confirmed
  }
  async function recover(config, expectedHash) {
    // Explicit administrator operation: exact receipt bytes and every public
    // binding must agree before a previously untagged Worker can be recognized.
    await markUpload(config, expectedHash)
    return { service: STAGING.name, ownership: 'verified artifact and tag', revision: config.vars.REMOTE_BUILD_REVISION }
  }
  async function provision() {
    const current = await inspect()
    if (current.databaseId) return { ...current, created: false }
    // No automatic retry of an ambiguous write. A rerun inventories first and
    // reuses an exact-name database if the earlier request actually succeeded.
    const created = await api('/d1/database', 'Staging D1 creation', {
      name: STAGING.name, primary_location_hint: 'enam', read_replication: { mode: 'disabled' },
    })
    requireValue(created.result?.name === STAGING.name, 'Cloudflare did not confirm the requested database name')
    const id = databaseId(created.result.uuid)
    requireValue(await databases() === id, 'The created staging database was not confirmed by inventory')
    return { ...current, databaseId: id, created: true }
  }
  return { inspect, provision, confirmUpload, markUpload, attachDomain, recover }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const mode = process.argv[2]
    requireValue(process.argv.length === 3 && ['inspect', 'provision', 'resolve', 'recover'].includes(mode), 'Use inspect, provision, resolve or recover')
    const api = cloudflare()
    let result
    if (mode === 'recover') {
      const { readFile } = await import('node:fs/promises')
      const { verifyPublicSource } = await import('./staging-artifact.mjs')
      const sha = revision(process.env.REMOTE_RECOVERY_REVISION)
      await verifyPublicSource(sha)
      const template = JSON.parse(await readFile(new URL('../wrangler.jsonc', import.meta.url), 'utf8'))
      const config = stagingConfig(template, { ...identityFromEnv(), revision: sha,
        databaseId: process.env.REMOTE_RECOVERY_DATABASE_ID })
      result = await api.recover(config, process.env.REMOTE_RECOVERY_WORKER_SHA256)
    } else result = mode === 'provision' ? await api.provision() : await api.inspect()
    if (mode === 'resolve') requireValue(result.databaseId, 'The staging database is absent; run provision first')
    if (process.env.GITHUB_OUTPUT && result.databaseId) await appendFile(process.env.GITHUB_OUTPUT, `database_id=${result.databaseId}\n`)
    console.log(JSON.stringify(result))
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
