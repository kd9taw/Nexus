import { useEffect, useState } from 'react'
import type { RefObject } from 'react'
import type { RemoteCollections } from './collections'
import { loadSstvImage } from './sstv'

// Two readers per browser. Leaving the viewport stops at the next page and
// releases the Blob URL, so scrolling does not retain the entire gallery.
const readers = new WeakMap<RemoteCollections,number>()
export function useSstvImage(source:RemoteCollections|null, id:string, target:RefObject<HTMLElement|null>, active:boolean) {
  const [visible,setVisible]=useState(false),[url,setUrl]=useState<string|null>(null),[failed,setFailed]=useState(false)
  const [refresh,setRefresh]=useState(0)
  useEffect(()=>{
    const el=target.current
    if(!source||!el||!active){setVisible(false);return}
    if(typeof IntersectionObserver==='undefined'){setVisible(true);return}
    const observer=new IntersectionObserver(entries=>setVisible(entries.some(e=>e.isIntersecting)),{rootMargin:'100px'})
    observer.observe(el)
    return()=>observer.disconnect()
  },[source,target,active])
  useEffect(()=>{
    setUrl(null);setFailed(false)
    if(!source||!active||!visible)return
    let current=true,url:string|undefined,timer:ReturnType<typeof setTimeout>|undefined
    async function read(){
      if(!current)return
      if((readers.get(source!)??0)>=2){timer=setTimeout(()=>void read(),125);return}
      readers.set(source!, (readers.get(source!)??0)+1)
      try {
        const blob=await loadSstvImage(source!,id,()=>current)
        if(current){url=URL.createObjectURL(blob);setUrl(url)}
      }catch{if(current)setFailed(true)}
      finally{readers.set(source!,Math.max(0,(readers.get(source!)??1)-1))}
    }
    void read()
    return()=>{current=false;clearTimeout(timer);if(url)URL.revokeObjectURL(url)}
  },[source,id,active,visible,refresh])
  return {url,failed,retry:()=>setRefresh(n=>n+1)}
}
