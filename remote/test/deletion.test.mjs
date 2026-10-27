// Deleting an account, and the copy of it the account can take first. A live subscription is ended through
// the billing Worker BEFORE anything is removed, and nothing is removed unless that Worker says it ended;
// with billing off there is nothing to cancel, unless the account still holds a live subscription, which
// nobody here can end. What a deletion keeps is what stops a second free trial: the account's address
// fingerprints and its trial, now ended, under an account no sign-in reaches; the trial markers; and the
// subscription record. Everything else of the account goes. The billing Worker is a stand-in that answers
// what each test says; no payment provider, key or network is used.
import { before, after, beforeEach, test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { runtime, roomStatus } from './runtime.mjs'

// A fixture, never a real key: the HMAC key trial markers are kept under.
const TRIAL_KEY = 'fixture-trial-key-not-a-secret'
const DAY = 86400000
const CONFIRM = { confirm: 'DELETE' }

let billed, plain
// What the stand-in billing Worker answers, and what it was asked, in order.
let answer = () => ({ cancelled: false })
const calls = []
before(async () => {
  billed = await runtime({ bindings: { TRIAL_KEY }, billing: async (method, args) => { calls.push({ method, args }); return answer(method, args) } })
  plain = await runtime({ bindings: { TRIAL_KEY } })
})
after(async () => { await Promise.all([billed, plain].map(app => app?.mf.dispose())) })
beforeEach(() => { calls.length = 0; answer = () => ({ cancelled: false }) })

const hex = bytes => Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
const random = () => hex(crypto.getRandomValues(new Uint8Array(32)))
const sha = value => createHash('sha256').update(value).digest('hex')

/** A person signed in on the page with a verified address. */
async function person(app, email = `op-${crypto.randomUUID()}@example.invalid`) {
  const subject = `auth0|${crypto.randomUUID()}`
  const jwt = await app.token(subject, { email, email_verified: true })
  const page = app.client(jwt)
  const { value } = await page.post('session')
  return { ...page, subject, email, jwt, accountId: value.accountId }
}
/** A station paired by code and approved at the shack, which starts the trial and spends a callsign and a PC. */
let calls_made = 0
const letters = n => [26 * 26, 26, 1].map(place => String.fromCharCode(65 + Math.floor(n / place) % 26)).join('')
async function station(app, owner, callsign = `DL1${letters(calls_made++)}`) {
  const desktop = app.client(), credential = random()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Shack' })
  await owner.post('pair/claim', { code: enrollment.code })
  const { response } = await owner.post('pair/confirm', { id: enrollment.id })
  owner.setCookie(response.headers.get('set-cookie'))
  await desktop.post('enroll/approve', { id: enrollment.id, proof: enrollment.proof, credential,
    callsign, pc: { hw: sha(`hw-${credential}`), os: sha(`os-${credential}`) } })
  return { stationId: enrollment.id, credential, shack: app.client(null, '', credential) }
}
const count = async (app, sql, ...values) => (await app.db.prepare(`SELECT COUNT(*) AS n FROM ${sql}`).bind(...values).first()).n
/** Every row the deleting account owns or is named by, table by table. */
async function held(app, accountId, stationId) {
  return {
    stations: await count(app, 'stations WHERE account_id=?', accountId),
    devices: await count(app, 'devices WHERE account_id=? OR station_id=?', accountId, stationId),
    invites: await count(app, 'invites WHERE station_id=?', stationId),
    enrollments: await count(app, 'enrollments WHERE account_id=?', accountId),
    tickets: await count(app, 'tickets WHERE station_id=?', stationId),
    challenges: await count(app, 'challenges WHERE account_id=?', accountId),
    notices: await count(app, 'notices WHERE account_id=?', accountId),
  }
}
const subscribe = (app, accountId, status = 'active') => app.db.prepare(`INSERT INTO subscriptions(account_id,customer_id,subscription_id,status,plan,
  period_end,cancel_at_period_end,stripe_updated,updated_at) VALUES(?,?,?,?,'nexus_remote_monthly',?,0,1,1)`)
  .bind(accountId, `cus_${accountId.slice(0, 8)}`, `sub_${accountId.slice(0, 8)}`, status, Date.now() + 20 * DAY).run()
/** An account with a station, a guest invited to it with a browser waiting there, a ticket, a challenge and
 *  a notice: one of each thing a deletion removes. */
async function busy(app) {
  const owner = await person(app), pair = await station(app, owner)
  const guest = await person(app)
  await pair.shack.post(`stations/${pair.stationId}/native/invite`, { email: guest.email, role: 'listen' })
  await guest.post(`stations/${pair.stationId}/device`, { name: 'Guest browser' })
  await app.db.prepare("INSERT INTO tickets(digest,station_id,account_id,device_id,device_generation,identity_until,expires_at) VALUES(?,?,?,?,1,?,?)")
    .bind(random(), pair.stationId, owner.accountId, crypto.randomUUID(), Date.now() + DAY, Date.now() + DAY).run()
  await app.db.prepare('INSERT INTO challenges(digest,account_id,expires_at) VALUES(?,?,?)').bind(random(), owner.accountId, Date.now() + DAY).run()
  await app.db.prepare('INSERT INTO notices(id,account_id,station_id,device_id,created_at) VALUES(?,?,?,?,?)')
    .bind(crypto.randomUUID(), owner.accountId, pair.stationId, crypto.randomUUID(), Date.now()).run()
  return { owner, guest, ...pair }
}
const NOTHING = { stations: 0, devices: 0, invites: 0, enrollments: 0, tickets: 0, challenges: 0, notices: 0 }

test('a live subscription is cancelled first, while everything is still there, and only then is the account deleted', async () => {
  const { owner, guest, stationId, credential, shack } = await busy(billed)
  await subscribe(billed, owner.accountId)
  // The shack is connected: deleting must cut it, not only remove its rows.
  const live = await shack.open(stationId)
  const rooms = await billed.mf.getDurableObjectNamespace('STATIONS'), room = () => roomStatus(rooms.get(rooms.idFromName(stationId)))
  assert.deepEqual({ ...(await room()), observers: 0, sockets: 0, runtimeSockets: 0 }, { enabled: true, online: true, observers: 0, sockets: 0, runtimeSockets: 0 })
  const before = await held(billed, owner.accountId, stationId)
  assert.deepEqual(before, { stations: 1, devices: 2, invites: 1, enrollments: 1, tickets: 1, challenges: 1, notices: 1 })
  let seen = null
  answer = async () => { seen = await held(billed, owner.accountId, stationId); return { cancelled: true } }

  assert.deepEqual((await owner.post('account/delete', CONFIRM)).value, { ok: true })
  // Asked once, for this account and nothing else, before a single row had gone.
  assert.deepEqual(calls, [{ method: 'cancel', args: [owner.accountId] }])
  assert.deepEqual(seen, before)
  assert.deepEqual(await held(billed, owner.accountId, stationId), NOTHING)

  // What is kept: the address fingerprints and the trial, ended now, under an account no sign-in reaches;
  // the trial markers; the subscription record, which the provider's last events still update.
  const kept = await billed.db.prepare('SELECT subject, email_hash, mailbox_hash, notice_address FROM accounts WHERE id=?').bind(owner.accountId).first()
  assert.equal(kept.subject, `deleted:${owner.accountId}`)
  assert.equal(kept.mailbox_hash, sha(`email:${owner.email}`))
  assert.equal(kept.notice_address, null)
  const trial = await billed.db.prepare('SELECT enabled, expires_at FROM trials WHERE account_id=?').bind(owner.accountId).first()
  assert.ok(trial.expires_at <= Date.now(), 'the trial ended with the account')
  assert.equal(await count(billed, 'trial_markers WHERE account_id=?', owner.accountId), 3)
  assert.equal(await count(billed, 'subscriptions WHERE account_id=?', owner.accountId), 1)
  // The guest keeps their own account; the station is gone, so their device there is too.
  assert.equal(await count(billed, 'accounts WHERE id=?', guest.accountId), 1)
  // The shack's credential opens nothing now.
  assert.deepEqual((await billed.client(null, '', credential).post(`stations/${stationId}/native/devices`, {}, 401)).value, { error: 'stationNotApproved' })
  // The room heard the station is off before its row went, and the shack's socket was closed.
  assert.deepEqual(await room(), { enabled: false, online: false, observers: 0, sockets: 0, runtimeSockets: 0 })
  for (let tries = 0; tries < 20 && live.ws.readyState === 1; tries++) await new Promise(resolve => setTimeout(resolve, 50))
  assert.notEqual(live.ws.readyState, 1)
  // Signing in again is a new, empty account, and the same person gets no second trial.
  const again = billed.client(owner.jwt)
  const { value: fresh } = await again.post('session')
  assert.notEqual(fresh.accountId, owner.accountId)
  assert.deepEqual(fresh.stations, [])
  const desktop = billed.client()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Shack again' })
  assert.deepEqual((await again.post('pair/claim', { code: enrollment.code }, 403)).value, { error: 'trialEnded' })
})

test('a cancel that fails deletes nothing, and says so by name', async () => {
  const failures = [
    () => ({ error: 'serviceUnavailable', status: 503 }),
    () => ({ error: 'invalidRequest', status: 400 }),
    () => { throw new Error('billing Worker down') },
    () => ({ cancelled: 'yes' }),
    () => ({ url: 'https://billing.stripe.com/p/session/fixture' }),
    () => null,
  ]
  for (const failure of failures) {
    const { owner, stationId } = await busy(billed)
    await subscribe(billed, owner.accountId)
    const before = await held(billed, owner.accountId, stationId)
    answer = failure
    assert.deepEqual((await owner.post('account/delete', CONFIRM, 503)).value, { error: 'subscriptionNotCancelled' }, String(failure))
    assert.deepEqual(await held(billed, owner.accountId, stationId), before, String(failure))
    const account = await billed.db.prepare('SELECT subject FROM accounts WHERE id=?').bind(owner.accountId).first()
    assert.equal(account.subject, owner.subject)
    // The station was not even switched off: the account is exactly as it was.
    assert.equal(await count(billed, 'stations WHERE id=? AND enabled=1', stationId), 1)
    assert.equal((await owner.post('session')).value.stations.length, 1)
  }
})

test('with billing off there is nothing to cancel, and the account is deleted', async () => {
  // An account that never had a trial or a plan leaves nothing behind at all.
  const empty = await person(plain)
  assert.deepEqual((await empty.post('account/delete', CONFIRM)).value, { ok: true })
  assert.equal(await count(plain, 'accounts WHERE id=?', empty.accountId), 0)
  assert.equal(await count(plain, 'accounts WHERE mailbox_hash=?', sha(`email:${empty.email}`)), 0)

  const { owner, stationId } = await busy(plain)
  // A subscription that has ended is no reason to wait.
  await subscribe(plain, owner.accountId, 'canceled')
  assert.deepEqual((await owner.post('account/delete', CONFIRM)).value, { ok: true })
  assert.deepEqual(await held(plain, owner.accountId, stationId), NOTHING)
  assert.equal((await plain.db.prepare('SELECT subject FROM accounts WHERE id=?').bind(owner.accountId).first()).subject, `deleted:${owner.accountId}`)
})

test('with billing off, a subscription that is still live cannot be ended here, so nothing is deleted', async () => {
  const { owner, stationId } = await busy(plain)
  await subscribe(plain, owner.accountId, 'past_due')
  const before = await held(plain, owner.accountId, stationId)
  assert.deepEqual((await owner.post('account/delete', CONFIRM, 503)).value, { error: 'subscriptionNotCancelled' })
  assert.deepEqual(await held(plain, owner.accountId, stationId), before)
})

test('deleting again is harmless: the second time finds nothing and changes nothing kept', async () => {
  const { owner, stationId } = await busy(billed)
  assert.deepEqual((await owner.post('account/delete', CONFIRM)).value, { ok: true })
  const kept = async () => ({
    accounts: (await billed.db.prepare('SELECT id, subject, email_hash, mailbox_hash FROM accounts WHERE mailbox_hash=? ORDER BY id').bind(sha(`email:${owner.email}`)).all()).results,
    trial: await billed.db.prepare('SELECT * FROM trials WHERE account_id=?').bind(owner.accountId).first(),
    markers: await count(billed, 'trial_markers WHERE account_id=?', owner.accountId),
  })
  const first = await kept()
  // The same sign-in again: the page's token still names the person, whose new, empty account goes entirely.
  assert.deepEqual((await owner.post('account/delete', CONFIRM)).value, { ok: true })
  assert.deepEqual(await kept(), first)
  assert.deepEqual(await held(billed, owner.accountId, stationId), NOTHING)
  assert.equal(first.accounts.length, 1)
})

test('a guest deleting their account leaves the station and its invite, and takes only their own devices there', async () => {
  const { owner, guest, stationId } = await busy(plain)
  assert.equal(await count(plain, 'devices WHERE account_id=?', guest.accountId), 1)
  assert.deepEqual((await guest.post('account/delete', CONFIRM)).value, { ok: true })
  assert.equal(await count(plain, 'devices WHERE account_id=?', guest.accountId), 0)
  assert.equal(await count(plain, 'devices WHERE station_id=?', stationId), 1)
  assert.equal(await count(plain, 'invites WHERE station_id=? AND removed_at IS NULL', stationId), 1)
  assert.equal((await owner.post('session')).value.stations.length, 1)
})

test('deletion needs the typed word and the page\'s own sign-in', async () => {
  const someone = await person(plain)
  for (const value of [{}, { confirm: 'delete' }, { confirm: 'DELETE', accountId: crypto.randomUUID() }]) {
    assert.deepEqual((await someone.post('account/delete', value, 400)).value, { error: 'invalidRequest' }, JSON.stringify(value))
  }
  assert.deepEqual((await plain.client().post('account/delete', CONFIRM, 403)).value, { error: 'originDenied' })
  assert.equal(await count(plain, 'accounts WHERE id=?', someone.accountId), 1)
})

test('a copy of the account: its trial, plan, markers, stations, devices and invitations, and no credential', async () => {
  const { owner, guest, stationId } = await busy(billed)
  await subscribe(billed, owner.accountId)
  const { value } = await owner.post('account/export', {})
  assert.equal(value.accountId, owner.accountId)
  assert.equal(value.trial.source, 'trial')
  assert.equal(typeof value.trial.startedAt, 'number')
  assert.deepEqual({ ...value.subscription, periodEnd: 0 }, { status: 'active', plan: 'nexus_remote_monthly', periodEnd: 0, cancelAtPeriodEnd: false,
    firstRefundAt: null, customerId: `cus_${owner.accountId.slice(0, 8)}`, subscriptionId: `sub_${owner.accountId.slice(0, 8)}` })
  assert.deepEqual(value.markers.map(m => m.kind).sort(), ['call', 'pc-hw', 'pc-os'])
  assert.deepEqual(value.stations, [{ id: stationId, name: 'Shack', enabled: true }])
  assert.deepEqual(value.devices.map(d => [d.name, d.stationId, d.approved]).sort(),
    [['Guest browser', stationId, false], ['Paired browser', stationId, true]])
  assert.deepEqual(value.invitationsMade.map(i => [i.stationId, i.role]), [[stationId, 'listen']])
  assert.deepEqual(value.invitationsReceived, [])
  assert.equal(value.checkouts.length, 0)
  // The guest's copy names the invitation they hold, not the owner's devices.
  const { value: theirs } = await guest.post('account/export', {})
  assert.deepEqual(theirs.invitationsReceived.map(i => [i.stationId, i.role]), [[stationId, 'listen']])
  assert.deepEqual(theirs.devices.map(d => d.name), ['Guest browser'])
  // No hash, proof, key or credential of any kind leaves the service in a copy.
  const text = JSON.stringify(value)
  for (const row of (await billed.db.prepare('SELECT credential_hash, proof_hash FROM stations s JOIN enrollments e ON e.id=s.id WHERE s.id=?').bind(stationId).all()).results) {
    assert.ok(!text.includes(row.credential_hash) && !text.includes(row.proof_hash))
  }
  assert.ok(!text.includes(sha(`email:${owner.email}`)), 'no address fingerprint')
  assert.doesNotMatch(text, /value_hmac|credential|proof|public_?key|lan_?key|subject|mailbox|email/i)
  // Behind the page's sign-in, as everything of the account is.
  assert.deepEqual((await billed.client().post('account/export', {}, 403)).value, { error: 'originDenied' })
})
