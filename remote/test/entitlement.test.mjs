import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash, createHmac } from 'node:crypto'
import { runtime } from './runtime.mjs'
import { ulsRows, ulsSql } from '../scripts/uls-import.mjs'

// A fixture, never a real key: the HMAC key trial markers and the licence copy are kept under.
const TRIAL_KEY = 'fixture-trial-key-not-a-secret'
const DAY = 86400000

let plain, keyed
before(async () => {
  plain = await runtime()
  keyed = await runtime({ bindings: { TRIAL_KEY } })
})
after(async () => { await Promise.all([plain, keyed].map(app => app?.mf.dispose())) })

const hex = bytes => Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
const credential = () => hex(crypto.getRandomValues(new Uint8Array(32)))
const sha = value => createHash('sha256').update(value).digest('hex')
const hmac = value => createHmac('sha256', TRIAL_KEY).update(value).digest('hex')

/** A signed-in account with a verified address, and no trial. */
async function person(app, email = `op-${crypto.randomUUID()}@example.invalid`) {
  const browser = app.client(await app.token(`auth0|${crypto.randomUUID()}`, { email, email_verified: true }))
  const { value } = await browser.post('session')
  return { ...browser, accountId: value.accountId }
}
/** Enroll a station, claim and confirm it from `owner`, and approve it at the shack with `extra`. */
async function approve(app, owner, extra = {}, expected = 200) {
  const desktop = app.client()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Shack' })
  await owner.post('pair/claim', { code: enrollment.code })
  await owner.post('pair/confirm', { id: enrollment.id })
  const { value } = await desktop.post('enroll/approve', { id: enrollment.id, proof: enrollment.proof, credential: credential(), ...extra }, expected)
  return { value, stationId: enrollment.id }
}
const session = async owner => (await owner.post('session')).value.entitlement
/** `approve`, keeping the cookie the confirming browser is given: it is approved with the pairing. */
async function approveHere(app, owner) {
  const desktop = app.client()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Shack' })
  await owner.post('pair/claim', { code: enrollment.code })
  const { response } = await owner.post('pair/confirm', { id: enrollment.id })
  owner.setCookie(response.headers.get('set-cookie'))
  await desktop.post('enroll/approve', { id: enrollment.id, proof: enrollment.proof, credential: credential() })
  return enrollment.id
}
const trialRow = (app, accountId) => app.db.prepare('SELECT * FROM trials WHERE account_id=?').bind(accountId).first()
const markers = (app, accountId) => app.db.prepare('SELECT kind,value_hmac,proven,method FROM trial_markers WHERE account_id=? ORDER BY kind')
  .bind(accountId).all().then(r => r.results)
async function subscribe(app, accountId, status, periodEnd, extra = {}) {
  await app.db.prepare(`INSERT INTO subscriptions(account_id,customer_id,subscription_id,status,plan,period_end,cancel_at_period_end,stripe_updated,updated_at)
    VALUES(?,?,?,?,'nexus_remote_monthly',?,?,1,1)`)
    .bind(accountId, `cus_${crypto.randomUUID().slice(0, 8)}`, `sub_${crypto.randomUUID().slice(0, 8)}`, status, periodEnd, extra.cancel ? 1 : 0).run()
}
async function licences(app, rows, syncedAt = Date.now(), mode = 'daily') {
  const sql = ulsSql(rows, TRIAL_KEY, mode, '2026-10-16', syncedAt)
  for (const statement of sql.split(';\n').map(s => s.trim()).filter(Boolean)) await app.db.prepare(statement).run()
}
const pc = (hw, os) => ({ hw: hw === null ? null : sha(`hw-${hw}`), os: os === null ? null : sha(`os-${os}`) })

// ---------------------------------------------------------------------------------------------------
// ONE Entitlement: the trial and the plan read together, by value at every state.

