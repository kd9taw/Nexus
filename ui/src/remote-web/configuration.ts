import type { Settings, RadioProgProject } from '../types'
import { SETTINGS_KEYS, SETTINGS_SHAPES, WITHHELD_RADIO_KEYS, WITHHELD_SETTINGS_KEYS } from './configuration-schema'
import { finite, integer, object, openObject, text } from './display-validation'
export type SettingsConfiguration = {settings:Record<string,unknown>;withheld:readonly string[];revision:string;platform:'linux'|'windows'|'macos'}
export type ProgrammingConfiguration = {mygrid:string;projects:RadioProgProject[];revision:string;saved:boolean}
const bad=():never=>{throw new Error('invalidConfiguration')}
const revision=(v:unknown)=>typeof v==='string'&&/^[0-9a-f]{64}$/.test(v)
export function parseConfiguration(raw:unknown,kind:'settings'|'programming'):SettingsConfiguration|ProgrammingConfiguration {
  if(kind==='settings'){
    // Still CLOSED - an unexpected top-level key is refused - but `radioWithheld` is allowed when
    // present. It is optional so a station older than it is not refused, and it must be LISTED when
    // present because object() counts keys exactly: leaving it out refused every document a current
    // station sends, while the fixtures (which never carried it) stayed green.
    const v=object(raw,['settings','withheld','revision','platform',
      ...(raw&&typeof raw==='object'&&Object.prototype.hasOwnProperty.call(raw,'radioWithheld')?['radioWithheld']:[])])
    // Tolerates a setting this browser has not heard of, so a station one release ahead is not
    // refused wholesale; every key we DO know must still be present and correctly typed.
    const values=openObject(v.settings,[...SETTINGS_KEYS])
    // The credential proof, stated rather than inferred. It used to rest on the key COUNT being
    // exact, which caught a leak only as arithmetic; now a withheld key appearing in the payload
    // is refused because it is withheld.
    if(WITHHELD_SETTINGS_KEYS.some(k=>Object.prototype.hasOwnProperty.call(values,k)))bad()
    if(!['linux','windows','macos'].includes(String(v.platform))||!revision(v.revision))bad()
    // SUPERSET, not equality. The station must declare every key THIS browser knows to be secret;
    // it may declare more. Exact equality made two harmless changes fatal - a station gaining an
    // 18th withheld credential, or somebody alphabetising a list that is currently grouped by
    // service - and each would have darkened Remote settings and Field Day for every operator
    // running an older browser. The security half is unchanged: a key we hold secret that the
    // station does NOT declare withheld is still refused, and line 17 above still refuses a
    // document that actually carries one. CI pins the two lists to each other so drift inside the
    // repo is caught at build time rather than by an operator.
    const declared=new Set((Array.isArray(v.withheld)?v.withheld:[bad()]).map(String))
    if(WITHHELD_SETTINGS_KEYS.some(k=>!declared.has(k)))bad()
    // A station that declares a key withheld and then sends it is contradicting itself. Refuse by the
    // STATION's own list, not only the keys this browser already knows - otherwise a credential newer
    // than this browser, declared and leaked together, sails through openObject untouched.
    if([...declared].some(k=>Object.prototype.hasOwnProperty.call(values,k)))bad()
    // OPTIONAL on purpose. A station older than this field cannot declare it, and refusing those
    // outright is the same outage this whole change exists to avoid - they also predate any
    // per-radio credential, so there is nothing for them to have leaked. When a station does
    // declare it, it must cover everything this browser holds secret. The content check below is
    // the defence either way, and it does not depend on the declaration at all.
    const declaredRadio=new Set(v.radioWithheld===undefined?[]:(Array.isArray(v.radioWithheld)?v.radioWithheld:[bad()]).map(String))
    if(v.radioWithheld!==undefined&&WITHHELD_RADIO_KEYS.some(k=>!declaredRadio.has(k)))bad()
    // The per-radio half of the credential proof: every key this browser holds secret AND every key
    // the station declared withheld.
    const radioSecret=[...WITHHELD_RADIO_KEYS,...declaredRadio]
    for(const radio of values.radios as Record<string,unknown>[])
      if(radioSecret.some(k=>Object.prototype.hasOwnProperty.call(radio,k)))bad()
    for(const [key,shape]of Object.entries(SETTINGS_SHAPES)){
      const value=values[key]
      if(value===null&&shape.startsWith('nullable-'))continue
      const type=shape.replace('nullable-','')
      if(type==='number'?!finite(value):type==='string'?!text(value,16*1024):type==='boolean'?typeof value!=='boolean':type==='array'?!Array.isArray(value):!value||typeof value!=='object'||Array.isArray(value))bad()
    }
    if((values.radios as unknown[]).length>64)bad()
    return raw as SettingsConfiguration
  }
  const v=object(raw,['mygrid','projects','revision','saved'])
  if(!text(v.mygrid,16)||!revision(v.revision)||typeof v.saved!=='boolean'||!Array.isArray(v.projects)||v.projects.length>128)bad()
  let count=0
  const ids=new Set<string>()
  for(const raw of v.projects as unknown[]){
    const p=object(raw,['id','name','createdUtc','updatedUtc','origin','radiusKm','channels'])
    if(!text(p.id,128)||!p.id||ids.has(p.id)||!text(p.name,1024)||!integer(p.createdUtc)||!integer(p.updatedUtc)||!finite(p.radiusKm)||!Array.isArray(p.channels))bad()
    ids.add(p.id as string);count+=(p.channels as unknown[]).length;if(count>10_000)bad()
    const origin=object(p.origin,['kind','grid','label','lat','lon']);if(!text(origin.kind,32)||!text(origin.grid,16)||!text(origin.label,1024)||!finite(origin.lat)||!finite(origin.lon))bad()
    for(const raw of p.channels as unknown[]){
      if(!raw||typeof raw!=='object'||Array.isArray(raw))bad()
      const c=raw as Record<string,unknown>
      if(!text(c.id,256)||!text(c.name,1024)||!finite(c.rxMhz)||!finite(c.offsetMhz)||!text(c.mode,32)||!text(c.duplex,32)||!text(c.toneMode,32)||!finite(c.rtoneHz)||!finite(c.ctoneHz)||!integer(c.dtcsCode)||!text(c.comment,4096))bad()
    }
  }
  return raw as ProgrammingConfiguration
}
/** The native form expects these fields structurally. They are not station
 * values: their account/backup sections are replaced by a station-managed note,
 * and all station writes are disabled. Never submit this adapter's output. */
export function settingsForm(doc:SettingsConfiguration):Settings {
  return {...doc.settings,...Object.fromEntries(WITHHELD_SETTINGS_KEYS.map(k=>[k,k==='voiceMessages'?[]:'']))} as unknown as Settings
}
