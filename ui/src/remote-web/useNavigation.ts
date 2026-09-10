import { createContext, useContext, useEffect, useState } from 'react'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { loadNavigation } from './navigation'
import type { ConnectData, NavigationDocument, SatelliteLive } from './navigation'
import type { DocumentCollection } from './application-query-protocol'
import type { SatView } from '../types'

export const NavigationMapContext=createContext<{connect:ConnectData|null;satellites:SatView|null;track:SatelliteLive['track'];ageMs:number}|null>(null)
// Leave time to assemble the next document (or whole favorites set). A fixed
// two-second lead let slow, healthy refreshes expire the displayed schedule and
// collapse the page underneath the operator. Expiry itself is never extended.
function refreshDelay(remaining:number,transferMs:number){
  return Math.min(25_000,Math.max(1000,remaining-Math.max(2000,transferMs*2)))
}
export function useNavigation<T>(kind:DocumentCollection,search='',active=true){
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
      const started=performance.now()
      let next=2000
      try{
        const doc=await loadNavigation<T>(source!,kind,search,()=>live)
        if(live){setCapture({doc,at:performance.now()-doc.ageMs,key});setLoading(false)}
        // Planning documents can contain the full catalog. Refresh before their
        // source deadline instead of retransmitting megabytes every two seconds.
        next=refreshDelay(doc.validForMs-doc.ageMs,performance.now()-started)
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
    type Anchor={name:string;id:string;context:string;grid:string;until:number}
    let anchor:Anchor|null=null
    async function read(){
      const started=performance.now()
      let next=2000
      try{
        if(names.length>64)throw new Error('applicationTooLarge')
        let context='',grid='',until=Infinity
        let nextAnchor:Anchor|null=null
        const rows:import('../types').SatPass[]=[]
        // Probe the earliest-expiring document first. If the native worker
        // still reuses that exact capture, rereading every other bird cannot
        // advance this schedule's deadline and wastes the next refresh window.
        const probe=anchor
        const ordered=probe?[probe.name,...names.filter(name=>name!==probe.name)]:names
        for(const name of ordered){
          const doc=await loadNavigation<import('./navigation').SatelliteDetailData>(source!,'satellite',name,()=>live)
          if(probe&&name===probe.name&&doc.documentId===probe.id&&doc.stationContextId===probe.context&&doc.value.mygrid===probe.grid&&performance.now()<probe.until){
            next=1000
            return
          }
          if((context&&context!==doc.stationContextId)||(grid&&grid!==doc.value.mygrid))throw new Error('queryExpired')
          context=doc.stationContextId;grid=doc.value.mygrid
          const expires=performance.now()+doc.validForMs-doc.ageMs
          if(!nextAnchor||expires<nextAnchor.until)nextAnchor={name,id:doc.documentId,context,grid,until:expires}
          until=Math.min(until,expires)
          rows.push(...doc.value.schedule)
          if(rows.length>5000)throw new Error('applicationTooLarge')
        }
        if(!live)return
        if(performance.now()>=until)throw new Error('queryExpired')
        rows.sort((a,b)=>a.aosUnix-b.aosUnix)
        anchor=nextAnchor
        setCapture({rows,key:namesKey,grid,until});setLoading(false)
        next=refreshDelay(until-performance.now(),performance.now()-started)
      }catch(error){
        // Congestion does not invalidate a still-fresh, coherent capture. The
        // original deadline and station availability checks continue to hide it.
        if(live&&(!(error instanceof Error)||error.message!=='applicationBusy')){anchor=null;setCapture(null);setLoading(false)}
      }finally{
        if(live){setNow(performance.now());timer=setTimeout(()=>void read(),next)}
      }
    }
    void read();return()=>{live=false;clearTimeout(timer)}
  },[source,available,namesKey])
  return {value:available&&capture?.key===namesKey&&now<capture.until?capture:null,loading}
}
