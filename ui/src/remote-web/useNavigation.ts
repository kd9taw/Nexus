import { createContext, useContext, useEffect, useState } from 'react'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { loadNavigation } from './navigation'
import type { ConnectData, NavigationDocument, SatelliteLive } from './navigation'
import type { NAVIGATION_COLLECTIONS } from './application-query-protocol'
import type { SatView } from '../types'

export const NavigationMapContext=createContext<{connect:ConnectData|null;satellites:SatView|null;track:SatelliteLive['track'];ageMs:number}|null>(null)
export function useNavigation<T>(kind:typeof NAVIGATION_COLLECTIONS[number],search='',active=true){
  const source=useContext(RemoteCollectionsContext),available=useStationData()
  const [capture,setCapture]=useState<{doc:NavigationDocument<T>;at:number;key:string}|null>(null)
  const [loading,setLoading]=useState(true),[refresh,setRefresh]=useState(0),[now,setNow]=useState(()=>performance.now())
  const key=kind+'|'+search
  useEffect(()=>{if(!source||!available)return;const timer=setInterval(()=>setNow(performance.now()),500);return()=>clearInterval(timer)},[source,available])
  useEffect(()=>{
    setCapture(null);setLoading(true)
    if(!source||!available||!active)return
    let live=true,timer:ReturnType<typeof setTimeout>
    async function read(){
      let next=2000
      try{
        const doc=await loadNavigation<T>(source!,kind,search,()=>live)
        if(live){setCapture({doc,at:performance.now()-doc.ageMs,key});setLoading(false)}
        // Planning documents can contain the full catalog. Refresh before their
        // source deadline instead of retransmitting megabytes every two seconds.
        next=Math.min(25_000,Math.max(1000,doc.validForMs-doc.ageMs-2000))
      }catch(error){
        if(live&&(!(error instanceof Error)||error.message!=='applicationBusy')){setCapture(null);setLoading(false)}
      }
      if(live){setNow(performance.now());timer=setTimeout(()=>void read(),next)}
    }
    void read();return()=>{live=false;clearTimeout(timer)}
  },[source,available,active,kind,search,key,refresh])
  const age=capture?Math.max(0,now-capture.at):Infinity
  const value=source&&available&&active&&capture?.key===key&&age<capture.doc.validForMs?capture.doc.value:null
  return {remote:!!source,value,loading,ageMs:age,refresh:()=>setRefresh(n=>n+1)}
}
export function useSatelliteLive(active=true){
  const source=useContext(RemoteCollectionsContext),available=useStationData()
  const [value,setValue]=useState<SatelliteLive|null>(null)
  useEffect(()=>{
    setValue(null)
    if(!source||!available||!active)return
    let live=true,timer:ReturnType<typeof setTimeout>
    async function read(){
      try{const sample=await source!.client.invoke<SatelliteLive>('get_remote_satellite_state');if(live)setValue(sample)}
      catch{if(live)setValue(null)}
      if(live)timer=setTimeout(()=>void read(),1000)
    }
    void read();return()=>{live=false;clearTimeout(timer)}
  },[source,available,active])
  return source&&available&&active?value:null
}

/** Favorites remain this browser's choice. Assemble one coherent native log/grid
 * context, clearing the whole ranking if any bird or revision is unavailable. */
export function useSatelliteSchedule(namesKey:string){
  const source=useContext(RemoteCollectionsContext),available=useStationData()
  const [capture,setCapture]=useState<{rows:import('../types').SatPass[];key:string;grid:string;until:number}|null>(null)
  const [loading,setLoading]=useState(true),[now,setNow]=useState(()=>performance.now())
  useEffect(()=>{if(!source||!available)return;const timer=setInterval(()=>setNow(performance.now()),500);return()=>clearInterval(timer)},[source,available])
  useEffect(()=>{
    setCapture(null);setLoading(true)
    if(!source||!available)return
    let live=true,timer:ReturnType<typeof setTimeout>
    const names=namesKey?namesKey.split(','):[]
    async function read(){
      let next=2000
      try{
        if(names.length>64)throw new Error('applicationTooLarge')
        let context='',grid='',until=Infinity
        const rows:import('../types').SatPass[]=[]
        for(const name of names){
          const doc=await loadNavigation<import('./navigation').SatelliteDetailData>(source!,'satellite',name,()=>live)
          if((context&&context!==doc.stationContextId)||(grid&&grid!==doc.value.mygrid))throw new Error('queryExpired')
          context=doc.stationContextId;grid=doc.value.mygrid
          until=Math.min(until,performance.now()+doc.validForMs-doc.ageMs)
          rows.push(...doc.value.schedule)
          if(rows.length>5000)throw new Error('applicationTooLarge')
        }
        if(!live)return
        if(performance.now()>=until)throw new Error('queryExpired')
        rows.sort((a,b)=>a.aosUnix-b.aosUnix)
        setCapture({rows,key:namesKey,grid,until});setLoading(false)
        next=Math.min(25_000,Math.max(1000,until-performance.now()-2000))
      }catch(error){if(live){setCapture(null);if(!(error instanceof Error)||error.message!=='applicationBusy')setLoading(false)}}
      if(live){setNow(performance.now());timer=setTimeout(()=>void read(),next)}
    }
    void read();return()=>{live=false;clearTimeout(timer)}
  },[source,available,namesKey])
  return {value:available&&capture?.key===namesKey&&now<capture.until?capture:null,loading}
}
