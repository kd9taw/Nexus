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
