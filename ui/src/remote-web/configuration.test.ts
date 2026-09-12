import {expect,it} from 'vitest'
import settings from './__fixtures__/configuration-settings.json'
import programming from './__fixtures__/configuration-programming.json'
import {parseConfiguration,settingsForm,type SettingsConfiguration} from './configuration'
import {SETTINGS_KEYS,WITHHELD_SETTINGS_KEYS} from './configuration-schema'
it('accepts the native serialized station choices and excludes account fields instead of supplying defaults',()=>{
  const parsed=parseConfiguration(structuredClone(settings),'settings') as SettingsConfiguration
  expect(parsed.settings.mycall).toBe('W1AW');expect(parsed.settings.mygrid).toBe('FN31RX09')
  expect(Object.keys(parsed.settings).sort()).toEqual([...SETTINGS_KEYS].sort())
  for(const key of WITHHELD_SETTINGS_KEYS)expect(parsed.settings).not.toHaveProperty(key)
  const adapted=settingsForm(parsed)
  expect(adapted.mycall).toBe('W1AW');expect(adapted.voiceMessages).toEqual([])
  for(const change of [{settings:{...settings.settings,clublogApiKey:'private'}},{withheld:[]},{revision:'current'},{platform:'browser'},{settings:{...settings.settings,radios:'unknown'}}])expect(()=>parseConfiguration({...settings,...change},'settings')).toThrow()
  for(const key of SETTINGS_KEYS){const broken=structuredClone(settings) as unknown as {settings:Record<string,unknown>};delete broken.settings[key];expect(()=>parseConfiguration(broken,'settings')).toThrow()}
})
it('retains all 1200 saved channels with exact metadata and fails closed on malformed or oversized project lists',()=>{
  expect(parseConfiguration(structuredClone(programming),'programming')).toEqual(programming)
  expect(programming.projects[0].channels).toHaveLength(1200)
  for(const change of [{revision:''},{projects:[programming.projects[0],programming.projects[0]]},{saved:'yes'},
    {projects:[{...programming.projects[0],channels:[{...programming.projects[0].channels[0],rxMhz:Infinity}]}]},
    {projects:[{...programming.projects[0],channels:Array.from({length:10001},()=>programming.projects[0].channels[0])}]}])expect(()=>parseConfiguration({...programming,...change},'programming')).toThrow()
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
})
