import {expect,it} from 'vitest'
import settings from './__fixtures__/configuration-settings.json'
import programming from './__fixtures__/configuration-programming.json'
import keysOf1130 from './__fixtures__/settings-keys-1.13.0.json'
import {parseConfiguration,settingsForm,type SettingsConfiguration} from './configuration'
import {NEWER_SETTINGS,SETTINGS_KEYS,STATION_LOCAL_SETTINGS_KEYS,WITHHELD_RADIO_KEYS,WITHHELD_SETTINGS_KEYS,WRITABLE_CONTROL_SETTINGS_KEYS,WRITABLE_LOGGING_SETTINGS_KEYS} from './configuration-schema'
it('accepts the native serialized station choices and excludes account fields instead of supplying defaults',()=>{
  const parsed=parseConfiguration(structuredClone(settings),'settings') as SettingsConfiguration
  expect(parsed.settings.mycall).toBe('W1AW');expect(parsed.settings.mygrid).toBe('FN31RX09')
  expect(Object.keys(parsed.settings).sort()).toEqual([...SETTINGS_KEYS].sort())
  for(const key of WITHHELD_SETTINGS_KEYS)expect(parsed.settings).not.toHaveProperty(key)
  const adapted=settingsForm(parsed)
  expect(adapted.mycall).toBe('W1AW');expect(adapted.voiceMessages).toEqual([])
  for(const change of [{settings:{...settings.settings,clublogApiKey:'private'}},{withheld:[]},{revision:'current'},{platform:'browser'},{settings:{...settings.settings,radios:'unknown'}}])expect(()=>parseConfiguration({...settings,...change},'settings')).toThrow()
  // Every key but the newer settings an older station never had (their own test is below).
  for(const key of SETTINGS_KEYS.filter(k=>!Object.prototype.hasOwnProperty.call(NEWER_SETTINGS,k))){const broken=structuredClone(settings) as unknown as {settings:Record<string,unknown>};delete broken.settings[key];expect(()=>parseConfiguration(broken,'settings')).toThrow()}
})
it('retains all 1200 saved channels with exact metadata and fails closed on malformed or oversized project lists',()=>{
  expect(parseConfiguration(structuredClone(programming),'programming')).toEqual(programming)
  expect(programming.projects[0].channels).toHaveLength(1200)
  for(const change of [{revision:''},{projects:[programming.projects[0],programming.projects[0]]},{saved:'yes'},
    {projects:[{...programming.projects[0],channels:[{...programming.projects[0].channels[0],rxMhz:Infinity}]}]},
    {projects:[{...programming.projects[0],channels:Array.from({length:10001},()=>programming.projects[0].channels[0])}]}])expect(()=>parseConfiguration({...programming,...change},'programming')).toThrow()
})

// The withheld declaration used to be compared with JSON.stringify, so it matched on ORDER as well
// as content. Two harmless changes were therefore fatal to every already-deployed browser: a station
// gaining an 18th withheld credential, and somebody alphabetising a list that is currently grouped
// by service. Both would have darkened Remote settings and Field Day for every operator - the exact
// outage this file's other gate exists to prevent, reproduced on the one list that carries
// credentials. Containment keeps the security half and drops the brittleness.
it('accepts a station that withholds MORE than this browser knows, in any order', async () => {
  const ahead = structuredClone(settings) as Record<string, unknown>
  ahead.withheld = [...(ahead.withheld as string[])].reverse().concat('someFutureCredential')
  expect(() => parseConfiguration(ahead, 'settings')).not.toThrow()

  // ...and still refuses a station that stops declaring something we hold secret.
  const short = structuredClone(settings) as Record<string, unknown>
  short.withheld = (short.withheld as string[]).slice(1)
  expect(() => parseConfiguration(short, 'settings')).toThrow()
})

// A station older than the start-at-sign-in fields cannot declare them withheld. They are station-local,
// not credentials, so that station must still be accepted - one browser serves every station version.
it('accepts an older station that predates the station-local withheld keys', () => {
  const older = structuredClone(settings) as Record<string, unknown>
  older.withheld = (older.withheld as string[]).filter(k => !(STATION_LOCAL_SETTINGS_KEYS as readonly string[]).includes(k))
  expect((older.withheld as string[]).length, 'positive control: the fixture declared them').toBe((settings.withheld as string[]).length - STATION_LOCAL_SETTINGS_KEYS.length)
  expect(() => parseConfiguration(older, 'settings')).not.toThrow()
  expect(settingsForm(parseConfiguration(older, 'settings') as SettingsConfiguration).launchAtLogin).toBe(false)
})

