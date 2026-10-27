// Sign in once, no pairing: a station bound to its account by the shack's own sign-in, devices
// approved because their sign-in proves who they are, the account's device list, invites, and a
// guest's use on the owner's plan. A proof is an ID token whose nonce is the hash of the keys it
// vouches for. The issuer is the runtime's local stand-in: no real tenant, token or key is used or
// made, and every key here is made in memory.
import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { generateKeyPair } from 'jose'
import { runtime } from './runtime.mjs'

const NATIVE = 'remote-test-native', SPA = 'remote-test-client'
let app
before(async () => { app = await runtime({ bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE } }) })
after(async () => { await app?.mf.dispose() })

const DAY = 86400000, MINUTE = 60000, UNTIL_REMOVED = Date.UTC(9999, 11, 31, 23, 59, 59, 999)
const PROOF = { 'x-nexus-device-proof': '1' }, KINDS = { 'x-nexus-device-kind': '1' }, LIFETIME = { 'x-nexus-device-lifetime': '1' }
const hex = bytes => Array.from(new Uint8Array(bytes), b => b.toString(16).padStart(2, '0')).join('')
const sha256 = async text => hex(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text)))
const random = () => hex(crypto.getRandomValues(new Uint8Array(32)))
const seconds = ms => Math.floor(ms / 1000)

/** The nonce a sign-in carries: SHA-256, in base64url, of the label, a zero byte, the kind and a zero
 *  byte (a device only), then each key's SPKI DER in order. Written out here from the definition, not
 *  imported, so the Worker's and this one must agree byte for byte. */
const nonceOf = (label, kind, keys) => createHash('sha256').update(Buffer.concat([
  Buffer.from(label), Buffer.from([0]), ...(kind === null ? [] : [Buffer.from(kind), Buffer.from([0])]),
  ...keys.map(key => Buffer.from(key, 'hex'))])).digest('base64url')
const stationNonce = (stationKey, lanKey) => nonceOf('nexus-station/1', null, [stationKey, lanKey])
const deviceNonce = (kind, keys) => nonceOf('nexus-device/1', kind, keys)

/** A P-256 key as Nexus holds one: its public half as SPKI in lowercase hex. */
async function keyPair() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify'])
  return { pair, publicKey: hex(await crypto.subtle.exportKey('spki', pair.publicKey)) }
}
/** A person with a verified address, signed in on the page and on Nexus, with no trial. */
async function person(email = `op-${crypto.randomUUID()}@example.invalid`, { verified = true } = {}) {
  const subject = `auth0|${crypto.randomUUID()}`, claims = { email, email_verified: verified }
  const page = app.client(await app.token(subject, claims))
  const { value } = await page.post('session')
  const native = app.client(null, '', await app.token(subject, { azp: NATIVE, ...claims }))
  return { subject, email, verified, page, native, accountId: value.accountId }
}
const entitle = (who, until = Date.now() + DAY) =>
  app.db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,1,?) ON CONFLICT(account_id) DO UPDATE SET expires_at=excluded.expires_at')
    .bind(who.accountId, until).run()
/** A proof from `who`'s sign-in to `audience`, for `nonce`. */
const proofOf = (who, nonce, options = {}) => app.idToken(who.subject, nonce,
  { audience: NATIVE, ...options, claims: { email: who.email, email_verified: who.verified, ...options.claims } })

/** The shack signing in: its keys made first, their hash the sign-in's nonce. */
async function bind(who, { expected = 200, proof, extra = {}, name = 'Shack' } = {}) {
  const stationKey = await keyPair(), lanKey = await keyPair(), credential = random()
  const body = { name, credential, stationKey: stationKey.publicKey, lanKey: lanKey.publicKey,
    proof: proof ?? await proofOf(who, stationNonce(stationKey.publicKey, lanKey.publicKey)), ...extra }
  const { value } = await who.native.post('client/station', body, expected)
  return { value, stationId: value.stationId, credential, stationKey, lanKey, body, shack: app.client(null, '', credential) }
}
/** A request as a computer signs it with `key`, over a challenge its account was just given. */
async function signed(native, key, path, value, expected = 200, extra = {}) {
  const { value: { challenge } } = await native.post('client/challenge')
  const text = ['nexus-native-request/1', challenge, 'POST', `/api/remote/${path}`, await sha256(JSON.stringify(value))].join('\n')
  const signature = hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, key.pair.privateKey, new TextEncoder().encode(text)))
  return native.post(path, value, expected, { 'x-nexus-challenge': challenge, 'x-nexus-signature': signature, ...extra })
}
/** A computer enrolling with `who`'s sign-in: its road and LAN keys, and a proof over both. */
async function enrol(who, stationId, { expected = 200, proof = true, kind = 'computer', platform = 'windows', lan = true } = {}) {
  const road = await keyPair(), lanKey = lan ? await keyPair() : null
  const keys = [road.publicKey, ...(lanKey ? [lanKey.publicKey] : [])]
  const body = { name: 'Laptop', kind, platform, publicKey: road.publicKey, ...(lanKey ? { lanKey: lanKey.publicKey } : {}) }
  if (proof === true) body.proof = await proofOf(who, deviceNonce(kind, keys))
  else if (proof) body.proof = proof
  const { value } = await signed(who.native, road, `stations/${stationId}/client/device`, body, expected)
  return { value, road, lanKey, body, deviceId: value.deviceId }
}
/** A browser enrolling with `who`'s page sign-in: its device key, and a proof over it. */
async function browser(who, stationId, { expected = 200, proof = true } = {}) {
  const key = await keyPair()
  const body = { name: 'Chrome on Windows', publicKey: key.publicKey }
  if (proof === true) body.proof = await proofOf(who, deviceNonce('browser', [key.publicKey]), { audience: SPA })
  else if (proof) body.proof = proof
  const { value, response } = await who.page.post(`stations/${stationId}/device`, body, expected)
  if (expected === 200) who.page.setCookie(response.headers.get('set-cookie'))
  return { value, key, deviceId: value.deviceId }
}
const deviceRow = id => app.db.prepare('SELECT * FROM devices WHERE id=?').bind(id).first()
const stationRow = id => app.db.prepare('SELECT * FROM stations WHERE id=?').bind(id).first()
const errorOf = async promise => (await promise).value.error

