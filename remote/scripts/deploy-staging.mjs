// Only the explicit migration/deployment steps receive Cloudflare credentials.
// Wrangler output may contain account metadata; do not copy it to public CI logs.
import { spawn } from 'node:child_process'
import { mkdtemp, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { verifyArtifact } from './staging-artifact.mjs'
import { cloudflare } from './cloudflare-staging.mjs'
import { STAGING, requireValue } from './staging-common.mjs'

const repository = fileURLToPath(new URL('../../', import.meta.url))
export async function uploadArtifact(root, mode, env = process.env) {
  requireValue(['dry-run', 'migrate', 'deploy'].includes(mode), 'Use dry-run, migrate or deploy')
  const { config } = await verifyArtifact(root, env.GITHUB_SHA)
  if (mode !== 'dry-run') {
    const current = await cloudflare(env).inspect()
    requireValue(current.databaseId === config.d1_databases[0].database_id, 'The artifact database does not match the staging account inventory')
  }
  // Wrangler's cache/diagnostic files must not change the verified artifact.
  // Only relocate its three relative filesystem paths into a disposable config.
  const directory = await mkdtemp(join(tmpdir(), 'nexus-remote-wrangler-'))
  try {
    const artifact = join(root, 'remote/staging-artifact')
    config.main = join(artifact, 'worker.js')
    config.assets.directory = join(artifact, 'assets')
    config.d1_databases[0].migrations_dir = join(artifact, 'migrations')
    const configPath = join(directory, 'wrangler.jsonc')
    await writeFile(configPath, JSON.stringify(config), { flag: 'wx', mode: 0o600 })
    const args = mode === 'migrate'
      ? ['d1', 'migrations', 'apply', STAGING.name, '--remote', '--config', configPath]
      : ['deploy', '--no-bundle', '--config', configPath, ...(mode === 'dry-run' ? ['--dry-run'] : [])]
    const child = spawn(process.execPath, [join(repository, 'remote/node_modules/wrangler/bin/wrangler.js'), ...args], {
      cwd: directory, stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...env, CI: 'true', WRANGLER_SEND_METRICS: 'false', WRANGLER_LOG: 'error', WRANGLER_LOG_PATH: directory },
    })
    child.stdout.resume()
    child.stderr.resume()
    const code = await new Promise((resolve, reject) => { child.once('error', () => reject(new Error('Wrangler could not start'))); child.once('close', resolve) })
    requireValue(code === 0, `Wrangler ${mode} failed; check the scoped token permissions and Cloudflare deployment state before retrying`)
    await verifyArtifact(root, env.GITHUB_SHA)
  } finally { await rm(directory, { recursive: true, force: true }) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    requireValue(process.argv.length === 3, 'Use dry-run, migrate or deploy')
    await uploadArtifact(repository, process.argv[2])
    console.log(`Staging ${process.argv[2]} completed; live verification is a separate required step.`)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
