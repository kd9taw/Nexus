// Only the explicit migration/deployment steps receive Cloudflare credentials.
// Wrangler output may contain account metadata; do not copy it to public CI logs.
import { spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, readdir, writeFile, rm } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { verifyArtifact } from './staging-artifact.mjs'
import { cloudflare } from './cloudflare-staging.mjs'
import { STAGING, requireValue } from './staging-common.mjs'

const repository = fileURLToPath(new URL('../../', import.meta.url))
export async function uploadArtifact(root, mode, env = process.env) {
  requireValue(['dry-run', 'migrate', 'deploy'].includes(mode), 'Use dry-run, migrate or deploy')
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
    const codes = [...new Set([...Buffer.concat(diagnostics).toString('utf8').matchAll(/\[code: (\d{4,6})\]/g)].map(match => match[1]))].slice(0, 4)
    requireValue(code === 0, `Wrangler ${mode} failed (exit ${code}${codes.length ? `; Cloudflare codes ${codes.join(', ')}` : ''}); inspect Cloudflare deployment state before retrying`)
    requireValue(digest(await readFile(config.main)) === manifest.files['worker.js'], 'Wrangler changed the upload module')
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
    requireValue(process.argv.length === 3, 'Use dry-run, migrate or deploy')
    await uploadArtifact(repository, process.argv[2])
    console.log(`Staging ${process.argv[2]} completed; live verification is a separate required step.`)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