// --- Binding the station ----------------------------------------------------------------------------

test('the shack signs in and its station is bound to the account, with its keys and its proof, and no trial started', async () => {
  const owner = await person(), shack = await bind(owner)
  assert.equal(shack.value.accountId, owner.accountId)
  const row = await stationRow(shack.stationId)
  assert.equal(row.account_id, owner.accountId)
  assert.equal(row.name, 'Shack')
  assert.equal(row.public_key, shack.stationKey.publicKey, 'the station key is kept as native/key would')
  assert.equal(row.lan_key, shack.lanKey.publicKey)
  assert.equal(row.proof, shack.body.proof)
  assert.equal(row.credential_hash, await sha256(shack.credential))
  // The trial starts at the first internet use, never at binding: a LAN-only operator never spends it.
  assert.equal(await app.db.prepare('SELECT * FROM trials WHERE account_id=?').bind(owner.accountId).first(), null)
  assert.equal(shack.value.entitlement.state, 'none')
  // The station's credential is a station's: its own routes open.
  await shack.shack.post(`stations/${shack.stationId}/native/devices`)
  // The same sign-in again with the same credential finds the same station, and makes no second.
  const again = (await owner.native.post('client/station', shack.body)).value
  assert.equal(again.stationId, shack.stationId)
  assert.equal((await app.db.prepare('SELECT COUNT(*) AS n FROM stations WHERE account_id=?').bind(owner.accountId).first()).n, 1)
  // The account's own computers see it, with the LAN key and the proof that vouches for it.
  const { value: session } = await owner.native.post('client/session')
  assert.deepEqual(session.stations, [{ id: shack.stationId, name: 'Shack', stationKey: shack.stationKey.publicKey,
    lanKey: shack.lanKey.publicKey, proof: shack.body.proof, role: 'owner' }])
})

test('a station proof is checked: its signature, issuer, application, subject, nonce and age, each by itself', async () => {
  const now = seconds(Date.now()), { privateKey } = await generateKeyPair('RS256')
  // Each case is a person of its own, so no case spends another's binding allowance.
  // Each case names itself in its failure, status included, so a red says which check moved.
  const attempt = async (expected, make, why) => {
    const who = await person(), stationKey = await keyPair(), lanKey = await keyPair()
    const proof = await make(who, stationNonce(stationKey.publicKey, lanKey.publicKey), stationKey.publicKey, lanKey.publicKey)
    const body = { name: 'Shack', credential: random(), stationKey: stationKey.publicKey, lanKey: lanKey.publicKey, proof }
    const { value } = await who.native.post('client/station', body, expected).catch(error => { throw new Error(`${why}: ${error.message}`) })
    return value
  }
  const refused = async (why, make) => assert.equal((await attempt(403, make, why)).error, 'proofInvalid', why)
  await refused('another key', (who, nonce) => proofOf(who, nonce, { key: privateKey }))
  await refused('another issuer', (who, nonce) => proofOf(who, nonce, { iss: 'https://elsewhere.invalid/' }))
  await refused('the page\'s application', (who, nonce) => proofOf(who, nonce, { audience: SPA }))
  await refused('an audience list with no azp', (who, nonce) => proofOf(who, nonce, { audience: [NATIVE, 'remote-test-api'] }))
  await refused('an azp of another application', (who, nonce) => proofOf(who, nonce, { claims: { azp: SPA } }))
  await refused('another person\'s sign-in', async (_who, nonce) => proofOf(await person(), nonce))
  await refused('the keys swapped', (who, _nonce, s, l) => proofOf(who, stationNonce(l, s)))
  await refused('a device nonce for the same keys', (who, _nonce, s, l) => proofOf(who, deviceNonce('computer', [s, l])))
  await refused('no separator after the label', (who, _nonce, s, l) => proofOf(who, createHash('sha256')
    .update(Buffer.concat([Buffer.from('nexus-station/1'), Buffer.from(s, 'hex'), Buffer.from(l, 'hex')])).digest('base64url')))
  await refused('no nonce', who => proofOf(who, undefined))
  await refused('signed eleven minutes ago', (who, nonce) => proofOf(who, nonce, { iat: now - 11 * 60 }))
  await refused('signed in the future', (who, nonce) => proofOf(who, nonce, { iat: now + 5 * 60 }))
  await refused('not a token', () => 'not.a.token')
  assert.equal((await attempt(400, () => 42)).error, 'invalidRequest', 'not a string')
  // CONTROLS, each the twin of a refusal above: an audience list WITH azp = Nexus is taken, a proof
  // signed nine minutes ago is taken, and so is one whose `exp` has passed while its `iat` is fresh: a
  // proof is bounded by when it was signed, never by the token's own lifetime.
  assert.ok((await attempt(200, (who, nonce) => proofOf(who, nonce, { audience: [NATIVE, 'remote-test-api'], claims: { azp: NATIVE } }))).stationId)
  assert.ok((await attempt(200, (who, nonce) => proofOf(who, nonce, { iat: now - 9 * 60 }))).stationId)
  assert.ok((await attempt(200, (who, nonce) => proofOf(who, nonce, { iat: now - 60, exp: now - 30 }))).stationId)
})

test('binding: two stations an account, a page cannot call it, a ban refuses it, and an ended trial does not', async () => {
  const owner = await person()
  await bind(owner); await bind(owner)
  assert.equal(await errorOf(bind(owner, { expected: 409 }).then(s => ({ value: s.value }))), 'stationLimit')
  // From a page: the Origin is refused before the token is read.
  const stationKey = await keyPair(), lanKey = await keyPair()
  const body = { name: 'Shack', credential: random(), stationKey: stationKey.publicKey, lanKey: lanKey.publicKey,
    proof: await proofOf(owner, stationNonce(stationKey.publicKey, lanKey.publicKey)) }
  assert.equal(await errorOf(owner.page.post('client/station', body, 403)), 'originDenied')
  // An operator's ban outranks binding; an ended trial does not (the LAN is free).
  const banned = await person(), ended = await person()
  await app.db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,0,?)').bind(banned.accountId, Date.now() + DAY).run()
  await app.db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,1,?)').bind(ended.accountId, Date.now() - DAY).run()
  assert.equal((await bind(banned, { expected: 403 })).value.error, 'trialDisabled')
  assert.equal((await bind(ended)).value.entitlement.state, 'ended')
  // A malformed key or credential is refused by shape.
  assert.equal((await bind(await person(), { expected: 400, extra: { lanKey: '04ab' } })).value.error, 'invalidRequest')
  assert.equal((await bind(await person(), { expected: 400, extra: { credential: 'short' } })).value.error, 'invalidRequest')
})

