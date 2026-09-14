// Only the explicit migration/deployment steps receive Cloudflare credentials.
// Wrangler output may contain account metadata; it reaches the public CI log ONLY through
// `redactedDiagnostic` below, and only when the command failed.
import { spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, readdir, writeFile, rm } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { verifyArtifact, verifyLiveSettled } from './staging-artifact.mjs'
import { cloudflare } from './cloudflare-staging.mjs'
import { requireValue, secretValue, secretVariable, target } from './staging-common.mjs'

/** Wrangler's own words, made safe for a PUBLIC CI log.
 *
 *  Two passes, because either alone has a gap. First the exact secret values this process was
 *  given - the only certain redaction, and the one that cannot be fooled by an unfamiliar format.
 *  Then any token-shaped run: 25+ of the characters Cloudflare uses, and containing a digit, which
 *  covers a value this process never held. The digit requirement is what keeps error slugs like
 *  `workers_dev_subdomain_not_configured` readable - they are the diagnostic, and a redaction that
 *  ate them would leave us exactly where we started.
 *
 *  Bounded to 40 lines and 4000 characters: a failing command must not be able to flood the log.
 *
 *  Every REMOTE_* value is redacted too. The Worker secrets arrive that way (ADMIN_SUBJECT as
 *  REMOTE_ADMIN_SUBJECT), and an Auth0 subject like `google-oauth2|<21 digits>` has no run long
 *  enough for the token pattern, so without this it would pass straight through. Unknown REMOTE_*
 *  values fall on the safe side: a redacted public value costs readability, never a leak.
 */
export function redactedDiagnostic(text, env = process.env) {
  let safe = text
  const remote = Object.entries(env).filter(([name]) => /^REMOTE_[A-Z0-9_]+$/.test(name) && !name.endsWith('_PRESENT')).map(([, value]) => value)
  for (const value of [env.CLOUDFLARE_API_TOKEN, env.CLOUDFLARE_ACCOUNT_ID, ...remote]) {
    if (typeof value === 'string' && value.length >= 8) safe = safe.split(value).join('[redacted]')
  }
  safe = safe.replace(/(?=[A-Za-z0-9_-]*\d)[A-Za-z0-9_-]{25,}/g, '[redacted]')
  return safe.split('\n').slice(0, 40).join('\n').slice(0, 4000)
}

/** Compare the migrations the live database reports against the ones the artifact carries.
 *
 *  Pure, and exported, because this is the half that can be quietly wrong: everything else in
 *  verify-schema is a wrangler invocation. A parser that returns an empty list on unfamiliar
 *  output would turn "the schema never landed" into a green step - which is exactly the failure
 *  this check exists to catch, so both sides are floored above zero before they are compared.
 */
export function compareSchema(output, shipped, { stderr = '', env = process.env } = {}) {
  // `output` is Wrangler's STDOUT only. The JSON may still have text before or after it, so each `[`
  // is tried as the start of one balanced, string-aware bracket run, and the first run that parses as
  // an array of D1 result sets is the answer. The old parser ended every candidate at the LAST `]` in
  // the buffer, so any bracketed line after the JSON made every candidate unparseable.
  const resultSets = value => Array.isArray(value) && value.length > 0
    && value.every(part => part && typeof part === 'object' && Array.isArray(part.results))
  let applied
  for (let at = output.indexOf('['), tries = 0; at !== -1 && tries < 200 && applied === undefined; at = output.indexOf('[', at + 1), tries++) {
    const end = balancedEnd(output, at)
    if (end === -1) continue
    let parsed
    try { parsed = JSON.parse(output.slice(at, end)) } catch { continue }
    if (resultSets(parsed)) applied = parsed.flatMap(part => part.results).map(row => row?.name).filter(Boolean)
  }
  // Name what came back, so the next failure is diagnosed from its own log line. Redacted like every
  // other Wrangler diagnostic; both streams are bounded to a short excerpt.
  const describe = (label, text) => `${label} ${Buffer.byteLength(text)} bytes${text.trim()
    ? ` ${JSON.stringify(redactedDiagnostic(text.replace(/\s+/g, ' ').trim(), env).slice(0, 200))}` : ''}`
  requireValue(applied !== undefined, `The schema query did not return readable JSON (${describe('stdout', output)}; ${describe('stderr', stderr)})`)
  requireValue(shipped.length > 0, 'The artifact carries no migrations to verify against')
  requireValue(applied.length > 0, 'The live database reports no applied migrations')
  const missing = shipped.filter(name => !applied.includes(name))
  requireValue(missing.length === 0, `The live database is missing migrations: ${missing.join(', ')}`)
  return { applied: applied.length, shipped: shipped.length }
}

