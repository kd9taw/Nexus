import { useContext, useEffect, useState } from 'react'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { parseJs8Context } from './js8'
import type { Js8Context } from './js8'

/** The native cockpit keeps its existing full-log read. Hosted observers share
 * the station's complete-log projection; a missing entry never means unworked. */
export function useJs8Context(active: boolean, calls: string) {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const [capture,setCapture] = useState<{value:Js8Context;at:number;key:string}|null>(null)
  const [loading,setLoading] = useState(false), [refresh,setRefresh] = useState(0)
  const key = JSON.stringify([calls,refresh])
  const [settledKey,setSettledKey] = useState<string|null>(null)
  const [now,setNow] = useState(() => performance.now())
  useEffect(() => {
    if (!source || !active) return
    const timer = setInterval(() => setNow(performance.now()),1000)
    return () => clearInterval(timer)
  },[source,active])
  useEffect(() => {
    setCapture(null)
    if (!source || !active || !available) { setLoading(false); return }
    let live = true, timer: ReturnType<typeof setTimeout>
    async function load() {
      const started = performance.now()
      setLoading(true)
      try {
        const page = await source!.page({collection:'js8Context',cursor:null,search:'',unconfirmed:false,after:null})
        const value = parseJs8Context(page)
        if (live) { setCapture({value,at:started-value.capturedAgeMs-page.ageMs,key}); setNow(performance.now()) }
      } catch { if (live) setCapture(null) }
      if (live) { setLoading(false); setSettledKey(key); timer = setTimeout(() => void load(),30_000) }
    }
    void load()
    return () => { live=false;clearTimeout(timer) }
  },[source,active,available,key])
  // A new heard-call set renders before its effect starts the next request.
  // The previous roster cannot claim to describe that new set in this interval.
  const value = source && available && capture?.key === key && now-capture.at < 60_000 ? capture.value : null
  return { remote:source !== null, value, loading:loading || !!source && active && available && settledKey !== key, refresh:() => setRefresh(n=>n+1) }
}