test('binding is five a day an account', async () => {
  const owner = await person()
  // Each try spends the bucket, refused or not: five refused proofs, then even a good one waits.
  for (let i = 0; i < 5; i++) await bind(owner, { expected: 403, proof: 'not.a.token' })
  assert.equal((await bind(owner, { expected: 429 })).value.error, 'tryLater')
  await bind(await person())
})

test('a station paired by code confirms its account at the station, and only with its own credential and key', async () => {
  const pair = await app.paired(), subject = JSON.parse(Buffer.from(pair.browser.jwt.split('.')[1], 'base64url')).sub
  const owner = { subject, email: null, verified: false, accountId: pair.browser.accountId,
    native: app.client(null, '', await app.token(subject, { azp: NATIVE })) }
  const stationKey = await keyPair(), lanKey = await keyPair()
  await pair.native.post(`stations/${pair.stationId}/native/key`, { publicKey: stationKey.publicKey })
  const path = `stations/${pair.stationId}/client/station-proof`
  const body = async (overrides = {}) => ({ credential: pair.stationCredential, stationKey: stationKey.publicKey, lanKey: lanKey.publicKey,
    proof: await proofOf(owner, stationNonce(stationKey.publicKey, lanKey.publicKey)), ...overrides })
  // Before: a device with a good proof waits for the station, as today.
  const waiting = await enrol(owner, pair.stationId)
  assert.equal(waiting.value.approved, false)
  assert.equal((await deviceRow(waiting.deviceId)).added_by, null)
  // Refused: another credential, another station key than the one it pinned, another account's token.
  assert.equal(await errorOf(owner.native.post(path, await body({ credential: random() }), 401)), 'stationNotApproved')
  const otherKey = await keyPair()
  assert.equal(await errorOf(owner.native.post(path, await body({ stationKey: otherKey.publicKey,
    proof: await proofOf(owner, stationNonce(otherKey.publicKey, lanKey.publicKey)) }), 409)), 'stationKeyPinned')
  const stranger = await person()
  assert.equal(await errorOf(stranger.native.post(path, await body({ proof: await proofOf(stranger, stationNonce(stationKey.publicKey, lanKey.publicKey)) }), 404)), 'stationUnavailable')
  assert.equal((await stationRow(pair.stationId)).proof, null)
  // Confirmed: the proof and the LAN key are kept, and the next device is approved by its sign-in.
  const confirmed = await body()
  await owner.native.post(path, confirmed)
  assert.equal((await stationRow(pair.stationId)).proof, confirmed.proof)
  assert.equal((await stationRow(pair.stationId)).lan_key, lanKey.publicKey)
  const approved = await enrol(owner, pair.stationId)
  assert.equal(approved.value.approved, true)
  // A device reopening by its key alone never reaches it: confirming takes the owner's own sign-in.
  assert.equal(await errorOf(app.client(null, '').post(path, await body(), 404, { 'x-nexus-device-id': approved.deviceId })), 'stationUnavailable')
  // Signing in again replaces the proof (a new LAN key after a reset, or a key the provider retired).
  const lan2 = await keyPair()
  const again = await body({ lanKey: lan2.publicKey, proof: await proofOf(owner, stationNonce(stationKey.publicKey, lan2.publicKey)) })
  await owner.native.post(path, again)
  assert.equal((await stationRow(pair.stationId)).lan_key, lan2.publicKey)
})

// --- Devices approved by sign-in -------------------------------------------------------------------

test('the owner\'s computer is approved by its sign-in: until removed, as Operate, with its proof and both keys', async () => {
  const owner = await person(), shack = await bind(owner)
  const before = (await stationRow(shack.stationId)).policy_version
  const c = await enrol(owner, shack.stationId)
  assert.deepEqual(c.value, { deviceId: c.deviceId, approved: true, expiresAt: UNTIL_REMOVED, renewsUntil: null })
  const row = await deviceRow(c.deviceId)
  assert.equal(row.approved, 1); assert.equal(row.expires_at, UNTIL_REMOVED)
  assert.equal(row.role, 'operate'); assert.equal(row.added_by, 'account')
  assert.equal(row.proof, c.body.proof)
  assert.equal(row.public_key, c.road.publicKey); assert.equal(row.lan_key, c.lanKey.publicKey)
  assert.equal(row.kind, 'computer'); assert.equal(row.platform, 'windows')
  assert.equal((await stationRow(shack.stationId)).policy_version, before + 1, 'the station is told')
  // The same key again is the same device.
  assert.equal((await signed(owner.native, c.road, `stations/${shack.stationId}/client/device`, c.body)).value.deviceId, c.deviceId)
  // CONTROL: without a proof the device waits for the station, as today, ten minutes.
  const plain = await enrol(owner, shack.stationId, { proof: false })
  assert.equal(plain.value.approved, false)
  assert.ok(plain.value.expiresAt <= Date.now() + 10 * MINUTE)
  const waiting = await deviceRow(plain.deviceId)
  assert.equal(waiting.approved, 0); assert.equal(waiting.added_by, null); assert.equal(waiting.proof, null)
  // A device that waited is approved in place by a sign-in for the same keys.
  const upgraded = await signed(owner.native, plain.road, `stations/${shack.stationId}/client/device`,
    { ...plain.body, proof: await proofOf(owner, deviceNonce('computer', [plain.road.publicKey, plain.lanKey.publicKey])) })
  assert.equal(upgraded.value.deviceId, plain.deviceId)
  assert.equal(upgraded.value.approved, true)
  assert.equal((await deviceRow(plain.deviceId)).generation, waiting.generation + 1)
})