test('the entitlement is read from the trial and the plan together, at every state', async () => {
  const now = Date.now()
  const cases = [
    { name: 'none', setup: async () => {}, state: 'none', access: 'lan', enabled: false, kind: null },
    { name: 'trial running', setup: async a => plain.db.prepare("INSERT INTO trials(account_id,enabled,expires_at,started_at,source) VALUES(?,1,?,?,'trial')").bind(a, now + 5 * DAY, now).run(),
      state: 'active', access: 'internet', enabled: true, kind: 'trial', until: now + 5 * DAY },
    { name: 'trial ended', setup: async a => plain.db.prepare("INSERT INTO trials(account_id,enabled,expires_at,started_at,source) VALUES(?,1,?,?,'trial')").bind(a, now - DAY, now - 15 * DAY).run(),
      state: 'ended', access: 'lan', enabled: true, kind: 'trial', until: now - DAY },
    { name: 'paid, in the period', setup: a => subscribe(plain, a, 'active', now + 20 * DAY),
      state: 'active', access: 'internet', enabled: true, kind: 'subscription', until: now + 21 * DAY, renews: true },
    { name: 'paid, period ended 12 h ago (one-day grace)', setup: a => subscribe(plain, a, 'active', now - DAY / 2),
      state: 'active', access: 'internet', enabled: true, kind: 'subscription', until: now + DAY / 2 },
    { name: 'paid, period ended 2 days ago', setup: a => subscribe(plain, a, 'active', now - 2 * DAY),
      state: 'ended', access: 'lan', enabled: true, kind: 'subscription', until: now - DAY },
    { name: 'past due 6 days (seven-day grace)', setup: a => subscribe(plain, a, 'past_due', now - 6 * DAY),
      state: 'active', access: 'internet', enabled: true, kind: 'subscription', until: now + DAY },
    { name: 'past due 8 days', setup: a => subscribe(plain, a, 'past_due', now - 8 * DAY),
      state: 'ended', access: 'lan', enabled: true, kind: 'subscription', until: now - DAY },
    { name: 'canceled with time left on the period', setup: a => subscribe(plain, a, 'canceled', now + 10 * DAY),
      state: 'ended', access: 'lan', enabled: false, kind: 'subscription', until: 0 },
    { name: 'unpaid', setup: a => subscribe(plain, a, 'unpaid', now + 10 * DAY),
      state: 'ended', access: 'lan', enabled: false, kind: 'subscription', until: 0 },
    { name: 'canceling at period end still runs to it', setup: a => subscribe(plain, a, 'active', now + 3 * DAY, { cancel: true }),
      state: 'active', access: 'internet', enabled: true, kind: 'subscription', until: now + 4 * DAY, renews: false },
    // Paying during the trial keeps its days: the plan's first period ends where the trial does.
    { name: 'paid during the trial', setup: async a => {
      await plain.db.prepare("INSERT INTO trials(account_id,enabled,expires_at,started_at,source) VALUES(?,1,?,?,'trial')").bind(a, now + 9 * DAY, now - 5 * DAY).run()
      await subscribe(plain, a, 'trialing', now + 9 * DAY)
    }, state: 'active', access: 'internet', enabled: true, kind: 'subscription', until: now + 10 * DAY, renews: true },
    // An operator's ban outranks a plan, and refuses the LAN too.
    { name: 'disabled, though paid', setup: async a => {
      await plain.db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,0,?)').bind(a, now + 5 * DAY).run()
      await subscribe(plain, a, 'active', now + 20 * DAY)
    }, state: 'disabled', access: 'none', enabled: false, kind: 'subscription', until: now + 5 * DAY },
  ]
  for (const c of cases) {
    const owner = await person(plain)
    await c.setup(owner.accountId)
    const e = await session(owner)
    assert.equal(e.state, c.state, `${c.name}: state`)
    assert.equal(e.access, c.access, `${c.name}: access`)
    assert.equal(e.enabled, c.enabled, `${c.name}: enabled`)
    assert.equal(e.plan.kind, c.kind, `${c.name}: plan`)
    if ('until' in c) assert.equal(e.expiresAt, c.until, `${c.name}: until`)
    if ('renews' in c) assert.equal(e.plan.renews, c.renews, `${c.name}: renews`)
  }
})

test('a subscriber whose trial has ended still passes every internet gate, and loses them when the plan ends', async () => {
  const pair = await plain.paired()
  // Registering a browser is free (enrolling is), so the gate is the ticket an approved browser takes.
  await plain.approved(pair)
  await plain.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() - DAY, pair.browser.accountId).run()
  const path = `stations/${pair.stationId}/ticket`
  const { value: refused } = await pair.browser.post(path, {}, 403)
  assert.equal(refused.error, 'trialRequired', 'positive control: an ended trial alone is refused')
  await subscribe(plain, pair.browser.accountId, 'active', Date.now() + 30 * DAY)
  await pair.browser.post(path, {})
  await plain.db.prepare("UPDATE subscriptions SET status='canceled' WHERE account_id=?").bind(pair.browser.accountId).run()
  assert.equal((await pair.browser.post(path, {}, 403)).value.error, 'trialRequired')
})

test('a subscriber pairs whatever a sibling sign-in spent, and paying starts no trial clock', async () => {
  const address = `op-${crypto.randomUUID()}@example.invalid`
  const first = await person(plain, address)
  await approve(plain, first)
  assert.equal((await trialRow(plain, first.accountId)).source, 'trial')
  const second = await person(plain, address)
  const desktop = plain.client()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Second shack' })
  assert.equal((await second.post('pair/claim', { code: enrollment.code }, 403)).value.error, 'trialActiveElsewhere',
    'positive control: the same person without a plan is refused')
  await subscribe(plain, second.accountId, 'active', Date.now() + 30 * DAY)
  await approve(plain, second)
  assert.equal(await trialRow(plain, second.accountId), null, 'no trial row: paying never touches trials')
  assert.equal((await session(second)).state, 'active')
  // And a subscriber with no sibling at all: still no trial clock.
  const payer = await person(plain)
  await subscribe(plain, payer.accountId, 'active', Date.now() + 30 * DAY)
  await approve(plain, payer)
  assert.equal(await trialRow(plain, payer.accountId), null)
})

