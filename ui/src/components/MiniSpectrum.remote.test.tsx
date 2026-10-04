// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest'
import {cleanup,render,waitFor} from '@testing-library/react'
import {MiniSpectrum} from './MiniSpectrum'
import {installApplicationTransport} from '../applicationTransport'
import type {SpectrumScene} from '../spectrum'
// The trace is the spectrum renderer's (jsdom has no canvas context): a stand-in records each scene.
const scenes:SpectrumScene[]=[]
vi.mock('../spectrum',async(importActual)=>({...(await importActual<typeof import('../spectrum')>()),createSpectrumRenderer:()=>({backend:'canvas2d',reason:'',canvas:null,rows:0,resize(){},commitRow(){},rowAt:()=>null,clearHistory(){},draw:(s:SpectrumScene)=>scenes.push(s),destroy(){}})}))
let dispose:(()=>void)|undefined
afterEach(()=>{cleanup();dispose?.();vi.restoreAllMocks();scenes.length=0})
it('clears a previously live remote trace and its axis on a failed reading and restores on recovery',async()=>{
  // The inks go through a 1x1 canvas, and the strip needs a box (jsdom lays nothing out).
  vi.spyOn(HTMLCanvasElement.prototype,'getContext').mockReturnValue({fillRect(){},getImageData:()=>({data:[1,2,3,255]})} as unknown as CanvasRenderingContext2D)
  vi.spyOn(HTMLElement.prototype,'clientWidth','get').mockReturnValue(300)
  vi.spyOn(HTMLElement.prototype,'clientHeight','get').mockReturnValue(84)
  let available=true
  dispose=installApplicationTransport({kind:'remote',invoke:async <T,>():Promise<T>=>{if(!available)throw new Error('applicationUnavailable');return {row:[0.1,0.5,0.2],loHz:14000000,hiHz:14010000,source:'flex'} as T}})
  const {container}=render(<MiniSpectrum pollMs={30} idleHint="Silent"/>)
  const traced=()=>scenes.length>0&&scenes[scenes.length-1].trace!==null
  await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('FLEX RF'))
  await waitFor(()=>expect(traced()).toBe(true))
  expect(scenes[scenes.length-1].view).toEqual({loHz:14000000,hiHz:14010000})
  expect(container.querySelector('.mini-spectrum-span')?.textContent).toContain('14.000 MHz')
  available=false
  await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('—'))
  expect(container.querySelector('.mini-spectrum-span')?.textContent).toBe('');expect(traced(),'the failed reading left the trace drawn').toBe(false)
  expect(container.querySelector('.mini-spectrum-idle')?.textContent).not.toBe('Silent')
  available=true;await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('FLEX RF'))
  await waitFor(()=>expect(traced()).toBe(true))
})
