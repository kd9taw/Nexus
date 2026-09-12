import type { Settings, RadioProgProject } from '../types'
import { SETTINGS_KEYS, SETTINGS_SHAPES, WITHHELD_SETTINGS_KEYS } from './configuration-schema'
import { finite, integer, object, openObject, text } from './display-validation'
export type SettingsConfiguration = {settings:Record<string,unknown>;withheld:readonly string[];revision:string;platform:'linux'|'windows'|'macos'}
export type ProgrammingConfiguration = {mygrid:string;projects:RadioProgProject[];revision:string;saved:boolean}
const bad=():never=>{throw new Error('invalidConfiguration')}
const revision=(v:unknown)=>typeof v==='string'&&/^[0-9a-f]{64}$/.test(v)
export function parseConfiguration(raw:unknown,kind:'settings'|'programming'):SettingsConfiguration|ProgrammingConfiguration {
  if(kind==='settings'){
    const v=object(raw,['settings','withheld','revision','platform'])
    // Tolerates a setting this browser has not heard of, so a station one release ahead is not
    // refused wholesale; every key we DO know must still be present and correctly typed.
    const values=openObject(v.settings,[...SETTINGS_KEYS])
    // The credential proof, stated rather than inferred. It used to rest on the key COUNT being
    // exact, which caught a leak only as arithmetic; now a withheld key appearing in the payload
    // is refused because it is withheld.
    if(WITHHELD_SETTINGS_KEYS.some(k=>Object.prototype.hasOwnProperty.call(values,k)))bad()
    if(!['linux','windows','macos'].includes(String(v.platform))||!revision(v.revision)||JSON.stringify(v.withheld)!==JSON.stringify(WITHHELD_SETTINGS_KEYS))bad()
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