// ---------------------------------------------------------------------------------------------------
// FREE DURING THE BETA: REMOTE_BETA_FREE='1' takes away the trial's end, and nothing else.

test('with the beta on, an account past its fourteen days keeps internet use; off, it ends as today', async () => {
  const beta = await runtime({ bindings: { REMOTE_BETA_FREE: '1' } })
  try {
    for (const [app, on] of [[beta, true], [plain, false]]) {
      const owner = await person(app)
      const stationId = await approveHere(app, owner)
      const started = await trialRow(app, owner.accountId)
      assert.equal(started.source, 'trial', `${on}: approval still starts the one trial`)
      assert.equal(started.expires_at - started.started_at, 14 * DAY, `${on}: the row keeps its fourteen days`)
      // Thirty days on: the trial row says it ended sixteen days ago.
      await app.db.prepare('UPDATE trials SET started_at=?, expires_at=? WHERE account_id=?')
        .bind(Date.now() - 30 * DAY, Date.now() - 16 * DAY, owner.accountId).run()
      const { value } = await owner.post('session')
      assert.equal(value.beta, on, `${on}: the page is told`)
      if (on) {
        assert.equal(value.entitlement.state, 'active')
        assert.equal(value.entitlement.access, 'internet')
        assert.equal(value.entitlement.plan.kind, 'trial')
        assert.ok(value.entitlement.expiresAt > Date.now(), 'a date ahead, so every session deadline still has one')
        // The paired browser is approved with the pairing, so the ticket is the internet gate itself.
        assert.equal(typeof (await owner.post(`stations/${stationId}/ticket`, {})).value.ticket, 'string')
      } else {
        assert.equal(value.entitlement.state, 'ended')
        assert.equal(value.entitlement.access, 'lan')
        assert.equal((await owner.post(`stations/${stationId}/ticket`, {}, 403)).value.error, 'trialRequired')
      }
    }
  } finally { await beta.mf.dispose() }
})

test('the beta removes the expiry only: a ban, the one-trial-per-person rule and deletion still refuse', async () => {
  const beta = await runtime({ bindings: { REMOTE_BETA_FREE: '1' } })
  try {
    // An operator's ban outranks the beta, on the internet and the LAN alike.
    const banned = await person(beta)
    const stationId = await approveHere(beta, banned)
    assert.equal(typeof (await banned.post(`stations/${stationId}/ticket`, {})).value.ticket, 'string', 'positive control: before the ban')
    await beta.db.prepare('UPDATE trials SET enabled=0 WHERE account_id=?').bind(banned.accountId).run()
    const ban = await session(banned)
    assert.deepEqual([ban.state, ban.access, ban.enabled], ['disabled', 'none', false])
    assert.equal((await banned.post(`stations/${stationId}/ticket`, {}, 403)).value.error, 'trialRequired')
    assert.equal((await banned.post(`stations/${stationId}/ticket`, { scope: 'lan' }, 403)).value.error, 'trialRequired')

    // One trial per person: a second sign-in on the same mailbox is refused, and told the first is
    // still running (it is, with no end), where today it would hear that the trial ended.
    for (const [app, code] of [[beta, 'trialActiveElsewhere'], [plain, 'trialEnded']]) {
      const address = `op-${crypto.randomUUID()}@example.invalid`
      const first = await person(app, address)
      await approve(app, first)
      await app.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() - DAY, first.accountId).run()
      const second = await person(app, address)
      const { value: enrollment } = await app.client().post('enroll', { name: 'Second shack' })
      assert.equal((await second.post('pair/claim', { code: enrollment.code }, 403)).value.error, code)
      assert.equal(await trialRow(app, second.accountId), null)
    }

    // Deletion: the account's stations, devices and sign-in go, and signing in again makes a new account
    // that the same mailbox cannot start a second trial on. Its deleted sibling's trial is not "running".
    const address = `op-${crypto.randomUUID()}@example.invalid`, subject = `auth0|${crypto.randomUUID()}`
    const signIn = async () => {
      const browser = beta.client(await beta.token(subject, { email: address, email_verified: true }))
      return { ...browser, accountId: (await browser.post('session')).value.accountId }
    }
    const gone = await signIn()
    const stationId2 = await approveHere(beta, gone)
    await beta.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() - DAY, gone.accountId).run()
    assert.equal(typeof (await gone.post(`stations/${stationId2}/ticket`, {})).value.ticket, 'string', 'positive control: past its days, in use')
    assert.deepEqual((await gone.post('account/delete', { confirm: 'DELETE' })).value, { ok: true })
    assert.equal((await gone.post(`stations/${stationId2}/ticket`, {}, 404)).value.error, 'stationUnavailable')
    const again = await signIn()
    assert.notEqual(again.accountId, gone.accountId)
    assert.equal((await session(again)).state, 'none')
    const { value: enrollment } = await beta.client().post('enroll', { name: 'Shack again' })
    assert.equal((await again.post('pair/claim', { code: enrollment.code }, 403)).value.error, 'trialEnded')
  } finally { await beta.mf.dispose() }
})