test('a device proof binds every key it registers and nothing else', async () => {
  const owner = await person(), other = await person(), shack = await bind(owner)
  const road = await keyPair(), lan = await keyPair(), stray = await keyPair()
  const send = async (body, expected) => signed(owner.native, road, `stations/${shack.stationId}/client/device`,
    { name: 'Laptop', kind: 'computer', platform: 'windows', publicKey: road.publicKey, lanKey: lan.publicKey, ...body }, expected)
  const refused = async (why, proof) => assert.equal((await send({ proof }, 403)).value.error, 'proofInvalid', why)
  await refused('the road key alone', await proofOf(owner, deviceNonce('computer', [road.publicKey])))
  await refused('another LAN key', await proofOf(owner, deviceNonce('computer', [road.publicKey, stray.publicKey])))
  await refused('the keys in the other order', await proofOf(owner, deviceNonce('computer', [lan.publicKey, road.publicKey])))
  await refused('another kind', await proofOf(owner, deviceNonce('phone', [road.publicKey, lan.publicKey])))
  await refused('the station\'s nonce for the same keys', await proofOf(owner, stationNonce(road.publicKey, lan.publicKey)))
  await refused('the page\'s application', await proofOf(owner, deviceNonce('computer', [road.publicKey, lan.publicKey]), { audience: SPA }))
  await refused('another person', await proofOf(other, deviceNonce('computer', [road.publicKey, lan.publicKey])))
  await refused('stale', await proofOf(owner, deviceNonce('computer', [road.publicKey, lan.publicKey]), { iat: seconds(Date.now()) - 11 * 60 }))
  assert.equal(await deviceRow((await app.db.prepare('SELECT id FROM devices WHERE public_key=?').bind(road.publicKey).first())?.id ?? ''), null,
    'nothing was written for a refused proof')
  // CONTROL: the proof over exactly these keys is taken.
  assert.equal((await send({ proof: await proofOf(owner, deviceNonce('computer', [road.publicKey, lan.publicKey])) })).value.approved, true)
  // A phone has no LAN key: its proof covers its one key.
  const phone = await enrol(owner, shack.stationId, { kind: 'phone', platform: 'ios', lan: false })
  assert.equal(phone.value.approved, true)
  assert.equal((await deviceRow(phone.deviceId)).lan_key, null)
})

test('the owner\'s browser is approved by its page sign-in, bound to its device key', async () => {
  const owner = await person(), shack = await bind(owner)
  await entitle(owner)
  const b = await browser(owner, shack.stationId)
  assert.deepEqual(b.value, { deviceId: b.deviceId, approved: true })
  const row = await deviceRow(b.deviceId)
  assert.equal(row.kind, null, 'a browser'); assert.equal(row.role, 'operate'); assert.equal(row.added_by, 'account')
  assert.equal(row.expires_at, UNTIL_REMOVED); assert.equal(row.public_key, b.key.publicKey)
  // Its cookie opens a ticket at once: no press at the station.
  await owner.page.post(`stations/${shack.stationId}/ticket`)
  // Refused: a Nexus proof on the page, a proof with no device key to bind, a proof for another key.
  const other = await person(), key = await keyPair()
  const s2 = await bind(other)
  assert.equal(await errorOf(other.page.post(`stations/${s2.stationId}/device`, { name: 'B', publicKey: key.publicKey,
    proof: await proofOf(other, deviceNonce('browser', [key.publicKey])) }, 403)), 'proofInvalid')
  assert.equal(await errorOf(other.page.post(`stations/${s2.stationId}/device`, { name: 'B',
    proof: await proofOf(other, deviceNonce('browser', [key.publicKey]), { audience: SPA }) }, 400)), 'invalidRequest')
  assert.equal(await errorOf(other.page.post(`stations/${s2.stationId}/device`, { name: 'B', publicKey: key.publicKey,
    proof: await proofOf(other, deviceNonce('browser', [(await keyPair()).publicKey]), { audience: SPA }) }, 403)), 'proofInvalid')
})

test('no device is approved by sign-in for a station that has no station proof', async () => {
  const pair = await app.paired(), subject = JSON.parse(Buffer.from(pair.browser.jwt.split('.')[1], 'base64url')).sub
  const owner = { subject, email: null, verified: false, native: app.client(null, '', await app.token(subject, { azp: NATIVE })) }
  const c = await enrol(owner, pair.stationId)
  assert.equal(c.value.approved, false)
  assert.equal((await deviceRow(c.deviceId)).approved, 0)
})

test('enrolling needs no internet entitlement; everything that uses the internet still does', async () => {
  const owner = await person(), shack = await bind(owner)
  await app.db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,1,?)').bind(owner.accountId, Date.now() - DAY).run()
  const c = await enrol(owner, shack.stationId)
  assert.equal(c.value.approved, true)
  const b = await browser(owner, shack.stationId)
  assert.equal(b.value.approved, true)
  // CONTROL: the relay is still the internet's.
  assert.equal((await signed(owner.native, c.road, `stations/${shack.stationId}/client/turn`, {}, 403, { 'x-nexus-device-id': c.deviceId })).value.error, 'trialRequired')
  assert.equal(await errorOf(owner.page.post(`stations/${shack.stationId}/turn`, {}, 403)), 'trialRequired')
})

test('auto-approval is five a day an account', async () => {
  const owner = await person(), a = await bind(owner), b = await bind(owner)
  for (let i = 0; i < 5; i++) assert.equal((await enrol(owner, i < 3 ? a.stationId : b.stationId)).value.approved, true)
  assert.equal((await enrol(owner, b.stationId, { expected: 429 })).value.error, 'tryLater')
  // CONTROL: registering to wait for the station is not counted against it.
  assert.equal((await enrol(owner, b.stationId, { proof: false })).value.approved, false)
})

// --- What the station reads -------------------------------------------------------------------------

