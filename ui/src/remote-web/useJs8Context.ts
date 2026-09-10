import { useContext, useEffect, useState } from 'react'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { parseJs8Context } from './js8'
import type { Js8Context } from './js8'

/** The native cockpit keeps its existing full-log read. Hosted observers share
 * the station's complete-log projection; a missing entry never means unworked. */
export function useJs8Context(active: boolean, calls: string) {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const [capture,setCapture] = useState<{value:Js8Context;at:number}|null>(null)
  const [loading,setLoading] = useState(false), [refresh,setRefresh] = useState(0)
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
        if (live) { setCapture({value,at:started-value.capturedAgeMs-page.ageMs}); setNow(performance.now()) }
      } catch { if (live) setCapture(null) }
      if (live) { setLoading(false); timer = setTimeout(() => void load(),30_000) }
    }
    void load()
    return () => { live=false;clearTimeout(timer) }
  },[source,active,available,calls,refresh])
  const value = source && available && capture && now-capture.at < 60_000 ? capture.value : null
  return { remote:source !== null, value, loading, refresh:() => setRefresh(n=>n+1) }
}