// ---------------------------------------------------------------------------------------------------
// Trial identity: callsign and shack PC.

test('approve accepts callsign and pc before any Nexus sends them, and still refuses an unknown field', async () => {
  const owner = await person(plain)
  const { value } = await approve(plain, owner, { callsign: 'DL1ABC', pc: pc('a', 'a') })
  assert.equal(value.entitlement.state, 'active', 'with no TRIAL_KEY the fields are accepted and a trial starts')
  assert.deepEqual(await markers(plain, owner.accountId), [], 'and no marker is written without the key')
  assert.equal((await approve(plain, await person(plain), { callsign: 'DL1ABC', station: 'x' }, 400)).value.error, 'invalidRequest')
  assert.equal((await approve(plain, await person(plain), { callsign: 'W1 ABC' }, 400)).value.error, 'callsignInvalid')
  assert.equal((await approve(plain, await person(plain), { callsign: '' }, 400)).value.error, 'callsignInvalid')
  assert.equal((await approve(plain, await person(plain), { pc: { hw: 'nothex' , os: null } }, 400)).value.error, 'invalidRequest')
})

test('one trial per callsign: a second account claiming the same call gets no trial, and portable forms are the same call', async () => {
  const first = await person(keyed)
  await approve(keyed, first, { callsign: 'dl1abc/p', pc: pc('c1', 'c1') })
  assert.deepEqual((await markers(keyed, first.accountId)).map(m => [m.kind, m.value_hmac, m.proven]),
    [['call', hmac('call:DL1ABC'), 0], ['pc-hw', hmac(`pc:hw:${sha('hw-c1')}`), 0], ['pc-os', hmac(`pc:os:${sha('os-c1')}`), 0]],
    'the markers are keyed hashes of the base call and the PC hashes, never the clear values')
  const rows = JSON.stringify((await keyed.db.prepare('SELECT * FROM trial_markers').all()).results)
  assert.ok(!rows.includes('DL1ABC') && !rows.includes(sha('hw-c1')), 'neither clear value reaches D1')

  const second = await person(keyed)
  const { value: refused, stationId } = await approve(keyed, second, { callsign: 'DL/DL1ABC', pc: pc('c2', 'c2') }, 403)
  assert.equal(refused.error, 'trialUsedByCallsign')
  assert.equal(await trialRow(keyed, second.accountId), null)
  assert.equal(await keyed.db.prepare('SELECT id FROM stations WHERE id=?').bind(stationId).first(), null, 'refused before the station row')
  // Positive control: a different call on the same terms starts a trial.
  await approve(keyed, second, { callsign: 'DL2XYZ', pc: pc('c2', 'c2') })
  assert.equal((await trialRow(keyed, second.accountId)).source, 'trial')
})

test('one trial per shack PC: either identifier matching refuses, and a null one matches nothing', async () => {
  const first = await person(keyed)
  await approve(keyed, first, { callsign: 'G4AAA', pc: pc('p1', 'p1') })
  for (const [label, sent] of [['hardware', pc('p1', 'other1')], ['installation', pc('other2', 'p1')]]) {
    const { value } = await approve(keyed, await person(keyed), { callsign: `G4${label === 'hardware' ? 'BBB' : 'CCC'}`, pc: sent }, 403)
    assert.equal(value.error, 'trialUsedOnThisPc', `${label} id`)
  }
  const ok = await person(keyed)
  await approve(keyed, ok, { callsign: 'G4DDD', pc: pc(null, 'p2') })
  assert.deepEqual((await markers(keyed, ok.accountId)).map(m => m.kind), ['call', 'pc-os'])
})

test('US calls are checked against the licence copy, fail closed when it is stale, and a matching mailbox proves the call', async () => {
  const fresh = await runtime({ bindings: { TRIAL_KEY } })
  try {
    const owner = await person(fresh, 'Real.Owner+remote@gmail.com')
    assert.equal((await approve(fresh, await person(fresh), { callsign: 'W1XYZ', pc: pc(null, 'u9') }, 503)).value.error, 'serviceUnavailable',
      'no copy yet: refused, not waved through')
    await licences(fresh, [{ call: 'W1XYZ', active: true, email: 'realowner@googlemail.com' }, { call: 'K2OLD', active: false, email: null }], Date.now() - 4 * DAY)
    assert.equal((await approve(fresh, await person(fresh), { callsign: 'W1XYZ', pc: pc(null, 'u9') }, 503)).value.error, 'serviceUnavailable', 'four days stale')
    await licences(fresh, [], Date.now())
    assert.equal((await approve(fresh, await person(fresh), { callsign: 'K2OLD', pc: pc(null, 'u9') }, 403)).value.error, 'callsignNotLicensed', 'expired licence')
    assert.equal((await approve(fresh, await person(fresh), { callsign: 'N3NONE', pc: pc(null, 'u9') }, 403)).value.error, 'callsignNotLicensed', 'unknown call')

    // A squatter takes the call first, unproven (their mailbox is not the licence's).
    const squatter = await person(fresh)
    await approve(fresh, squatter, { callsign: 'W1XYZ', pc: pc(null, 'u1') })
    assert.equal((await markers(fresh, squatter.accountId))[0].proven, 0)
    // The owner's verified mailbox is the licence's (gmail dots, +tag and googlemail folded): proven, and not blocked.
    await approve(fresh, owner, { callsign: 'W1XYZ', pc: pc(null, 'u0') })
    assert.deepEqual((await markers(fresh, owner.accountId)).filter(m => m.kind === 'call').map(m => [m.proven, m.method]), [[1, 'uls-email']])
    // Now the call is proven, a third unproven claimant is refused.
    assert.equal((await approve(fresh, await person(fresh), { callsign: 'W1XYZ', pc: pc(null, 'u3') }, 403)).value.error, 'trialUsedByCallsign')
  } finally { await fresh.mf.dispose() }
})