test('the station is told each device\'s role, how it was added, its proof and LAN key, only when it asks', async () => {
  const owner = await person(), shack = await bind(owner)
  const c = await enrol(owner, shack.stationId), w = await enrol(owner, shack.stationId, { proof: false })
  const list = async headers => (await shack.shack.post(`stations/${shack.stationId}/native/devices`, {}, 200, headers)).value.devices
  // Without the header: exactly the fields an older Nexus parses, and a computer only to one asking for kinds.
  for (const headers of [{}, LIFETIME, KINDS, { ...KINDS, ...LIFETIME }]) {
    for (const d of await list(headers)) for (const field of ['role', 'addedBy', 'proof', 'lanKey']) assert.ok(!(field in d), `${field} unasked`)
  }
  const asked = await list({ ...KINDS, ...LIFETIME, ...PROOF })
  const byId = Object.fromEntries(asked.map(d => [d.id, d]))
  assert.deepEqual([byId[c.deviceId].role, byId[c.deviceId].addedBy, byId[c.deviceId].proof, byId[c.deviceId].lanKey],
    ['operate', 'account', c.body.proof, c.lanKey.publicKey])
  assert.deepEqual([byId[w.deviceId].role, byId[w.deviceId].addedBy, byId[w.deviceId].proof], ['operate', 'shack', null])
})

// --- Invites and guests ----------------------------------------------------------------------------

test('an invited person signs in with their own account and is approved with the role they were given', async () => {
  const owner = await person(), shack = await bind(owner)
  const guest = await person('Friend.Op+radio@Gmail.com'), stranger = await person()
  // Invited by the mailbox the address delivers to: dots and a +tag do not matter at Gmail.
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: 'friendop@gmail.com', role: 'listen' })
  const { value: session } = await guest.native.post('client/session')
  assert.deepEqual(session.stations.map(s => [s.id, s.role, s.lanKey, s.proof]), [[shack.stationId, 'listen', shack.lanKey.publicKey, shack.body.proof]])
  const g = await enrol(guest, shack.stationId)
  assert.equal(g.value.approved, true)
  const row = await deviceRow(g.deviceId)
  assert.equal(row.role, 'listen'); assert.equal(row.account_id, guest.accountId); assert.equal(row.added_by, 'account')
  // A guest's proof must itself carry the invited address, verified, as the station will check it: one
  // without leaves the device waiting for the station.
  const road = await keyPair(), lan = await keyPair()
  const unverifiedProof = await proofOf(guest, deviceNonce('computer', [road.publicKey, lan.publicKey]), { claims: { email_verified: false } })
  assert.equal((await signed(guest.native, road, `stations/${shack.stationId}/client/device`, { name: 'Laptop', kind: 'computer',
    platform: 'windows', publicKey: road.publicKey, lanKey: lan.publicKey, proof: unverifiedProof })).value.approved, false)
  // Not invited: the station is not theirs to see, and no device of theirs is written.
  assert.equal((await stranger.native.post('client/session')).value.stations.length, 0)
  assert.equal((await enrol(stranger, shack.stationId, { expected: 404 })).value.error, 'stationUnavailable')
  // The role can change by inviting again; a person's devices keep the role they were approved with
  // until the station grants again.
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: 'friendop@gmail.com', role: 'operate' })
  assert.equal((await guest.native.post('client/session')).value.stations[0].role, 'operate')
  assert.equal((await enrol(guest, shack.stationId)).value.approved, true)
})

test('an invite matches only a verified address, and is the station\'s to make', async () => {
  const owner = await person(), shack = await bind(owner)
  const unverified = await person(`u-${crypto.randomUUID()}@example.invalid`, { verified: false })
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: unverified.email, role: 'operate' })
  assert.equal((await unverified.native.post('client/session')).value.stations.length, 0)
  assert.equal((await enrol(unverified, shack.stationId, { expected: 404 })).value.error, 'stationUnavailable')
  // Refused shapes, and the page or a stranger cannot invite.
  for (const body of [{ email: 'x@example.invalid', role: 'admin' }, { email: 'nobody', role: 'listen' }, { email: 'x@example.invalid' }]) {
    assert.equal(await errorOf(shack.shack.post(`stations/${shack.stationId}/native/invite`, body, 400)), 'invalidRequest')
  }
  assert.equal(await errorOf(owner.page.post(`stations/${shack.stationId}/native/invite`, { email: 'x@example.invalid', role: 'listen' }, 403)), 'originDenied')
  assert.equal(await errorOf(app.client(null, '', random()).post(`stations/${shack.stationId}/native/invite`, { email: 'x@example.invalid', role: 'listen' }, 401)), 'stationNotApproved')
})

test('removing an invite removes every device that person has at the station, and their access, at once', async () => {
  const owner = await person(), shack = await bind(owner), other = await bind(owner)
  const guest = await person()
  for (const s of [shack, other]) await s.shack.post(`stations/${s.stationId}/native/invite`, { email: guest.email, role: 'operate' })
  const g1 = await enrol(guest, shack.stationId), g2 = await browser(guest, shack.stationId), g3 = await enrol(guest, other.stationId)
  const mine = await enrol(owner, shack.stationId)
  const before = await stationRow(shack.stationId)
  await shack.shack.post(`stations/${shack.stationId}/native/uninvite`, { email: guest.email.toUpperCase() })
  for (const id of [g1.deviceId, g2.deviceId]) {
    const row = await deviceRow(id)
    assert.equal(row.approved, 0); assert.ok(row.expires_at <= Date.now()); assert.equal(row.proof, null)
  }
  assert.equal((await stationRow(shack.stationId)).policy_version, before.policy_version + 1)
  // Only that station, and only that person.
  assert.equal((await deviceRow(g3.deviceId)).approved, 1)
  assert.equal((await deviceRow(mine.deviceId)).approved, 1)
  assert.deepEqual((await guest.native.post('client/session')).value.stations.map(s => s.id), [other.stationId])
  assert.equal((await enrol(guest, shack.stationId, { expected: 404 })).value.error, 'stationUnavailable')
  // Invited again, the same row, live.
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'listen' })
  assert.equal((await app.db.prepare('SELECT COUNT(*) AS n FROM invites WHERE station_id=?').bind(shack.stationId).first()).n, 1)
  assert.equal((await enrol(guest, shack.stationId)).value.approved, true)
  // The owner's own address invited and removed takes nothing of the owner's.
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: owner.email, role: 'listen' })
  await shack.shack.post(`stations/${shack.stationId}/native/uninvite`, { email: owner.email })
  assert.equal((await deviceRow(mine.deviceId)).approved, 1)
})

