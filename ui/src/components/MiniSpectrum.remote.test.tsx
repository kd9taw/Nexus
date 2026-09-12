// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest'
import {cleanup,render,waitFor} from '@testing-library/react'
import {MiniSpectrum} from './MiniSpectrum'
import {installApplicationTransport} from '../applicationTransport'
let dispose:(()=>void)|undefined
afterEach(()=>{cleanup();dispose?.();vi.restoreAllMocks()})
it('clears a previously live remote trace and its axis on a failed reading and restores on recovery',async()=>{
  const clear=vi.fn(),ctx=new Proxy({clearRect:clear},{get:(o,k)=>k in o?o[k as keyof typeof o]:vi.fn(),set:()=>true})
  vi.spyOn(HTMLCanvasElement.prototype,'getContext').mockReturnValue(ctx as unknown as CanvasRenderingContext2D)
  let available=true
  dispose=installApplicationTransport({kind:'remote',invoke:async <T,>():Promise<T>=>{if(!available)throw new Error('applicationUnavailable');return {row:[0.1,0.5,0.2],loHz:14000000,hiHz:14010000,source:'flex'} as T}})
  const {container}=render(<MiniSpectrum pollMs={30} idleHint="Silent"/>)
  await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('FLEX RF'))
  expect(container.querySelector('.mini-spectrum-span')?.textContent).toContain('14.000 MHz')
  clear.mockClear();available=false
  await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('—'))
  expect(container.querySelector('.mini-spectrum-span')?.textContent).toBe('');expect(clear).toHaveBeenCalled()
  expect(container.querySelector('.mini-spectrum-idle')?.textContent).not.toBe('Silent')
  available=true;await waitFor(()=>expect(container.querySelector('.mini-spectrum-src')?.textContent).toBe('FLEX RF'))
})
