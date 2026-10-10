import { expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
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

// The station's `qsoCount` is the contacts that COUNT (the summary sheet's raw non-dupe number) and
// its `log` is every row, so the two differ in two kinds of contest: one whose sponsor cross-checks
// logs has a duplicate LOGGED and scored zero (Sweepstakes, CQ WW and WPX, the VHF runnings, the New
// York QSO Party), and Winter Field Day logs a satellite contact that counts for nothing there. This
// validator required the count to equal the rows, so one logged dupe refused the whole capture: the
// contest view went blank through Remote and the status line called the station unavailable.
type CountedFd={event:string,qsoCount:number,log:Record<string,unknown>[],satelliteCredit?:boolean}
const countedPage=(event:string)=>{
 const page=fieldDayPage(),source=(page.meta as {source:{fieldDay:CountedFd,ruleset:{event:string}}}).source
 source.fieldDay.event=event;source.ruleset.event=event
 return {page,fd:source.fieldDay}
}
it('takes a log holding a logged dupe or a contact that counts for nothing, counted as the station counts it',()=>{
 // Sweepstakes works a station once: K2ABC again is logged, marked, and leaves the count at two.
 const ss=countedPage('arrlss_cw')
 ss.fd.log.push({...ss.fd.log[1],whenUnix:1782583380,dupe:true})
 expect(parseFieldDay(ss.page).fieldDay).toMatchObject({qsoCount:2,log:[{call:'K1ABC'},{call:'K2ABC'},{call:'K2ABC',dupe:true}]})
 // Winter Field Day: a contact through a bird counts for nothing there, and the status says so.
 const wfd=countedPage('wfd')
 wfd.fd.satelliteCredit=false
 wfd.fd.log.push({...wfd.fd.log[1],call:'K3ABC',whenUnix:1782583380,sat:'SAUDISAT 1C (SO-50)'})
 expect(parseFieldDay(wfd.page).fieldDay).toMatchObject({qsoCount:2,log:[{},{},{call:'K3ABC'}]})
 // The count is still checked exactly: counting the dupe or the satellite contact, or leaving a
 // real contact out, is refused.
 for(const [name,{page,fd}] of [['Sweepstakes',ss],['Winter Field Day',wfd]] as const) for(const count of [3,1]){
  fd.qsoCount=count
  expect(()=>parseFieldDay(page),`${name}: ${count}`).toThrow('invalidFieldDay')
 }
 // CONTROL: a contest that credits a satellite contact (ARRL Field Day) counts it with the rest.
 const arrl=countedPage('arrlfd')
 arrl.fd.log.push({...arrl.fd.log[1],call:'K3ABC',whenUnix:1782583380,sat:'SAUDISAT 1C (SO-50)'})
 arrl.fd.qsoCount=3
 expect(parseFieldDay(arrl.page).fieldDay?.qsoCount).toBe(3)
 arrl.fd.qsoCount=2
 expect(()=>parseFieldDay(arrl.page)).toThrow('invalidFieldDay')
})

// Every ruleset the app ships, read from its own rules file. Where every row counts, the count must
// equal the rows exactly as it always had to, one more or one fewer refused; a row leaves the count
// only by a mark that ruleset's station sends: `sat` where it gives a satellite contact no credit
// (the status then says `satelliteCredit: false`), and `dupe` where it logs duplicates.
it('checks the count exactly under every shipped ruleset, for the rows that ruleset can send',()=>{
 const {rulesets}=JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)),'../../../crates/tempo-core/src/fd_rules.seed.json'),'utf8')) as
  {rulesets:{event:string,dupe:{log_dupes?:boolean},scoring:{satellite_credit?:boolean}}[]}
 // Scene guard: the file holds rulesets of both kinds that leave a row out of the count.
 expect([rulesets.some(r=>r.dupe.log_dupes===true),rulesets.some(r=>r.scoring.satellite_credit===false)]).toEqual([true,true])
 const off:string[]=[]
 for(const rs of rulesets){
  const {page,fd}=countedPage(rs.event)
  if(rs.scoring.satellite_credit===false) fd.satelliteCredit=false
  const exactly=(scene:string,counted:number)=>{
   const verdicts=[counted-1,counted,counted+1].map(count=>{fd.qsoCount=count;try{parseFieldDay(page);return 'taken'}catch{return 'refused'}})
   if(verdicts.join()!=='refused,taken,refused') off.push(`${rs.event} ${scene}: ${verdicts.join()}`)
  }
  exactly('every row counts',2)
  fd.log.push({...fd.log[1],call:'K3ABC',whenUnix:1782583380,sat:'AO-91'})
  exactly('a satellite contact',rs.scoring.satellite_credit===false?2:3)
  if(rs.dupe.log_dupes){
   fd.log.push({...fd.log[0],whenUnix:1782583440,dupe:true})
   exactly('a logged dupe',rs.scoring.satellite_credit===false?2:3)
  }
 }
 expect(off).toEqual([])
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
// hardcoded — the rule for two of the eighteen shipped rulesets. This validator refuses keys it
// does not know, so all three had to be taught here or a station sending them goes blank through
// Remote, which is the defect this file's own header records happening twice before.
it('accepts the ruleset dupe rule and the keys built from it, and bounds all three',()=>{
 type Src={source:{fieldDay:Record<string,unknown>}}
 const page=(mutate:(fd:Record<string,unknown>)=>void)=>{
  const p=fieldDayPage()
  const fd=(p.meta as Src).source.fieldDay
  fd.dupeRule={byCall:true,byBand:true,byModeClass:true,byFields:['QTH'],bySentFields:['QTH'],modeClassGroups:[['CW','DIG']],logDupes:false}
  ;(fd.log as Record<string,unknown>[])[0].dkey=['W8XYZ','20M','CW','CUYA','MI']
  ;(fd.log as Record<string,unknown>[])[0].dupe=true
  // A marked dupe is not in the station's count.
  fd.qsoCount=1
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
  ['a rule with no logDupes at all',(fd:Record<string,unknown>)=>{delete (fd.dupeRule as Record<string,unknown>).logDupes}],
  ['a logDupes that is not a boolean',(fd:Record<string,unknown>)=>{(fd.dupeRule as Record<string,unknown>).logDupes='yes'}],
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

// Winter Field Day's objective multiplier rides the status beside the claimed total it
// multiplies. A station sends it only for a contest that scores by objectives, and this page
// refuses keys it does not know, so it is taught here (and must be deployed before a station
// that sends it) or that station's Field Day view goes blank through Remote.
it('accepts the objective multiplier a Winter Field Day station sends, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.objectiveMultiplier=7
 expect(()=>parseFieldDay(page)).not.toThrow()
 // Negative controls: not a count, and not a number at all.
 for(const bad of [-1,1.5,'7']){
  fd.objectiveMultiplier=bad
  expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
 }
})

// Winter Field Day's objectives are a setting of their own, ticked and planned like the bonuses.
// A station sends the two lists only for a contest that scores by objectives; an older station
// sends neither, and both shapes must validate.
it('accepts the objective lists a Winter Field Day station sends, and still bounds them',()=>{
 const page=fieldDayPage()
 const settings=(page.meta as {source:{settings:Record<string,unknown>}}).source.settings
 expect(()=>parseFieldDay(page)).not.toThrow()
 settings.fdObjectives=['wfd-alt-power-100','wfd-qrp']
 settings.fdObjectivesPlanned=['wfd-six-hours']
 expect(parseFieldDay(page).settings.fdObjectives).toEqual(['wfd-alt-power-100','wfd-qrp'])
 // Negative controls: too many ids, an id that is not a string, and a key the page still refuses.
 for(const mutate of [
  ()=>{settings.fdObjectives=Array.from({length:65},(_,i)=>`o${i}`)},
  ()=>{settings.fdObjectivesPlanned=[7]},
  ()=>{settings.fdObjectiveNotes=['x']},
 ]){
  const before=structuredClone(settings)
  mutate()
  expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
  for(const k of Object.keys(settings)) delete settings[k]
  Object.assign(settings,before)
 }
})

// Whether a satellite contact counts rides the status as `false` from a contest where it does
// not (Winter Field Day 2027), and is absent everywhere else. Taught here for the same reason as
// the objective multiplier.
it('accepts the satellite-credit flag a Winter Field Day station sends, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.satelliteCredit=false
 expect(()=>parseFieldDay(page)).not.toThrow()
 fd.satelliteCredit='false'
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// A ruleset that allows spotting only over amateur RF while its event runs (Winter Field Day 2027)
// says so, `spotsRfOnly: true`, and every other ruleset leaves the key out.
it('accepts the ruleset\'s spots-over-RF-only rule, and still bounds it',()=>{
 const page=fieldDayPage()
 const ruleset=(page.meta as {source:{ruleset:Record<string,unknown>}}).source.ruleset
 ruleset.spotsRfOnly=true
 expect(()=>parseFieldDay(page)).not.toThrow()
 ruleset.spotsRfOnly='true'
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})

// Each club board row may carry that position's clock against the host's, in ms (`clockMs`,
// null when it has not measured one). This validator refuses board keys it does not know, so a
// station sending the column goes blank through Remote unless the page that takes it is
// deployed first; and a station older than the field sends no key at all.
it('accepts a board row\'s measured clock, null or absent, and refuses anything but whole ms',()=>{
 type Row=Record<string,unknown>
 const page=(clockMs?:unknown)=>{
  const p=fieldDayPage()
  const row=((p.meta as {source:{fieldDay:{club:{board:Row[]}}}}).source.fieldDay.club.board)[0]
  if (clockMs!==undefined) row.clockMs=clockMs
  return p
 }
 for (const ok of [-3000,0,45600,null,undefined]) expect(()=>parseFieldDay(page(ok)),String(ok)).not.toThrow()
 expect(parseFieldDay(page(-3000)).fieldDay?.club?.board[0].clockMs).toBe(-3000)
 for (const bad of [1.5,'-3000',Number.MAX_SAFE_INTEGER+2,true,{}]) expect(()=>parseFieldDay(page(bad)),String(bad)).toThrow('invalidFieldDay')
})

// Three keys a station sends only when they apply, so an older station sends none: what its
// journal restore kept out of the session (`keptOut`), and, on a host, how full its club board
// is (`boardFull`) and the positions it turned away (`refused`). This validator refuses a key it
// does not know, so the page that takes them is deployed before the release that writes them.
it('accepts what the journal restore kept out, and still bounds it',()=>{
 const page=fieldDayPage()
 const fd=(page.meta as {source:{fieldDay:Record<string,unknown>}}).source.fieldDay
 fd.keptOut={otherContest:1,otherRunning:3}
 expect(parseFieldDay(page).fieldDay?.keptOut).toEqual({otherContest:1,otherRunning:3})
 for(const bad of [{otherContest:1},{otherContest:-1,otherRunning:0},{otherContest:1,otherRunning:'3'},{otherContest:1,otherRunning:0,extra:1},null,[]]){
  fd.keptOut=bad
  expect(()=>parseFieldDay(page),JSON.stringify(bad)).toThrow('invalidFieldDay')
 }
})
it('accepts a host club\'s full board and the positions it turned away, and still bounds them',()=>{
 const page=fieldDayPage()
 const club=(page.meta as {source:{fieldDay:{club:Record<string,unknown>}}}).source.fieldDay.club
 club.boardFull={positions:70,shown:59}
 club.refused=[{posName:'SSB tent',call:'W9XYZ',reason:'this club sends the in-state IL QSO Party exchange (its county)'}]
 const got=parseFieldDay(page).fieldDay?.club
 expect(got?.boardFull).toEqual({positions:70,shown:59})
 expect(got?.refused?.[0].call).toBe('W9XYZ')
 for(const bad of [{positions:70},{positions:70,shown:1.5},{positions:70,shown:59,extra:0},null]){
  club.boardFull=bad
  expect(()=>parseFieldDay(page),JSON.stringify(bad)).toThrow('invalidFieldDay')
 }
 club.boardFull={positions:70,shown:59}
 for(const bad of [[{posName:'a',call:'b'}],[{posName:'a',call:'b',reason:7}],Array(17).fill({posName:'a',call:'b',reason:'c'}),{}]){
  club.refused=bad
  expect(()=>parseFieldDay(page),JSON.stringify(bad).slice(0,60)).toThrow('invalidFieldDay')
 }
})
