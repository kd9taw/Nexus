// @vitest-environment jsdom
import { renderHook, waitFor } from '@testing-library/react'
import { expect, it } from 'vitest'
import type { ReactNode } from 'react'
import { RemoteCollectionsContext } from './collections'
import type { RemoteCollections } from './collections'
import { useJs8Context } from './useJs8Context'

it('never renders the previous call set as the current complete-log history', async () => {
  const pending:Array<(value:unknown)=>void>=[]
  const source={page:()=>new Promise(resolve=>pending.push(resolve))} as unknown as RemoteCollections
  const frames:Array<{calls:string;history:unknown;loading:boolean}>=[]
  const page=(call:string)=>({collection:'js8Context',offset:0,total:0,retained:0,rows:[],nextCursor:null,ageMs:0,
    meta:{capturedAgeMs:0,source:{plan:[],history:{[call]:{count:0,lastUnix:null,grid:'',name:'',comment:''}}}}})
  const {result,rerender,unmount}=renderHook(({calls})=>{
    const value=useJs8Context(true,calls)
    frames.push({calls,history:value.value?.history,loading:value.loading})
    return value
  },{initialProps:{calls:'W1AW'},wrapper:({children}:{children:ReactNode})=><RemoteCollectionsContext.Provider value={source}>{children}</RemoteCollectionsContext.Provider>})
  await waitFor(()=>expect(pending.length).toBe(1))
  pending.shift()!(page('W1AW'))
  await waitFor(()=>expect(result.current.value?.history.W1AW.count).toBe(0))
  rerender({calls:'K2ABC'})
  expect(result.current.value).toBeNull()
  expect(result.current.loading).toBe(true)
  expect(frames.filter(f=>f.calls==='K2ABC').every(f=>f.history===undefined&&f.loading)).toBe(true)
  pending.shift()!(page('K2ABC'))
  await waitFor(()=>expect(result.current.value?.history.K2ABC.count).toBe(0))
  unmount()
})