test('a station invites at most twenty people a day, and has at most twenty invited at once', async () => {
  const owner = await person(), shack = await bind(owner), path = `stations/${shack.stationId}/native/invite`
  for (let i = 0; i < 20; i++) await shack.shack.post(path, { email: `p${i}@example.invalid`, role: 'listen' })
  assert.equal(await errorOf(shack.shack.post(path, { email: 'p20@example.invalid', role: 'listen' }, 429)), 'tryLater')
  // The day's allowance aside, the twenty already invited are the most a station holds: a twenty-first
  // is refused, and changing one already there is not.
  await app.db.prepare('DELETE FROM rate_limits').run()
  assert.equal(await errorOf(shack.shack.post(path, { email: 'p20@example.invalid', role: 'listen' }, 409)), 'inviteLimit')
  await shack.shack.post(path, { email: 'p3@example.invalid', role: 'operate' })
  await shack.shack.post(`stations/${shack.stationId}/native/uninvite`, { email: 'p4@example.invalid' })
  await shack.shack.post(path, { email: 'p20@example.invalid', role: 'listen' })
})

test('a guest uses the internet on the owner\'s plan, and only on station business', async () => {
  const owner = await person(), shack = await bind(owner), guest = await person()
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'operate' })
  const g = await enrol(guest, shack.stationId), gb = await browser(guest, shack.stationId)
  await entitle(owner)
  // The guest has no plan of their own; past the entitlement, the relay is reached (and is not set up here).
  assert.equal((await signed(guest.native, g.road, `stations/${shack.stationId}/client/turn`, {}, 503, { 'x-nexus-device-id': g.deviceId })).value.error, 'relayNotConfigured')
  const { value: ticket } = await guest.page.post(`stations/${shack.stationId}/ticket`)
  assert.match(ticket.ticket, /^[0-9a-f]{64}$/)
  const issued = await app.db.prepare('SELECT account_id,device_id FROM tickets WHERE digest=?').bind(await sha256(ticket.ticket)).first()
  assert.deepEqual(issued, { account_id: owner.accountId, device_id: gb.deviceId }, 'the session runs under the station\'s account')
  assert.equal(await app.db.prepare('SELECT * FROM trials WHERE account_id=?').bind(guest.accountId).first(), null, 'the guest spent no trial')
  // The owner's plan ends: so does the guest's internet use.
  await entitle(owner, Date.now() - DAY)
  assert.equal((await signed(guest.native, g.road, `stations/${shack.stationId}/client/turn`, {}, 403, { 'x-nexus-device-id': g.deviceId })).value.error, 'trialRequired')
  // A guest cannot rename, revoke, confirm the station's account, or approve anything; the owner's own
  // routes stay the owner's.
  assert.equal(await errorOf(guest.native.post(`stations/${shack.stationId}/client/station-proof`, { credential: random(),
    stationKey: shack.stationKey.publicKey, lanKey: shack.lanKey.publicKey, proof: shack.body.proof }, 404)), 'stationUnavailable')
  assert.equal(await errorOf(guest.page.post(`stations/${shack.stationId}/rename`, { name: 'Mine' }, 404)), 'stationUnavailable')
  assert.equal(await errorOf(guest.page.post(`stations/${shack.stationId}/revoke`, {}, 404)), 'stationUnavailable')
  assert.equal((await stationRow(shack.stationId)).enabled, 1)
})

test('a guest\'s computer reopens with its key alone, and stops when its invite is removed', async () => {
  const owner = await person(), shack = await bind(owner), guest = await person()
  await entitle(owner)
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'operate' })
  const g = await enrol(guest, shack.stationId)
  const reopen = async expected => {
    const anon = app.client(null, '')
    const { value } = await anon.post(`stations/${shack.stationId}/client/challenge`, {}, expected === 401 ? 401 : 200, { 'x-nexus-device-id': g.deviceId })
    if (expected === 401) return value.error
    const path = `stations/${shack.stationId}/client/turn`
    const text = ['nexus-native-request/1', value.challenge, 'POST', `/api/remote/${path}`, await sha256('{}')].join('\n')
    const signature = hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, g.road.pair.privateKey, new TextEncoder().encode(text)))
    return (await anon.post(path, {}, expected, { 'x-nexus-device-id': g.deviceId, 'x-nexus-challenge': value.challenge, 'x-nexus-signature': signature })).value.error
  }
  assert.equal(await reopen(503), 'relayNotConfigured', 'reopened, and past every check to the relay')
  await shack.shack.post(`stations/${shack.stationId}/native/uninvite`, { email: guest.email })
  assert.equal(await reopen(401), 'signInRequired')
})

// --- The account's device list --------------------------------------------------------------------