test('the licence import keeps what any active licence says, and the latest file wins', () => {
  const en = ['EN|1|||W9AAA|L||||||||||op@example.invalid', 'EN|2|||W9AAA|L||||||||||new@example.invalid'].join('\n')
  const hd = ['HD|1|||W9AAA|E', 'HD|2|||W9AAA|A', 'HD|3|||not a call|A', 'HD|4|||K9BBB|C'].join('\r\n')
  assert.deepEqual(ulsRows(en, hd), [{ call: 'W9AAA', active: true, email: 'new@example.invalid' }, { call: 'K9BBB', active: false, email: null }])
  assert.throws(() => ulsSql([], '', 'full', '2026-10-11', 1), /TRIAL_KEY/)
  assert.throws(() => ulsSql([], 'k', 'weekly', '2026-10-11', 1), /--mode/)
  const sql = ulsSql([{ call: 'W9AAA', active: true, email: 'new@example.invalid' }], 'k', 'full', '2026-10-11', 7)
  assert.ok(!sql.includes('W9AAA') && !sql.includes('new@example'), 'no clear value in the SQL')
  assert.match(sql, /DELETE FROM uls_calls WHERE refreshed_at<7;/)
})

test('the licence import takes the email only from the active licence, never an expired or cancelled one', () => {
  const en = ['EN|1|||W9AAA|L||||||||||old@example.invalid', 'EN|3|||K9BBB|L||||||||||gone@example.invalid',
    'EN|5|||N9CCC|L||||||||||was@example.invalid'].join('\n')
  // W9AAA and N9CCC: an earlier licence (either side of the active one) carries an email, the active one none.
  const hd = ['HD|1|||W9AAA|E', 'HD|2|||W9AAA|A', 'HD|3|||K9BBB|C', 'HD|4|||N9CCC|A', 'HD|5|||N9CCC|T'].join('\n')
  assert.deepEqual(ulsRows(en, hd), [{ call: 'W9AAA', active: true, email: null }, { call: 'K9BBB', active: false, email: null },
    { call: 'N9CCC', active: true, email: null }])
})

test('a call that changed hands or lost its email is no longer proven by the old email, daily or weekly', async () => {
  const app = await runtime({ bindings: { TRIAL_KEY } })
  try {
    const stored = async call => (await app.db.prepare('SELECT email_hmac FROM uls_calls WHERE call_hmac=?').bind(hmac(`call:${call}`)).first()).email_hmac
    const mail = address => hmac(`mailbox:${sha(`email:${address}`)}`)
    const now = Date.now()
    await licences(app, [{ call: 'W1OLD', active: true, email: 'old@example.invalid' }, { call: 'W2LOST', active: true, email: 'lost@example.invalid' },
      { call: 'W3WEEK', active: true, email: 'week@example.invalid' }], now - 2000)
    assert.equal(await stored('W1OLD'), mail('old@example.invalid'))
    assert.equal(await stored('W3WEEK'), mail('week@example.invalid'))

    // The daily file: W1OLD's old licence is cancelled and a new holder (no email) is granted; W2LOST's licence drops its email.
    const en = ['EN|1|||W1OLD|L||||||||||old@example.invalid', 'EN|2|||W1OLD|L||||||||||', 'EN|3|||W2LOST|L||||||||||'].join('\n')
    const hd = ['HD|1|||W1OLD|C', 'HD|2|||W1OLD|A', 'HD|3|||W2LOST|A'].join('\n')
    await licences(app, ulsRows(en, hd), now - 1000)
    assert.equal(await stored('W1OLD'), null, 'the previous holder\'s email no longer proves the reassigned call')
    assert.equal(await stored('W2LOST'), null, 'an active licence without an email clears the proof')
    const previous = await person(app, 'old@example.invalid')
    await approve(app, previous, { callsign: 'W1OLD', pc: pc(null, 'r1') })
    assert.deepEqual((await markers(app, previous.accountId)).filter(m => m.kind === 'call').map(m => m.proven), [0])

    // The weekly file replaces too: W3WEEK has lost its email, and W1OLD's new holder now has one.
    await licences(app, [{ call: 'W1OLD', active: true, email: 'new@example.invalid' }, { call: 'W3WEEK', active: true, email: null }], now, 'full')
    assert.equal(await stored('W3WEEK'), null, 'the weekly file clears an email it no longer has')
    assert.equal(await stored('W1OLD'), mail('new@example.invalid'))
    const holder = await person(app, 'new@example.invalid')
    await approve(app, holder, { callsign: 'W1OLD', pc: pc(null, 'r2') })
    assert.deepEqual((await markers(app, holder.accountId)).filter(m => m.kind === 'call').map(m => m.proven), [1], 'positive control: the current email proves it')
  } finally { await app.mf.dispose() }
})

