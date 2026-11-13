// The deep health route an uptime monitor reads, and the limits added with it (R2, R6): what each says,
// what each refuses, and that an operator's ordinary use never meets either.
import { after, before, test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { runtime } from './runtime.mjs'

const sha = value => createHash('sha256').update(value).digest('hex')
const DAY = 86400000

let bare, billed, billingCalls = []
before(async () => {
  bare = await runtime()
  billed = await runtime({ billing: (method, args) => {
    billingCalls.push({ method, args })
    return method === 'checkout' ? { url: 'https://checkout.stripe.com/c/pay/cs_test_fixture' } : { url: 'https://billing.stripe.com/p/session/fixture' }
  } })
})
after(async () => { await Promise.all([bare, billed].map(app => app?.mf.dispose())) })

/** A limit counts in fixed wall-clock minutes. A case that counts one out waits for a fresh minute when
 *  fewer than five seconds are left in this one, so no count can be split across two windows. */
async function freshMinute() {
  const left = 60000 - Date.now() % 60000
  if (left < 5000) await new Promise(resolve => setTimeout(resolve, left + 50))
}
let callerNo = 0
const nextCaller = () => `203.0.113.${++callerNo}`
async function health(app, ip, expected) {
  const response = await app.mf.dispatchFetch(`${app.origin}/api/remote/health`, { headers: { 'cf-connecting-ip': ip } })
  if (expected !== undefined) assert.equal(response.status, expected, `health from ${ip}`)
  return { status: response.status, value: await response.json(), headers: response.headers }
}

test('health: database, room and billing each answer a fixed word, and nothing else is said', async () => {
  // Bound billing: all three ok, 200. The billing Worker is only looked for, never called.
  billingCalls = []
  const ok = await health(billed, nextCaller(), 200)
  assert.deepEqual(ok.value, { status: 'ok', database: 'ok', room: 'ok', billing: 'ok' })
  assert.equal(ok.headers.get('cache-control'), 'no-store')
  assert.deepEqual(billingCalls, [], 'a health read reaches no payment provider')

  // No billing Worker bound, as every deploy before billing: a reading that says so, 503.
  const unbilled = await health(bare, nextCaller(), 503)
  assert.deepEqual(unbilled.value, { status: 'degraded', database: 'ok', room: 'ok', billing: 'absent' })

  // It touches no station, account or entitlement: only the caller's own counter.
  const tables = ['accounts', 'stations', 'enrollments', 'devices', 'tickets', 'trials']
  const counts = async () => Object.fromEntries(await Promise.all(tables.map(async table =>
    [table, (await bare.db.prepare(`SELECT COUNT(*) AS n FROM ${table}`).first()).n])))
  const before = await counts()
  await health(bare, nextCaller(), 503)
  assert.deepEqual(await counts(), before)

  // The URL exactly, as every API route: a query is refused, and so is any method but GET.
  const queried = await bare.mf.dispatchFetch(`${bare.origin}/api/remote/health?nocache=1`, { headers: { 'cf-connecting-ip': nextCaller() } })
  assert.equal(queried.status, 403)
  assert.deepEqual(await queried.json(), { error: 'originDenied' })
  const posted = await bare.mf.dispatchFetch(`${bare.origin}/api/remote/health`, { method: 'POST', headers: { 'cf-connecting-ip': nextCaller() } })
  assert.notEqual(posted.status, 200)
  await posted.body?.cancel()
})

test('health: six reads a minute a caller, thirty an IPv6 /48, and one caller over never spends another\'s', async () => {
  await freshMinute()
  const flooding = nextCaller()
  for (let i = 0; i < 6; i++) await health(billed, flooding, 200)
  assert.deepEqual((await health(billed, flooding, 429)).value, { error: 'tryLater' })
  assert.deepEqual((await health(billed, flooding, 429)).value, { error: 'tryLater' }, 'still refused, and refused before any probe')
  // A monitor's other region, at its own address, reads on.
  await health(billed, nextCaller(), 200)

  // IPv6: a caller is its /64, so rotating addresses inside one is the same caller.
  for (let i = 0; i < 6; i++) await health(billed, `2001:db8:a:1::${i + 1}`, 200)
  await health(billed, '2001:db8:a:1::ffff', 429)
  // Its /48 is thirty: the six reads above that /64 was given, and four more /64s of six each, fill it;
  // a sixth /64 there is refused at once. The /64's refused read was refused by its own count first,
  // so it never spent one of the /48's.
  for (let net = 2; net <= 5; net++) for (let i = 0; i < 6; i++) await health(billed, `2001:db8:a:${net}::1`, 200)
  await health(billed, '2001:db8:a:6::1', 429)
  // Another /48 is untouched.
  await health(billed, '2001:db8:b:1::1', 200)
})

test('billing: forty pages a day an account on top of ten a minute, refused before the payment provider is asked', async () => {
  const signedIn = async () => {
    const browser = billed.client(await billed.token(`auth0|${crypto.randomUUID()}`))
    const { value } = await browser.post('session')
    return { ...browser, accountId: value.accountId }
  }
  const busy = await signedIn(), now = Date.now()
  // Thirty-nine already opened today: the day's counter, as rate() keeps it.
  await billed.db.prepare('INSERT INTO rate_limits(id,hits,expires_at) VALUES(?,?,?)')
    .bind(sha(`billing-day:${busy.accountId}`), 39, (Math.floor(now / DAY) + 1) * DAY).run()
  billingCalls = []
  await busy.post('billing/portal', {})
  assert.deepEqual((await busy.post('billing/checkout', { plan: 'monthly' }, 429)).value, { error: 'tryLater' })
  assert.deepEqual((await busy.post('billing/portal', {}, 429)).value, { error: 'tryLater' })
  assert.deepEqual(billingCalls, [{ method: 'portal', args: [busy.accountId] }], 'the fortieth went through; nothing after it did')
  // Another account's day is its own.
  await (await signedIn()).post('billing/portal', {})
})

test('an operator\'s ordinary day trips no limit: pair, stream, renew, subscribe, manage, look at the account; a monitor reads throughout', async () => {
  await freshMinute()
  const monitor = [nextCaller(), nextCaller(), nextCaller()]
  const pair = await billed.paired()
  await billed.approved(pair)
  const path = `stations/${pair.stationId}`
  for (const region of monitor) await health(billed, region, 200)
  await pair.browser.post('session')
  // A session: its ticket, and its lease renewed every thirty seconds; here four renewals back to back.
  const station = await pair.native.open(pair.stationId)
  await station.take(value => value.type === 'watch' && value.enabled === false)
  const { value: { ticket } } = await pair.browser.post(`${path}/ticket`, {})
  const observer = await pair.browser.open(pair.stationId, ticket)
  const { sessionId } = await observer.take(message => message.type === 'session')
  for (let i = 0; i < 4; i++) await pair.browser.post(`${path}/renew`, { sessionId })
  for (const region of monitor) await health(billed, region, 200)
  // Subscribing and managing it: a few checkouts and portal pages, more than anyone opens in a minute.
  for (let i = 0; i < 3; i++) await pair.browser.post('billing/checkout', { plan: i % 2 ? 'annual' : 'monthly' })
  for (let i = 0; i < 5; i++) await pair.browser.post('billing/portal', {})
  await pair.browser.post('account/devices')
  await pair.browser.post('account/export')
  await pair.browser.post('session')
  for (const region of monitor) await health(billed, region, 200)
  observer.close(); station.close()
})

test('health: a database that cannot answer is reported down by name, and the room is still probed', async () => {
  const broken = await runtime()
  try {
    // The probe is the caller's counter, a read and a write: without its table the database cannot answer it.
    await broken.db.prepare('DROP TABLE rate_limits').run()
    const reading = await health(broken, nextCaller(), 503)
    assert.deepEqual(reading.value, { status: 'degraded', database: 'down', room: 'ok', billing: 'absent' })
  } finally { await broken.mf.dispose() }
})

test('the ban STAGING.md gives, run as written, outranks a trial and a paid plan, and lifting it restores what was there', async () => {
  const doc = await readFile(new URL('../STAGING.md', import.meta.url), 'utf8')
  const section = doc.slice(doc.indexOf('### Banning an account'))
  const ban = /```sql\n(.+)\n```/.exec(section)[1]
  const lift = /`(UPDATE trials SET enabled = 1 WHERE account_id = '<account-uuid>';)`/.exec(section)[1]
  const run = (sql, accountId) => bare.db.prepare(sql.replaceAll('<account-uuid>', accountId)).run()
  const now = Date.now()
  const signedIn = async () => {
    const browser = bare.client(await bare.token(`auth0|${crypto.randomUUID()}`))
    const { value } = await browser.post('session')
    return { ...browser, accountId: value.accountId }
  }
  const state = async who => (await who.post('session')).value.entitlement.state
  const never = await signedIn(), trialing = await signedIn(), paying = await signedIn()
  await bare.db.prepare("INSERT INTO trials(account_id,enabled,expires_at,started_at,source) VALUES(?,1,?,?,'trial')")
    .bind(trialing.accountId, now + 5 * DAY, now - DAY).run()
  await bare.db.prepare(`INSERT INTO subscriptions(account_id,customer_id,subscription_id,status,plan,period_end,cancel_at_period_end,stripe_updated,updated_at)
    VALUES(?,'cus_fixture','sub_fixture','active','nexus_remote_monthly',?,0,1,1)`).bind(paying.accountId, now + 20 * DAY).run()
  assert.deepEqual([await state(never), await state(trialing), await state(paying)], ['none', 'active', 'active'])
  for (const who of [never, trialing, paying]) await run(ban, who.accountId)
  assert.deepEqual([await state(never), await state(trialing), await state(paying)], ['disabled', 'disabled', 'disabled'])
  // Banned, an account cannot pair either.
  assert.deepEqual((await never.post('pair/claim', { code: '0123456789abcdef' }, 403)).value, { error: 'trialDisabled' })
  // Run twice, it changes nothing more; an id that is no account writes nothing.
  await run(ban, trialing.accountId)
  await run(ban, crypto.randomUUID())
  const row = await bare.db.prepare('SELECT enabled, expires_at, started_at, source FROM trials WHERE account_id=?').bind(trialing.accountId).first()
  assert.deepEqual(row, { enabled: 0, expires_at: now + 5 * DAY, started_at: now - DAY, source: 'trial' }, 'the trial\'s own dates are kept')
  for (const who of [trialing, paying]) await run(lift, who.accountId)
  assert.deepEqual([await state(trialing), await state(paying)], ['active', 'active'])
})