test('the account lists every device of its stations and its own elsewhere, and removes one, or all', async () => {
  const owner = await person(), shack = await bind(owner), guest = await person(), stranger = await person()
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'listen' })
  const mine = await enrol(owner, shack.stationId), theirs = await enrol(guest, shack.stationId)
  const strangerShack = await bind(stranger), elsewhere = await enrol(stranger, strangerShack.stationId)
  const { value: list } = await owner.page.post('account/devices')
  const byId = Object.fromEntries(list.devices.map(d => [d.id, d]))
  assert.deepEqual(Object.keys(byId).sort(), [mine.deviceId, theirs.deviceId].sort())
  assert.deepEqual(byId[theirs.deviceId], { id: theirs.deviceId, stationId: shack.stationId, stationName: 'Shack', name: 'Laptop',
    kind: 'computer', platform: 'windows', role: 'listen', addedBy: 'account', guest: true, approved: true, removing: false })
  assert.equal(byId[mine.deviceId].guest, false)
  assert.deepEqual(list.invites.map(i => [i.stationId, i.role]), [[shack.stationId, 'listen']])
  // The guest sees only their own device there.
  assert.deepEqual((await guest.page.post('account/devices')).value.devices.map(d => d.id), [theirs.deviceId])
  // Not yours: refused alike whether it exists or not.
  assert.equal(await errorOf(stranger.page.post(`account/devices/${mine.deviceId}/remove`, {}, 404)), 'deviceUnavailable')
  assert.equal(await errorOf(owner.page.post(`account/devices/${elsewhere.deviceId}/remove`, {}, 404)), 'deviceUnavailable')
  assert.equal(await errorOf(owner.page.post(`account/devices/${crypto.randomUUID()}/remove`, {}, 404)), 'deviceUnavailable')
  assert.equal((await deviceRow(mine.deviceId)).approved, 1)
  // Removed: ended now, a new generation, the proof gone, the station told, and "removing" until it lists again.
  const before = await deviceRow(mine.deviceId), policy = (await stationRow(shack.stationId)).policy_version
  await owner.page.post(`account/devices/${mine.deviceId}/remove`)
  const after = await deviceRow(mine.deviceId)
  assert.equal(after.approved, 0); assert.equal(after.generation, before.generation + 1); assert.equal(after.proof, null)
  assert.ok(after.expires_at <= Date.now())
  assert.equal((await stationRow(shack.stationId)).policy_version, policy + 1)
  assert.equal((await owner.page.post('account/devices')).value.devices.find(d => d.id === mine.deviceId).removing, true)
  await shack.shack.post(`stations/${shack.stationId}/native/devices`, {}, 200, { ...KINDS, ...PROOF })
  assert.ok((await deviceRow(mine.deviceId)).revoke_seen_at > 0)
  assert.equal((await owner.page.post('account/devices')).value.devices.find(d => d.id === mine.deviceId), undefined, 'heard, and gone')
  // A guest removes their own; the owner removes everyone's at once.
  const g2 = await enrol(guest, shack.stationId), m2 = await enrol(owner, shack.stationId)
  await guest.page.post(`account/devices/${g2.deviceId}/remove`)
  assert.equal((await deviceRow(g2.deviceId)).approved, 0)
  const { value: all } = await owner.page.post('account/remove-all')
  assert.equal(all.removed, 2)
  for (const id of [theirs.deviceId, m2.deviceId]) assert.equal((await deviceRow(id)).approved, 0)
  assert.equal((await deviceRow(elsewhere.deviceId)).approved, 1, 'never another account\'s station')
  // From Nexus rather than the page: refused.
  assert.equal(await errorOf(owner.native.post('account/devices', {}, 403)), 'originDenied')
})

test('the owner removes an invite from the web, with every device that person has there', async () => {
  const owner = await person(), shack = await bind(owner), guest = await person(), stranger = await person()
  await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'operate' })
  const g = await enrol(guest, shack.stationId)
  const [invite] = (await owner.page.post('account/devices')).value.invites
  assert.equal(await errorOf(stranger.page.post(`account/invites/${invite.id}/remove`, {}, 404)), 'inviteUnavailable')
  assert.equal(await errorOf(guest.page.post(`account/invites/${invite.id}/remove`, {}, 404)), 'inviteUnavailable')
  await owner.page.post(`account/invites/${invite.id}/remove`)
  assert.equal((await deviceRow(g.deviceId)).approved, 0)
  assert.deepEqual((await owner.page.post('account/devices')).value.invites, [])
  assert.equal((await guest.native.post('client/session')).value.stations.length, 0)
})

// --- The trial, at the first internet use ----------------------------------------------------------

test('a bound station\'s trial starts at its first internet use, never on the LAN', async () => {
  const owner = await person(), shack = await bind(owner)
  const b = await browser(owner, shack.stationId)
  assert.equal(b.value.approved, true)
  const trial = () => app.db.prepare('SELECT source,started_at,expires_at FROM trials WHERE account_id=?').bind(owner.accountId).first()
  await owner.page.post(`stations/${shack.stationId}/ticket`, { scope: 'lan' })
  assert.equal(await trial(), null, 'a LAN ticket spends nothing')
  const at = Date.now()
  await owner.page.post(`stations/${shack.stationId}/ticket`)
  const started = await trial()
  assert.equal(started.source, 'trial')
  assert.ok(started.started_at >= at && started.expires_at === started.started_at + 14 * DAY)
  // A person whose trial runs under another sign-in is pointed at it, as at pairing.
  const shared = `shared-${crypto.randomUUID()}@example.invalid`, first = await person(shared), second = await person(shared)
  await entitle(first)
  const s2 = await bind(second), b2 = await browser(second, s2.stationId)
  assert.equal(b2.value.approved, true)
  assert.equal(await errorOf(second.page.post(`stations/${s2.stationId}/ticket`, {}, 403)), 'trialActiveElsewhere')
})

// --- Owner-device notice (the 2026-10-23 sign-in decision) ----------------------------------------------------------

test('a device added by sign-in emails the owner exactly once, with who/which-station/how-to-revoke', async () => {
  const main = app, sent = []
  const rt = await runtime({
    bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE, OWNER_NOTICE_API_KEY: 'test-key',
      OWNER_NOTICE_FROM: 'Nexus Remote <noreply@test.invalid>', OWNER_NOTICE_ENDPOINT: 'https://email.test.invalid/send' },
    outbound: async request => { sent.push({ url: request.url, auth: request.headers.get('authorization'), body: await request.json() }); return Response.json({ id: 'ok' }) },
  })
  app = rt // the helpers (person/bind/browser) use the module `app`; point them at this runtime.
  try {
    const owner = await person(), shack = await bind(owner), b = await browser(owner, shack.stationId)
    assert.equal(b.value.approved, true, 'granted on its sign-in')
    assert.equal(sent.length, 1, 'the owner notice fired exactly once on add')
    assert.equal(sent[0].url, 'https://email.test.invalid/send')
    assert.equal(sent[0].auth, 'Bearer test-key')
    assert.equal(sent[0].body.from, 'Nexus Remote <noreply@test.invalid>')
    assert.equal(sent[0].body.to, owner.email, 'to the account holder')
    assert.match(sent[0].body.text, /Chrome on Windows/, 'names the device')
    assert.match(sent[0].body.text, /Shack/, 'names the station')
    assert.match(sent[0].body.text, /remove it now/, 'says how to revoke')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})

