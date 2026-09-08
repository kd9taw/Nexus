// Fixed-resource Cloudflare administration. Never prints provider response bodies,
// account identifiers, token values, other services or user/organization details.
import { appendFile } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'
import { STAGING, databaseId, requireValue, requestJson } from './staging-common.mjs'

export function cloudflare(env = process.env, fetcher = fetch) {
  requireValue(/^[0-9a-f]{32}$/.test(env.CLOUDFLARE_ACCOUNT_ID ?? ''), 'CLOUDFLARE_ACCOUNT_ID is missing or invalid')
  requireValue(typeof env.CLOUDFLARE_API_TOKEN === 'string' && env.CLOUDFLARE_API_TOKEN.length > 0
    && !/\s/.test(env.CLOUDFLARE_API_TOKEN), 'CLOUDFLARE_API_TOKEN is missing or invalid')
  const base = `https://api.cloudflare.com/client/v4/accounts/${env.CLOUDFLARE_ACCOUNT_ID}`
  async function api(path, label, value) {
    const response = await requestJson(`${base}${path}`, {
      method: value === undefined ? 'GET' : 'POST',
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
  async function inspect() {
    const id = await databases()
    const workers = await api('/workers/scripts', 'Worker inventory')
    requireValue(Array.isArray(workers.result) && workers.result.every(row => typeof row.id === 'string'),
      'Worker inventory returned an unexpected shape')
    const named = workers.result.filter(row => row.id === STAGING.name)
    requireValue(named.length <= 1 && named.every(row => row.tags?.includes(STAGING.tag)),
      'The staging Worker name belongs to an unrecognized deployment; review it before replacement')
    const domains = await api(`/workers/domains?hostname=${new URL(STAGING.origin).hostname}`, 'Worker domain inventory')
    requireValue(Array.isArray(domains.result), 'Worker domain inventory returned an unexpected shape')
    const matches = domains.result.filter(row => row.hostname === new URL(STAGING.origin).hostname)
    requireValue(matches.length <= 1 && matches.every(row => row.service === STAGING.name),
      'The staging hostname is assigned to another service; no takeover is allowed')
    return { service: STAGING.name, databaseId: id, workerExists: named.length === 1, domainAttached: matches.length === 1,
      readAccess: 'D1 and Workers', writeAccess: 'not established by read-only inspection' }
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
  return { inspect, provision }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const mode = process.argv[2]
    requireValue(process.argv.length === 3 && ['inspect', 'provision', 'resolve'].includes(mode), 'Use inspect, provision or resolve')
    const api = cloudflare()
    const result = mode === 'provision' ? await api.provision() : await api.inspect()
    if (mode === 'resolve') requireValue(result.databaseId, 'The staging database is absent; run provision first')
    if (process.env.GITHUB_OUTPUT && result.databaseId) await appendFile(process.env.GITHUB_OUTPUT, `database_id=${result.databaseId}\n`)
    console.log(JSON.stringify(result))
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