// The contest log's EMAIL is withheld (a browser has no use for it) and newer than every station
// already in the field. A station that predates it cannot declare it withheld, and refusing that
// station's whole settings document over a field it never had is the outage the station-local list
// exists to prevent — so the older station is accepted, and the form still gets a STRING.
it('accepts an older station that predates the contest email field', () => {
  const older = structuredClone(settings) as Record<string, unknown>
  older.withheld = (older.withheld as string[]).filter(k => k !== 'contestEmail')
  expect((older.withheld as string[]).length, 'positive control: the fixture declared it').toBe((settings.withheld as string[]).length - 1)
  expect(() => parseConfiguration(older, 'settings')).not.toThrow()
  expect(settingsForm(parseConfiguration(older, 'settings') as SettingsConfiguration).contestEmail).toBe('')
  // …and a station that declares it withheld and sends it anyway is still refused.
  const leaked = structuredClone(settings) as {settings: Record<string, unknown>}
  leaked.settings.contestEmail = 'op@example.com'
  expect(() => parseConfiguration(leaked, 'settings')).toThrow()
})

// A 1.13.0 station predates two settings this page knows: automatic cluster-node choice and the
// login SSID. The page required every key it knew, so the relay deploy that carries it would have
// refused the whole settings document of every station still on 1.13.0 over two fields it never had.
// It is accepted, and the form describes what that station actually does: it connects to its node
// list as written, and logs in with the bare callsign.
it('accepts a 1.13.0 station, which predates the cluster node choice and the login SSID', () => {
  const older = structuredClone(settings) as unknown as {settings: Record<string, unknown>}
  for (const key of ['clusterNodesAuto', 'clusterSsid']) {
    expect(older.settings, `positive control: the fixture sends ${key}`).toHaveProperty(key)
    delete older.settings[key]
  }
  const parsed = parseConfiguration(older, 'settings') as SettingsConfiguration
  // The document stays what the station sent; only the form fills the gap.
  expect(parsed.settings).not.toHaveProperty('clusterNodesAuto')
  const form = settingsForm(parsed)
  expect(form.clusterNodesAuto).toBe(false)
  expect(form.clusterSsid).toBe('')
})

// The tolerance is for ABSENCE, of those settings only. A station that sends one is read exactly as
// before: its own value, type-checked, never replaced by the stand-in.
it('still type-checks a newer setting that a station sends, and never replaces its value', () => {
  for (const [key, value] of [['clusterSsid', 2], ['clusterSsid', null], ['clusterNodesAuto', 'yes'], ['clusterNodesAuto', null]] as const) {
    const malformed = structuredClone(settings) as unknown as {settings: Record<string, unknown>}
    malformed.settings[key] = value
    expect(() => parseConfiguration(malformed, 'settings'), `${key}: ${JSON.stringify(value)}`).toThrow('invalidConfiguration')
  }
  const current = structuredClone(settings) as unknown as {settings: Record<string, unknown>}
  current.settings.clusterSsid = '7'
  expect(current.settings.clusterNodesAuto, 'positive control: the station value differs from the stand-in').toBe(true)
  const parsed = parseConfiguration(current, 'settings') as SettingsConfiguration
  expect(parsed).toEqual(current)
  expect(settingsForm(parsed).clusterNodesAuto).toBe(true)
  expect(settingsForm(parsed).clusterSsid).toBe('7')
})

// THE GATE FOR THE NEXT ONE. A relay deploy reaches every station at once, and each station updates
// on its own time, so a setting added to SETTINGS_KEYS is one that stations in the field do not send.
// Each must be in NEWER_SETTINGS, with the value that says what such a station does. The frozen list
// is the 1.13.0 station's own projection (SETTINGS_KEYS in query/configuration.rs at v1.13.0), the
// oldest station this page serves.
it('lets a station omit exactly the settings newer than 1.13.0, and none of them is writable', () => {
  expect(keysOf1130.length, 'positive control: the frozen 1.13.0 list is really read').toBe(272)
  expect(SETTINGS_KEYS.filter(key => !keysOf1130.includes(key)).sort()).toEqual(Object.keys(NEWER_SETTINGS).sort())
  // Each stand-in is a value a station could send: a document carrying them all parses.
  const standIns = structuredClone(settings) as unknown as {settings: Record<string, unknown>}
  Object.assign(standIns.settings, NEWER_SETTINGS)
  expect(() => parseConfiguration(standIns, 'settings')).not.toThrow()
  // None is writable, so no edit is offered for a key the station did not report, and a stand-in can
  // never be counted as a change and sent to a station that does not have the setting.
  const writable: readonly string[] = [...WRITABLE_CONTROL_SETTINGS_KEYS, ...WRITABLE_LOGGING_SETTINGS_KEYS]
  expect(Object.keys(NEWER_SETTINGS).filter(key => writable.includes(key))).toEqual([])
})

