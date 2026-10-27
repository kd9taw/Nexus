// Administrator tooling for the Remote service environments. Public IDs are configuration;
// provider credentials are consumed only by the separate Cloudflare commands.
//
// ONE TABLE, one row per environment. Every name a script inventories, deploys or verifies comes
// from its row, so staging and production cannot become two drifting copies of the same tooling.
// Auth0 issuer and client ID VALUES are not here: they arrive from the GitHub environment's
// variables (REMOTE_AUTH0_ISSUER, REMOTE_AUTH0_CLIENT_ID). What the row pins is the RULE an issuer
// must satisfy and the exact API audience.
//
// `billing` names the private billing Worker the relay reaches through a `BILLING` service binding to
// its `BillingService` entrypoint (the RPC methods checkout and portal; its public route is the webhook
// alone). The binding is written only once `deployed` is true, a reviewed one-line change made after that
// Worker exists: until then the relay's deploys carry no binding and verify that none is present, its
// billing routes answer `billingNotAvailable`, and the page shows no Subscribe or Manage button.
export const TARGETS = Object.freeze({
  staging: Object.freeze({
    target: 'staging',
    name: 'nexus-remote-staging',
    database: 'nexus-remote-staging',
    origin: 'https://remote-staging.hamradiotools.io',
    zone: 'hamradiotools.io',
    audience: 'https://remote-staging.hamradiotools.io/api',
    tag: 'nexus-remote-observation-staging',
    issuer: Object.freeze({ hostSuffix: '.auth0.com' }),
    billing: Object.freeze({ service: 'nexus-billing-staging', deployed: false }),
  }),
  // PLACEHOLDER. None of these resources exist, and the hostname and sign-in domain are proposals
  // still awaiting the operator's decisions. `deployable()` refuses this row everywhere a script
  // could write, build a deploy configuration or contact the provider; remove `placeholder` only
  // in the reviewed change that creates production.
  production: Object.freeze({
    target: 'production',
    placeholder: 'proposed names; production resources do not exist and are not approved',
    name: 'nexus-remote',
    database: 'nexus-remote',
    origin: 'https://remote.hamradiotools.io',
    zone: 'hamradiotools.io',
    audience: 'https://remote.hamradiotools.io/api',
    tag: 'nexus-remote-production',
    issuer: Object.freeze({ exact: 'https://login.hamradiotools.io/' }),
    billing: Object.freeze({ service: 'nexus-billing', deployed: false }),
  }),
})
export const STAGING = TARGETS.staging

export function requireValue(condition, message) {
  if (!condition) throw new Error(message)
}

// Unset means staging, which is what every existing workflow run and local command meant. An
// explicit empty or unknown value is refused rather than guessed.
export function target(env = process.env) {
  const key = env.REMOTE_TARGET ?? 'staging'
  requireValue(typeof key === 'string' && Object.hasOwn(TARGETS, key), 'REMOTE_TARGET must be staging or production')
  return TARGETS[key]
}

// A Worker's required secret NAME reaches a deploy as the GitHub environment secret REMOTE_<NAME>,
// and every deploy applies it, so it cannot be forgotten or left as a plain var. Steps that only need
// to know it EXISTS receive REMOTE_<NAME>_PRESENT ('true'/'false', from `secrets.X != ''`); the value
// itself is handed to the one upload step, and the relay key's also to the preflight, which mints one
// credential with it (cloudflare-staging.mjs `relayKeyCheck`) and nothing else.
export const secretVariable = name => `REMOTE_${name}`
export function secretValue(env, name) {
  const variable = secretVariable(name), value = env[variable]
  // The message names the variable and never echoes what was found.
  requireValue(typeof value === 'string' && value.length >= 8 && value.length <= 1024 && value.trim() === value
    && ![...value].some(character => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127),
  `${variable} is not set in this GitHub environment as a usable secret`)
  return value
}
export function secretSupplied(env, name) {
  try { secretValue(env, name); return true } catch { return env[`${secretVariable(name)}_PRESENT`] === 'true' }
}

// Only a row of the table itself - not a copy with its placeholder deleted - and never a placeholder.
export function deployable(row) {
  requireValue(row && TARGETS[row.target] === row, 'Unknown Remote deployment target')
  requireValue(!row.placeholder, `The ${row.target} target holds placeholder values (${row.placeholder}); nothing was sent`)
  return row
}

