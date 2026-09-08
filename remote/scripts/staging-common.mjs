// Administrator tooling for one staging service. Public IDs are configuration;
// provider credentials are consumed only by the separate Cloudflare commands.
export const STAGING = Object.freeze({
  name: 'nexus-remote-staging',
  origin: 'https://remote-staging.hamradiotools.io',
  zone: 'hamradiotools.io',
  audience: 'https://remote-staging.hamradiotools.io/api',
  tag: 'nexus-remote-observation-staging',
})

export function requireValue(condition, message) {
  if (!condition) throw new Error(message)
}

export function databaseId(value) {
  requireValue(typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)
    && !/^0+$/.test(value.replaceAll('-', '')), 'A provisioned D1 UUID is required')
  return value
}

export function revision(value) {
  requireValue(typeof value === 'string' && /^[0-9a-f]{40}$/.test(value) && !/^0+$/.test(value), 'A full source commit SHA is required')
  return value
}

export function identity(values) {
  let issuer
  try { issuer = new URL(values.issuer) } catch { throw new Error('An Auth0 HTTPS issuer is required') }
  requireValue(issuer.protocol === 'https:' && !issuer.username && !issuer.password && !issuer.port
    && issuer.pathname === '/' && !issuer.search && !issuer.hash
    && issuer.hostname.endsWith('.auth0.com'), 'Use the Auth0 tenant HTTPS domain for this pilot')
  requireValue(typeof values.clientId === 'string' && /^[A-Za-z0-9_-]{8,128}$/.test(values.clientId)
    && values.clientId !== 'unconfigured', 'A public Auth0 SPA client ID is required')
  requireValue(values.audience === STAGING.audience, 'The staging API audience must match the documented identifier')
  return { issuer: issuer.href, clientId: values.clientId, audience: values.audience }
}

export function identityFromEnv(env = process.env) {
  return identity({ issuer: env.REMOTE_AUTH0_ISSUER, clientId: env.REMOTE_AUTH0_CLIENT_ID,
    audience: env.REMOTE_AUTH0_AUDIENCE || STAGING.audience })
}

export function stagingConfig(template, values) {
  const ids = identity(values), config = structuredClone(template)
  requireValue(config.name === STAGING.name && config.d1_databases?.length === 1
    && config.d1_databases[0].database_name === STAGING.name && config.d1_databases[0].binding === 'DB',
  'The template must describe the dedicated staging Worker and database')
  config.vars = { PUBLIC_REMOTE_ORIGIN: STAGING.origin, AUTH0_ISSUER: ids.issuer,
    AUTH0_CLIENT_ID: ids.clientId, AUTH0_AUDIENCE: ids.audience,
    REMOTE_BUILD_REVISION: values.revision ? revision(values.revision) : 'local' }
  config.d1_databases[0].database_id = databaseId(values.databaseId)
  config.routes = [{ pattern: new URL(STAGING.origin).hostname, custom_domain: true }]
  config.workers_dev = false
  config.preview_urls = false
  config.observability = { enabled: false }
  config.tags = [STAGING.tag]
  return config
}

// Do not reflect response bodies, exception details or authenticated request URLs
// into public CI logs. Keep the timeout active while reading the body as well.
export async function requestBytes(url, options = {}, fetcher = fetch, label = 'HTTP request') {
  let response
  try {
    response = await fetcher(url, { ...options, redirect: 'error', signal: AbortSignal.timeout(20000) })
    const reader = response.body?.getReader(), chunks = []
    let size = 0
    if (reader) for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      size += value.length
      if (size > 8 * 1024 * 1024) { await reader.cancel(); throw new Error() }
      chunks.push(Buffer.from(value))
    }
    return { status: response.status, headers: response.headers, bytes: Buffer.concat(chunks) }
  } catch { throw new Error(`${label} failed before a complete response`) }
}

export async function requestJson(url, options, fetcher, label) {
  const result = await requestBytes(url, options, fetcher, label)
  requireValue(result.status >= 200 && result.status < 300, `${label} failed (HTTP ${result.status})`)
  try { return JSON.parse(result.bytes.toString('utf8')) }
  catch { throw new Error(`${label} returned invalid JSON`) }
}

export async function verifyIdentity(ids, fetcher = fetch) {
  ids = identity(ids)
  const doc = await requestJson(new URL('.well-known/openid-configuration', ids.issuer), {}, fetcher, 'Auth0 discovery')
  requireValue(doc.issuer === ids.issuer && doc.authorization_endpoint === `${ids.issuer}authorize`
    && doc.token_endpoint === `${ids.issuer}oauth/token` && doc.jwks_uri === `${ids.issuer}.well-known/jwks.json`
    && doc.response_types_supported?.includes('code') && doc.id_token_signing_alg_values_supported?.includes('RS256')
    && doc.code_challenge_methods_supported?.includes('S256'), 'Auth0 discovery does not match the required issuer and PKCE configuration')
  const keys = await requestJson(doc.jwks_uri, {}, fetcher, 'Auth0 public signing keys')
  requireValue(keys.keys?.some(key => key.kty === 'RSA' && key.use === 'sig' && key.kid && key.n && key.e),
    'Auth0 has no usable public RSA signing key')
  // Public discovery cannot attest this application's callbacks or API settings.
  return { issuer: ids.issuer, discovery: 'passed', applicationSettings: 'require dashboard review and real sign-in' }
}