// RADIO_WITHHELD_KEYS is empty today, so this pins the mechanism rather than a current secret: the
// first per-radio credential must be refused on arrival, not merely undeclared.
it('refuses a per-radio credential even when the station declares it withheld', async () => {
  const leaked = structuredClone(settings) as Record<string, unknown>
  const values = leaked.settings as Record<string, unknown>
  const radios = values.radios as Record<string, unknown>[]
  expect(radios.length, 'positive control: there is a radio to leak from').toBeGreaterThan(0)
  radios[0].flexPassword = 'hunter2'
  leaked.radioWithheld = ['flexPassword']
  // Declaring it withheld and then sending it anyway is a station contradicting itself, and it is
  // refused by name using the keys the STATION declared. This test once passed for the wrong reason:
  // object() refused the extra top-level radioWithheld key before the per-radio check ever ran.
  expect(() => parseConfiguration(leaked, 'settings')).toThrow()
})

// THE SHAPE A CURRENT STATION SENDS. The fixture predates radioWithheld, so every test above passed
// while the browser refused every real settings document: object() counts keys exactly, and the field
// was on the wire but not on the list. Only the native suite, reading a real Rust document, saw it.
it('accepts the document a current station actually sends, and stays closed to anything else', async () => {
  const current = structuredClone(settings) as Record<string, unknown>
  current.radioWithheld = []
  expect(() => parseConfiguration(current, 'settings')).not.toThrow()
  // Positive control on "closed": one unexpected top-level key is still refused.
  const intruder = structuredClone(current) as Record<string, unknown>
  intruder.somethingUnexpected = true
  expect(() => parseConfiguration(intruder, 'settings')).toThrow()
})

// The settings half of the same rule: a credential newer than this browser, declared withheld and sent
// anyway, would otherwise pass straight through openObject, which tolerates keys it has not heard of.
it('refuses a settings value the station itself declared withheld', async () => {
  const leaked = structuredClone(settings) as Record<string, unknown>
  ;(leaked.settings as Record<string, unknown>).futureSecret = 'hunter2'
  leaked.withheld = [...(leaked.withheld as string[]), 'futureSecret']
  expect(() => parseConfiguration(leaked, 'settings')).toThrow()
})

