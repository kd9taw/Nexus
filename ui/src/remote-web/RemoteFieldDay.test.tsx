// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RemoteFieldDay } from './RemoteFieldDay'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import type { ApplicationClient } from './application-client'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import { fieldDayPage } from './__fixtures__/field-day-page'
import { t } from '../i18n'
afterEach(()=>{cleanup();vi.useRealTimers();vi.restoreAllMocks()})
function setup(invoke=vi.fn(async():Promise<unknown>=>fieldDayPage())) {
 const source=new RemoteCollections({invoke,supports:()=>true,getPhase:()=> 'ready'} as unknown as ApplicationClient)
 const view=(available=true)=><StationDataContext.Provider value={available}><RemoteCollectionsContext.Provider value={source}><RemoteFieldDay tier="FT8"/></RemoteCollectionsContext.Provider></StationDataContext.Provider>
 return {...render(view()),view,invoke}
}
it('uses the real event boards and disclosures without native loaders or station writes',async()=>{
 const forbidden=vi.fn(async()=>{throw new Error('unexpected native operation')})
 const uninstall=installApplicationTransport({kind:'remote',invoke:forbidden})
 try {
  const test=setup();await screen.findByText('K1ABC')
  expect(screen.getByText('CW tent')).toBeTruthy()
  expect(test.container.querySelector('.fd-score-math')?.textContent).toContain('106')
  const op=screen.getByRole('textbox',{name:t('fieldDay.operator.aria')}) as HTMLInputElement
  expect(op.value).toBe('W1AW');expect(op.readOnly).toBe(true)
  fireEvent.change(op,{target:{value:'K9NEW'}});fireEvent.blur(op)
  expect(test.container.querySelector('.fd-export')).toBeNull()
  const actions=[...test.container.querySelectorAll<HTMLButtonElement>('.fd-role-btn,.fd-power-chip,.fd-bonus-plan')]
  expect(actions).toHaveLength(20)
  expect(actions.every(e=>e.disabled)).toBe(true)
  const bonuses=[...test.container.querySelectorAll<HTMLInputElement>('.fd-bonus-row input')]
  expect(bonuses).toHaveLength(15)
  expect(bonuses.every(e=>e.disabled)).toBe(true)
  expect(test.container.querySelector('[data-bonus-state=planned]')?.textContent).toContain('Natural Power')
  fireEvent.click(test.container.querySelector('.fd-bonuses-toggle')!)
  fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
  await screen.findByText('K1ABC')
  expect(test.invoke).toHaveBeenCalledTimes(2)
  expect(test.container.querySelector('.fd-bonuses-list')).toBeNull()
  expect(forbidden).not.toHaveBeenCalled()
 } finally {uninstall()}
})
it('clears loss, refresh failures and inactive events without a fake zero score',async()=>{
 const test=setup();await screen.findByText('K1ABC')
 test.rerender(test.view(false));expect(test.container.textContent).not.toContain('K1ABC')
 test.rerender(test.view());await screen.findByText('K1ABC')
 test.invoke.mockRejectedValueOnce(new Error('applicationTooLarge'))
 fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
 await screen.findByText(t('remote.fieldDayTooLarge'))
 expect(test.container.textContent).not.toContain('K1ABC')
 const off=fieldDayPage();Object.assign((off.meta as {source:object}).source,{active:false,fieldDay:null})
 test.invoke.mockResolvedValueOnce(off)
 fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
 await screen.findByText(t('remote.fieldDayInactive'))
 expect(test.container.querySelector('.remote-field-day-bank:not([hidden])')).toBeNull()
})
it('ages club presence evidence and expires the whole captured event',async()=>{
 vi.useFakeTimers({toFake:['setInterval','clearInterval','performance']})
 const test=setup();await act(async()=>{await Promise.resolve()})
 expect(screen.getByText('K1ABC')).toBeTruthy()
 await act(async()=>{vi.advanceTimersByTime(7000)})
 expect(screen.getByTitle(t('fieldDay.club.board.stale',{secs:17}))).toBeTruthy()
 await act(async()=>{vi.advanceTimersByTime(60000)})
 expect(test.container.textContent).not.toContain('K1ABC')
 expect(test.container.textContent).not.toContain('CW tent')
})
it('keeps Winter Field Day raw-point scoring distinct from ARRL totals',async()=>{
 const page=fieldDayPage()
 const source=(page.meta as unknown as {source:{fieldDay:{event:string};ruleset:{event:string;bannedModes:string[]}}}).source
 source.fieldDay.event='wfd';source.ruleset.event='wfd';source.ruleset.bannedModes=['FT8']
 const test=setup(vi.fn(async()=>page));await screen.findByText('K1ABC')
 const math=test.container.querySelector('.fd-score-math')!.textContent!
 expect(math).toContain('3');expect(math).not.toContain('106');expect(math.toLowerCase()).not.toContain('power')
})