export const BILLING_ENTRYPOINT = 'BillingService'
export function billingServices(row) {
  return row.billing.deployed ? [{ binding: 'BILLING', service: row.billing.service, entrypoint: BILLING_ENTRYPOINT }] : []
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

// An unset GitHub variable reaches a step as an empty string.
const given = value => value !== undefined && value !== ''

export function identity(values, row = target()) {
  let issuer
  try { issuer = new URL(values.issuer) } catch { throw new Error('An Auth0 HTTPS issuer is required') }
  requireValue(issuer.protocol === 'https:' && !issuer.username && !issuer.password && !issuer.port
    && issuer.pathname === '/' && !issuer.search && !issuer.hash, 'Use the Auth0 tenant HTTPS domain for this pilot')
  // Staging accepts the tenant's own auth0.com domain; production pins one exact custom sign-in
  // domain, so an auth0.com issuer there is refused, and so is the custom domain on staging.
  requireValue(row.issuer.hostSuffix ? issuer.hostname.endsWith(row.issuer.hostSuffix) : issuer.href === row.issuer.exact,
    row.issuer.hostSuffix ? 'Use the Auth0 tenant HTTPS domain for this pilot' : `The ${row.target} issuer must be exactly ${row.issuer.exact}`)
  requireValue(typeof values.clientId === 'string' && /^[A-Za-z0-9_-]{8,128}$/.test(values.clientId)
    && values.clientId !== 'unconfigured', 'A public Auth0 SPA client ID is required')
  requireValue(values.audience === row.audience, `The ${row.target} API audience must match the documented identifier`)
  const ids = { issuer: issuer.href, clientId: values.clientId, audience: values.audience }
  // The desktop's Native application, optional: without it the account routes answer 503. Never the
  // SPA's own id, which authority.ts refuses at run time; refusing it here stops it before a deploy.
  if (given(values.nativeClientId)) {
    requireValue(typeof values.nativeClientId === 'string' && /^[A-Za-z0-9_-]{8,128}$/.test(values.nativeClientId)
      && values.nativeClientId !== 'unconfigured', 'REMOTE_AUTH0_NATIVE_CLIENT_ID is not a public Auth0 Native client ID')
    requireValue(values.nativeClientId !== values.clientId, 'REMOTE_AUTH0_NATIVE_CLIENT_ID must differ from the SPA client ID')
    ids.nativeClientId = values.nativeClientId
  }
  return ids
}

// Worker settings a deploy writes only when the GitHub environment sets them. Each is refused by
// value, so a typo cannot reach a Worker as a setting that silently reads as off.
export const MOVE_STAGES = Object.freeze(['announce', 'pairing-closed', 'closed'])
export function optionalVars(values, row = target()) {
  const vars = {}
  if (given(values.trialIdentityRequired)) {
    requireValue(values.trialIdentityRequired === '1', 'REMOTE_TRIAL_IDENTITY_REQUIRED must be 1 or unset')
    vars.TRIAL_IDENTITY_REQUIRED = '1'
  }
  // Free during the beta: the trial's end taken away (authority.ts). Staging's alone, so production can
  // only ever have it through a reviewed change to this line, never by a variable set in its environment.
  if (given(values.betaFree)) {
    requireValue(row === STAGING, 'REMOTE_BETA_FREE is for staging only')
    requireValue(values.betaFree === '1', 'REMOTE_BETA_FREE must be 1 or unset')
    vars.REMOTE_BETA_FREE = '1'
  }
  // The overlap after a move: where this service went, and how far its retirement has got.
  requireValue(given(values.movedTo) === given(values.moveStage), 'REMOTE_MOVED_TO and REMOTE_MOVE_STAGE are set together or not at all')
  if (given(values.movedTo)) {
    requireValue(Object.values(TARGETS).some(other => other !== row && other.origin === values.movedTo),
      'REMOTE_MOVED_TO must be the origin of another Remote environment')
    requireValue(MOVE_STAGES.includes(values.moveStage), `REMOTE_MOVE_STAGE must be one of ${MOVE_STAGES.join(', ')}`)
    vars.REMOTE_MOVED_TO = values.movedTo
    vars.REMOTE_MOVE_STAGE = values.moveStage
  }
  return vars
}

// Worker secrets a deploy applies only when the GitHub environment supplies them. One that is supplied
// joins the artifact's `secrets.required`, so the deploy treats it exactly like the required ones: the
// preflight demands it, the upload applies it through --secrets-file, and the post-upload check counts
// it and demands it encrypted. TRIAL_KEY can never be rotated (STAGING.md), so it is never generated here.
// OWNER_NOTICE_API_KEY and OWNER_NOTICE_FROM are the email provider's key and the verified sender the
// new-device notice goes out with (authority.ts `deviceAddedNotice`); with either unset it sends nothing.
export const OPTIONAL_SECRETS = Object.freeze(['TRIAL_KEY', 'OWNER_NOTICE_API_KEY', 'OWNER_NOTICE_FROM'])
export function optionalSecrets(names = []) {
  requireValue(Array.isArray(names) && names.every(name => OPTIONAL_SECRETS.includes(name)), 'Unknown optional Worker secret')
  return OPTIONAL_SECRETS.filter(name => names.includes(name))
}

export function identityFromEnv(env = process.env, row = target(env)) {
  return identity({ issuer: env.REMOTE_AUTH0_ISSUER, clientId: env.REMOTE_AUTH0_CLIENT_ID,
    audience: env.REMOTE_AUTH0_AUDIENCE || row.audience, nativeClientId: env.REMOTE_AUTH0_NATIVE_CLIENT_ID }, row)
}

// Everything a deploy configuration is built from that the GitHub environment holds: the identity,
// the optional vars, and the NAMES of the optional secrets it supplies (presence only, never a value).
export function settingsFromEnv(env = process.env, row = target(env)) {
  return { ...identityFromEnv(env, row),
    trialIdentityRequired: env.REMOTE_TRIAL_IDENTITY_REQUIRED, movedTo: env.REMOTE_MOVED_TO, moveStage: env.REMOTE_MOVE_STAGE,
    betaFree: env.REMOTE_BETA_FREE,
    secrets: OPTIONAL_SECRETS.filter(name => secretSupplied(env, name)) }
}

export function stagingConfig(template, values, row = target()) {
  deployable(row)
  const ids = identity(values, row), config = structuredClone(template)
  // The checked-in template is the local/staging one; the target's names are written over it.
  requireValue(config.name === STAGING.name && config.d1_databases?.length === 1
    && config.d1_databases[0].database_name === STAGING.database && config.d1_databases[0].binding === 'DB',
  'The template must describe the dedicated staging Worker and database')
  config.name = row.name
  config.d1_databases[0].database_name = row.database
  config.vars = { PUBLIC_REMOTE_ORIGIN: row.origin, AUTH0_ISSUER: ids.issuer,
    AUTH0_CLIENT_ID: ids.clientId, AUTH0_AUDIENCE: ids.audience,
    ...(ids.nativeClientId ? { AUTH0_NATIVE_CLIENT_ID: ids.nativeClientId } : {}),
    ...optionalVars(values, row),
    REMOTE_BUILD_REVISION: values.revision ? revision(values.revision) : 'local' }
  const secrets = optionalSecrets(values.secrets)
  if (secrets.length > 0) config.secrets = { ...config.secrets, required: [...(config.secrets?.required ?? []), ...secrets] }
  config.d1_databases[0].database_id = databaseId(values.databaseId)
  config.routes = [{ pattern: new URL(row.origin).hostname, custom_domain: true }]
  config.workers_dev = false
  config.preview_urls = false
  config.observability = { enabled: false }
  const services = billingServices(row)
  if (services.length > 0) config.services = services
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
  if (result.status < 200 || result.status >= 300) {
    let codes = []
    try { codes = JSON.parse(result.bytes.toString('utf8')).errors?.map(error => error.code)
      .filter(code => Number.isSafeInteger(code) && code >= 1000 && code <= 999999).slice(0, 4) ?? [] } catch {}
    throw new Error(`${label} failed (HTTP ${result.status}${codes.length ? `; Cloudflare codes ${codes.join(', ')}` : ''})`)
  }
  try { return JSON.parse(result.bytes.toString('utf8')) }
  catch { throw new Error(`${label} returned invalid JSON`) }
}

export async function verifyIdentity(ids, fetcher = fetch, row = target()) {
  ids = identity(ids, row)
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