test('the new identity is required once switched on, and refuses rather than checks nothing without a key', async () => {
  const strict = await runtime({ bindings: { TRIAL_KEY, TRIAL_IDENTITY_REQUIRED: '1' } })
  const unkeyed = await runtime({ bindings: { TRIAL_IDENTITY_REQUIRED: '1' } })
  try {
    assert.equal((await approve(strict, await person(strict), {}, 403)).value.error, 'updateRequired')
    assert.equal((await approve(strict, await person(strict), { callsign: 'F4AAA' }, 403)).value.error, 'updateRequired')
    await approve(strict, await person(strict), { callsign: 'F4AAA', pc: pc(null, 's1') })
    assert.equal((await approve(unkeyed, await person(unkeyed), { callsign: 'F4BBB', pc: pc(null, 's2') }, 503)).value.error, 'serviceNotConfigured')
    // Positive control: an account that already holds a trial pairs another station with no identity.
    const pair = await strict.paired()
    assert.ok(pair.stationId)
  } finally { await Promise.all([strict.mf.dispose(), unkeyed.mf.dispose()]) }
})

test('markers are spent only by an approval that started the trial', async () => {
  const owner = await person(keyed)
  await approve(keyed, owner, { callsign: 'EA1AAA', pc: pc(null, 'm1') })
  await approve(keyed, owner, { callsign: 'EA1ZZZ', pc: pc(null, 'm2') })
  assert.deepEqual((await markers(keyed, owner.accountId)).map(m => m.value_hmac),
    [hmac('call:EA1AAA'), hmac(`pc:os:${sha('os-m1')}`)], 'the second station spent nothing')
  // Neither of its values is spent, so another account may use them.
  await approve(keyed, await person(keyed), { callsign: 'EA1ZZZ', pc: pc(null, 'm2') })
})

// Racing, as the mailbox race in service.test.mjs is raced: its own cold runtime and ten attempts,
// because a warm instance stops overlapping the two approvals and a single race passes on broken code.
test('two approvals racing with the same callsign start one trial', { timeout: 300000 }, async () => {
  const service = await runtime({ bindings: { TRIAL_KEY } })
  try {
    const armed = async owner => {
      const desktop = service.client()
      const { value: enrollment } = await desktop.post('enroll', { name: 'Shack' })
      await owner.post('pair/claim', { code: enrollment.code })
      await owner.post('pair/confirm', { id: enrollment.id })
      return { owner, desktop, enrollment }
    }
    const race = (ready, call) => Promise.all(ready.map(async ({ desktop, enrollment }) => {
      const response = await service.mf.dispatchFetch(`${service.origin}/api/remote/enroll/approve`, { method: 'POST', headers: desktop.headers(),
        body: JSON.stringify({ id: enrollment.id, proof: enrollment.proof, credential: credential(), callsign: call, pc: pc(null, crypto.randomUUID()) }) })
      return { status: response.status, error: (await response.json()).error }
    }))
    for (let attempt = 0; attempt < 10; attempt++) {
      const owners = [await person(service), await person(service)]
      const statuses = await race(await Promise.all(owners.map(armed)), `VK${attempt}RACE`)
      const trials = (await Promise.all(owners.map(o => trialRow(service, o.accountId)))).filter(Boolean)
      assert.equal(trials.length, 1, `attempt ${attempt}: exactly one trial`)
      assert.deepEqual(statuses.map(s => s.status).sort(), [200, 403], `attempt ${attempt}`)
      assert.equal(statuses.find(s => s.status === 403).error, 'trialUsedByCallsign', 'the loser is told why')
    }
    // Positive control: two different calls racing both start.
    const owners = [await person(service), await person(service)]
    const ready = await Promise.all(owners.map(armed))
    const statuses = await Promise.all(ready.map(async ({ desktop, enrollment }, i) => (await service.mf.dispatchFetch(`${service.origin}/api/remote/enroll/approve`,
      { method: 'POST', headers: desktop.headers(), body: JSON.stringify({ id: enrollment.id, proof: enrollment.proof, credential: credential(),
        callsign: `VK${i}OTH`, pc: pc(null, crypto.randomUUID()) }) })).status))
    assert.deepEqual(statuses, [200, 200])
  } finally { await service.mf.dispose() }
})