// Index just past the bracket matching the one at `at`, honouring JSON strings and escapes; -1 if unclosed.
function balancedEnd(text, at) {
  let depth = 0, string = false
  for (let i = at; i < text.length; i++) {
    const c = text[i]
    if (string) { if (c === '\\') i++; else if (c === '"') string = false; continue }
    if (c === '"') string = true
    else if (c === '[' || c === '{') depth++
    else if ((c === ']' || c === '}') && --depth === 0) return i + 1
  }
  return -1
}

/** Wrangler's log level for each mode.
 *
 *  `error` keeps account metadata out of the public log for the commands whose output is only ever
 *  a failure diagnostic. It CANNOT be used where the output is the result: Wrangler prints
 *  `d1 execute --json` through the same level-filtered logger, so under WRANGLER_LOG=error the query
 *  ran, exited 0 and printed nothing at all. That is staging run 34797528705 - the first run ever to
 *  reach verify-schema - failing "did not return readable JSON" on an empty buffer.
 */
export const wranglerLogLevel = mode => mode === 'verify-schema' ? 'log' : 'error'

/** The read-only query verify-schema runs. `location` is `--remote` in the deploy, `--local` in tests. */
export const schemaQuery = (database, configPath, location = '--remote') =>
  ['d1', 'execute', database, location, '--config', configPath, '--json', '--command', 'SELECT name FROM d1_migrations ORDER BY id']

/** Run the pinned Wrangler with stdout and stderr kept APART (a result is parsed from stdout only),
 *  each bounded, plus both in arrival order for a failure diagnostic. No REMOTE_* value is inherited:
 *  Worker secrets reach Wrangler only through --secrets-file. */
export async function runWrangler(argv, { cwd, env, logLevel }) {
  requireValue(['error', 'log'].includes(logLevel), 'Unexpected Wrangler log level')
  const inherited = Object.fromEntries(Object.entries(env).filter(([name]) => !name.startsWith('REMOTE_')))
  const child = spawn(process.execPath, [join(repository, 'remote/node_modules/wrangler/bin/wrangler.js'), ...argv], {
    cwd, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...inherited, CI: 'true', WRANGLER_SEND_METRICS: 'false', WRANGLER_LOG: logLevel, WRANGLER_LOG_PATH: cwd },
  })
  const limit = 65536, streams = { stdout: [], stderr: [], output: [] }, sizes = { stdout: 0, stderr: 0, output: 0 }
  const keep = (name, bytes) => {
    if (sizes[name] < limit) { streams[name].push(bytes.subarray(0, limit - sizes[name])); sizes[name] += bytes.length }
  }
  child.stdout.on('data', bytes => { keep('stdout', bytes); keep('output', bytes) })
  child.stderr.on('data', bytes => { keep('stderr', bytes); keep('output', bytes) })
  const code = await new Promise((resolve, reject) => { child.once('error', () => reject(new Error('Wrangler could not start'))); child.once('close', resolve) })
  const text = name => Buffer.concat(streams[name]).toString('utf8')
  return { code, stdout: text('stdout'), stderr: text('stderr'), output: text('output') }
}

/** Hand wrangler the Worker's required secrets as a private file for `--secrets-file`, then remove it.
 *
 *  Values come from REMOTE_<NAME> and are refused, by variable name only, before anything is written.
 *  The file lives in a fresh 0700 temporary directory outside the repository, is created exclusively
 *  with mode 0600, and is removed when the upload returns or throws. Wrangler documents the flag as
 *  additive: secrets not in the file are kept, so this never deletes one.
 */
