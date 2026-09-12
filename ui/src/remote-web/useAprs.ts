import { useContext, useEffect, useState } from 'react'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { loadAprsRoster } from './aprs'
import type { AprsLive, AprsRoster } from './aprs'

export function useAprs(active: boolean) {
  const source=useContext(RemoteCollectionsContext), available=useStationData()
  const [sample,setSample]=useState<AprsLive|null>(null)
  const [sampleLoading,setSampleLoading]=useState(true)
  const [capture,setCapture]=useState<{value:AprsRoster;at:number}|null>(null)
  const [loading,setLoading]=useState(false),[refresh,setRefresh]=useState(0)
  const [now,setNow]=useState(()=>performance.now())
  useEffect(()=>{
    setSample(null)
    setSampleLoading(true)
    if(!source||!active||!available)return
    let live=true,timer:ReturnType<typeof setTimeout>
    async function read(){
      try { const value=await source!.client.invoke<AprsLive>('get_remote_aprs_state');if(live)setSample(value) }
      catch { if(live)setSample(null) }
      if(live){setSampleLoading(false);setNow(performance.now());timer=setTimeout(()=>void read(),1000)}
    }
    void read()
    return()=>{live=false;clearTimeout(timer)}
  },[source,active,available])
  useEffect(()=>{
    setCapture(null)
    if(!source||!active||!available){setLoading(false);return}
    let live=true,timer:ReturnType<typeof setTimeout>
    async function read(){
      setLoading(true)
      try { const value=await loadAprsRoster(source!,()=>live);if(live)setCapture({value,at:performance.now()-value.capturedAgeMs}) }
      catch { if(live)setCapture(null) }
      if(live){setLoading(false);setNow(performance.now());timer=setTimeout(()=>void read(),10_000)}
    }
    void read()
    return()=>{live=false;clearTimeout(timer)}
  },[source,active,available,refresh])
  const value=source&&available&&active&&capture&&now-capture.at<60_000?capture.value:null
  return {remote:!!source,sample:available&&active?sample:null,sampleLoading:available&&active&&sampleLoading,value,loading,ageMs:capture?Math.max(0,now-capture.at):Infinity,refresh:()=>setRefresh(n=>n+1)}
}
