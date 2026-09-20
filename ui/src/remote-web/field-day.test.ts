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

// A station sends `sentExchange` (what {EXCH} keys next) on every contest payload. The validator
// refuses keys it does not know, so without it here every Field Day view went blank through Remote.
it('accepts the sent exchange a newer station sends, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.sentExchange='5 MA'
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative control: the same key over the per-string bound is refused.
 fd.sentExchange='x'.repeat(1025)
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// The ruleset's DUPE RULE, each row's key under it, and the club's generalised keys. The strip
// builds the key the ENGINE will refuse on instead of the (call, band, mode class) triple it
// hardcoded — the rule for two of the seventeen shipped rulesets. This validator refuses keys it
// does not know, so all three had to be taught here or a station sending them goes blank through
// Remote, which is the defect this file's own header records happening twice before.
it('accepts the ruleset dupe rule and the keys built from it, and bounds all three',()=>{
 type Src={source:{fieldDay:Record<string,unknown>}}
 const page=(mutate:(fd:Record<string,unknown>)=>void)=>{
  const p=fieldDayPage()
  const fd=(p.meta as Src).source.fieldDay
  fd.dupeRule={byCall:true,byBand:true,byModeClass:true,byFields:['QTH'],bySentFields:['QTH'],modeClassGroups:[['CW','DIG']]}
  ;(fd.log as Record<string,unknown>[])[0].dkey=['W8XYZ','20M','CW','CUYA','MI']
  ;(fd.log as Record<string,unknown>[])[0].dupe=true
  ;(fd.club as Record<string,unknown>).dkeys=[['K9CLUB','20M','CW']]
  mutate(fd)
  return p
 }
 expect(()=>parseFieldDay(page(()=>{}))).not.toThrow()
 // Negative controls, one per field and one per way of being wrong — a validator that only
 // ever accepts is half a test, and the half that matters here is the refusal: a rule read
 // with a component missing silently becomes a NARROWER key, which shows "new" for a contact
 // the log is about to reject.
 for(const [name,mutate] of [
  ['a flag that is not a boolean',(fd:Record<string,unknown>)=>{(fd.dupeRule as Record<string,unknown>).byCall='yes'}],
  ['a rule missing a component',(fd:Record<string,unknown>)=>{delete (fd.dupeRule as Record<string,unknown>).byFields}],
  ['a slot list that is not strings',(fd:Record<string,unknown>)=>{(fd.dupeRule as Record<string,unknown>).bySentFields=[7]}],
  ['a row key wider than any rule can build',(fd:Record<string,unknown>)=>{(fd.log as Record<string,unknown>[])[0].dkey=Array(17).fill('X')}],
  ['a row key component over the string bound',(fd:Record<string,unknown>)=>{(fd.log as Record<string,unknown>[])[0].dkey=['x'.repeat(1025)]}],
  ['a club key over the string bound',(fd:Record<string,unknown>)=>{(fd.club as Record<string,unknown>).dkeys=[['x'.repeat(1025)]]}],
  ['a club key that is not an array',(fd:Record<string,unknown>)=>{(fd.club as Record<string,unknown>).dkeys=['K9CLUB']}],
  ['a dupe mark that is not a boolean',(fd:Record<string,unknown>)=>{(fd.log as Record<string,unknown>[])[0].dupe='yes'}],
 ] as [string,(fd:Record<string,unknown>)=>void][]) {
  expect(()=>parseFieldDay(page(mutate)),name).toThrow('invalidFieldDay')
 }
})

// A contest that is not Field Day carries each row's received values (`rcvd`) for the log
// table's columns. Same refusal hazard as above for an unknown key, and still bounded.
it('accepts a row\'s received values, and still bounds them',()=>{
 const page=fieldDayPage()
 const log=(page.meta as {source:{fieldDay:{log:Record<string,unknown>[]}}}).source.fieldDay.log
 log[0].rcvd=['599','14']
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative controls: more values than any role receives, and a value that is not text.
 log[0].rcvd=Array(9).fill('5')
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 log[0].rcvd=[14]
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// A contest that names its bands (CQ WW RTTY) sends them as advice for the entry strip. Same
// refusal hazard as the keys above for a key the validator does not know, and still bounded.
it('accepts the contest\'s advisory band list, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.bands=['80m','40m','20m','15m','10m']
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative controls: not a list of text, and a list past any contest's band count.
 fd.bands=[20]
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 fd.bands=Array(33).fill('20m')
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// ILQP counts CW and digital as ONE mode for dupes, so its status carries the grouping the
// browser's DUPE badge folds through. A key the validator does not know is a REFUSED payload —
// the whole contest view blank through Remote — which is why every addition gets a row here.
it('accepts the dupe rule\'s mode grouping, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.dupeModeGroups=[['CW','DIG']]
 expect(()=>parseFieldDay(page)).not.toThrow()
 // …and the empty list every other contest sends.
 fd.dupeModeGroups=[]
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative controls: not a list of lists, a group that is not text, and a list past any
 // plausible mode vocabulary.
 fd.dupeModeGroups=['CW','DIG']
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 fd.dupeModeGroups=[[1,2]]
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 fd.dupeModeGroups=Array(9).fill(['CW','DIG'])
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 fd.dupeModeGroups=[Array(9).fill('CW')]
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// Every contest in the rules file has to survive the browser's validator. This pins the defect
// where it only accepted arrlfd and wfd: thirteen of the fifteen events then in the rules table
// had their ENTIRE Field Day payload rejected, so a station running a QSO party or a VHF contest
// saw nothing at all through Remote. The list below is read from the same seed the app ships, so
// adding a contest without teaching the validator fails here rather than on the air.
it('accepts every event id the shipped rules file can produce',()=>{
 const shipped=["arrlfd", "arrlss_cw", "arrlss_ssb", "arrlvhf_jan", "arrlvhf_jun", "arrlvhf_sep", "cqp", "cqwpx_cw", "cqwpx_ssb", "cqww_cw", "cqww_rtty", "cqww_ssb", "ohqp", "tnqp", "txqp", "wfd"]
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

// The same defect, one object over: the station sends the ruleset facts of the contest it is
// running (`fd_ruleset_dto` puts the rules-file id in `ruleset.event`), and the browser still
// accepted only arrlfd and wfd there, so a station running CQ WW RTTY or a QSO party showed
// nothing through Remote. Same list as above, same shape rule.
it('accepts the ruleset facts of every shipped contest, not only Field Day\'s',()=>{
 const shipped=["arrlfd", "arrlss_cw", "arrlss_ssb", "arrlvhf_jan", "arrlvhf_jun", "arrlvhf_sep", "cqp", "cqwpx_cw", "cqwpx_ssb", "cqww_cw", "cqww_rtty", "cqww_ssb", "ohqp", "tnqp", "txqp", "wfd"]
 for(const event of shipped){
  const page=fieldDayPage()
  ;(page.meta as {source:{ruleset:{event:string}}}).source.ruleset.event=event
  expect(()=>parseFieldDay(page),event).not.toThrow()
 }
 // Negative control: the shape check still refuses something that is not an event id.
 for(const bad of ['','ARRLFD','arrl fd','x'.repeat(33),1 as unknown as string]){
  const page=fieldDayPage()
  ;(page.meta as {source:{ruleset:{event:string}}}).source.ruleset.event=bad
  expect(()=>parseFieldDay(page),String(bad)).toThrow('invalidFieldDay')
 }
})

// A station whose call is in the USA or Canada and whose contest state would send the DX exchange
// carries a location warning (what was typed, and the listed codes it likely means). The validator
// refuses unknown keys, so it must be taught this one — and still bounds it.
it('accepts the W/VE location warning, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.locationWarning={typed:'NL',hints:['NF','LB']}
 expect(()=>parseFieldDay(page)).not.toThrow()
 fd.locationWarning={typed:'',hints:[]}
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative controls: a hint that is not text, too many hints, an extra key, a missing key.
 for(const bad of [{typed:'NL',hints:[1]},{typed:'NL',hints:Array(9).fill('NF')},{typed:'NL',hints:[],extra:1},{hints:[]}]){
  fd.locationWarning=bad
  expect(()=>parseFieldDay(page),JSON.stringify(bad)).toThrow('invalidFieldDay')
 }
})
