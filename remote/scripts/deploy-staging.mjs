// Only the explicit migration/deployment steps receive Cloudflare credentials.
// Wrangler output may contain account metadata; it reaches the public CI log ONLY through
// `redactedDiagnostic` below, and only when the command failed.
import { spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, readdir, writeFile, rm } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { verifyArtifact } from './staging-artifact.mjs'
import { cloudflare } from './cloudflare-staging.mjs'
import { STAGING, requireValue } from './staging-common.mjs'

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
 */
export function redactedDiagnostic(text, env = process.env) {
  let safe = text
  for (const value of [env.CLOUDFLARE_API_TOKEN, env.CLOUDFLARE_ACCOUNT_ID]) {
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
export function compareSchema(output, shipped) {
  // stdout and stderr are captured together and wrangler prefixes its lines with `[info]` and
  // friends, so the first `[` in the buffer is usually NOT the start of the JSON. Try every
  // candidate rather than guessing one; the first that parses to an array of result sets is it.
  let applied
  const end = output.lastIndexOf(']') + 1
  for (let at = output.indexOf('['); at !== -1 && at < end; at = output.indexOf('[', at + 1)) {
    let parsed
    try { parsed = JSON.parse(output.slice(at, end)) } catch { continue }
    if (!Array.isArray(parsed)) continue
    applied = parsed.flatMap(part => part?.results ?? []).map(row => row?.name).filter(Boolean)
    break
  }
  requireValue(applied !== undefined, 'The schema query did not return readable JSON')
  requireValue(shipped.length > 0, 'The artifact carries no migrations to verify against')
  requireValue(applied.length > 0, 'The live database reports no applied migrations')
  const missing = shipped.filter(name => !applied.includes(name))
  requireValue(missing.length === 0, `The live database is missing migrations: ${missing.join(', ')}`)
  return { applied: applied.length, shipped: shipped.length }
}

const repository = fileURLToPath(new URL('../../', import.meta.url))
export async function uploadArtifact(root, mode, env = process.env) {
  requireValue(['dry-run', 'migrate', 'deploy', 'verify-schema'].includes(mode),
    'Use dry-run, migrate, deploy or verify-schema')
  const { manifest, config } = await verifyArtifact(root, env.GITHUB_SHA)
  if (mode !== 'dry-run') {
    const current = await cloudflare(env).inspect()
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
      ? ['d1', 'migrations', 'apply', STAGING.name, '--remote', '--config', configPath]
      : mode === 'verify-schema'
        ? ['d1', 'execute', STAGING.name, '--remote', '--config', configPath, '--json',
          '--command', 'SELECT name FROM d1_migrations ORDER BY id']
        : ['deploy', '--no-bundle', '--config', configPath, ...(mode === 'dry-run' ? ['--dry-run', '--outdir', join(directory, 'output')] : [])]
    const child = spawn(process.execPath, [join(repository, 'remote/node_modules/wrangler/bin/wrangler.js'), ...args], {
      cwd: directory, stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...env, CI: 'true', WRANGLER_SEND_METRICS: 'false', WRANGLER_LOG: 'error', WRANGLER_LOG_PATH: directory },
    })
    const diagnostics = []
    let diagnosticSize = 0
    for (const stream of [child.stdout, child.stderr]) stream.on('data', bytes => {
      if (diagnosticSize < 65536) { diagnostics.push(bytes.subarray(0, 65536 - diagnosticSize)); diagnosticSize += bytes.length }
    })
    const code = await new Promise((resolve, reject) => { child.once('error', () => reject(new Error('Wrangler could not start'))); child.once('close', resolve) })
    const output = Buffer.concat(diagnostics).toString('utf8')
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
      const result = compareSchema(output, shipped)
      console.log(`Schema verified: ${result.applied} migration(s) applied, all ${result.shipped} from the artifact present`)
      return { mode, ...result }
    }
    if (mode === 'dry-run') {
      const files = (await readdir(join(directory, 'output'))).sort()
      requireValue(JSON.stringify(files) === JSON.stringify(['README.md', 'worker.js'])
        && digest(await readFile(join(directory, 'output/worker.js'))) === manifest.files['worker.js'],
      'Wrangler output module inventory or bytes differ from the artifact')
    }
    await verifyArtifact(root, env.GITHUB_SHA)
    if (mode === 'deploy') {
      const api = cloudflare(env)
      await api.markUpload(config, manifest.files['worker.js'])
      await api.attachDomain()
    }
  } finally { await rm(directory, { recursive: true, force: true }) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    requireValue(process.argv.length === 3, 'Use dry-run, migrate, deploy or verify-schema')
    await uploadArtifact(repository, process.argv[2])
    console.log(`Staging ${process.argv[2]} completed; live verification is a separate required step.`)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