test('support records the proven owner of a squatted callsign with the trial it grants', async () => {
  const admin = await runtime({ bindings: { TRIAL_KEY, ADMIN_SUBJECT: 'auth0|operator' } })
  const noKey = await runtime({ bindings: { ADMIN_SUBJECT: 'auth0|operator' } })
  try {
    const operator = admin.client(await admin.token('auth0|operator'))
    await operator.post('session')
    const squatter = await person(admin)
    await approve(admin, squatter, { callsign: 'JA1OWN', pc: pc(null, 'q1') })
    const owner = await person(admin)
    assert.equal((await approve(admin, owner, { callsign: 'JA1OWN', pc: pc(null, 'q2') }, 403)).value.error, 'trialUsedByCallsign')
    await operator.post('admin/grant-trial', { accountId: owner.accountId, days: 14, callsign: 'ja1own' })
    assert.deepEqual(await markers(admin, owner.accountId), [{ kind: 'call', value_hmac: hmac('call:JA1OWN'), proven: 1, method: 'admin' }])
    assert.equal((await session(owner)).state, 'active')
    assert.equal((await operator.post('admin/grant-trial', { accountId: owner.accountId, days: 14, callsign: 'not a call' }, 400)).value.error, 'callsignInvalid')
    const noKeyOperator = noKey.client(await noKey.token('auth0|operator'))
    await noKeyOperator.post('session')
    const target = await person(noKey)
    assert.equal((await noKeyOperator.post('admin/grant-trial', { accountId: target.accountId, days: 14, callsign: 'JA1OWN' }, 503)).value.error, 'serviceNotConfigured')
  } finally { await Promise.all([admin.mf.dispose(), noKey.mf.dispose()]) }
})

// ---------------------------------------------------------------------------------------------------
// The relay reads `subscriptions` and never writes it: the payment provider's webhook is not served here.

test('a plan that is not one of ours pays for nothing, at every status that pays', async () => {
  const periodEnd = Date.now() + 30 * DAY
  const planned = async (status, plan) => {
    const owner = await person(plain)
    await subscribe(plain, owner.accountId, status, periodEnd)
    await plain.db.prepare('UPDATE subscriptions SET plan=? WHERE account_id=?').bind(plan, owner.accountId).run()
    return session(owner)
  }
  for (const status of ['active', 'trialing', 'past_due']) {
    const e = await planned(status, null)
    assert.deepEqual([e.enabled, e.state, e.access, e.expiresAt, e.plan.renews], [false, 'ended', 'lan', 0, false], status)
  }
  // Positive control: either of our plans entitles.
  for (const plan of ['nexus_remote_monthly', 'nexus_remote_annual']) {
    const e = await planned('active', plan)
    assert.deepEqual([e.enabled, e.state, e.access, e.plan.renews], [true, 'active', 'internet', true], plan)
  }
})


test('the relay serves no payment webhook, even with payment keys configured', async () => {
  const secret = 'whsec_fixture_not_a_secret'
  const app = await runtime({ bindings: { STRIPE_KEY: 'rk_test_fixture', STRIPE_WEBHOOK_SECRET: secret } })
  try {
    const owner = await person(app)
    const payload = JSON.stringify({ id: 'evt_fixture', type: 'customer.subscription.updated', data: { object: { id: 'sub_fixture' } } })
    const at = Math.floor(Date.now() / 1000)
    const signature = createHmac('sha256', secret).update(`${at}.${payload}`).digest('hex')
    const delivered = async path => {
      const response = await app.mf.dispatchFetch(`${app.origin}/api/remote/${path}`, { method: 'POST',
        headers: { 'content-type': 'application/json; charset=utf-8', 'stripe-signature': `t=${at},v1=${signature}` }, body: payload })
      return { status: response.status, value: await response.json() }
    }
    // A signed delivery is answered exactly as at a path the relay never had: refused before anything
    // is read, because every path the relay does not route before sign-in needs a signed-in browser.
    assert.deepEqual(await delivered('billing/webhook'), { status: 403, value: { error: 'originDenied' } })
    assert.deepEqual(await delivered('billing/webhook'), await delivered('billing/never-a-route'))
    // A signed-in browser reaches the end of the routes: there is no such route.
    assert.deepEqual((await owner.post('billing/webhook', {}, 404)).value, { error: 'notFound' })
    assert.deepEqual(await app.db.prepare('SELECT COUNT(*) AS n FROM billing_events').first(), { n: 0 })
    // Positive control: the same runtime and browser reach the account's own route.
    assert.equal((await session(owner)).state, 'none')
  } finally { await app.mf.dispose() }
})

// ---------------------------------------------------------------------------------------------------
// SUBSCRIBE AND MANAGE: the relay's two billing routes, which reach the private billing Worker through its
// BILLING binding. Every deploy carries none until that Worker is deployed, and the routes say so.

