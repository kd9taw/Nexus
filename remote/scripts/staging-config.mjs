// Public configuration only. Does not authenticate, provision or deploy anything.
import { readFile, writeFile } from 'node:fs/promises'
import { stagingConfig, requireValue } from './staging-common.mjs'
try {
  const keys = ['issuer', 'client-id', 'audience', 'database-id', 'revision']
  const args = process.argv.slice(2), values = {}
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]?.slice(2)
    requireValue(args[i]?.startsWith('--') && keys.includes(key) && args[i + 1] && !values[key],
      'Use each required public configuration option once')
    values[key] = args[i + 1]
  }
  const template = JSON.parse(await readFile(new URL('../wrangler.jsonc', import.meta.url), 'utf8'))
  const config = stagingConfig(template, { issuer: values.issuer, clientId: values['client-id'],
    audience: values.audience, databaseId: values['database-id'], revision: values.revision })
  await writeFile(new URL('../wrangler.staging.jsonc', import.meta.url), JSON.stringify(config, null, 2) + '\n', { flag: 'wx', mode: 0o600 })
  console.log('Created remote/wrangler.staging.jsonc for review; no provider operation performed.')
} catch (error) {
  console.error(error.code === 'EEXIST' ? 'Staging configuration already exists; review it before replacing it.' : error.message)
  process.exitCode = 1
}
