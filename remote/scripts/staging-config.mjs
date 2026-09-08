// Public configuration only. Does not authenticate, provision or deploy anything.
import { readFile, writeFile } from 'node:fs/promises'
const keys = ['issuer', 'client-id', 'audience', 'database-id']
const args = process.argv.slice(2), values = {}
for (let i = 0; i < args.length; i += 2) {
  const key = args[i]?.slice(2)
  if (!args[i]?.startsWith('--') || !keys.includes(key) || !args[i+1] || values[key]) throw new Error('Use each required public configuration option once')
  values[key] = args[i+1]
}
if (keys.some(key => !values[key])) throw new Error('Required: --issuer --client-id --audience --database-id')
const issuer = new URL(values.issuer)
if (issuer.protocol !== 'https:' || issuer.username || issuer.password || issuer.pathname !== '/' || issuer.search || issuer.hash) throw new Error('Issuer must be an HTTPS origin with a trailing slash')
if (!/^[A-Za-z0-9_-]{8,128}$/.test(values['client-id']) || !/^https:\/\/[^\s]{1,200}$/.test(values.audience)) throw new Error('Invalid public client ID or API audience')
if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(values['database-id']) || /^0+$/.test(values['database-id'].replace(/-/g,''))) throw new Error('A provisioned D1 UUID is required')
const config = JSON.parse(await readFile(new URL('../wrangler.jsonc', import.meta.url), 'utf8'))
const origin = 'https://remote-staging.hamradiotools.io'
config.vars = { PUBLIC_REMOTE_ORIGIN: origin, AUTH0_ISSUER: issuer.href, AUTH0_CLIENT_ID: values['client-id'], AUTH0_AUDIENCE: values.audience }
config.d1_databases[0].database_id = values['database-id']
config.routes = [{ pattern: new URL(origin).host, custom_domain: true }]
await writeFile(new URL('../wrangler.staging.jsonc', import.meta.url), JSON.stringify(config, null, 2)+'\n', { flag: 'wx', mode: 0o600 })
console.log('Created remote/wrangler.staging.jsonc for review; no provider operation performed.')