export async function withSecretsFile(env, required, upload) {
  const secrets = Object.fromEntries(required.map(name => [name, secretValue(env, name)]))
  if (required.length === 0) return upload(null)
  const directory = await mkdtemp(join(tmpdir(), 'nexus-remote-secrets-'))
  try {
    const path = join(directory, 'secrets.json')
    await writeFile(path, JSON.stringify(secrets), { flag: 'wx', mode: 0o600 })
    return await upload(path)
  } finally { await rm(directory, { recursive: true, force: true }) }
}

/** Once `wrangler deploy` returns, the Worker is LIVE, whatever the checks after it decide.
 *
 *  The post-upload check used to end the step on failure, so the live verification never ran and a
 *  Worker nobody had verified went on serving (staging run 34792345723). Both run now, every time,
 *  and both results are reported together. The deploy still fails if either did: a live check that
 *  passed does not excuse a binding that differs from the artifact, and the reverse.
 *
 *  The domain is attached only after the upload check passes, exactly as before.
 */
export async function settleUpload(api, config, manifest, verify, log = console.log) {
  const report = {}
  try {
    await api.markUpload(config, manifest.files['worker.js'])
    await api.attachDomain()
    report.uploadCheck = { passed: true }
  } catch (error) { report.uploadCheck = { passed: false, error: error.message } }
  try { report.liveVerification = { passed: true, ...await verify() } }
  catch (error) { report.liveVerification = { passed: false, error: error.message } }
  log(JSON.stringify(report))
  const describe = part => part.passed ? 'passed' : `failed (${part.error})`
  requireValue(report.uploadCheck.passed && report.liveVerification.passed,
    `The uploaded Worker is live and not fully verified: post-upload check ${describe(report.uploadCheck)}; live verification ${describe(report.liveVerification)}`)
  return report
}