test('with no provider secret the notice is a no-op for mail, but the durable record is still written', async () => {
  const main = app, sent = []
  const rt = await runtime({
    bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE },
    outbound: async request => { sent.push(request.url); return Response.json({}) },
  })
  app = rt
  try {
    const owner = await person(), shack = await bind(owner), b = await browser(owner, shack.stationId)
    assert.equal(b.value.approved, true, 'the device is added as before')
    assert.equal(sent.length, 0, 'no email is sent without the provider secret')
    // The precondition the station's transmit hold lifts against is written regardless of provider.
    const row = await rt.db.prepare('SELECT COUNT(*) AS n, MAX(sent_at) AS sent FROM notices WHERE device_id=?')
      .bind(b.deviceId).first()
    assert.equal(row.n, 1, 'the durable owner-notice row is written as the precondition')
    assert.equal(row.sent, null, 'and left unsent (no provider configured)')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})

// --- "No email, no unlock" (operator 2026-10-24): notified only when the owner's mail was accepted ---

// The provider secrets, wired, for the tests below. The endpoint is the stubbed capturer's.
const NOTICE_BINDINGS = {
  AUTH0_NATIVE_CLIENT_ID: NATIVE, OWNER_NOTICE_API_KEY: 'test-key',
  OWNER_NOTICE_FROM: 'Nexus Remote <noreply@test.invalid>', OWNER_NOTICE_ENDPOINT: 'https://email.test.invalid/send',
}
/** `notified` as the station reads it off the device list (the precondition its transmit hold lifts
 *  against): 1 only when the durable row exists AND the owner's mail was accepted. */
const notifiedOf = async (shack, stationId, deviceId) =>
  (await shack.shack.post(`stations/${stationId}/native/devices`, {}, 200, { ...KINDS, ...LIFETIME, ...PROOF }))
    .value.devices.find(d => d.id === deviceId)?.notified

test('with the provider wired and the send accepted, the device lists as notified', async () => {
  const main = app
  const rt = await runtime({ bindings: NOTICE_BINDINGS, outbound: async () => Response.json({ id: 'ok' }) })
  app = rt
  try {
    const owner = await person(), shack = await bind(owner), c = await enrol(owner, shack.stationId)
    assert.equal(c.value.approved, true, 'granted on its sign-in')
    const row = await rt.db.prepare('SELECT sent_at FROM notices WHERE device_id=?').bind(c.deviceId).first()
    assert.notEqual(row.sent_at, null, 'the accepted send marks the notice sent')
    assert.equal(await notifiedOf(shack, shack.stationId, c.deviceId), 1, 'so the station is told it is notified')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})

test('a provider rejection (non-2xx) leaves the device un-notified, the row still written', async () => {
  const main = app, sent = []
  const rt = await runtime({
    bindings: NOTICE_BINDINGS,
    outbound: async request => { sent.push(request.url); return new Response('rejected', { status: 422 }) },
  })
  app = rt
  try {
    const owner = await person(), shack = await bind(owner), c = await enrol(owner, shack.stationId)
    assert.equal(c.value.approved, true, 'the device is still added')
    assert.equal(sent.length, 1, 'the send was attempted')
    const row = await rt.db.prepare('SELECT COUNT(*) AS n, MAX(sent_at) AS sent FROM notices WHERE device_id=?').bind(c.deviceId).first()
    assert.equal(row.n, 1, 'the durable row is written regardless')
    assert.equal(row.sent, null, 'but a rejected send is NOT marked sent')
    assert.equal(await notifiedOf(shack, shack.stationId, c.deviceId), 0, 'so the station holds it')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})

test('a send that throws (network failure) leaves the device un-notified, the row still written', async () => {
  const main = app
  const rt = await runtime({
    bindings: NOTICE_BINDINGS,
    outbound: async () => { throw new Error('network down') },
  })
  app = rt
  try {
    const owner = await person(), shack = await bind(owner), c = await enrol(owner, shack.stationId)
    assert.equal(c.value.approved, true, 'the device is still added — a failed notice never fails registration')
    const row = await rt.db.prepare('SELECT COUNT(*) AS n, MAX(sent_at) AS sent FROM notices WHERE device_id=?').bind(c.deviceId).first()
    assert.equal(row.n, 1, 'the durable row is written regardless')
    assert.equal(row.sent, null, 'and a thrown send is NOT marked sent')
    assert.equal(await notifiedOf(shack, shack.stationId, c.deviceId), 0, 'so the station holds it')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})

test('a guest sign-in emails nobody and the guest device stays un-notified', async () => {
  const main = app, sent = []
  const rt = await runtime({
    bindings: NOTICE_BINDINGS,
    outbound: async request => { sent.push({ to: (await request.json()).to }); return Response.json({ id: 'ok' }) },
  })
  app = rt
  try {
    const owner = await person(), shack = await bind(owner), guest = await person()
    await shack.shack.post(`stations/${shack.stationId}/native/invite`, { email: guest.email, role: 'operate' })
    const g = await enrol(guest, shack.stationId)
    assert.equal(g.value.approved, true, 'the guest is approved by their invite and sign-in')
    // The owner's address is not available on a guest's sign-in, so nothing is sent (the guest is
    // never emailed), and the device stays held until a shack Approve.
    assert.equal(sent.length, 0, 'no email is sent on a guest sign-in')
    const row = await rt.db.prepare('SELECT COUNT(*) AS n, MAX(sent_at) AS sent FROM notices WHERE device_id=?').bind(g.deviceId).first()
    assert.equal(row.n, 1, 'the durable row is written regardless')
    assert.equal(row.sent, null, 'but it is left unsent')
    assert.equal(await notifiedOf(shack, shack.stationId, g.deviceId), 0, 'so the station holds the guest device')
  } finally {
    app = main
    await rt.mf.dispose()
  }
})
