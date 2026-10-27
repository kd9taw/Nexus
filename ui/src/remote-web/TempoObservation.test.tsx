// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { Conversation } from '../components/Conversation'
import { StationList } from '../components/StationList'
import type { ChatMessage, RadioStatus } from '../types'

afterEach(cleanup)
const message: ChatMessage = { from:'N0CALL',to:'W1AW',text:'TEST',slot:1,outbound:true,
  directedToMe:false,snr:null,freqHz:null,dtSec:null,tier:'TempoDeep',noAck:true }
const macros = {chat:['TNX'],qso:['73'],band:['CQ TEST']}

it('preserves native conversation actions',()=>{
  const send=vi.fn(),resend=vi.fn(),broadcast=vi.fn(),cq=vi.fn(),beacon=vi.fn(),roam=vi.fn(),gear=vi.fn()
  const props={conversation:{peer:'W1AW',messages:[message]},peer:'W1AW',radio:{transmitting:false} as RadioStatus,
    mode:'chat' as const,fieldDay:null,macros,onSend:send,onResend:resend,onBroadcast:broadcast,onCallCq:cq,
    beaconOn:false,onToggleBeacon:beacon,mycall:'N0CALL',mygrid:'AA00',onToggleRoam:roam,onRoamSettings:gear}
  const view=(peer:string|null)=><Conversation {...props} peer={peer}/>
  const {container,rerender}=render(view('W1AW'))
  const input=container.querySelector<HTMLInputElement>('.composer-input')!
  expect(input.disabled).toBe(false)
  fireEvent.change(input,{target:{value:'TEST SEND'}});fireEvent.submit(input.closest('form')!)
  fireEvent.click(container.querySelector('.quick-chip')!)
  expect(send.mock.calls).toEqual([['TEST SEND'],['TNX']])
  const bubble=container.querySelector('.bubble')!
  expect(bubble.getAttribute('role')).toBe('button')
  fireEvent.click(bubble);fireEvent.keyDown(bubble,{key:'Enter'})
  expect(resend).toHaveBeenCalledTimes(2)
  for(const selector of ['.heartbeat-chip:not(.roam-toggle):not(.roam-gear)','.roam-toggle','.roam-gear']) fireEvent.click(container.querySelector(selector)!)
  for(const callback of [beacon,roam,gear])expect(callback).toHaveBeenCalledTimes(1)
  rerender(view(null))
  fireEvent.click(container.querySelector('.cq-btn')!);fireEvent.click(container.querySelector('.quick-chip')!)
  expect(cq).toHaveBeenCalledTimes(1)
  expect(broadcast.mock.calls).toEqual([['CQ TEST']])
})

it('keeps recent-thread browsing separate from archiving',()=>{
  const select=vi.fn(),archive=vi.fn()
  const pane=<StationList stations={[]} myGrid="AA00" currentSlot={1} activePeer={null}
    unreadByPeer={{}} needByCall={new Map()} onSelect={select} onCall={vi.fn()}
    conversations={[{peer:'W1AW',messages:[message]}]} onArchive={archive} bandActive={false}
    bandUnread={0} onSelectBand={vi.fn()}/>
  const {container}=render(pane)
  fireEvent.click(container.querySelector('.recent-open')!)
  expect(select).toHaveBeenCalledWith('W1AW')
  const button=container.querySelector<HTMLButtonElement>('.recent-archive')!
  expect(button.disabled).toBe(false);fireEvent.click(button)
  expect(archive.mock.calls).toEqual([['W1AW']])
})