const repository = fileURLToPath(new URL('../../', import.meta.url))
export async function uploadArtifact(root, mode, env = process.env, fetcher = fetch) {
  requireValue(['dry-run', 'migrate', 'deploy', 'verify-schema'].includes(mode),
    'Use dry-run, migrate, deploy or verify-schema')
  const row = target(env)
  const { manifest, config } = await verifyArtifact(root, env.GITHUB_SHA, row)
  if (mode === 'migrate' || mode === 'deploy') {
    // Every precondition BEFORE the first write: ownership, database and required secrets. Migrate
    // checks them itself rather than trusting an earlier workflow step to have run.
    await cloudflare(env, fetcher, row).preflight(config.secrets?.required ?? [], config.d1_databases[0].database_id)
  } else if (mode === 'verify-schema') {
    // Read-only, and it must still run after a failed upload check, so no secrets precondition.
    const current = await cloudflare(env, fetcher, row).inspect()
    requireValue(current.databaseId === config.d1_databases[0].database_id, 'The artifact database does not match the staging account inventory')
  }
  // Wrangler's cache/diagnostic files must not change the verified artifact.
  // Relocate its three filesystem paths. Install the single custom domain through
  // the scoped API after upload verification, avoiding Wrangler's bulk route
  // reconciliation and its noninteractive DNS-overwrite defaults.
  const directory = await mkdtemp(join(tmpdir(), 'nexus-remote-wrangler-'))
  try {
    const artifact = join(root, 'remote/staging-artifact')
    // Even --no-bundle discovers adjacent text/SQL files as extra modules.
    // Give it a directory containing only the verified Worker bytes.
    const moduleDirectory = join(directory, 'module')
    await mkdir(moduleDirectory)
    const worker = await readFile(join(artifact, 'worker.js'))
    const digest = bytes => createHash('sha256').update(bytes).digest('hex')
    requireValue(digest(worker) === manifest.files['worker.js'], 'Worker bytes changed before upload')
    config.main = join(moduleDirectory, 'worker.js')
    await writeFile(config.main, worker, { flag: 'wx', mode: 0o600 })
    config.assets.directory = join(artifact, 'assets')
    config.d1_databases[0].migrations_dir = join(artifact, 'migrations')
    config.routes = []
    const configPath = join(directory, 'wrangler.jsonc')
    await writeFile(configPath, JSON.stringify(config), { flag: 'wx', mode: 0o600 })
    const args = mode === 'migrate'
      ? ['d1', 'migrations', 'apply', row.database, '--remote', '--config', configPath]
      : mode === 'verify-schema'
        ? schemaQuery(row.database, configPath)
        : ['deploy', '--no-bundle', '--config', configPath, ...(mode === 'dry-run' ? ['--dry-run', '--outdir', join(directory, 'output')] : [])]
    const run = argv => runWrangler(argv, { cwd: directory, env, logLevel: wranglerLogLevel(mode) })
    // deploy applies the real secrets from the environment on every upload. dry-run hands the pinned
    // Wrangler the same flag and file shape holding SYNTHETIC values, so the exact deploy arguments are
    // validated offline without the dry-run step ever holding a real secret.
    const required = config.secrets?.required ?? []
    const secretsEnv = mode === 'deploy' ? env : mode === 'dry-run'
      ? Object.fromEntries(required.map(name => [secretVariable(name), 'dry-run-synthetic-value'])) : null
    const { code, output, stdout, stderr } = secretsEnv
      ? await withSecretsFile(secretsEnv, required, path => run(path ? [...args, '--secrets-file', path] : args))
      : await run(args)
    const codes = [...new Set([...output.matchAll(/\[code: (\d{4,6})\]/g)].map(match => match[1]))].slice(0, 4)
    // A failure that will not say why costs a 38-minute run every time it is guessed at. Two
    // deploys failed in exactly four seconds with no Cloudflare code, and six hypotheses had to be
    // eliminated by other means because this sentence was the only evidence that existed.
    if (code !== 0) console.error(redactedDiagnostic(output, env))
    requireValue(code === 0, `Wrangler ${mode} failed (exit ${code}${codes.length ? `; Cloudflare codes ${codes.join(', ')}` : ''}); inspect Cloudflare deployment state before retrying`)
    requireValue(digest(await readFile(config.main)) === manifest.files['worker.js'], 'Wrangler changed the upload module')
    if (mode === 'verify-schema') {
      // The live verification is thorough about the Worker and the browser bytes, and touches the
      // DATABASE not at all: `ready` is only a var comparison, and both admission probes are
      // refused before any query is issued. So the migrate step's exit code was the only evidence
      // the schema had landed - and a schema that did not land is not a loud failure, it is
      // endpoints refusing with `serviceUnavailable` once somebody tries to use them.
      const shipped = (await readdir(join(artifact, 'migrations'))).filter(name => name.endsWith('.sql')).sort()
      const result = compareSchema(stdout, shipped, { stderr, env })
      console.log(`Schema verified: ${result.applied} migration(s) applied, all ${result.shipped} from the artifact present`)
      return { mode, ...result }
    }
    if (mode === 'dry-run') {
      const files = (await readdir(join(directory, 'output'))).sort()
      requireValue(JSON.stringify(files) === JSON.stringify(['README.md', 'worker.js'])
        && digest(await readFile(join(directory, 'output/worker.js'))) === manifest.files['worker.js'],
      'Wrangler output module inventory or bytes differ from the artifact')
    }
    const verified = await verifyArtifact(root, env.GITHUB_SHA, row)
    if (mode === 'deploy') await settleUpload(cloudflare(env, fetcher, row), config, manifest, () => verifyLiveSettled(verified, fetch, row))
  } finally { await rm(directory, { recursive: true, force: true }) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    requireValue(process.argv.length === 3, 'Use dry-run, migrate, deploy or verify-schema')
    await uploadArtifact(repository, process.argv[2])
    console.log(`Staging ${process.argv[2]} completed; live verification is a separate required step.`)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