test('with no billing Worker bound, Subscribe and Manage are not available yet, and the page is told so', async () => {
  const config = async app => (await app.mf.dispatchFetch(`${app.origin}/api/remote/config`)).json()
  assert.equal((await config(plain)).billing, false)
  const owner = await person(plain)
  assert.deepEqual((await owner.post('billing/checkout', { plan: 'monthly' }, 503)).value, { error: 'billingNotAvailable' })
  assert.deepEqual((await owner.post('billing/portal', {}, 503)).value, { error: 'billingNotAvailable' })
  // Still behind the sign-in: a request with no account gets nothing about billing at all.
  assert.deepEqual((await plain.client().post('billing/checkout', { plan: 'monthly' }, 403)).value, { error: 'originDenied' })
  // Positive control: the same config route says true where the billing Worker is bound.
  const bound = await runtime({ billing: () => ({ error: 'serviceUnavailable', status: 503 }) })
  try { assert.equal((await config(bound)).billing, true) } finally { await bound.mf.dispose() }
})

test('with the billing Worker bound, checkout and the portal pass only the signed-in account and hand back the provider page', async () => {
  const calls = []
  let answer = null
  const app = await runtime({ billing: (method, args) => {
    calls.push({ method, args })
    if (answer) return answer(method)
    return method === 'checkout' ? { url: 'https://checkout.stripe.com/c/pay/cs_test_fixture' } : { url: 'https://billing.stripe.com/p/session/fixture' }
  } })
  try {
    const owner = await person(app)
    assert.deepEqual((await owner.post('billing/checkout', { plan: 'annual' })).value, { url: 'https://checkout.stripe.com/c/pay/cs_test_fixture' })
    assert.deepEqual((await owner.post('billing/portal', {})).value, { url: 'https://billing.stripe.com/p/session/fixture' })
    assert.deepEqual(calls, [{ method: 'checkout', args: [owner.accountId, 'annual'] }, { method: 'portal', args: [owner.accountId] }])
    // Opening a checkout buys nothing: only the webhook's write, read by the one entitlement read, does.
    assert.equal((await session(owner)).state, 'none')

    // The page cannot name another account, or a plan that is not ours; nothing reaches the billing Worker.
    calls.length = 0
    assert.deepEqual((await owner.post('billing/checkout', { plan: 'weekly' }, 400)).value, { error: 'invalidRequest' })
    assert.deepEqual((await owner.post('billing/checkout', { plan: 'monthly', accountId: crypto.randomUUID() }, 400)).value, { error: 'invalidRequest' })
    assert.deepEqual((await owner.post('billing/portal', { accountId: crypto.randomUUID() }, 400)).value, { error: 'invalidRequest' })
    assert.deepEqual((await app.client().post('billing/portal', {}, 403)).value, { error: 'originDenied' })
    assert.deepEqual(calls, [])

    // Its refusals are passed on by name; anything else it says, or a page anywhere else, is not.
    const cases = [
      [{ error: 'alreadySubscribed', status: 409 }, 409, 'alreadySubscribed'],
      [{ error: 'noSubscription', status: 404 }, 404, 'noSubscription'],
      [{ error: 'secret detail rk_test_fixture', status: 400 }, 503, 'serviceUnavailable'],
      [{ error: 'alreadySubscribed', status: 200 }, 503, 'serviceUnavailable'],
      [{ url: 'https://example.invalid/pay' }, 503, 'serviceUnavailable'],
    ]
    for (const [given, status, error] of cases) {
      answer = () => given
      for (const [path, value] of [['billing/checkout', { plan: 'monthly' }], ['billing/portal', {}]]) {
        assert.deepEqual((await (await person(app)).post(path, value, status)).value, { error }, `${path} ${JSON.stringify(given)}`)
      }
    }
    // Each route takes only its own provider page: a portal answering with a checkout page is refused, and the reverse.
    answer = method => ({ url: method === 'checkout' ? 'https://billing.stripe.com/p/session/fixture' : 'https://checkout.stripe.com/c/pay/cs_test_fixture' })
    assert.deepEqual((await owner.post('billing/checkout', { plan: 'monthly' }, 503)).value, { error: 'serviceUnavailable' })
    assert.deepEqual((await owner.post('billing/portal', {}, 503)).value, { error: 'serviceUnavailable' })
    // The billing Worker failing outright is a plain 503 too.
    answer = () => { throw new Error('billing Worker down') }
    assert.deepEqual((await owner.post('billing/checkout', { plan: 'monthly' }, 503)).value, { error: 'serviceUnavailable' })

    // Each one is a call to the payment provider: ten a minute an account.
    answer = null
    const busy = await person(app)
    for (let i = 0; i < 10; i++) await busy.post('billing/portal', {})
    assert.deepEqual((await busy.post('billing/checkout', { plan: 'monthly' }, 429)).value, { error: 'tryLater' })

    // Bound or not, the relay still serves no webhook.
    assert.deepEqual((await owner.post('billing/webhook', {}, 404)).value, { error: 'notFound' })
  } finally { await app.mf.dispose() }
})
