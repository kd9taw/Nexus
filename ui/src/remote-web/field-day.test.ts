import { expect, it } from 'vitest'
import { parseFieldDay } from './field-day'
import { queryPage } from './application-query-protocol'
import fixture from './__fixtures__/field-day.json'
import { fieldDayPage } from './__fixtures__/field-day-page'
it('retains the complete event, scoring choices and club context behind v10',()=>{
 const page=fieldDayPage()
 expect(queryPage(page,10).collection).toBe('fieldDay')
 for(const version of [3,4,6,7,8,9]) expect(()=>queryPage(page,version)).toThrow()
 expect(parseFieldDay(page)).toEqual({...fixture,capturedAgeMs:0})
 expect(parseFieldDay({...page,meta:{capturedAgeMs:0,source:{...fixture,active:false,fieldDay:null}}}).fieldDay).toBeNull()
})
it('refuses stale, incomplete, oversized and open-ended display shapes',()=>{
 for(const mutate of [
  (v:typeof fixture)=>{v.fieldDay.qsoCount++},
  (v:typeof fixture)=>{v.fieldDay.log[0].whenUnix=-1},
  (v:typeof fixture)=>{v.fieldDay.log[0].call='x'.repeat(1025)},
  (v:typeof fixture)=>{v.fieldDay.club.board[0].lastSeenSecs=-1},
  (v:typeof fixture)=>{v.settings.fdPowerMult=3},
  (v:typeof fixture)=>{v.active=false},
  (v:typeof fixture)=>{Object.assign(v.settings,{qrzPassword:'excluded'})},
  (v:typeof fixture)=>{Object.assign(v.fieldDay.club,{command:'fd_log_manual'})},
 ]) {
  const v=structuredClone(fixture);mutate(v)
  expect(()=>parseFieldDay({...fieldDayPage(),meta:{capturedAgeMs:0,source:v}})).toThrow()
 }
 expect(()=>parseFieldDay({...fieldDayPage(),meta:{capturedAgeMs:60000,source:fixture}})).toThrow()
})

// Every contest in the rules file has to survive the browser's validator. This pins the defect
// where it only accepted arrlfd and wfd: thirteen of the fifteen events then in the rules table
// had their ENTIRE Field Day payload rejected, so a station running a QSO party or a VHF contest
// saw nothing at all through Remote. The list below is read from the same seed the app ships, so
// adding a contest without teaching the validator fails here rather than on the air.
it('accepts every event id the shipped rules file can produce',()=>{
 const shipped=["arrlfd", "arrlss_cw", "arrlss_ssb", "arrlvhf_jan", "arrlvhf_jun", "arrlvhf_sep", "cqp", "cqwpx_cw", "cqwpx_ssb", "cqww_cw", "cqww_ssb", "ohqp", "tnqp", "txqp", "wfd"]
 for(const event of shipped){
  const page=fieldDayPage()
  ;(page.meta as {source:{fieldDay:{event:string}}}).source.fieldDay.event=event
  expect(()=>parseFieldDay(page),event).not.toThrow()
 }
 // Negative control: the shape check must still refuse something that is not an event id.
 for(const bad of ['','ARRLFD','arrl fd','a','x'.repeat(33),'arrl-fd',1 as unknown as string]){
  const page=fieldDayPage()
  ;(page.meta as {source:{fieldDay:{event:string}}}).source.fieldDay.event=bad
  expect(()=>parseFieldDay(page),String(bad)).toThrow('invalidFieldDay')
 }
})