// THE GATE THAT WOULD HAVE CAUGHT TONIGHT'S BUG.
//
// The settings projection is maintained by hand in three places - settings.rs says outright that
// `ui/src/types.ts` and `defaultSettings.json` are copy-pasted from it - and the browser validates
// the payload against a CLOSED schema. So exposing a setting on the Rust side without adding it
// here does not degrade gracefully: the browser refuses the whole settings document and the views
// that depend on it go blank. Eleven contest settings and `cwReverse` were added natively and not
// here, and Remote's Field Day and settings surfaces were dark for every operator until it was
// found by reading a CI failure backwards.
//
// This reads the Rust list the way BrowserApplication.test.tsx already does rather than keeping a
// fourth copy to drift. `application.rs` carries the smaller LIVE-STREAM projection, which is a
// deliberate subset, so it is checked for containment and not equality.
it('keeps the browser settings schema identical to the native projection it must accept', async () => {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  const list = (path: string) => {
    const src = readFileSync(resolve(path), 'utf8')
    const block = src.match(/const SETTINGS_KEYS: &\[&str\] = &\[([\s\S]*?)\];/)
    expect(block, `no SETTINGS_KEYS in ${path}`).toBeTruthy()
    return [...block![1].matchAll(/"([a-zA-Z0-9]+)"/g)].map(m => m[1])
  }
  const projection = list('../src-tauri/src/remote_service/query/configuration.rs')
  const stream = list('../src-tauri/src/remote_service/application.rs')

  // Positive control: the reader really finds keys, so an empty match cannot pass as agreement.
  expect(projection.length).toBeGreaterThan(200)
  expect(stream.length).toBeGreaterThan(20)

  expect([...SETTINGS_KEYS].sort()).toEqual([...projection].sort())
  expect(stream.filter(key => !projection.includes(key)),
    'the live settings stream may narrow the projection, never exceed it').toEqual([])
  expect(SETTINGS_KEYS.filter(key => (WITHHELD_SETTINGS_KEYS as readonly string[]).includes(key)),
    'a withheld key must never also be exposed').toEqual([])

  // The WITHHELD lists were never tied together in either direction. That matters more than the
  // exposed list, not less: the most likely edit to it is adding a NEWLY WITHHELD CREDENTIAL, and
  // until now nothing would have noticed the browser's copy going stale. Compared as SETS - the
  // runtime rule is containment, deliberately, so a station may withhold more than this browser
  // knows - but inside the repo the two must say the same thing.
  const named = (path: string, constant: string) => {
    const src = readFileSync(resolve(path), 'utf8')
    const block = src.match(new RegExp(`const ${constant}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`))
    expect(block, `no ${constant} in ${path}`).toBeTruthy()
    return [...block![1].matchAll(/"([a-zA-Z0-9]+)"/g)].map(m => m[1])
  }
  const CONFIG = '../src-tauri/src/remote_service/query/configuration.rs'
  const withheld = named(CONFIG, 'WITHHELD_KEYS')
  expect(withheld.length, 'positive control: the reader really finds withheld keys').toBeGreaterThan(10)
  expect([...WITHHELD_SETTINGS_KEYS, ...STATION_LOCAL_SETTINGS_KEYS].sort()).toEqual([...withheld].sort())

  // RADIO_WITHHELD_KEYS is empty today and that is the point - it is where the first per-radio
  // credential goes. Empty on both sides is agreement; empty on one is the drift this catches.
  const radioWithheld = named(CONFIG, 'RADIO_WITHHELD_KEYS')
  expect([...WITHHELD_RADIO_KEYS].sort()).toEqual([...radioWithheld].sort())
  const radioKeys = named(CONFIG, 'RADIO_KEYS')
  expect(radioKeys.length, 'positive control: RADIO_KEYS really read').toBeGreaterThan(30)
  expect(radioKeys.filter(key => radioWithheld.includes(key)),
    'a withheld per-radio key must never also be exposed').toEqual([])

  // The settings a browser may CHANGE: the same on both sides, and every other exposed setting
  // explicitly denied by the station. The station's own coverage test holds the denial list too.
  const writableControl = named(CONFIG, 'WRITABLE_CONTROL_KEYS'), writableLogging = named(CONFIG, 'WRITABLE_LOGGING_KEYS')
  const denied = named(CONFIG, 'WRITE_DENIED_KEYS')
  expect(writableControl.length, 'positive control: WRITABLE_CONTROL_KEYS really read').toBeGreaterThan(20)
  expect(denied.length, 'positive control: WRITE_DENIED_KEYS really read').toBeGreaterThan(200)
  expect([...WRITABLE_CONTROL_SETTINGS_KEYS].sort()).toEqual([...writableControl].sort())
  expect([...WRITABLE_LOGGING_SETTINGS_KEYS].sort()).toEqual([...writableLogging].sort())
  expect(SETTINGS_KEYS.filter(key => ![...writableControl, ...writableLogging, ...denied].includes(key)),
    'an exposed setting nobody classified as writable or denied').toEqual([])
})

// Relaxing the exact key count is what lets a station one release ahead be understood at all.
// It also removes the arithmetic that INCIDENTALLY caught a leaked credential, so the leak check
// is now stated outright - and this is the test that proves it did not become weaker.
it('understands a station one release ahead, and still refuses a leaked credential', () => {
  const base = structuredClone(settings) as unknown as { settings: Record<string, unknown> }

  // A setting this browser has never heard of must not sink the whole document.
  const ahead = structuredClone(base)
  ahead.settings.somethingAddedNextRelease = 'value'
  expect(() => parseConfiguration(ahead, 'settings')).not.toThrow()

  // A withheld key appearing in the payload is a credential leak and must be refused, by name
  // rather than by the key count happening to come out wrong.
  for (const key of WITHHELD_SETTINGS_KEYS) {
    const leaked = structuredClone(base)
    leaked.settings[key] = 'secret'
    expect(() => parseConfiguration(leaked, 'settings'), `${key} leaked into settings`).toThrow('invalidConfiguration')
  }

  // Positive control: the leak check is doing the refusing, not some unrelated guard. Removing
  // the extra key from the same document makes it parse again.
  const control = structuredClone(base)
  control.settings[WITHHELD_SETTINGS_KEYS[0]] = 'secret'
  delete control.settings[WITHHELD_SETTINGS_KEYS[0]]
  expect(() => parseConfiguration(control, 'settings')).not.toThrow()

  // A known key that is MISSING is still a refusal - this relaxes what may be extra, never what
  // must be present.
  const short = structuredClone(base)
  delete short.settings.mycall
  expect(() => parseConfiguration(short, 'settings')).toThrow()
})
