// Compiled browser + actual Auth0 SDK + workerd. The simulated issuer grants one
// ephemeral authorization code only after checking the SDK's PKCE verifier.
// It is a test provider, not evidence of a live Auth0 tenant or physical phone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join } from 'node:path'
import { createHash, randomBytes } from 'node:crypto'
import { chrome, cleanupAfterTest } from './browser-runtime.mjs'
import './browser-lifecycle.test.mjs'
import { runtime } from './runtime.mjs'
import { applicationFixture, collectionFixture, recallFixture } from './application-fixture.mjs'
import { stationModesFixture } from './station-modes-fixture.mjs'
import { navigationFixture } from './navigation-fixture.mjs'
import { tempoConversations } from './tempo-fixture.mjs'

/** A browser's proof on a lane message (security review S1-M1) in its exact shape: the device key's
 *  SPKI, a number from 1, a 64-byte signature. Whether it holds is the station's to say. */
const laneProofShape = proof => !!proof && typeof proof === 'object' && Object.keys(proof).sort().join() === 'publicKey,seq,signature'
  && /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/.test(proof.publicKey)
  && Number.isSafeInteger(proof.seq) && proof.seq > 0 && /^[0-9a-f]{128}$/.test(proof.signature)
/** The stand-in station's own key (security review S3-M1), and its signature on an answer, made as
 *  remote/test/fixtures/stream/README.md says ("Binding the answer to the station"): the page takes
 *  only an answer the key the service lists for the station signed, for its own offer and session. */
async function stationSigner() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
  const fingerprint = sdp => Uint8Array.from(/a=fingerprint:sha-256 ([0-9A-Fa-f:]+)/.exec(sdp)[1].split(':'), pair => parseInt(pair, 16))
  const sha256 = async bytes => new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))
  const text = new TextEncoder()
  return {
    publicKey: Buffer.from(await crypto.subtle.exportKey('spki', pair.publicKey)).toString('hex'),
    async sign(answer, offer, stationId, deviceId, sessionId) {
      const signed = new Uint8Array([...text.encode('nexus-stream-answer/1'), ...await sha256(fingerprint(answer)), ...await sha256(fingerprint(offer)),
        ...text.encode(stationId), ...text.encode(deviceId), ...text.encode(sessionId)])
      const signature = Buffer.from(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, signed)).toString('hex')
      const media = answer.indexOf('\r\nm=') + 2
      return `${answer.slice(0, media)}a=nexus-station-signature:${signature}\r\n${answer.slice(media)}`
    },
  }
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

// Scroll a node into view only once it has MOUNTED, and read it while it is still there, in one
// page turn. A viewport/zoom/theme change re-renders whole panes, so the `document.querySelector`
// a CDP round trip later returns null on a loaded runner: about thirty sites below dereferenced it
// with no wait, which is why the suite's failure set MOVED from run to run instead of naming a
// regression. Re-queries the locator on every animation frame against a bounded deadline, scrolls
// only a node that exists, and re-queries after the scroll because the scroll itself can remount
// it. `measure` runs once, on a node that is mounted at that instant.
// A node that never arrives REJECTS, naming the locator. It must never resolve quietly: a guard
// that skipped the scroll would trade a flaky red for a permanent green that measures nothing.
// The setTimeout is the case the frame loop cannot see - if frames stop, the deadline is never
// read, and without it the promise would hang out the whole test timeout with no message.
// Both bounds sit UNDER browser-runtime's 10 s per-command ceiling, which an awaited Runtime.evaluate
// is subject to: a longer in-page deadline can never be reached, and the caller is handed
// "Browser command timed out: Runtime.evaluate" instead of the locator that never mounted. The
// mountedMeasure this replaces used 12 s and could not reach it.
const SCROLL_MOUNT_TIMEOUT_MS = 8000
const scrolledIntoView = (expression, options, measure = '()=>true', what = '') => `new Promise((resolve,reject)=>{
  const deadline=performance.now()+${SCROLL_MOUNT_TIMEOUT_MS},
    find=()=>{try{return (${expression})||null}catch{return null}},
    settle=value=>{clearTimeout(timer);resolve(value)},
    fail=()=>{clearTimeout(timer);reject(new Error('scrolledIntoView: no node matched '+${JSON.stringify(expression)}+' within ${SCROLL_MOUNT_TIMEOUT_MS} ms'+${JSON.stringify(what ? ` (${what})` : '')}))},
    timer=setTimeout(fail,${SCROLL_MOUNT_TIMEOUT_MS + 500}),
    attempt=()=>{
      const element=find()
      if(!element)return performance.now()>deadline?fail():requestAnimationFrame(attempt)
      element.scrollIntoView(${JSON.stringify(options)})
      requestAnimationFrame(()=>requestAnimationFrame(()=>{
        const mounted=find()
        if(!mounted)return performance.now()>deadline?fail():attempt()
        settle((${measure})(mounted))
      }))
    }
  attempt()
})`

// The application versions come from the app's own table (ui/src/remote-web/application-capabilities.ts):
// a hard-coded list here silently stops covering the newest version the moment one is added, which is how
// v15/v16 arrived with every subtest failing on an 'unreviewed' applicationHello.
const APPLICATION_VERSIONS = (await readFile(new URL('../../ui/src/remote-web/application-capabilities.ts', import.meta.url), 'utf8'))
  .match(/export const APPLICATION_VERSIONS = \[([^\]]*)\]/)[1].split(',').map(v => Number(v.trim()))
if (!APPLICATION_VERSIONS.length || APPLICATION_VERSIONS.some(Number.isNaN)) throw new Error('could not read APPLICATION_VERSIONS')
const NEWEST_APPLICATION_VERSION = Math.max(...APPLICATION_VERSIONS)
// What a held PTT may go without: the station ends the over once no hold has arrived for GAP. Read
// from the station's own source, so the stream scenario judges a held press by the real number.
const PTT_GAP_MS = Number((await readFile(new URL('../../crates/tempo-app/src/remote_control/ptt_hold.rs', import.meta.url), 'utf8'))
  .match(/pub const GAP: Duration = Duration::from_millis\((\d+)\);/)?.[1])
if (!(PTT_GAP_MS > 0)) throw new Error('could not read the station\'s PTT hold GAP')
// Every scenario this suite runs: the stream, at a desktop's size and at a phone's. The hosted page is
// the stream page; the old workspace's scenarios went with it.
const SCENARIOS = [{applicationVersion:NEWEST_APPLICATION_VERSION,operating:true,stream:true},{applicationVersion:NEWEST_APPLICATION_VERSION,operating:true,stream:true,phone:true}]

// Shard selection. Unset means shard 0 of 1 — i.e. EVERYTHING — so a local run,
// `npm --prefix remote run test:browser` and `scripts/gates` all keep full coverage. CI
// sets these from a one-dimensional matrix, where the total comes from `strategy.job-total`
// rather than a second hard-coded number: the count of shards IS the count of jobs, so a
// shard cannot be added or removed without the partition following it.
//
// Refuse loudly on anything unparseable. A shard that silently selects nothing is the
// failure this guards — it would go green having run no browser test at all.
const SHARD  = Number(process.env.NEXUS_BROWSER_SHARD ?? 0)
const SHARDS = Number(process.env.NEXUS_BROWSER_SHARDS ?? 1)
if (!Number.isInteger(SHARDS) || SHARDS < 1) throw new Error(`NEXUS_BROWSER_SHARDS must be a positive integer, got ${process.env.NEXUS_BROWSER_SHARDS}`)
if (!Number.isInteger(SHARD) || SHARD < 0 || SHARD >= SHARDS) throw new Error(`NEXUS_BROWSER_SHARD must be 0..${SHARDS - 1}, got ${process.env.NEXUS_BROWSER_SHARD}`)
if (!SCENARIOS.length) throw new Error('the scenario list is empty — the extraction above broke')
const SHARD_SCENARIOS = SCENARIOS.filter((_, index) => index % SHARDS === SHARD)
if (!SHARD_SCENARIOS.length) throw new Error(`browser shard ${SHARD} of ${SHARDS} selects no scenario: there are ${SCENARIOS.length}`)
if (SHARDS === 1 && SHARD_SCENARIOS.length !== SCENARIOS.length) throw new Error('unsharded run must select every scenario')
console.log(`# browser shard ${SHARD} of ${SHARDS}: running ${SHARD_SCENARIOS.length} of ${SCENARIOS.length} scenarios`)

for (const {applicationVersion,operating,sessionLayout,quickLayout,quickMode='phone',contactContinuity,workSpot,radioSelection,routedTier,routedWorkspace,ftOperating,stream,phone} of SHARD_SCENARIOS) test(`compiled hosted browser ${phone?'phone':stream?'stream':ftOperating?'FT operating':routedWorkspace?'routed workspace':routedTier?'routed decoder':radioSelection?'radio selection':workSpot?'DX work':contactContinuity?'contact continuity':quickLayout?`quick layout${quickMode==='cw'?' CW':''}`:sessionLayout?'session layout':operating?'operations':`v${applicationVersion}`} completes PKCE, local device approval, observation and viewport checks`, { timeout: applicationVersion >= 14 ? 540000 : applicationVersion >= 13 ? 420000 : 180000 }, async context => {
  const app=await runtime(), artifacts=process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS ? join(process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS, phone?'phone':stream?'stream':ftOperating?'ft-operating':routedWorkspace?'routed-workspace':routedTier?'routed-decoder':radioSelection?'radio-selection':workSpot?'dx-work':contactContinuity?'contact-continuity':quickLayout?`quick-layout${quickMode==='cw'?'-cw':''}`:sessionLayout?'session-layout':operating?'operations':`v${applicationVersion}`) : undefined
  let browser, station, producing=true, pauseObservations=false, observationReadingAgeMs=0, observationPtt=true, producer, applicationProducer, stationSupervisor
  const results=[]
  const stop = cleanupAfterTest(context, async () => {
    producing=false;station?.close()
    // Stop the browser even if a producer rejected; no failed fixture may skip
    // process cleanup and overlap the next compatibility or operating case.
    try { await Promise.all([producer,applicationProducer,stationSupervisor]) }
    finally { try { await browser?.stop() } finally { await app.mf.dispose() } }
  })
  try {
    browser=await chrome(stream?['--use-fake-device-for-media-stream']:[])
    if(artifacts){await mkdir(artifacts,{recursive:true});await writeFile(join(artifacts,'chrome-version.json'),JSON.stringify(await browser.call('Browser.getVersion'),null,2)+'\n')}
    // One approval (operator decision 2026-09-13): the browser that confirms a pairing is approved
    // with the station. So Chrome must be that browser: the account claims the code, Chrome confirms
    // it below, and only then does the shack approve. app.paired() confirms from a Node client,
    // which would leave Chrome an ordinary second browser and never exercise the pairing path.
    const owner=await app.owner(), shack=app.client(), subject=JSON.parse(Buffer.from(owner.jwt.split('.')[1],'base64url')).sub
    const {value:enrollment}=await shack.post('enroll',{name:'Synthetic test station'})
    await owner.post('pair/claim',{code:enrollment.code})
    const stationCredential=randomBytes(32).toString('hex')
    const pair={browser:owner,native:app.client(null,'',stationCredential),stationId:enrollment.id}
    const shell = await fetch(app.origin,{signal:AbortSignal.timeout(3000)})
    assert.equal(shell.status,200)
    assert.match(await shell.text(), /Nexus Remote/)
    const stationHeaders = { 'x-nexus-application-version': '1',...(ftOperating||stream?{'x-nexus-operation-ft-version':'1'}:{}),...(stream?{'x-nexus-stream-version':'1'}:{}),...(operating?{'x-nexus-operation-version':'2','x-nexus-operation-max-version':'3'}:{}), ...(applicationVersion >= 2 ? { 'x-nexus-application-stream-version': '2' } : {}), ...(applicationVersion >= 3 ? { 'x-nexus-application-query-version': '1' } : {}), ...(applicationVersion >= 4 ? { 'x-nexus-application-recall-version': '1' } : {}), ...(applicationVersion >= 5 ? { 'x-nexus-application-keyboard-version': '1' } : {}), ...(applicationVersion >= 6 ? { 'x-nexus-application-insights-version': '1' } : {}), ...(applicationVersion >= 7 ? { 'x-nexus-application-dxpeditions-version': '1' } : {}), ...(applicationVersion >= 8 ? { 'x-nexus-application-memories-version': '1' } : {}), ...(applicationVersion >= 9 ? { 'x-nexus-application-ota-version': '1' } : {}), ...(applicationVersion >= 10 ? { 'x-nexus-application-field-day-version': '1' } : {}), ...(applicationVersion >= 11 ? { 'x-nexus-application-js8-version': '1' } : {}), ...(applicationVersion >= 12 ? {'x-nexus-application-station-modes-version':'1'} : {}), ...(applicationVersion >= 13 ? {'x-nexus-application-navigation-version':'1'} : {}), ...(applicationVersion >= 14 ? {'x-nexus-application-configuration-version':'1'} : {}) }
    let code=null, oauth=null, exchanges=0, providerFailure=false, exceptions=0, acknowledgements=0, unexpectedMessages=0
    const applicationTraffic = { reads: 0, acks: 0, subscriptions: 0, batches: 0, bytes: 0, byCommand: {}, maxResponseBytes: 0 }
    browser.on('Runtime.exceptionThrown', event=>{exceptions++; console.error(event.exceptionDetails?.exception?.description ?? 'Browser runtime exception')})
    browser.on('Fetch.requestPaused', (event,session) => { void (async()=>{
      const url=new URL(event.request.url)
      assert.equal(url.origin,'https://identity.remote-test.invalid')
      let body='', responseCode=200, headers=[{name:'content-type',value:'application/json'},
        {name:'access-control-allow-origin',value:app.origin},{name:'access-control-allow-headers',value:'content-type,auth0-client'}]
      if(event.request.method==='OPTIONS') responseCode=204
      else if(url.pathname==='/authorize') {
        assert.equal(url.searchParams.get('code_challenge_method'),'S256')
        assert.equal(url.searchParams.get('redirect_uri'),app.origin)
        code=randomBytes(32).toString('hex'); oauth={challenge:url.searchParams.get('code_challenge'),nonce:url.searchParams.get('nonce')}
        const redirect=new URL(app.origin);redirect.searchParams.set('code',code);redirect.searchParams.set('state',url.searchParams.get('state'))
        if(url.searchParams.get('response_mode')==='web_message'){
          headers[0].value='text/html';
          body='<script>window.parent.postMessage('+JSON.stringify({type:'authorization_response',response:{code,state:url.searchParams.get('state')}})+','+JSON.stringify(app.origin)+')</script>';
        }else{responseCode=302;headers.push({name:'location',value:redirect.href})}
      } else if(url.pathname==='/oauth/token') {
        const raw=event.request.postData||'', input=raw.startsWith('{')?JSON.parse(raw):Object.fromEntries(new URLSearchParams(raw))
        assert.ok(code && input.code===code,'one-use authorization code must match')
        assert.ok(createHash('sha256').update(input.code_verifier).digest('base64url')===oauth.challenge,'PKCE verification must succeed')
        assert.equal(input.grant_type,'authorization_code');code=null;exchanges++
        body=JSON.stringify({access_token:await app.token(subject),id_token:await app.idToken(subject,oauth.nonce),token_type:'Bearer',expires_in:3600,scope:'openid profile'})
      } else throw new Error('Unexpected simulated issuer operation')
      await browser.call('Fetch.fulfillRequest',{requestId:event.requestId,responseCode,responseHeaders:headers,body:Buffer.from(body).toString('base64')},session)
    })().catch(async()=>{providerFailure=true;await browser.call('Fetch.failRequest',{requestId:event.requestId,errorReason:'Failed'},session).catch(()=>{})}) })
    const target=(await browser.call('Target.createTarget',{url:'about:blank'})).targetId
    const session=(await browser.call('Target.attachToTarget',{targetId:target,flatten:true})).sessionId
    for(const method of ['Page.enable','Runtime.enable','Network.enable'])await browser.call(method,{},session)
    const operationWire=[]
    // Declared before until(), whose operating diagnostic reads it: any wait that timed out before
    // the logging actor's declarations threw a ReferenceError and hid which wait had failed.
    const loggedRequests=[]
    let receiverRead=null
    browser.on('Network.webSocketFrameReceived',event=>{try{const v=JSON.parse(event.response.payloadData);if(v.type==='observation')receiverRead={at:performance.now(),radio:v.frame?.station?.radio};if(v.type==='operationResponse')operationWire.push({at:performance.now(),direction:'in',requestId:v.requestId,error:v.error,phase:v.value?.phase,outcome:v.value?.outcome,evidence:v.value?.evidence,reason:v.value?.reason})}catch{}})
    browser.on('Network.webSocketFrameSent', event => {
      if (event.response.opcode !== 1) return
      try {
        const message = JSON.parse(event.response.payloadData)
        if (message.type === 'ack' && Object.keys(message).sort().join(',') === 'epoch,sequence,type') acknowledgements++
        else if (message.type === 'applicationHello' && (Object.keys(message).length === 1 || (Object.keys(message).length === 2 && APPLICATION_VERSIONS.includes(message.version)))) {}
        // S1-M1: beside its request, the browser's own proof, in its exact shape, on all but Stop and state.
        else if(message.type==='operationRequest'&&(({proof,...rest})=>(proof===undefined||laneProofShape(proof))&&((Object.keys(rest).length===2)||Object.keys(rest).length===3&&[2,3,4,5].includes(rest.operationVersion)))(message)&&['state','acquire','heartbeat','release','result','logManual','stationControl','stopTransmit'].includes(message.request?.type)){if(!operating)assert.equal(message.request.type,'state');operationWire.push({at:performance.now(),direction:'out',type:message.request.type,requestId:message.request.requestId,action:message.request.action?.action,on:message.request.action?.on,signed:'proof' in message})}
        else if (stream && message.type === 'streamSignal' && Object.keys(message).length === 3 && ['offer', 'candidate', 'close'].includes(message.payload?.kind)) {}
        else if (message.type === 'applicationRead' && Object.keys(message).length === 4) applicationTraffic.reads++
        else if (message.type === 'applicationQuery' && Object.keys(message).length === 7) applicationTraffic.queries=(applicationTraffic.queries??0)+1
        else if (message.type === 'applicationQueryAck' && Object.keys(message).length === 2) {}
        else if (message.type === 'applicationAck' && Object.keys(message).length === 2) applicationTraffic.acks++
        else if (message.type === 'applicationSubscribe' && Object.keys(message).length === 3) applicationTraffic.subscriptions++
        else if (message.type === 'applicationFrameAck' && Object.keys(message).length === 3) applicationTraffic.acks++
        else { unexpectedMessages++; console.error('Unreviewed browser message', message) }
      } catch { unexpectedMessages++ }
    })
    await browser.call('Page.addScriptToEvaluateOnNewDocument',{source:`if(window===window.top){
      // Pin the synthetic main window so auto-fit cannot overwrite the scale
      // being measured after a viewport resize. No operator storage is used.
      localStorage.setItem('nexus-ui-scale-mode','100');
      window.__queryTrace=[];window.__availabilityTrace=[];window.__socketClosures=[];window.__protocolTrace=[];const originalWebSocket=window.WebSocket;
      window.WebSocket=class extends originalWebSocket{
        constructor(...args){super(...args);this.addEventListener('close',e=>window.__socketClosures.push({at:performance.now(),code:e.code,reason:e.reason}));this.addEventListener('message',e=>{try{const v=JSON.parse(e.data);if(['applicationQueryError','applicationPage'].includes(v.type)){const m=v.meta?.source;window.__queryTrace.push({at:performance.now(),type:v.type,id:v.requestId,error:v.error,collection:v.collection,age:v.ageMs,document:{kind:m?.kind,search:m?.search,id:m?.contextId,context:m?.stationContextId,validForMs:m?.validForMs,ageMs:m?.documentAgeMs,capturedAgeMs:v.meta?.capturedAgeMs}});window.__queryTrace=window.__queryTrace.slice(-80)}if(v.type==='applicationFrame'){window.__protocolTrace.push({at:performance.now(),updates:v.updates.map(u=>({command:u.command,type:u.type,age:u.ageMs,revision:u.revision}))});window.__protocolTrace=window.__protocolTrace.slice(-40)}}catch{}})}
        send(raw){try{const v=JSON.parse(raw);if(v.type==='applicationQuery'){window.__queryTrace.push({at:performance.now(),out:true,id:v.requestId,collection:v.collection,search:v.search,cursor:v.cursor});window.__queryTrace=window.__queryTrace.slice(-80)}window.__protocolTrace.push({at:performance.now(),out:v.type,requestId:v.requestId,topics:v.topics,buffered:this.bufferedAmount});window.__protocolTrace=window.__protocolTrace.slice(-80)}catch{}return super.send(raw)}
        close(...args){window.__socketClosures.push({at:performance.now(),requested:true,code:args[0],reason:args[1],buffered:this.bufferedAmount});return super.close(...args)}
      };
      window.__imagePolicyViolations=[];window.addEventListener('securitypolicyviolation',e=>{if(e.violatedDirective.startsWith('img-src'))window.__imagePolicyViolations.push({directive:e.violatedDirective,scheme:e.blockedURI.split(':')[0]})});
      window.__frameDelay=0;
      const originalRaf=window.requestAnimationFrame.bind(window),originalCancel=window.cancelAnimationFrame.bind(window),delayedFrames=new Map();
      window.requestAnimationFrame=callback=>{const id=originalRaf(time=>{if(!window.__frameDelay){callback(time);return}delayedFrames.set(id,setTimeout(()=>{delayedFrames.delete(id);callback(performance.now())},window.__frameDelay))});return id};
      window.cancelAnimationFrame=id=>{originalCancel(id);clearTimeout(delayedFrames.get(id));delayedFrames.delete(id)};
      window.__keyboardInset=0;const actual=visualViewport;const area=new EventTarget();
      Object.defineProperties(area,{width:{get:()=>actual.width},height:{get:()=>actual.height-window.__keyboardInset},
        offsetLeft:{get:()=>actual.offsetLeft},offsetTop:{get:()=>actual.offsetTop},
        pageLeft:{get:()=>actual.pageLeft},pageTop:{get:()=>actual.pageTop},scale:{get:()=>actual.scale}});
      actual.addEventListener('resize',()=>area.dispatchEvent(new Event('resize')));
      actual.addEventListener('scroll',()=>area.dispatchEvent(new Event('scroll')));
      Object.defineProperty(window,'visualViewport',{value:area});
      // The waterfall's picture is the spectrum renderer's canvas in .waterfall-render: a draw is a WebGL2 drawArrays, or a canvas-2D putImageData without WebGL2.
      // Its WebGL2 drawing buffer is kept after it is shown, so the picture can be read back after the frame that drew it (it changes no pixel).
      const getContext=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(id,attrs,...rest){return getContext.call(this,id,id==='webgl2'&&this.classList.contains('spectrum-canvas')?{...attrs,preserveDrawingBuffer:true}:attrs,...rest)};
      window.__waterfallDraws=0;const inWaterfall=c=>!!c?.closest?.('.waterfall-render');const draw=CanvasRenderingContext2D.prototype.putImageData;
      CanvasRenderingContext2D.prototype.putImageData=function(...args){if(inWaterfall(this.canvas))window.__waterfallDraws++;return draw.apply(this,args)};
      if(typeof WebGL2RenderingContext!=='undefined'){const drawGl=WebGL2RenderingContext.prototype.drawArrays;
      WebGL2RenderingContext.prototype.drawArrays=function(...args){if(inWaterfall(this.canvas))window.__waterfallDraws++;return drawGl.apply(this,args)}}
    }`},session)
    const runEvaluate=async expression=>{
      const value=await browser.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},session)
      if(value.exceptionDetails)throw new Error('Browser evaluation failed: '+String(value.exceptionDetails.exception?.description??value.exceptionDetails.text).split('\n')[0])
      return value.result?.value
    }
    // A measurement can arrive between React committing and the node existing, and on a loaded
    // runner that window is wide: CI has measured a single case at 165 s against ~60 s here, and
    // read getBoundingClientRect off a null querySelector for it. Two animation frames prove a
    // PAINT happened, not that the element was ever mounted, so settledLayout cannot close it.
    // One bounded retry after a settle does. This cannot hide a genuinely missing element - that
    // fails both attempts and still throws, with the same message it always had.
    const evaluate=async expression=>{
      try { return await runEvaluate(expression) }
      catch (error) {
        if(!/Cannot read properties of null|Cannot read property .* of null/.test(String(error.message)))throw error
        await runEvaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))')
        return await runEvaluate(expression)
      }
    }
    // useViewport publishes dimensions on an animation frame. A fixed sleep can
    // measure the previous viewport on a busy renderer. Wait through the update
    // and its layout frame; all pixel/overflow assertions below stay unchanged.
    const settledLayout=()=>evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))')
    // settledLayout proves a PAINT happened, not that the geometry stopped moving, and the Tempo
    // feed keeps resizing after its first paint. A CI failure on 2026-09-22 traced .message-scroll
    // shrinking clientHeight 260 -> 214 BETWEEN arming the trace and the wheel gesture; because
    // scrollIntoView({block:'nearest'}) asks for the MINIMUM scroll, against a box whose height is
    // still changing it resolved to 89 px of a 3710 px range and left the first bubble 3040 px
    // above the viewport — which read as "unreachable" and was nothing of the kind. Wait for the
    // scroller's own numbers to hold still across consecutive frames before measuring against them.
    const settledScroller=(selector,timeout=4000)=>evaluate(`new Promise(resolve=>{
      const deadline=performance.now()+${timeout};let last='',stable=0;
      const tick=()=>{const e=document.querySelector(${JSON.stringify(selector)});
        if(!e)return resolve(null);
        const now=e.clientHeight+'x'+e.scrollHeight;
        if(now===last){if(++stable>=3)return resolve(now)}else{stable=0;last=now}
        if(performance.now()>deadline)return resolve(now);
        requestAnimationFrame(tick)};
      tick()})`)
    // scrollIntoView can no-op against a settling box, so do not trust that it landed: drive the
    // scroller the remaining distance and say whether the row actually arrived. Returns the final
    // offset of the row from the container's top edge, which the trace then records.
    const scrollRowToTop=(container,row,tries=6)=>evaluate(`(()=>{
      const c=document.querySelector(${JSON.stringify(container)});if(!c)return null;
      for(let i=0;i<${tries};i++){
        const r=document.querySelector(${JSON.stringify(row)});if(!r)return null;
        const delta=r.getBoundingClientRect().top-c.getBoundingClientRect().top;
        if(Math.abs(delta)<=1)return 0;
        const before=c.scrollTop;
        // scrollTo({behavior:'instant'}), never scrollTop+=: .message-scroll carries CSS
        // scroll-behavior: smooth, so a plain assignment starts an ANIMATION and reading
        // scrollTop straight back returns the OLD value. The first draft of this helper did
        // exactly that, saw before===after, concluded "clamped" and gave up on iteration one --
        // reporting a -3997 px miss it had itself caused. usePinnedScroll's snapToBottom
        // documents the same trap and is why it uses scrollTo.
        c.scrollTo({top:before+delta,behavior:'instant'});
        if(Math.abs(c.scrollTop-before)<0.5)return delta;
      }
      const r=document.querySelector(${JSON.stringify(row)});
      return r?r.getBoundingClientRect().top-c.getBoundingClientRect().top:null})()`)
    const sessionDiagnostic=()=>evaluate(`(()=>{const e=document.querySelector('.app');let fiber=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))],client;while(fiber){client=fiber.memoizedProps?.connection?.application;if(client)break;fiber=fiber.return}return {now:performance.now(),stale:e?.dataset.remoteStale,phase:client?.getPhase(),snapshotAge:client?.age('get_snapshot'),topics:client?.stream?.topics,waiting:client?.stream?.waiting?[...client.stream.waiting.keys()]:null,interests:client?.stream?.interests?[...client.stream.interests].map(([name,at])=>({name,age:performance.now()-at})):null,closures:window.__socketClosures,trace:window.__protocolTrace}})()`)
    // What React last committed, read twice a second apart: App's `remote` (stale, staleShown), the hook values
    // of the component holding the connection (the workspace's 500 ms tick and the stale display's hold among
    // them) on both of its fibers, and the root's lanes. A wait for the stale display that times out with the
    // session already unavailable says nothing else about which link failed: a render that stopped, a hold
    // that never fired, or work React never committed.
    const reactDiagnostic=async()=>{const read=()=>evaluate(`(()=>{const e=document.querySelector('.app'),key=e&&Object.keys(e).find(k=>k.startsWith('__reactFiber$'));let f=key?e[key]:null,app=null,host=null;while(f){if(!app&&f.memoizedProps?.remote)app=f;if(f.memoizedProps?.connection?.application){host=f;break}f=f.return}const hooks=x=>{const out=[];for(let h=x?.memoizedState;h&&out.length<40;h=h.next){const v=h.memoizedState;out.push(v==null||['boolean','number','string'].includes(typeof v)?v??null:typeof v)}return out},root=document.getElementById('root'),rootKey=root&&Object.keys(root).find(k=>k.startsWith('__reactContainer$')),fiberRoot=rootKey?root[rootKey]?.stateNode:null;return {now:performance.now(),remote:app?{stale:app.memoizedProps.remote.stale,staleShown:app.memoizedProps.remote.staleShown}:null,hooks:hooks(host),alternate:hooks(host?.alternate),lanes:fiberRoot?{pending:fiberRoot.pendingLanes,suspended:fiberRoot.suspendedLanes,pinged:fiberRoot.pingedLanes,expired:fiberRoot.expiredLanes}:null}})()`);const first=await read();await sleep(1000);return {first,second:await read()}}
    // A wait that times out says what the page showed, in every scenario: the first `.app` and the page's
    // text (the workspace, the station list or the crash panel), the application session, and whether the
    // station stand-in's socket is still open. Two of the three reds of 2026-10-05 left only "Expected
    // browser state did not appear" in the log, and neither could be read back. The read can never
    // replace the failure.
    async function until(expression,timeout=12000) { for(let i=0;i<Math.ceil(timeout/100);i++){ if(providerFailure)throw new Error('Simulated provider failed'); if(await evaluate(expression))return;await sleep(100) } if(operating)console.log('Operation diagnostic',expression,operationWire.slice(-30),loggedRequests.map(r=>({call:r.record.call,mode:r.record.mode})),await evaluate(`({status:document.querySelector('.remote-application-status')?.textContent,entries:[...document.querySelectorAll('.remote-log-entry')].map(e=>({text:e.textContent,error:e.dataset.operationError}))})`));try{console.log('Browser state diagnostic',JSON.stringify({expression:expression.slice(0,240),page:await evaluate(`({app:document.querySelector('.app')?.className??null,text:document.body.innerText.slice(0,300)})`),session:await sessionDiagnostic(),react:await reactDiagnostic().catch(error=>String(error)),station:{closed:station?.closed,code:station?.closeCode,reason:station?.closeReason}}))}catch(error){console.log('Browser state diagnostic unavailable:',String(error))}throw new Error('Expected browser state did not appear') }
    // A WAIT FOR RESTORED STATION DATA OUTLASTS A DROPPED STATION (2026-10-06). A stall of 3 s or more on the runner
    // drops the station stand-in: 1008 `applicationTimeout` when the local Worker (workerd) stalls, `stationTooSlow`
    // when the stand-in does. The page then has no station data until the stand-in has connected again (2-3 s, 4-5 s
    // for a second drop within a minute) and the page after it (its own 1, 2, 4 s ladder), which can outlast a 12 s
    // wait. CI run 37410816282 went red so in v17: workerd stalled twice about 36 s apart, and the wait for the
    // restored Conditions data ran out while the page said "Station data unavailable". Stopping only workerd for 4 s
    // twice, once before that data is withheld and once at the wait, reproduces it (the data was back after 12.2 s);
    // one such stop alone, at the wait, had it back after 8.4 s. So a wait for data the scenario has just restored,
    // when it runs out after the page lost its station during it (a socket closed, or no current station data), waits
    // up to 30 s more for the data, and the log says so. Never skipped: without such a lapse it fails at its own
    // deadline, exactly as before.
    async function untilThroughLapse(expression) {
      const since=await evaluate('performance.now()')
      try { await until(expression) }
      catch (error) {
        const session=error.message==='Expected browser state did not appear'?await sessionDiagnostic().catch(()=>null):null
        if(!session)throw error
        const closures=(session.closures??[]).filter(closure=>closure.at>since)
        const lapse={closures,phase:session.phase,snapshotAge:session.snapshotAge,stationSocket:{closed:station?.closed,code:station?.closeCode,reason:station?.closeReason}}
        if(!closures.length&&session.phase==='ready'&&session.snapshotAge<3000){console.log('Restored station data missing while the page kept its station',expression.slice(0,160),JSON.stringify(lapse));throw error}
        const started=performance.now(),back=await until(expression,30000).then(()=>true,next=>{if(next.message!=='Expected browser state did not appear')throw next;return false})
        console.log(`Restored station data: the page lost its station during the wait; ${back?`read again after ${Math.round(performance.now()-started)} ms`:'not back within 30 s more'}`,expression.slice(0,160),JSON.stringify(lapse))
        if(!back)throw error
      }
    }
    const button=name=>`[...document.querySelectorAll('button')].find(e=>e.textContent===${JSON.stringify(name)})`
    // At the small size (sm/xs) the top bar folds its eleven tier pills into one mode button with a menu (2026-10-01):
    // there the reachable tier selector is that button, and a tier gesture opens it and picks the item in the pill's place.
    const smallBar=`['xs','sm'].includes(document.documentElement.dataset.viewport)`
    // A logManual or stationControl reply clears the client's station state in the same update
    // that shows its outcome (operation-client.ts receive(): state:null beside controlResult or
    // resolved), and only the next heartbeat reply restores it. Until then every station control is
    // disabled. That round trip also used to move the page: Release unmounted and the sticky banner
    // changed height (traced 116.6 -> 61.6 -> 84.6 px, nav +23 px; a tab click measured before the
    // re-read landed in the rail's gap). The banner now keeps its shape and draws the retained state,
    // and the layout probe below asserts it stays still. The wait remains for the controls to be
    // enabled again: `data-station-state` is 'current' only once the client holds a re-read state.
    // The outcome text is the first half of the round trip; wait for both halves.
    // THE STATION'S OWN STATE IS BACK. `data-station-state` alone stopped meaning that: a control
    // keeps its state now (a held control stays lit while its command confirms), so the row reads
    // 'current' throughout the re-read it is supposed to wait for. The row says the re-read is
    // outstanding in words — "Updating station controls…" — so wait for that to go as well.
    const stationStateRead=`(()=>{const e=document.querySelector('.remote-logging-authority');return e?.dataset.stationState==='current'&&!e.textContent.includes('Updating station controls')})()`
    // THE STATION'S OWN READING SHOWS THIS DIAL — not the browser's optimistic digits, which show
    // what was asked for until the station catches up and carry `aria-busy` while they do.
    const stationDial=(root,mhz)=>`(()=>{const v=document.querySelector('${root} .readout-val');return !!v&&!v.closest('.readout')?.hasAttribute('aria-busy')&&v.textContent.includes('${mhz}')})()`
    // Stale readings are retired in place (faded, unclickable), never blanked: a blanked workspace
    // was the black flash. Opacity is not inherited, so multiply it up the ancestor chain.
    const retiredReadings=selector=>`(()=>{const e=document.querySelector(${JSON.stringify(selector)});let opacity=1;for(let p=e;p;p=p.parentElement)opacity*=Number(getComputedStyle(p).opacity);return {opacity,pointer:getComputedStyle(e).pointerEvents,visibility:getComputedStyle(e).visibility}})()`
    const isRetired=r=>r.visibility==='visible'&&r.pointer==='none'&&r.opacity<=.5
    // Layout probe for the 2026-09-13 flicker report. Samples, on every animation frame, the session
    // banner's height and the layout offsets (offsetTop) of the TopBar and shell below it; records
    // every element inserted into or removed from the banner and every removal of a tuning digit
    // anywhere; and notes the authority label texts, the stale flag and frames where the shell was
    // blanked. offsetTop, not a viewport rect: click() scrolls its target into view, and a scroll
    // moved the TopBar's rect 114 px with the banner perfectly still (a false shift).
    const layoutProbeStart=`(()=>{const s=document.querySelector('.remote-application-status'),p={samples:[],removed:[],added:[],digitRemovals:0,labels:new Set(),stale:new Set(),stop:false};const doc=e=>e?e.offsetTop:null;p.mo=new MutationObserver(rs=>{for(const r of rs){const inBanner=s.contains(r.target);for(const n of r.removedNodes)if(n.nodeType===1){if(inBanner)p.removed.push(n.outerHTML.slice(0,90));if(n.matches('.readout-digit')||n.querySelector('.readout-digit'))p.digitRemovals++}for(const n of r.addedNodes)if(n.nodeType===1&&inBanner)p.added.push(n.outerHTML.slice(0,90))}});p.mo.observe(document.body,{childList:true,subtree:true});const frame=()=>{if(p.stop)return;const shell=document.querySelector('.shell'),app=document.querySelector('.app');p.samples.push({banner:s.getBoundingClientRect().height,topbar:doc(document.querySelector('.topbar')),shell:doc(shell),hidden:!!shell&&getComputedStyle(shell).visibility==='hidden'});p.labels.add(document.querySelector('.remote-logging-authority [role=status]')?.textContent??'');p.stale.add(app?.dataset.remoteStale??'');requestAnimationFrame(frame)};frame();window.__layoutProbe=p;return true})()`
    const assertStill=(probe,what)=>{
      assert.ok(probe.frames>0,`${what}: the layout probe recorded no frames`)
      assert.ok(probe.bannerRange<=1&&probe.topbarRange<=1&&probe.bannerRemoved===0&&probe.bannerAdded===0,`${what}: the session banner must not move the page: ${JSON.stringify(probe)}`)
    }
    const layoutProbeRead=`(()=>{const p=window.__layoutProbe;p.stop=true;p.mo.disconnect();const range=k=>{const v=p.samples.map(s=>s[k]).filter(x=>x!==null);return v.length?Math.max(...v)-Math.min(...v):0};return {frames:p.samples.length,bannerRange:range('banner'),topbarRange:range('topbar'),shellRange:range('shell'),bannerHeights:[...new Set(p.samples.map(s=>Math.round(s.banner*10)/10))],bannerRemoved:p.removed.length,bannerAdded:p.added.length,removedSample:p.removed.slice(0,6),digitRemovals:p.digitRemovals,labels:[...p.labels],stale:[...p.stale],hiddenFrames:p.samples.filter(s=>s.hidden).length}})()`
    async function click(expression, clickCount = 1) {
      let point
      try {
        // A positive user action waits for its control to finish loading. Keep
        // the actual enabled, hit-target and mouse-event checks below intact.
        for(let attempt=0;attempt<30&&!point;attempt++){
          await until(`(()=>{const e=${expression},r=e?.getBoundingClientRect();return !!e&&!e.disabled&&r.width>0&&r.height>0})()`)
          // Center controls below the sticky session banner. Nearest can leave
          // an offscreen tier row behind it after switching cockpit tabs.
          await evaluate(`(()=>{const e=${expression};if(e&&!e.disabled)e.scrollIntoView({block:'center',behavior:'instant'})})()`)
          await settledLayout()
          // A heartbeat can disable the control between CDP read turns. Wait
          // again BEFORE the single mouse gesture; never retry a sent click.
          point=await evaluate(`(()=>{const e=${expression};if(!e||e.disabled)return null;const r=e.getBoundingClientRect();if(r.width<=0||r.height<=0)return null;const x=r.x+r.width/2,y=r.y+r.height/2;if(!e.contains(document.elementFromPoint(x,y)))throw Error('occludedControl');return{x,y}})()`)
        }
        assert.ok(point,'control must become enabled before the single mouse gesture')
      }
      catch(error){console.log('Application session diagnostic',JSON.stringify(await sessionDiagnostic()),{...applicationTraffic,nativeSnapshotAge:performance.now()-(sentAt.get('get_snapshot')??0),stationClosed:station?.closed});console.log('Click diagnostic',expression,await evaluate(`(()=>{const e=${expression},r=e?.getBoundingClientRect();return {stale:document.querySelector('.app')?.dataset.remoteStale,status:document.querySelector('.remote-application-status')?.textContent,rect:r?.toJSON(),hit:r?document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.outerHTML.slice(0,600):null}})()`));if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-click-failure.png'),Buffer.from(shot.data,'base64'))}throw error}
      await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount},session)
      await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount},session)
      await sleep(50)
    }
    async function geometry(width,height,zoom=1,theme='dark',keyboard=0) {
      await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.__keyboardInset=${keyboard};window.dispatchEvent(new Event('resize'));visualViewport.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      const shape=await evaluate(`(()=>{const app=document.querySelector('.remote-monitor-app'),r=app.getBoundingClientRect(),main=document.querySelector('.rm-scroll');return{appW:r.width,appH:r.height,w:innerWidth,h:visualViewport.height,docW:document.documentElement.scrollWidth,mainW:main.clientWidth,scrollW:main.scrollWidth,scrollers:[...document.querySelectorAll('body *')].filter(e=>/auto|scroll/.test(getComputedStyle(e).overflowY)&&e.scrollHeight>e.clientHeight+1).map(e=>e.className)}})()`)
      assert.ok(shape.docW<=shape.w+1 && shape.scrollW<=shape.mainW+1 && Math.abs(shape.appW-shape.w)<=1 && Math.abs(shape.appH-shape.h)<=1 && shape.scrollers.every(name=>name==='rm-scroll'),'one bounded scroll owner must fit the effective viewport: '+JSON.stringify({width,height,zoom,theme,keyboard,shape}))
      results.push({width,height,zoom,theme,keyboard,shape})
    }
    await browser.call('Page.navigate',{url:app.origin},session)
    // Signed out, the page offers "Sign in" and "Create an account" (the site restyle); both open the
    // same SDK flow, so the PKCE exchange below is unchanged.
    await until(`!!${button('Sign in')}&&!!${button('Create an account')}`)
    // The old watch/control workspace and observer are gone. A browser that kept the flag that once
    // showed them still gets the stream page alone.
    await evaluate(`localStorage.setItem('nexus.remote.workspace','on');true`)
    await geometry(390,844)
    await browser.call('Fetch.enable',{patterns:[{urlPattern:'https://identity.remote-test.invalid/*'}]},session)
    await click(button('Sign in'))
    await until(`document.body.textContent.includes(${JSON.stringify(pair.browser.accountId)})`)
    assert.equal(exchanges,1,'the actual SDK must exchange a PKCE authorization code')
    // The pairing browser: Chrome confirms the claimed station, then the shack approves the pairing.
    await click(button('Attach this station'))
    await until(`document.body.textContent.includes('waiting for approval in Nexus at the shack')`)
    assert.equal((await app.db.prepare('SELECT COUNT(*) AS count FROM devices WHERE station_id=?').bind(pair.stationId).first()).count,0,'nothing is approved before the shack approves')
    await shack.post('enroll/approve',{id:enrollment.id,proof:enrollment.proof,credential:stationCredential})
    // S3-M1: the station records its own key before it connects, as Nexus at the shack does.
    const signer=await stationSigner()
    await pair.native.post(`stations/${pair.stationId}/native/key`,{publicKey:signer.publicKey})
    station=await pair.native.open(pair.stationId, undefined, 101, stationHeaders)
    // One approval: the confirming browser is approved with no request and no approve-device call.
    await until(`!!${button('Remove this browser’s approval')}`)
    assert.equal(await evaluate(`document.body.textContent.includes("isn't approved for this station yet")||document.body.textContent.includes('Waiting for approval at the shack')`),false,'the pairing browser is never asked to request a separate approval')
    assert.equal(await evaluate(`!!${button('Observe station')}||!!${button('Open Nexus')}`),false,'the old workspace entry points are gone, flag or no flag')
    const pairedDevices=(await app.db.prepare('SELECT id,approved FROM devices WHERE station_id=?').bind(pair.stationId).all()).results
    assert.equal(pairedDevices.length,1);assert.equal(pairedDevices[0].approved,1,'approving the pairing approved the browser that confirmed it')
    // Any OTHER browser still needs its own approval at the shack. Removing this browser's approval
    // makes Chrome one: the card's Stream asks the shack (under the browser's own name and key, with
    // nothing to type), and nothing opens until the station approves.
    await click(button('Remove this browser’s approval'))
    await until(`document.body.textContent.includes("isn't approved for this station yet")&&!!${button('Stream')}`)
    await click(button('Stream'))
    await until(`document.body.textContent.includes('Waiting for approval at the shack')`)
    assert.equal(await evaluate(`!!document.querySelector('.remote-stream-app')`),false,'asking opens nothing')
    const device=await app.db.prepare('SELECT id FROM devices WHERE station_id=? AND id<>? AND approved=0').bind(pair.stationId,pairedDevices[0].id).first()
    assert.ok(device,'the browser must enroll through its own HTTP-only cookie')
    // Deterministically exercise a frame that arrives after the former sleep.
    await evaluate('window.__frameDelay=120')
    await geometry(360,740,1.75,'light',260)
    await evaluate('window.__frameDelay=0')
    await pair.native.post(`stations/${pair.stationId}/native/approve-device`,{deviceId:device.id})
    await until(`!!${button('Remove this browser’s approval')}`)
    // S3-L1 in real Chrome: the page keeps the key the service lists for the station the first time it
    // sees it, in a database of its own. With another key kept (what a changed listing looks like to
    // the page), the card says so with both keys, laid out at a phone's size and a desktop's, Stream is
    // off, and only Accept keeps the listed key again.
    if(stream){
      const keptKey=`new Promise(done=>{const open=indexedDB.open('nexus-remote-station-keys');open.onsuccess=()=>{const got=open.result.transaction('keys').objectStore('keys').get(${JSON.stringify(pair.stationId)});got.onsuccess=()=>done(got.result??null)}})`
      const changed=`document.body.textContent.includes('This station’s key has changed')`
      await until(`document.body.textContent.includes('This station’s key: ')`)
      assert.equal(await evaluate(keptKey),signer.publicKey,'S3-L1: the page kept the key the service lists')
      assert.equal(await evaluate(changed),false,'control: nothing changed yet')
      // A made-up key shape (the P-256 SPKI prefix, 04, then 0xab sixty-four times), not a key.
      const other='3059301306072a8648ce3d020106082a8648ce3d03010703420004'+'ab'.repeat(64)
      await evaluate(`new Promise(done=>{const open=indexedDB.open('nexus-remote-station-keys');open.onsuccess=()=>{const t=open.result.transaction('keys','readwrite');t.objectStore('keys').put(${JSON.stringify(other)},${JSON.stringify(pair.stationId)});t.oncomplete=()=>done(true)}})`)
      // The card reads the stations again every five seconds.
      await until(changed,15000)
      assert.ok(await evaluate(`document.body.textContent.includes('Key this browser kept: A8DD D2FF AD49 30AC 6B77 647B 8DE0 D379')`),'S3-L1: both keys are shown, in eight groups')
      assert.equal(await evaluate(`${button('Stream')}.disabled`),true,'S3-L1: nothing connects while the key has changed')
      for(const [w,h] of [[360,740],[1280,800]])await geometry(w,h,1,'dark')
      await click(button('Accept the new key'))
      await until(`!${changed}&&!${button('Stream')}.disabled`)
      assert.equal(await evaluate(keptKey),signer.publicKey,'S3-L1: Accept kept the listed key')
    }
    for(const [w,h] of [[360,740],[390,844],[844,390],[1024,768],[1280,800],[1366,768],[3440,1440]])for(const theme of ['dark','light'])await geometry(w,h,1,theme)
    if(artifacts){await mkdir(artifacts,{recursive:true});await geometry(390,844);const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-account.png'),Buffer.from(shot.data,'base64'))}
    const fixture=JSON.parse(await readFile(new URL('../../ui/src/remote-monitor/fixtures.v2.json',import.meta.url),'utf8')).spe
    let sequence=0
    producer=(async()=>{while(producing){const source=station;let watch;try{watch=await source.take(value=>value.type==='watch'&&value.enabled,1000)}catch{continue}await sleep(200);while(producing&&pauseObservations)await sleep(50);if(!producing)break;if(source!==station||source.closed)continue;const radio=fixture.station.radio
      // The station's own PTT reading can be older than its 1 s window while the link itself is
      // healthy — the poll that took it simply ran late at the shack. Publish it that way rather
      // than slowing the stream, which would age the frame instead and trip the 3 s observation bound.
      // `observationPtt=false` is a station with no PTT sample at all — what an expired one becomes.
      // The wire schema pairs a reading with its value, so `rigKeyed` goes with it.
      const ptt=!observationPtt?null:observationReadingAgeMs&&radio.readings.ptt?{...radio.readings.ptt,ageMs:observationReadingAgeMs}:radio.readings.ptt
      const aged=ptt===radio.readings.ptt?fixture.station:{...fixture.station,radio:{...radio,rigKeyed:ptt?radio.rigKeyed:null,readings:{...radio.readings,ptt}}}
      source.send({type:'publication',requestId:watch.requestId,frame:{...fixture,station:aged,source:'native',sequence:++sequence}})}})()
    const applicationData = await applicationFixture()
    applicationData.get_settings.fdActive = true
    if(operating)applicationData.get_settings.bandChoices=Object.fromEntries(['cw','phone'].map(mode=>[mode,
      ['40m','20m'].map(band=>({band,group:'HF',dialMhz:band==='40m'?7.2:14.25,mode:'USB',label:band,note:'',tx:true}))]))
    const collections = collectionFixture()
    const navigation=await navigationFixture(),navigationQueries=[]
    if(operating){
      // One coherent synthetic station for observation, application display,
      // configuration and authority. Hardware/native saves have separate tests.
      applicationData.get_snapshot.activeRadioId=1
      applicationData.get_snapshot.radio.source='native'
      applicationData.get_snapshot.radio.decodeDepth=3
      Object.assign(applicationData.get_snapshot.radio,{nb:false,nr:false,notch:false,manualNotch:false,comp:false,vox:false,agc:'fast'})
      applicationData.get_snapshot.radio.rigKeyed=false
      fixture.station.radio.id=1;fixture.station.radio.catConnected=true;fixture.station.radio.rigKeyed=false;fixture.station.radio.nexusBusy=false
      for(const key of ['cat','dial','mode','ptt'])if(fixture.station.radio.readings[key])fixture.station.radio.readings[key]={connectionGeneration:1,readSequence:1,ageMs:0}
      Object.assign(fixture.station.amplifier,applicationData.get_snapshot.radio.amp,{followBand:false,reading:{connectionGeneration:1,readSequence:1,ageMs:0}})
      const doc=navigation.documents.settings
      Object.assign(doc.settings,{activeRadio:1,ampModel:'spe',ampPort:'synthetic-browser-amp',ampFollowBand:false})
      doc.settings.radios[0].id=1;doc.settings.radios[0].ampFollowBand=false
      doc.revision=createHash('sha256').update(JSON.stringify(doc.settings)).digest('hex')
    }
    let radioConnection=1
    const controlContext=()=>({radioId:(radioSelection||workSpot)?applicationData.get_snapshot.activeRadioId:1,radioConnection,ampConnection:1,ampReadSequence:1})
    const adoptBrowserRadio=id=>{
      assert.ok((radioSelection||workSpot)&&[1,2].includes(id))
      assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
      if(applicationData.get_snapshot.activeRadioId===id)return
      radioConnection++
      applicationData.get_snapshot.activeRadioId=id
      for(const radio of applicationData.get_snapshot.radios)radio.isActive=radio.id===id
      applicationData.get_settings.activeRadio=id
      const doc=navigation.documents.settings
      doc.settings.activeRadio=id
      doc.revision=createHash('sha256').update(JSON.stringify(doc.settings)).digest('hex')
      fixture.station.radio.id=id
      for(const key of ['cat','dial','mode','ptt'])if(fixture.station.radio.readings[key])fixture.station.radio.readings[key]={connectionGeneration:radioConnection,readSequence:1,ageMs:0}
    }
    if(radioSelection||workSpot){
      const doc=navigation.documents.settings
      doc.settings.radios=[1,2].map(id=>({...structuredClone(doc.settings.radios[0]),id,name:`Test radio ${id}`,rigctldPort:4532+id}))
      doc.revision=createHash('sha256').update(JSON.stringify(doc.settings)).digest('hex')
      applicationData.get_settings.activeRadio=1
      applicationData.get_snapshot.radios=doc.settings.radios.map(r=>({id:r.id,name:r.name,band:'80m',dialMhz:3.573,sideband:'USB',isActive:r.id===1,catOk:true,smeterDb:-12,transmitting:false,bands:[]}))
    }
    applicationData.get_remote_satellite_state=navigation.live
    const stationModes=await stationModesFixture()
    applicationData.get_sstv_state=stationModes.sstv;applicationData.get_remote_aprs_state=stationModes.aprs
    collections.aprs=stationModes.roster
    // The oldest JS8 station (v11) sends JS8 as 1.18.0 and every release before it did; the rest send it as this
    // release does, with the desktop's heartbeat networking, waterfall controls, too-long notes, rows' places and marks.
    const js8Fixture=JSON.parse(await readFile(new URL(`../../ui/src/remote-web/__fixtures__/${applicationVersion===11?'js8-1.18.0':'js8'}.json`,import.meta.url),'utf8'))
    const js8Start=Date.now(), js8Delta=js8Start-js8Fixture.capturedAtMs
    for(const row of js8Fixture.state.activity)row.atMs+=js8Delta
    for(const row of js8Fixture.state.stations)for(const key of ['lastMs','ackMs','cqMs'])if(key in row)row[key]+=js8Delta
    for(const row of js8Fixture.state.inbox)row.atMs+=js8Delta
    js8Fixture.state.hbNextAtMs+=js8Delta;js8Fixture.state.cqNextAtMs+=js8Delta;js8Fixture.state.pendingReply.firesAtMs+=js8Delta
    applicationData.get_js8_state=js8Fixture
    collections.js8Context={rows:[],meta:{plan:[],history:{W1AW:{count:2302,lastUnix:1700000000,grid:'FN31',name:'PRIOR CONTACT',comment:'COMPLETE LOG'},
      K2ABC:{count:0,lastUnix:null,grid:'',name:'',comment:''}}}}
    const js8Queries=[]
    let js8Revision=100_000
    const fieldDay = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/field-day.json', import.meta.url), 'utf8'))
    const fieldDayQueries = []
    collections.fieldDay = { rows: [], meta: fieldDay }
    const dxpeditions = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/dxpeditions.json', import.meta.url), 'utf8'))
    dxpeditions.asOf = Math.floor(Date.now()/1000)
    const bank = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/memories.json', import.meta.url), 'utf8'))
    const ota = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/ota.json', import.meta.url), 'utf8'))
    // From v18 the board's spots may say where each activator is. This stand-in speaks the ladder only
    // to v14, so the states go on its board directly: what only a real browser can check is that rows
    // carrying them keep the board's geometry (a row's hidden screen-reader text once lengthened the
    // whole page). Which pages are SENT states is the relay's and station's rule, tested in
    // service.test.mjs and native.test.mjs. One trail spans sixteen states, the longest label a row shows.
    if (applicationVersion >= 18) {
      const trail = ['US-IL', 'US-MO', 'US-KS', 'US-NE', 'US-IA', 'US-SD', 'US-ND', 'US-MT', 'US-ID', 'US-OR', 'US-WA', 'US-KY', 'US-IN', 'US-OH', 'US-PA', 'US-WV']
      const placed = { 'US-0002': [['US-ND'], ['US-ND']], 'US-0003': [['US-MT', 'US-ND'], []], 'US-0004': [trail, ['US-ND', 'US-MT']], 'W7A/MN-001': [['US-AZ'], ['US-AZ']] }
      for (const spot of ota.feeds.flatMap(feed => feed.spots)) {
        const [states, neededStates] = placed[spot.reference] ?? [[], []]
        Object.assign(spot, { states, neededStates })
      }
    }
    const otaQueries = []
    collections.ota = { rows: [], meta: ota }
    const memoryQueries = []
    collections.memories = { rows: [], meta: { bank, sourceAgeMs: 250 } }
    const dxQueries = []
    collections.dxpeditions = { rows: [], meta: dxpeditions }
    const insights = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/insights.json', import.meta.url), 'utf8'))
    collections.awards = { rows: [], meta: { logCount: 2301, awards: insights.awards } }
    collections.statistics = { rows: [], meta: { logCount: 2301, statistics: insights.statistics, geography: insights.geography } }
    if (applicationVersion >= 4) applicationData.get_snapshot.stations = [{ call:'W1AW', grid:'FN31', snr:-8, lastHeardSlot:0,
      heardCount:2, presence:'heard', worked:true, workedBand:false, country:'United States', tier:'FT8', freqHz:1500 }]
    // This actor supplies synthetic native outcomes to the compiled UI. The
    // separate real-native/workerd test proves the ADIF append and durability.
    let loggingLeaseUntil=0, heartbeatReplyDelayMs=0, heartbeatReplyJitterMs=0
    let loggingAllowed=false,loggingLease=null,loggingRevision=1,loggingSequence=0,loseLogReply=false
    let qsoPrompt=true
    const qsoRecord={call:'W1AW',grid:'FN31',country:null,state:null,band:'20m',freqMhz:14.0755,mode:'FT8',rstSent:'-10',rstRcvd:'-12',name:null,qth:null,comment:null,notes:null,whenUnix:1700000000,confirmed:false,awardConfirmed:false}
    let stationControls=false,listenOffered=false,loseSpotReply=false,unknownResults=0,transmitAllowed=false,transmitEpoch='0000000000000001'
    const stopRequests=[],ftTxLog=[]
    if(workSpot){
      const original=collections.needs.rows[0]
      collections.needs.rows=[['N2DXCW','CW',14.02345],['N2DXPH','Phone',14.19876],['N3DXCW','CW',14.05543],['N4DXCW','CW',14.077],['N2DIG','FT8',14.074]].map(([call,mode,freqMhz])=>({...original,call,mode,freqMhz,band:'20m'}))
    }
    const stationRequests=[]
    let lastDxTier='FT8',lastMsgTier='TempoFast'
    if(operating)applicationData.get_snapshot.mode='qso'
    const loggingBoot=crypto.randomUUID(),loggingWindow=crypto.randomUUID(),loggingReceipts=new Map()
    const querySnapshots = new Map()
    let applicationRevision = 1, applicationAvailable = true
    const unavailableTopics = new Set()
    const unavailableCollections = new Set(), insightQueries = []
    const withheld = []
    let streamWatch = null
    const sentAt = new Map(), bases = new Map()
    const intervals = { get_snapshot:500, get_settings:1000, get_band_plan:1000, get_spectrum_row:100, get_meters:200, get_scope_snapshot:100, get_cw_state:200, get_rtty_state:200, get_psk_state:200, get_js8_state:500, get_sstv_state:1000, get_remote_aprs_state:1000, get_remote_satellite_state:1000 }
    applicationProducer=(async()=>{while(producing){const source=station;let request;try{request=await source.take(value=>['applicationRead','applicationWatch','applicationCredit','applicationQuery','operationRequest','operationDisconnect'].includes(value.type),1000)}catch{continue}
      if(source!==station||source.closed)continue
      if (!applicationAvailable || !producing) { withheld.push({type:request.type,collection:request.collection});continue }
      if(request.type==='operationDisconnect'){loggingLease=null;continue}
      if(request.type==='operationRequest'){
        assert.ok(operating);const r=request.request;let value,error
        if(loggingLease&&performance.now()>=loggingLeaseUntil)loggingLease=null
        assert.equal(request.deviceId,device.id)
        if(r.type==='acquire'){if(!loggingAllowed&&!stationControls)error='localPermissionRequired';else{loggingLease=crypto.randomUUID();loggingLeaseUntil=performance.now()+5000}}
        if(r.type==='heartbeat'&&loggingLease&&r.leaseId===loggingLease)loggingLeaseUntil=performance.now()+5000
        if(r.type==='release')loggingLease=null
        if(!loggingAllowed&&!stationControls)loggingLease=null
        if(r.type==='stopTransmit'){
          assert.equal(request.operationVersion,4)
          if(!(transmitAllowed&&loggingLease&&r.leaseId===loggingLease))console.log('FT Stop authority diagnostic',JSON.stringify({now:performance.now(),loggingLeaseUntil,loggingLease,requestedLease:r.leaseId,transmitAllowed,wire:operationWire.slice(-40),actions:stationRequests.map(r=>r.action)}))
          assert.ok(transmitAllowed&&loggingLease&&r.leaseId===loggingLease)
          assert.equal(r.transmitEpoch,transmitEpoch)
          stopRequests.push(r);transmitEpoch=(BigInt('0x'+transmitEpoch)+1n).toString(16).padStart(16,'0')
          applicationData.get_snapshot.radio.txEnabled=false;applicationRevision++
          value={stop:'accepted'}
        }else if(r.type==='logManual'){
          assert.ok(loggingAllowed&&loggingLease&&r.leaseId===loggingLease)
          assert.equal(r.expectedRevision,loggingRevision);assert.equal(r.clientSequence,loggingSequence+1);loggingSequence++
          loggedRequests.push(r);value={outcome:'applied',evidence:'fileSynced',uploads:'stationPipeline',operationId:r.requestId};loggingReceipts.set(r.requestId,value);loggingRevision++
          if(loseLogReply)continue
        }else if(r.type==='stationControl'){
          assert.equal(request.operationVersion,ftOperating?4:3);assert.ok(stationControls&&loggingLease&&r.leaseId===loggingLease)
          assert.equal(r.expectedRevision,loggingRevision);assert.equal(r.clientSequence,loggingSequence+1);loggingSequence++
          assert.deepEqual(r.context,controlContext())
          stationRequests.push(r);const a=r.action
          let qsoEvidence=null
          if(a.action==='ft.setting'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            const s=applicationData.get_snapshot, before={...s.remoteFtSettings}
            assert.deepEqual(a.expected,before)
            const c=a.change
            if(c.kind==='txOffset'||c.kind==='bothOffsets')s.radio.txOffsetHz=c.hz
            if(c.kind==='bothOffsets')s.radio.rxOffsetHz=c.hz
            if(c.kind==='hold')s.radio.holdTxFreq=c.on
            if(c.kind==='even'){s.radio.txEven=c.even;s.radio.txCycleAuto=false}
            if(c.kind==='auto')s.radio.txCycleAuto=c.auto
            s.remoteFtSettings={key:(BigInt('0x'+before.key)+1n).toString(16).padStart(32,'0'),
              ...Object.fromEntries(['txOffsetHz','rxOffsetHz','holdTxFreq','txEven','txCycleAuto'].map(k=>[k,s.radio[k]]))}
            s.remoteFtRuntime={settings:s.remoteFtSettings,skipTx1:s.remoteFtRuntime.skipTx1}
            qsoEvidence=c.kind==='auto'?'stationState':'settingsSaved'
          }else if(a.action==='ft.runtime'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            const s=applicationData.get_snapshot
            assert.equal(a.expectedTier,s.link.tier);assert.deepEqual(a.expected,s.remoteFtRuntime)
            assert.equal(s.radio.txEnabled,true,'browser runtime actions must exercise an owned FT session')
            if(a.change.kind==='rxOffset')s.radio.rxOffsetHz=a.change.hz
            else {assert.equal(a.change.kind,'skipTx1');s.remoteFtRuntime.skipTx1=a.change.on}
            s.remoteFtSettings={...s.remoteFtSettings,rxOffsetHz:s.radio.rxOffsetHz,
              key:(BigInt('0x'+s.remoteFtSettings.key)+1n).toString(16).padStart(32,'0')}
            s.remoteFtRuntime={settings:s.remoteFtSettings,skipTx1:s.remoteFtRuntime.skipTx1}
            qsoEvidence=a.change.kind==='rxOffset'?'settingsSaved':'stationState'
          }else if(a.action==='qso.logCurrent'){
            assert.ok(loggingAllowed)
            assert.equal(a.expectedKey,applicationData.get_snapshot.currentQsoLogKey)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            const q=applicationData.get_snapshot.qso
            assert.deepEqual(a.expectedQso,{dxcall:q.dxcall,state:q.state,txNow:q.txNow,cqRunning:q.cqRunning})
            if(qsoPrompt){applicationData.get_snapshot.pendingLog={...qsoRecord};applicationData.get_snapshot.pendingQsoLogKey='0000000000000002'}
            qsoEvidence=qsoPrompt?'pendingConfirmationSynced':'fileSynced'
          }else if(a.action==='qso.confirm'||a.action==='qso.discard'){
            assert.ok(loggingAllowed)
            assert.equal(a.expectedKey,applicationData.get_snapshot.pendingQsoLogKey)
            if(a.action==='qso.confirm')assert.deepEqual(a.edits,{call:'W1AW',grid:'FN32',rstSent:'-10',rstRcvd:'-12'})
            applicationData.get_snapshot.pendingLog=null;applicationData.get_snapshot.pendingQsoLogKey=null
            qsoEvidence=a.action==='qso.confirm'?'fileSynced':'pendingDiscarded'
          }else if(a.action==='ft.message'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            const qso=applicationData.get_snapshot.qso
            assert.deepEqual(a.expectedQso,{dxcall:qso.dxcall,state:qso.state,txNow:qso.txNow,cqRunning:qso.cqRunning})
            assert.equal(a.call,'K2ABC');assert.equal(a.grid,'FN42')
            Object.assign(qso,{dxcall:a.call,dxgrid:a.grid,state:'awaitReport',txNow:a.text,cqRunning:false,running:true})
          }else if(a.action==='ft.exchange'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            const qso=applicationData.get_snapshot.qso
            assert.deepEqual(a.expectedQso,{dxcall:qso.dxcall,state:qso.state,txNow:qso.txNow,cqRunning:qso.cqRunning})
            if(a.change.kind==='resend')qso.txCount=0
            else if(a.change.kind==='freeText')qso.txNow=`${qso.dxcall} N0CALL ${a.change.text}`
            else if(a.change.kind==='monitor')Object.assign(qso,{state:'idle',dxcall:null,dxgrid:null,running:false,cqRunning:false,txNow:null})
            else assert.fail('unknown FT exchange change')
          }else if(a.action==='ft.call'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            assert.deepEqual(a.selection,{call:'W1AW',grid:'FN31',message:null,snr:null,freq:1500})
            applicationData.get_snapshot.radio.txEnabled=true
            applicationData.get_snapshot.qso={state:'awaitReport',dxcall:'W1AW',dxgrid:'FN31',rxReport:null,running:true,cqRunning:false,txNow:'W1AW N0CALL AA00'}
          }else if(a.action==='ft.cq'||a.action==='ft.txEnabled'){
            assert.ok(ftOperating&&transmitAllowed);assert.equal(a.transmitEpoch,transmitEpoch)
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            applicationData.get_snapshot.radio.txEnabled=a.action==='ft.cq'||a.on;ftTxLog.push({t:Date.now(),action:a.action,on:a.on,txEnabled:applicationData.get_snapshot.radio.txEnabled})
          }else if(a.action==='radio.select'){
            adoptBrowserRadio(a.radioId)
          }else if(a.action==='decoder.arm'){
            if(a.receiver==='sstv'){applicationData.get_sstv_state.state.armed=a.on;applicationData.get_sstv_state.state.health.armed=a.on}
            else if(a.receiver==='aprs')applicationData.get_remote_aprs_state.health.arm=a.on?'auto':'off'
            else applicationData[`get_${a.receiver}_state`].armed=a.on
          }else if(a.action==='radio.workSpot'){
            if(workSpot)adoptBrowserRadio(applicationData.get_snapshot.activeRadioId===1?2:1)
            assert.ok(workSpot&&['cw','phone'].includes(a.mode));assert.equal(a.band,'20m');assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
            Object.assign(applicationData.get_snapshot.radio,{operatingMode:a.mode,dialMhz:a.dialMhz,band:a.band,sideband:'USB',sidebandOverride:null,rigMode:a.mode==='cw'?'CW':'USB',rigKeyed:false})
            Object.assign(applicationData.get_snapshot,{workTick:(applicationData.get_snapshot.workTick??0)+1,workView:a.mode,workCall:a.call.toUpperCase()})
          }else if(a.action==='radio.frequency'){
            assert.ok(a.dialMhz>0);assert.equal(a.band,a.dialMhz===10?'':'40m');assert.ok(['USB','LSB'].includes(a.sideband))
            if(radioSelection)adoptBrowserRadio(a.dialMhz<10?1:2)
            applicationData.get_snapshot.radio.dialMhz=a.dialMhz;applicationData.get_snapshot.radio.band=a.band;applicationData.get_snapshot.radio.sideband=a.sideband
          }else if(a.action==='radio.phoneMode'){
            const radio=applicationData.get_snapshot.radio
            assert.equal(radio.operatingMode,'phone');assert.equal(a.expectedMode,radio.sidebandOverride??'auto')
            assert.ok(['auto','USB','LSB','FM','AM'].includes(a.mode));assert.equal(radio.txEnabled,false);assert.equal(radio.rigKeyed,false)
            radio.sidebandOverride=a.mode==='auto'?null:a.mode
            radio.rigMode=a.mode==='auto'?(radio.dialMhz<10?'LSB':'USB'):a.mode
          }else if(a.action==='radio.level'){
            const radio=applicationData.get_snapshot.radio,fields={power:'rfPower',micGain:'micGain',nr:'nrLevel',compression:'compLevel',notch:'notchFreqHz'}
            assert.ok(Object.hasOwn(fields,a.level));assert.equal(a.mode,radio.operatingMode)
            assert.equal(a.expected,radio[fields[a.level]]);assert.equal(radio.txEnabled,false);assert.equal(radio.rigKeyed,false)
            assert.ok(Number.isFinite(a.value)&&(a.level==='notch'?a.value>=300&&a.value<=3400:a.value>=0&&a.value<=1))
            radio[fields[a.level]]=a.value
          }else if(a.action==='radio.function'||a.action==='radio.agc'){
            const radio=applicationData.get_snapshot.radio
            assert.ok(['cw','phone'].includes(a.mode));assert.equal(a.mode,radio.operatingMode)
            assert.equal(radio.txEnabled,false);assert.equal(radio.rigKeyed,false)
            if(a.action==='radio.function'){
              assert.ok(['nb','nr','notch','manualNotch'].includes(a.func));assert.equal(a.expectedOn,radio[a.func]);assert.equal(typeof a.on,'boolean')
              radio[a.func]=a.on
            }else{
              assert.ok(['auto','fast','mid','slow','off'].includes(a.speed));assert.equal(a.expectedSpeed,radio.agc)
              radio.agc=a.speed
            }
          }else if(a.action==='radio.filterWidth'){
            const radio=applicationData.get_snapshot.radio
            assert.equal(a.mode,radio.operatingMode);assert.equal(a.expectedHz,radio.filterWidthHz)
            assert.ok(a.mode==='cw'?a.hz>=50&&a.hz<=2000:a.hz>=300&&a.hz<=4000)
            assert.equal(radio.txEnabled,false);assert.equal(radio.rigKeyed,false)
            radio.filterWidthHz=a.hz
          }else if(a.action==='radio.band'){
            assert.ok(['cw','phone'].includes(a.mode));assert.ok(['40m','20m'].includes(a.band))
            assert.equal(applicationData.get_snapshot.radio.operatingMode,a.mode)
            // The station's remembered destination intentionally differs from
            // the dropdown's published default. The browser only names a band.
            applicationData.get_snapshot.radio.dialMhz=a.mode==='cw'?(a.band==='40m'?7.055:14.055):(a.band==='40m'?7.255:14.275)
            applicationData.get_snapshot.radio.band=a.band
          }else if(a.action==='radio.mode'){
            const dial={digital:7.074,cw:7.030,phone:7.150,rtty:7.080,keyboard:7.070}[a.mode]
            assert.ok(dial);assert.equal(a.followFrequency,true)
            if(radioSelection)adoptBrowserRadio(a.mode==='phone'?2:1)
            applicationData.get_snapshot.radio.operatingMode=a.mode;applicationData.get_snapshot.radio.dialMhz=dial
            if(a.mode==='cw'||a.mode==='phone')applicationData.get_snapshot.radio.filterWidthHz=a.mode==='cw'?500:2400
            assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
          }else if(a.action==='radio.workspace'){
            const snap=applicationData.get_snapshot,chat=['TempoFast','TempoDeep'].includes(snap.link.tier)
            if(a.workspace==='tempo'){
              if(!chat){lastDxTier=snap.link.tier;snap.link.tier=lastMsgTier}
              snap.mode='chat'
            }else if(a.workspace==='ft'){
              if(chat){lastMsgTier=snap.link.tier;snap.link.tier=lastDxTier}
              if(snap.link.tier==='JS8')snap.link.tier='FT8'
              snap.mode='qso'
            }else if(a.workspace==='js8')snap.link.tier='JS8'
            else assert.fail(`Unreviewed workspace ${a.workspace}`)
            snap.radio.operatingMode='digital';snap.radio.dialMhz=7.074
            if(routedWorkspace){adoptBrowserRadio(a.workspace==='js8'?1:2);snap.radio.dialMhz=a.workspace==='tempo'?7.0355:a.workspace==='js8'?7.078:7.074;snap.radio.band='40m';snap.radio.sideband='USB'}
            assert.equal(snap.radio.txEnabled,false)
          }else if(a.action==='radio.tier'){
            assert.ok(['FT8','FT4','FT2','WSPR','Q65','MSK144','JT65','FST4','FST4W','TempoFast','TempoDeep'].includes(a.tier))
            assert.equal(applicationData.get_snapshot.radio.operatingMode,'digital')
            // The provider proves the real UI/relay gesture and later snapshot.
            // Native engine/worker tests separately prove channel/source policy.
            applicationData.get_snapshot.link.tier=a.tier
            if(routedTier){
              const radio=applicationData.get_snapshot.radio
              if(a.tier==='MSK144'){adoptBrowserRadio(2);Object.assign(radio,{dialMhz:50.260,band:'6m',sideband:'USB',rxOffsetHz:1500,txOffsetHz:1500})}
              else if(a.tier==='FST4'){adoptBrowserRadio(1);Object.assign(radio,{dialMhz:0.136,band:'2200m',sideband:'USB'})}
              else assert.equal(a.tier,'FT8') // no FT8 channel on 2200m: keep the dial/radio
            }
            if(a.tier==='MSK144')applicationData.get_snapshot.link.periodSecs=15
            assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
          }else if(a.action==='decoder.js8Speed'){
            const speeds=['slow','normal','fast','turbo','ultra'];assert.equal(applicationData.get_snapshot.link.tier,'JS8')
            assert.equal(a.expectedSpeed,speeds.indexOf(js8Fixture.state.speed));assert.notEqual(a.expectedSpeed,a.speed)
            js8Fixture.state.speed=speeds[a.speed];applicationData.get_snapshot.link.periodSecs=[30,15,10,6,4][a.speed]
          }else if(a.action==='decoder.msk144Period'){
            assert.equal(applicationData.get_snapshot.link.tier,'MSK144')
            assert.equal(a.expectedPeriodSecs,applicationData.get_snapshot.link.periodSecs);assert.notEqual(a.expectedPeriodSecs,a.periodSecs)
            assert.ok([5,10,15,30].includes(a.periodSecs));applicationData.get_snapshot.link.periodSecs=a.periodSecs
          }else if(a.action==='decoder.depth'){
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            assert.equal(a.expectedDepth,applicationData.get_snapshot.radio.decodeDepth)
            assert.ok([1,2,3].includes(a.depth));assert.notEqual(a.expectedDepth,a.depth)
            applicationData.get_snapshot.radio.decodeDepth=a.depth
          }else if(a.action==='receiver.rxOffset'){
            assert.equal(a.expectedTier,applicationData.get_snapshot.link.tier)
            assert.equal(a.expectedHz,applicationData.get_snapshot.radio.rxOffsetHz)
            assert.ok(a.hz>=200&&a.hz<=4000);assert.notEqual(a.expectedHz,a.hz)
            applicationData.get_snapshot.radio.rxOffsetHz=a.hz
            assert.equal(applicationData.get_snapshot.radio.txOffsetHz,1500)
          }else if(a.action==='receiver.rxGain'){
            const doc=navigation.documents.settings,tx=doc.settings.txLevel
            assert.equal(a.radioId,1);assert.equal(a.expectedSettingsRevision,doc.revision)
            assert.equal(a.expectedGain,doc.settings.rxGain);assert.notEqual(a.gain,a.expectedGain)
            assert.ok(a.gain>=1&&a.gain<=8)
            doc.settings.rxGain=a.gain;doc.settings.radios[0].rxGain=a.gain
            doc.revision=createHash('sha256').update(JSON.stringify(doc.settings)).digest('hex')
            assert.equal(doc.settings.txLevel,tx)
          }else if(a.action==='decoder.clear'){
            const state=applicationData[`get_${a.receiver}_state`];state.text='';if(state.charConf)state.charConf=[]
          }else if(a.action==='decoder.afcReset')applicationData[`get_${a.receiver}_state`].afcHz=0
          else if(a.action==='decoder.pskMode'){applicationData.get_psk_state.mode=a.mode.toLowerCase();applicationData.get_psk_state.reverse=a.reverse}
          else if(a.action==='amplifier.operate'){
            assert.equal(a.expectedOperate,applicationData.get_snapshot.radio.amp.operate);assert.notEqual(a.expectedOperate,a.operate)
            applicationData.get_snapshot.radio.amp.operate=a.operate
            fixture.station.amplifier.operate=a.operate
          }else if(a.action==='amplifier.band'){
            assert.equal(a.expectedBand,applicationData.get_snapshot.radio.amp.bandLabel)
            const ladder=['160m','80m','60m','40m','30m','20m','17m','15m','12m','10m','6m','4m'];applicationData.get_snapshot.radio.amp.bandLabel=ladder[ladder.indexOf(a.expectedBand)+a.direction]
            fixture.station.amplifier.bandLabel=applicationData.get_snapshot.radio.amp.bandLabel
          }else if(a.action==='amplifier.followBand'){
            const doc=navigation.documents.settings
            assert.equal(a.radioId,1);assert.equal(a.expectedSettingsRevision,doc.revision)
            assert.equal(a.expectedFollow,doc.settings.ampFollowBand);assert.notEqual(a.follow,a.expectedFollow)
            doc.settings.ampFollowBand=a.follow;doc.settings.radios[0].ampFollowBand=a.follow
            doc.revision=createHash('sha256').update(JSON.stringify(doc.settings)).digest('hex')
            fixture.station.amplifier.followBand=a.follow
          }else assert.fail(`Unreviewed station action ${a.action}`)
          value={operation:'stationControl',operationId:r.requestId,outcome:'applied',evidence:qsoEvidence??(a.action.startsWith('ft.')?'stationState':['amplifier.followBand','decoder.js8Speed','decoder.msk144Period','decoder.depth','receiver.rxOffset','receiver.rxGain'].includes(a.action)?'settingsSaved':a.action.startsWith('radio.')?'radioReadback':a.action.startsWith('amplifier.')?'amplifierReadback':'receiverState')}
          loggingReceipts.set(r.requestId,value);loggingRevision++;applicationRevision++
          if(loseSpotReply&&a.action==='radio.workSpot')continue
        }else if(r.type==='result'){value=loggingReceipts.get(r.operationId);if(!value)error='resultExpired';else if(loseSpotReply&&value.operation==='stationControl'){value={operation:'stationControl',operationId:r.operationId,outcome:'unknown',reason:'hardwareUnconfirmed'};unknownResults++}}
        else value={stationBootId:loggingBoot,allowed:loggingAllowed||stationControls,phase:loggingLease?'controlling':loggingAllowed||stationControls?'available':'localPermissionRequired',leaseId:loggingLease,revision:loggingRevision,commandWindowId:loggingLease?loggingWindow:null,nextSequence:loggingLease?loggingSequence+1:null,leaseRemainingMs:loggingLease?Math.max(0,Math.floor(loggingLeaseUntil-performance.now())):null,actions:loggingAllowed?['log.manual']:[],txArmed:!!ftOperating&&!!loggingLease&&transmitAllowed&&applicationData.get_snapshot.radio.txEnabled,...(ftOperating||stream?{transmitEpoch:transmitAllowed?transmitEpoch:null}:{}),...(stationControls?{controls:{context:controlContext(),capabilities:request.operationVersion>=3?['decoder','amplifier','frequency','mode','tier','ampFollowBand','workspace','decoderSettings','receiverSettings','receiverGain','bandSelection','receiverFilter','receiverDsp','phoneMode','fmTuning','fmReceiver','radioLevels',...(transmitAllowed?['ftOperate','ftCall','ftExchange','ftMessages','ftSettings','ftRuntime']:[]),...(ftOperating&&loggingAllowed?['qsoLogging']:[]),...(workSpot?['workSpot']:[]),...(radioSelection?['radioSelection']:[]),...(listenOffered?['audioListen']:[])]:['decoder','amplifier']}}:{})}
        // A slow link answers heartbeats late; the station's own reply content is unchanged.
        if(r.type==='heartbeat'&&(heartbeatReplyDelayMs||heartbeatReplyJitterMs))await sleep(heartbeatReplyDelayMs+Math.random()*heartbeatReplyJitterMs)
        source.send({type:'operationResponse',sessionId:request.sessionId,requestId:r.requestId,...(error?{error}:{value})});continue
      }
      if (request.type === 'applicationQuery') {
        assert.ok(applicationVersion >= 3)
        if(['settings','programming'].includes(request.collection))assert.ok(applicationVersion>=14)
        if(['connect','path','satellites','satellite'].includes(request.collection)){assert.ok(applicationVersion>=13);navigationQueries.push({collection:request.collection,search:request.search})}
        if (['sstvImage','aprs'].includes(request.collection)) assert.ok(applicationVersion>=12)
        if (request.collection === 'js8Context') { assert.ok(applicationVersion >= 11); js8Queries.push(request.collection) }
        if (request.collection === 'fieldDay') { assert.ok(applicationVersion >= 10); fieldDayQueries.push(request.collection) }
        if (request.collection === 'ota') { assert.ok(applicationVersion >= 9); otaQueries.push(request.collection) }
        if (request.collection === 'memories') { assert.ok(applicationVersion >= 8); memoryQueries.push(request.collection) }
        if (request.collection === 'dxpeditions') { assert.ok(applicationVersion >= 7); dxQueries.push(request.collection) }
        if (request.collection === 'recall') assert.ok(applicationVersion >= 4)
        if (['awards', 'statistics'].includes(request.collection)) {
          assert.ok(applicationVersion >= 6, 'older stations must never receive summary queries')
          insightQueries.push(request.collection)
        }
        if (unavailableCollections.has(request.collection)) {
          source.send({ type: 'applicationQueryError', requestId: request.requestId, error: 'applicationUnavailable' })
          continue
        }
        const [givenId, offsetText] = (request.cursor??'').split(':')
        const offset = Number(offsetText??0), snapshotId=givenId||crypto.randomUUID()
        if (!givenId) {
          const collection = request.collection === 'sstvImage' ? stationModes.images.get(request.search) : request.collection === 'recall' ? recallFixture(request.search) : ['connect','path','satellites','satellite','settings','programming'].includes(request.collection)?navigation.collection(request.collection,request.search):collections[request.collection]
          let rows=collection.rows
          if(request.collection==='log') rows=rows.filter(q=>(!request.unconfirmed||!q.awardConfirmed)&&(!request.search||q.call.toLowerCase().includes(request.search.toLowerCase())))
          if(request.collection==='decodes'&&request.after!==null) rows=rows.filter(q=>q.sequence>request.after)
          querySnapshots.set(snapshotId,{...collection, rows})
          if(querySnapshots.size>16) querySnapshots.delete(querySnapshots.keys().next().value)
        }
        const capture=querySnapshots.get(snapshotId)
        assert.ok(capture,'browser must keep a valid sealed cursor')
        const rows=capture.rows.slice(offset,offset+(request.collection==='sstvImage'?3:['connect','path','satellites','satellite','settings','programming'].includes(request.collection)?7:128)), end=offset+rows.length
        source.send({type:'applicationPage',requestId:request.requestId,collection:request.collection,snapshotId,offset,
          total:capture.total??capture.rows.length,retained:capture.rows.length,nextCursor:end<capture.rows.length?`${snapshotId}:${end}`:null,
          ageMs:0,rows,meta:{capturedAgeMs:0,source:capture.meta}})
        continue
      }
      if (request.type !== 'applicationRead') {
        assert.ok(applicationVersion >= 2, 'legacy native contract must never receive stream messages')
        if (request.type === 'applicationWatch') { streamWatch=request; sentAt.clear(); bases.clear() }
        else { if (request.watchId !== streamWatch?.watchId) continue; await sleep(100) }
        if (!streamWatch?.topics.length || !producing) continue
        const updates=[]
        while (!updates.length && producing) {
          const now=performance.now()
          for (const command of streamWatch.topics) {
          assert.ok(Object.hasOwn(applicationData, command), 'only reviewed topics may reach the station')
          if (now-(sentAt.get(command)??-Infinity)<intervals[command]) continue
          if (unavailableTopics.has(command)) {
            updates.push({type:'applicationError',requestId:request.requestId,command,error:'applicationUnavailable'})
            sentAt.set(command,now);bases.delete(command);continue
          }
          if(command==='get_js8_state') { assert.ok(applicationVersion>=11); js8Fixture.capturedAtMs=Date.now() }
          if(['get_sstv_state','get_remote_aprs_state'].includes(command)){assert.ok(applicationVersion>=12);applicationData[command].capturedAtMs=Date.now()}
          if(command==='get_remote_satellite_state'){assert.ok(applicationVersion>=13);applicationData[command].capturedAtMs=Date.now()}
          const revision=['get_js8_state','get_sstv_state','get_remote_aprs_state','get_remote_satellite_state'].includes(command)?++js8Revision:applicationRevision
          const data=applicationData[command], delta=bases.get(command)===revision && !Array.isArray(data)
          updates.push({type:'applicationResult', requestId:request.requestId, command, revision,
            baseRevision:delta?revision:null, ageMs:0, data:delta?{}:data, removed:[]})
          sentAt.set(command,now);bases.set(command,revision)
          applicationTraffic.byCommand[command]=(applicationTraffic.byCommand[command]??0)+1
          }
          // Retain the one credit until a topic is due, as the native tick does.
          if (!updates.length) await sleep(100)
        }
        if (!producing) break
        if(source!==station||source.closed)continue
        const response={type:'applicationBatch',watchId:streamWatch.watchId,requestId:request.requestId,updates}
        const bytes=Buffer.byteLength(JSON.stringify(response));applicationTraffic.bytes+=bytes;applicationTraffic.batches++
        applicationTraffic.maxResponseBytes=Math.max(applicationTraffic.maxResponseBytes,bytes)
        source.send(response);continue
      }
      assert.ok(Object.hasOwn(applicationData, request.command), 'only the closed read vocabulary may reach the station')
      applicationTraffic.byCommand[request.command] = (applicationTraffic.byCommand[request.command] ?? 0) + 1
      const data=applicationData[request.command], delta=request.revision===applicationRevision && !Array.isArray(data)
      const response={type:'applicationResult',requestId:request.requestId,command:request.command,revision:applicationRevision,
        baseRevision:delta?applicationRevision:null,ageMs:0,data:delta?{}:data,removed:[]}
      const bytes=Buffer.byteLength(JSON.stringify(response));applicationTraffic.bytes+=bytes;applicationTraffic.maxResponseBytes=Math.max(applicationTraffic.maxResponseBytes,bytes)
      source.send(response)
    }})()
    // THE STAND-IN RECONNECTS AS NEXUS DOES (`supervise`, src-tauri/src/remote_service/transport.rs). A runner stall
    // of about 2 s makes the stand-in answer its credit late, and the relay then closes its socket (1008
    // `applicationTimeout`, or `invalidApplicationResult` for a batch that expired on the way). Nexus connects again,
    // and a stand-in that did not would leave every later wait for station data to time out. So after a close it did
    // not make itself, the stand-in waits what Nexus waits, 2^attempts s plus up to 1 s of jitter (attempts 1 to 6,
    // from 0 again after a connection that lasted over 60 s), and connects again; a 401 or 403 ends it, as Nexus then
    // turns Remote off. While the scenario holds station data back on purpose (`applicationAvailable=false`) it waits
    // for that to end: the scenario reconnects itself where it recovers. Every connection goes through
    // `connectStation`, one at a time, so the scenario's and the stand-in's never cross.
    // ONE EXCEPTION: a close only a protocol fault produces fails the scenario at once and names the reason. The room
    // refuses a message it cannot read (`invalidMessage`, remote/src/room.ts), and the relays a station message they
    // cannot take (`invalidObservation`, `invalidOperation`, `invalidStream`, `invalidAudio`); late answers never
    // produce these, so a slow runner cannot, and reconnecting would only ride out a broken message. A reason a late
    // answer also produces (`invalidPublication`, `invalidApplicationResult`, `invalidApplicationPage`) stays a drop.
    const protocolFaults=['invalidMessage','invalidObservation','invalidOperation','invalidStream','invalidAudio']
    const ownCloses=new WeakSet()
    let connecting=Promise.resolve()
    const connectStation=(dropped=null)=>{const turn=connecting.then(async()=>{
      if(dropped){if(!producing||station!==dropped)return}else{ownCloses.add(station);station.close()}
      const next=await pair.native.open(pair.stationId,undefined,101,stationHeaders)
      if(producing)station=next;else next.close()
    });connecting=turn.catch(()=>{});return turn}
    stationSupervisor=(async()=>{let attempts=0
      while(producing){
        const source=station,started=performance.now()
        while(producing&&station===source&&(!source.closed||ownCloses.has(source)))await sleep(100)
        if(!producing||station!==source)continue
        if(protocolFaults.includes(source.closeReason))assert.fail(`The service closed the station stand-in's socket for a protocol fault: ${source.closeCode} ${source.closeReason}`)
        if(performance.now()-started>60000)attempts=0
        attempts=Math.min(attempts+1,6)
        const wait=(1000<<attempts)+Math.floor(Math.random()*1000),due=performance.now()+wait
        // Said when the close is seen, so a scenario that fails during the wait still shows it. A close while the
        // scenario holds station data back on purpose is its own doing: said only if the stand-in reconnects after it.
        const reconnecting=`Station stand-in: the service closed its socket (${source.closeCode} ${source.closeReason}); connecting again after ${wait} ms, as Nexus does`,said=applicationAvailable
        if(said)console.log(reconnecting)
        while(producing&&station===source&&(performance.now()<due||!applicationAvailable))await sleep(100)
        if(!producing||station!==source)continue
        if(!said)console.log(reconnecting)
        try{await connectStation(source)}
        catch(error){if(error instanceof assert.AssertionError&&[401,403].includes(error.actual)){console.log(`Station stand-in: refused (${error.actual}), so it stays off, as Nexus does`);return}}
      }
    })()
    if(stream){
      // REMOTE AS A STREAM, end to end in real Chrome, under the Worker's real CSP and through the real
      // relay lane. The station's WebRTC end is a second Chrome window standing in for str0m: it answers
      // the page's offer with a canvas picture and records what reaches each data channel. Nothing of
      // the page is faked. The wire contract is fixtures/stream/; this proves the page speaks it.
      // A station built with its audio lane advertises Listen, as the operator's does.
      stationControls=true;transmitAllowed=true;listenOffered=true
      let shackLive=true
      const shackWindow=(await browser.call('Target.createTarget',{url:'about:blank',newWindow:true})).targetId
      const shack=(await browser.call('Target.attachToTarget',{targetId:shackWindow,flatten:true})).sessionId
      await browser.call('Runtime.enable',{},shack)
      const atShack=async expression=>{const v=await browser.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},shack);if(v.exceptionDetails)throw new Error('Station evaluation failed: '+String(v.exceptionDetails.exception?.description??v.exceptionDetails.text).split('\n')[0]);return v.result?.value}
      const untilShack=async(expression,timeout=12000)=>{for(let i=0;i<Math.ceil(timeout/100);i++){if(await atShack(expression))return;await sleep(100)}console.log('Station diagnostic',JSON.stringify(await atShack('({received:__shack.received,state:window.__pc?.connectionState})')));throw new Error('Expected station state did not appear: '+expression)}
      // The shack's window: 16:9, and on the phone the 21:9 of an ultrawide (3440 x 1440 scaled), the shape the phone layout
      // was measured against (2026-10-03).
      await atShack(`window.__shack={received:{control:[],ptt:[],audio:[]},candidates:[]};(()=>{const c=document.createElement('canvas');c.width=${phone?860:640};c.height=360;document.body.appendChild(c);const x=c.getContext('2d');let n=0;setInterval(()=>{x.fillStyle='hsl('+(n*7%360)+' 55% 35%)';x.fillRect(0,0,c.width,c.height);x.fillStyle='#fff';x.font='40px sans-serif';x.fillText('Nexus at the shack '+(n++),24,190)},50);window.__picture=c.captureStream(20)})();true`)
      // The station's receive audio, as the contract has it on `audio`: when the channel opens it says
      // "audio started", then sends `audioRx` bundles of five 20 ms Opus frames every 100 ms for the
      // whole stream. The Opus is a tone from this Chrome's own encoder, run on a page of its own
      // because WebCodecs needs a secure context and the station's window is about:blank.
      // "Audio started" is said until it is SENT. Chrome, standing in for the station here, can drop the
      // first message a channel the page opened sends: the send is taken, nothing goes out, the channel's
      // own messagesSent does not count it, and the page never hears it. On one core, Chrome 154 sent no
      // word in 6 of 1000 streams that said it in ondatachannel, and 4 of 600 that said it on the open
      // event; the bundles after it went. That is how CI's shard 0 read "heard null" on 2026-10-03, and
      // timed out waiting for the word on 2026-10-05. The station (str0m) is not Chrome. So the stand-in
      // reads its own stats first, says the word until they count it (at most three times), and only then
      // sends audio, as the station does: 0 of 1000 went unsaid, and a word dropped on purpose is said again.
      const encoderWindow=(await browser.call('Target.createTarget',{url:app.origin+'/remote-licenses.txt',background:true})).targetId
      const encoder=(await browser.call('Target.attachToTarget',{targetId:encoderWindow,flatten:true})).sessionId
      let packets
      for(let i=0;i<50&&!packets;i++){const v=await browser.call('Runtime.evaluate',{expression:`isSecureContext&&document.readyState==='complete'&&typeof AudioEncoder==='function'&&(async()=>{const out=[];const enc=new AudioEncoder({output:c=>{const b=new Uint8Array(c.byteLength);c.copyTo(b);out.push(btoa(String.fromCharCode(...b)))},error:()=>{}});enc.configure({codec:'opus',sampleRate:48000,numberOfChannels:1,bitrate:24000});for(let f=0;f<50;f++){const d=new Float32Array(960);for(let i=0;i<960;i++)d[i]=0.3*Math.sin(2*Math.PI*440*(f*960+i)/48000);enc.encode(new AudioData({format:'f32-planar',sampleRate:48000,numberOfFrames:960,numberOfChannels:1,timestamp:f*20000,data:d}))}await enc.flush();enc.close();return out})()`,returnByValue:true,awaitPromise:true},encoder).catch(()=>null);if(Array.isArray(v?.result?.value)&&v.result.value.length)packets=v.result.value;else await sleep(100)}
      await browser.call('Target.closeTarget',{targetId:encoderWindow})
      assert.ok(packets?.length>=40,'the station stand-in encoded its tone')
      await atShack(`__shack.largestBundle=0;__shack.feeds=0;__shack.feed=async(ch,pc)=>{__shack.feeds++;__shack.said=0;const word=JSON.stringify({type:'audioState',listening:true}),sent=async()=>{for(const r of (await pc.getStats()).values())if(r.type==='data-channel'&&r.label==='audio')return r.messagesSent;return 0};try{while(__shack.said<3&&await sent()===0){ch.send(word);__shack.said++;for(let i=0;i<10&&await sent()===0;i++)await new Promise(r=>setTimeout(r,25))}}catch{};const packets=${JSON.stringify(packets)};let frame=0;const id=setInterval(()=>{if(ch.readyState!=='open'){if(ch.readyState==='closed')clearInterval(id);return}const bytes=[];for(let i=0;i<5;i++){const p=atob(packets[(frame+i)%packets.length]);bytes.push(p.length>>8,p.length&255,...[...p].map(c=>c.charCodeAt(0)))}const m=JSON.stringify({type:'audioRx',seq:frame,epoch:'00000000000000a1',firstFrameMs:frame*20,frameMs:20,count:5,payload:btoa(String.fromCharCode(...bytes))});__shack.largestBundle=Math.max(__shack.largestBundle,m.length);ch.send(m);frame+=5},100)};true`)
      let streamSession=null,realAnswer=null
      const offers=[],closes=[]
      const signalling=(async()=>{while(producing&&shackLive){const source=station;let signal;try{signal=await source.take(value=>value.type==='streamSignal',500)}catch{continue}
        try{
          streamSession=signal.sessionId
          assert.equal(signal.deviceId,device.id,'the relay stamps the approved device')
          if(signal.payload.kind==='offer'){
            offers.push(signal)
            // The station's own answer, recorded: no station is behind it, so it is all this offer gets.
            // Each signed for this offer and session with the station's key (S3-M1), as the shack signs it.
            const signed=answer=>signer.sign(answer,signal.payload.sdp,pair.stationId,signal.deviceId,signal.sessionId)
            if(realAnswer){source.send({type:'streamSignal',sessionId:signal.sessionId,payload:{kind:'answer',sdp:await signed(realAnswer)}});continue}
            const sdp=await atShack(`(async()=>{const pc=new RTCPeerConnection({iceServers:[]});window.__pc=pc;pc.onicecandidate=e=>{if(e.candidate&&e.candidate.candidate)__shack.candidates.push({candidate:e.candidate.candidate,sdpMid:e.candidate.sdpMid})};pc.ondatachannel=e=>{const ch=e.channel;__shack[ch.label]=ch;ch.onmessage=m=>__shack.received[ch.label]?.push(JSON.parse(m.data));if(ch.label==='audio'){if(ch.readyState==='open')__shack.feed(ch,pc);else ch.onopen=()=>__shack.feed(ch,pc)}};await pc.setRemoteDescription({type:'offer',sdp:${JSON.stringify(signal.payload.sdp)}});const t=pc.getTransceivers()[0];t.direction='sendonly';await t.sender.replaceTrack(__picture.getVideoTracks()[0]);await pc.setLocalDescription(await pc.createAnswer());return pc.localDescription.sdp})()`)
            source.send({type:'streamSignal',sessionId:signal.sessionId,payload:{kind:'answer',sdp:await signed(sdp)}})
            source.send({type:'streamState',sessionId:signal.sessionId,streaming:true})
          }else if(signal.payload.kind==='candidate')await atShack(`__pc?.addIceCandidate(${JSON.stringify({candidate:signal.payload.candidate,sdpMid:signal.payload.sdpMid})}).then(()=>true,()=>false)`)
          else if(signal.payload.kind==='close'){closes.push(signal);await atShack('(window.__pc?.close(),true)')}
        }catch(error){if(producing&&shackLive)throw error}
      }})()
      // The station's own candidates, trickled back as they appear - one per message, as the contract has it.
      const trickle=(async()=>{let sent=0;while(producing&&shackLive){await sleep(100);if(!streamSession)continue;const all=await atShack('__shack.candidates').catch(()=>[]);for(;sent<all.length;sent++)station.send({type:'streamSignal',sessionId:streamSession,payload:{kind:'candidate',...all[sent]}})}})()
      const center=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`)
      const press=async(point,hold=0)=>{await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1},session);if(hold)await sleep(hold);await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1},session);await sleep(50)}
      const typeKey=async(key,code,text)=>{await browser.call('Input.dispatchKeyEvent',{type:text?'keyDown':'rawKeyDown',key,code,text,windowsVirtualKeyCode:key==='Tab'?9:key.toUpperCase().charCodeAt(0)},session);await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:key==='Tab'?9:key.toUpperCase().charCodeAt(0)},session);await sleep(50)}
      try{
        await geometry(1280,800)
        // The policy's refusals as Chrome reports them, beside the page's own event: a refused worklet
        // module fires no securitypolicyviolation at all (measured, Chrome 140), and its one trace is this
        // console line. That gap is how a Listen the policy refused shipped with this check green.
        const refused=[]
        browser.on('Log.entryAdded',(event,from)=>{if(from===session&&event.entry.text.includes('Content Security Policy'))refused.push(event.entry.text.slice(0,160))})
        await browser.call('Log.enable',{},session)
        await evaluate(`window.__csp=[];document.addEventListener('securitypolicyviolation',e=>window.__csp.push(e.violatedDirective+' '+e.blockedURI));true`)
        assert.equal(offers.length,0,'A3: nothing is offered before the operator asks, even with station control available')
        // The card's Stream IS the ask (2026-10-02): the page it opens starts the stream once, on its own.
        await click(button('Stream'))
        const streamDiagnostic=()=>evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c){const o=c.operations.getSnapshot();return {status:document.querySelector('.remote-stream-status')?.textContent,link:c.stream.getSnapshot(),ops:{connected:o.connected,fresh:o.fresh,busy:o.busy,error:o.error,phase:o.state?.phase,lease:o.state?.leaseId}}}f=f.return}return null})()`)
        try{await until(`document.querySelector('.remote-stream-app')?.dataset.streamPhase==='live'&&document.querySelector('.remote-stream-video')?.videoWidth>0`,20000)}
        catch(error){console.log('Stream diagnostic',JSON.stringify({page:await streamDiagnostic(),offers:offers.length,shack:await atShack('({state:window.__pc?.connectionState,received:__shack.received,candidates:__shack.candidates.length})').catch(e=>String(e))}));throw error}
        assert.equal(offers.length,1,'one offer, under the lease the station issued')
        const heldLease=await evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c)return c.operations.getSnapshot().state?.leaseId;f=f.return}return null})()`)
        assert.equal(offers[0].leaseId,heldLease,'A3: the offer carries the lease this browser holds')
        if(phone){
          // A PHONE, in Chrome's own mobile emulation with touch, upright (412 x 915) and turned (915 x 412): the
          // operator's report of 2026-10-03, "the streaming window is very small and cannot really be zoomed into".
          // Every finger is judged by what reached the shack. A tap is a press and a release there. A PINCH IS NOTHING
          // THERE: two fingers zoom and pan the picture on this page, where a pinch used to send the shack two presses
          // and a drag, which could press a control at the shack. Zoomed, the station is asked for the stage times the
          // zoom, so the picture is drawn from more of the shack's pixels. The page keeps its size under the browser's
          // own zoom, and Fit is the whole picture again.
          const touch=async(type,touchPoints)=>{await browser.call('Input.dispatchTouchEvent',{type,touchPoints},session);await sleep(25)}
          const tap=async point=>{await touch('touchStart',[{...point,id:1}]);await touch('touchEnd',[])}
          const record=()=>atShack(`({pointers:__shack.received.control.filter(m=>m.type==='pointer'),held:__shack.received.ptt.filter(m=>m.type==='held'&&m.buttons!==0),views:__shack.received.control.filter(m=>m.type==='view')})`)
          const zoomOf=()=>evaluate(`(()=>{const t=getComputedStyle(document.querySelector('.remote-stream-video')).transform;if(t==='none')return {zoom:1,x:0,y:0};const m=t.slice(t.indexOf('(')+1,-1).split(',').map(Number);return {zoom:m[0],x:m[4],y:m[5]}})()`)
          const page=()=>evaluate(`(()=>{const s=document.querySelector('.remote-stream-stage'),r=s.getBoundingClientRect(),v=document.querySelector('.remote-stream-video'),css=getComputedStyle(document.documentElement);return {stage:{x:r.x,y:r.y,w:r.width,h:r.height},video:[v.videoWidth,v.videoHeight],eff:[parseFloat(css.getPropertyValue('--vw-eff')),parseFloat(css.getPropertyValue('--vh-eff'))],scale:visualViewport.scale,touchAction:getComputedStyle(s).touchAction}})()`)
          const centreOf=expression=>evaluate(`(()=>{const e=${expression};e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`)
          const settledView=async()=>{await sleep(700);return (await record()).views.at(-1)}
          const phoneRecord=[]
          for(const [w,h] of [[412,915],[915,412]]){
            await browser.call('Emulation.setDeviceMetricsOverride',{width:w,height:h,deviceScaleFactor:3,mobile:true},session)
            await browser.call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:5},session)
            await evaluate(`window.dispatchEvent(new Event('resize'));true`)
            await settledLayout()
            const at=await page(),fitted=await settledView()
            const stop=await evaluate(`(()=>{const e=${button('Stop TX')},r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return {inside:r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,hit:e.contains(document.elementFromPoint(x,y))}})()`)
            assert.ok(stop.inside&&stop.hit,`Stop TX reachable on a ${w}x${h} phone: ${JSON.stringify(stop)}`)
            // THE PHONE LAYOUT (the operator's pick, 2026-10-03). Upright, a bar over the picture with Stop TX and a bar of
            // thumb controls under it; on its side, a rail of controls beside the picture. Stop TX is the first control and the
            // largest in both, the PTT is about 96 px, and the picture takes the room the header took: on its side the 21:9
            // picture was 569 x 238 under the header (measured 2026-10-03), and the rail is 120 px.
            const laid=await evaluate(`(()=>{const app=document.querySelector('.remote-stream-app'),box=e=>{const r=e.getBoundingClientRect();return {x:Math.round(r.x),y:Math.round(r.y),w:Math.round(r.width),h:Math.round(r.height)}},buttons=[...app.querySelectorAll('button')].filter(e=>e.getClientRects().length>0),stop=${button('Stop TX')},ptt=document.querySelector('.remote-stream-ptt'),v=document.querySelector('.remote-stream-video'),r=v.getBoundingClientRect(),s=Math.min(r.width/v.videoWidth,r.height/v.videoHeight),rail=app.querySelector('.remote-stream-header');return {layout:app.dataset.layout,railFits:app.dataset.layout==='rail'?rail.scrollHeight<=rail.clientHeight+1:null,first:buttons[0]===stop,stop:box(stop),ptt:box(ptt),picture:[+(v.videoWidth*s).toFixed(1),+(v.videoHeight*s).toFixed(1)],turn:document.querySelector('.remote-stream-turn')?.textContent??null,others:buttons.filter(e=>e!==stop).map(e=>({name:e.textContent,...box(e)}))}})()`)
            console.log(`Phone layout ${w}x${h}: `+JSON.stringify(laid))
            const upright=h>w
            assert.equal(laid.layout,upright?'bars':'rail',`the ${upright?'upright':'turned'} layout`)
            assert.equal(laid.railFits,upright?null:true,'on its side the rail holds every control with nothing to scroll')
            assert.ok(laid.first&&laid.others.every(o=>o.y>=laid.stop.y),`Stop TX is the first control, and none sits above it: ${JSON.stringify(laid)}`)
            assert.ok(laid.stop.w>=laid.ptt.w&&laid.stop.h>=laid.ptt.h&&laid.others.every(o=>o.w*o.h<=laid.stop.w*laid.stop.h),`Stop TX is the largest control: ${JSON.stringify(laid)}`)
            assert.ok(Math.abs(laid.ptt.h-96)<=4&&(upright||Math.abs(laid.ptt.w-96)<=4),`a PTT of about 96 px: ${JSON.stringify(laid.ptt)}`)
            const fills=upright?[412,412*360/860]:[915-120,(915-120)*360/860]
            assert.ok(Math.abs(laid.picture[0]-fills[0])<=2&&Math.abs(laid.picture[1]-fills[1])<=2,`the picture is ${laid.picture.join(' x ')}, against ${fills.map(n=>n.toFixed(1)).join(' x ')}`)
            assert.equal(laid.turn,upright?'Turn the phone on its side for a bigger picture.':null,'upright, a phone is told it can turn for a bigger picture')
            // KEYBOARD, in the thumbs' bar upright and on the rail on its side (the operator's pick, 2026-10-05): whole, on screen,
            // its label on one line, and taking its own taps.
            const keys=await evaluate(`(()=>{const e=${button('Keyboard')};if(!e)return null;const r=e.getBoundingClientRect(),rail=document.querySelector('.remote-stream-header').getBoundingClientRect();return {x:Math.round(r.x),y:Math.round(r.y),w:Math.round(r.width),h:Math.round(r.height),inside:r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,onRail:r.left>=rail.left&&r.right<=rail.right,hit:e.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))}})()`)
            assert.ok(keys&&keys.inside&&keys.hit&&(upright||(keys.onRail&&keys.h<=45)),`Keyboard, whole and on screen ${upright?'in the thumbs\' bar':'on the rail'}: ${JSON.stringify(keys)}`)
            const centre={x:at.stage.x+at.stage.w/2,y:at.stage.y+at.stage.h/2}
            // A TAP: a press and a release at the shack, where the finger was.
            let mark=(await record()).pointers.length
            await tap(centre)
            await untilShack(`__shack.received.control.filter(m=>m.type==='pointer').length>=${mark+2}`)
            await sleep(300)
            const tapped=(await record()).pointers.slice(mark)
            assert.deepEqual(tapped.map(m=>`${m.action} ${m.pointerType} ${m.clicks}`),['down touch 1','up touch 1'],`a tap on a ${w}x${h} phone: ${JSON.stringify(tapped)}`)
            assert.ok(tapped.every(m=>Math.abs(m.x-0.5)<0.01&&Math.abs(m.y-0.5)<0.01),'where the finger was: '+JSON.stringify(tapped))
            // A PINCH on the picture: two fingers 40 px apart about its centre, the second landing 25 ms after the first,
            // spread to 232 px apart. Nothing at all reaches the shack: no press, no drag, nothing held.
            const before=await record()
            await touch('touchStart',[{x:centre.x-20,y:centre.y,id:1}])
            await touch('touchStart',[{x:centre.x-20,y:centre.y,id:1},{x:centre.x+20,y:centre.y,id:2}])
            for(let i=1;i<=8;i++)await touch('touchMove',[{x:centre.x-20-i*12,y:centre.y,id:1},{x:centre.x+20+i*12,y:centre.y,id:2}])
            await touch('touchEnd',[])
            await sleep(500)
            const after=await record(),pinched={pointers:after.pointers.slice(before.pointers.length),held:after.held.slice(before.held.length)}
            const entry={phone:`${w}x${h}`,layout:laid.layout,picture:laid.picture,stop:laid.stop,ptt:laid.ptt,tap:tapped.map(m=>({action:m.action,x:+m.x.toFixed(3),y:+m.y.toFixed(3),pointerType:m.pointerType,clicks:m.clicks})),pinch:{pointers:pinched.pointers.map(m=>`${m.action} ${m.buttons}`),held:pinched.held.length}}
            phoneRecord.push(entry)
            console.log(`Phone input record ${w}x${h}: `+JSON.stringify(entry))
            assert.deepEqual(pinched,{pointers:[],held:[]},`a pinch on a ${w}x${h} phone sends the shack nothing: no press, no drag`)
            assert.equal(at.touchAction,'none','the stage takes every touch for itself, so the browser neither pans nor zooms under the picture')
            // ...and zooms the picture HERE, about the point between the fingers (5.8 times), with the page as it was.
            const zoomed=await zoomOf(),still=await page()
            assert.ok(Math.abs(zoomed.zoom-5.8)<0.01&&Math.abs(zoomed.x)<0.5&&Math.abs(zoomed.y)<0.5,'the picture zoomed here: '+JSON.stringify(zoomed))
            assert.deepEqual({eff:still.eff,scale:still.scale,stage:still.stage},{eff:at.eff,scale:1,stage:at.stage},'the browser did not zoom the page, and the page kept its size')
            assert.equal(await evaluate(`!!${button('Fit')}`),true,'Fit is offered while zoomed')
            // SHARPER: once the zoom settles the station is asked for the stage times the zoom.
            const sharp=await settledView()
            assert.ok([sharp.width-Math.min(16384,Math.round(fitted.width*zoomed.zoom)),sharp.height-Math.min(16384,Math.round(fitted.height*zoomed.zoom))].every(d=>Math.abs(d)<=1),'zoomed, the station is asked for the stage times the zoom: '+JSON.stringify({fitted,sharp,zoom:zoomed.zoom}))
            Object.assign(entry,{zoom:zoomed.zoom,view:{fitted:[fitted.width,fitted.height],zoomed:[sharp.width,sharp.height]}})
            // THE ZOOM MAPS: a tap 30 px right of the centre lands where the zoomed picture shows it, not the whole one.
            mark=(await record()).pointers.length
            await tap({x:centre.x+30,y:centre.y})
            await untilShack(`__shack.received.control.filter(m=>m.type==='pointer').length>=${mark+2}`)
            const shown=Math.min(at.stage.w/at.video[0],at.stage.h/at.video[1])*at.video[0],[through]=(await record()).pointers.slice(mark)
            assert.ok(Math.abs(through.x-(0.5+30/(zoomed.zoom*shown)))<0.002&&Math.abs(through.y-0.5)<0.002,'the tap maps through the zoom: '+JSON.stringify({x:through.x,y:through.y,zoomed:0.5+30/(zoomed.zoom*shown),whole:0.5+30/shown}))
            entry.zoomedTap={x:+through.x.toFixed(4),zoomed:+(0.5+30/(zoomed.zoom*shown)).toFixed(4),whole:+(0.5+30/shown).toFixed(4)}
            // FIT: the whole picture again, and the stage's own size asked for again.
            await tap(await centreOf(button('Fit')))
            await until(`getComputedStyle(document.querySelector('.remote-stream-video')).transform==='none'&&!${button('Fit')}`,5000)
            assert.deepEqual(await settledView(),fitted,'Fit asks for the stage\'s own size again')
            // THE BROWSER'S OWN ZOOM, as a pinch on the header makes it: the page keeps its size, where it once shrank into
            // a corner of the zoomed view and would have asked the station for a smaller picture.
            const viewsBefore=(await record()).views.length
            await browser.call('Emulation.setPageScaleFactor',{pageScaleFactor:2.5},session)
            // Long enough for the page to tell the station a new size, had it one to tell.
            await sleep(700);await settledLayout()
            const browserZoomed=await page(),duringZoom=(await record()).views.slice(viewsBefore)
            await browser.call('Emulation.setPageScaleFactor',{pageScaleFactor:1},session)
            assert.ok(browserZoomed.scale>2.4,'control: the browser did zoom: '+JSON.stringify(browserZoomed))
            assert.ok([browserZoomed.eff[0]-at.eff[0],browserZoomed.eff[1]-at.eff[1],browserZoomed.stage.w-at.stage.w,browserZoomed.stage.h-at.stage.h].every(d=>Math.abs(d)<0.5),`the page kept its size under the browser's zoom: ${JSON.stringify({at,browserZoomed})}`)
            assert.ok(duringZoom.every(v=>v.width>=fitted.width&&v.height>=fitted.height),'and asked the station for no smaller picture: '+JSON.stringify({fitted,duringZoom}))
            entry.browserZoom={scale:browserZoomed.scale,eff:browserZoomed.eff,views:duringZoom.map(v=>[v.width,v.height])}
            await settledView()
            if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`phone-${w}x${h}.png`),Buffer.from(shot.data,'base64'))}
          }
          // A SMALL TABLET ON ITS SIDE, in a browser tab: 960 x 480 is under the rail's 500 px, so it has the rail, Keyboard on it,
          // and every control whole, on screen and taking its own taps. (A tablet 500 px tall or more has the header row.)
          {
            await browser.call('Emulation.setDeviceMetricsOverride',{width:960,height:480,deviceScaleFactor:2,mobile:true},session)
            await evaluate(`window.dispatchEvent(new Event('resize'));true`)
            await until(`getComputedStyle(document.documentElement).getPropertyValue('--vw-eff')==='960px'`,3000)
            await settledLayout()
            const tablet=await evaluate(`(()=>{const app=document.querySelector('.remote-stream-app'),rail=app.querySelector('.remote-stream-header'),buttons=[...app.querySelectorAll('button')].filter(e=>e.getClientRects().length>0),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {layout:app.dataset.layout,railFits:rail.scrollHeight<=rail.clientHeight+1,first:buttons[0]===${button('Stop TX')},order:[...buttons].sort((a,b)=>a.getBoundingClientRect().y-b.getBoundingClientRect().y).map(e=>e.textContent),whole:buttons.filter(e=>{const r=e.getBoundingClientRect();return !(r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth&&hit(e))}).map(e=>e.textContent)}})()`)
            console.log('Phone layout 960x480, a small tablet: '+JSON.stringify(tablet))
            assert.deepEqual(tablet,{layout:'rail',railFits:true,first:true,order:['Stop TX','Hold PTT','Mic off','Listen','Keyboard','More'],whole:[]},'a small tablet on its side has the rail, with Keyboard on it and nothing cut off')
            if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'tablet-960x480.png'),Buffer.from(shot.data,'base64'))}
            await browser.call('Emulation.setDeviceMetricsOverride',{width:915,height:412,deviceScaleFactor:3,mobile:true},session)
            await evaluate(`window.dispatchEvent(new Event('resize'));true`)
            await until(`getComputedStyle(document.documentElement).getPropertyValue('--vw-eff')==='915px'`,3000)
            await settledLayout()
          }
          // MORE, on its side: End the stream, Full screen (moved here to make room for Keyboard on the rail, 2026-10-05, as it is
          // upright), Disconnect, Sign out and the beta line, over the picture beside the rail, every one reachable and Stop TX
          // still uncovered. A tap on the picture closes it and reaches nothing at the shack: the cover under it takes the tap, so
          // a tap meant to close a menu is never a click at the shack.
          await tap(await centreOf(button('More')))
          await until(`!!document.querySelector('.remote-stream-more-panel')`,3000)
          const opened=await evaluate(`(()=>{const p=document.querySelector('.remote-stream-more-panel'),r=p.getBoundingClientRect(),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {items:[...p.querySelectorAll('button')].map(b=>b.textContent),beta:p.querySelector('.remote-beta')?.textContent??null,reachable:[...p.querySelectorAll('button')].every(hit),stop:hit(${button('Stop TX')}),inside:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight}})()`)
          assert.deepEqual(opened,{items:['End the stream','Full screen','Disconnect and return to stations','Sign out'],beta:'Beta Remote streaming is a beta feature. Access could be revoked at any time.',reachable:true,stop:true,inside:true},'More, on its side')
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'phone-915x412-more.png'),Buffer.from(shot.data,'base64'))}
          {
            const stage=(await page()).stage,before=(await record()).pointers.length
            await tap({x:stage.x+40,y:stage.y+stage.h/2})
            await until(`!document.querySelector('.remote-stream-more-panel')`,3000)
            await sleep(400)
            assert.deepEqual((await record()).pointers.slice(before),[],'the tap that closed More reached nothing at the shack')
            // CONTROL: the same tap with More closed is a click at the shack.
            await tap({x:stage.x+40,y:stage.y+stage.h/2})
            await untilShack(`__shack.received.control.filter(m=>m.type==='pointer').length>=${before+2}`)
          }
          // FULL SCREEN, a phone's rule: the whole page, turned to landscape where the browser can (this emulation cannot,
          // and says NotSupportedError), no Esc to keep, and the back gesture leaving it sends no Stop. Under More.
          await evaluate(`window.__locks=[];const o=screen.orientation.lock.bind(screen.orientation);screen.orientation.lock=k=>{__locks.push('orientation '+k);return o(k)};const l=navigator.keyboard.lock.bind(navigator.keyboard);navigator.keyboard.lock=k=>{__locks.push('keyboard '+k);return l(k)};true`)
          const underMore=async()=>{await tap(await centreOf(button('More')));await until(`!!document.querySelector('.remote-stream-more-panel')`,3000)}
          await underMore()
          await tap(await centreOf(button('Full screen')))
          await until(`document.fullscreenElement===document.documentElement`,5000)
          assert.deepEqual(await evaluate('window.__locks'),['orientation landscape'],'a phone turns to landscape, and has no Esc to lock')
          const stopsBefore=[await atShack(`__shack.received.control.filter(m=>m.type==='stopTransmit').length`),stopRequests.length]
          await evaluate('document.exitFullscreen().then(()=>true)')
          await until('!document.fullscreenElement',5000)
          await sleep(600)
          assert.deepEqual([await atShack(`__shack.received.control.filter(m=>m.type==='stopTransmit').length`),stopRequests.length],stopsBefore,'on a phone, leaving full screen by the back gesture sends no Stop')
          await underMore()
          assert.equal(await evaluate(`!!${button('Full screen')}`),true,'and Full screen is offered again')
          await tap(await centreOf(button('More')))
          await until(`!document.querySelector('.remote-stream-more-panel')`,3000)
          // THE TYPING BOX, on its side (the operator's pick, 2026-10-05): Keyboard on the rail opens the same field under it,
          // focused by the tap, whole and on screen with Stop TX still uncovered, and what is typed goes to the shack as it does
          // upright. Keyboard again closes it.
          await tap(await centreOf(button('Keyboard')))
          await until(`document.activeElement?.classList.contains('remote-stream-typing-field')`,3000)
          {
            const placed=await evaluate(`(()=>{const f=document.querySelector('.remote-stream-typing-field'),rail=document.querySelector('.remote-stream-header'),r=f.getBoundingClientRect(),q=rail.getBoundingClientRect(),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {layout:document.querySelector('.remote-stream-app').dataset.layout,onRail:rail.contains(f)&&r.left>=q.left&&r.right<=q.right,inside:r.top>=0&&r.bottom<=innerHeight,field:hit(f),stop:hit(${button('Stop TX')}),w:Math.round(r.width),h:Math.round(r.height)}})()`)
            console.log('Phone typing on its side: '+JSON.stringify(placed))
            assert.ok(placed.layout==='rail'&&placed.onRail&&placed.inside&&placed.field&&placed.stop,`the typing box on the rail, whole and on screen, and Stop TX uncovered: ${JSON.stringify(placed)}`)
            const from=await atShack('__shack.received.control.length')
            await browser.call('Input.insertText',{text:'W1AW'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
            await untilShack(`__shack.received.control.slice(${from}).some(m=>m.type==='key'&&m.key==='Enter'&&m.action==='up')`)
            await sleep(300)
            const sent=await atShack(`__shack.received.control.slice(${from}).filter(m=>['text','key','pointer','wheel'].includes(m.type)).map(m=>m.type==='text'?'text '+m.text:m.type+' '+(m.action??'')+' '+(m.key??''))`)
            assert.deepEqual(sent,['text W1AW','key down Enter','key up Enter'],'on its side, exactly the committed text, then Enter')
            if(artifacts){await browser.call('Input.insertText',{text:'CQ'},session);await settledLayout();const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'phone-915x412-typing.png'),Buffer.from(shot.data,'base64'))}
            phoneRecord.push({typingOnItsSide:{placed,sent}})
            await tap(await centreOf(button('Keyboard')))
            await until(`!document.querySelector('.remote-stream-typing-field')`,3000)
          }
          // THE TYPING BOX, upright: Keyboard opens a one-line field, focused by the tap so a phone raises its keyboard. What the
          // keyboard commits goes to the field focused at the shack as the contract's `text`, a word still being composed goes
          // when it is finished, and Enter goes as Enter: exactly that, and nothing through the picture's own input.
          await browser.call('Emulation.setDeviceMetricsOverride',{width:412,height:915,deviceScaleFactor:3,mobile:true},session)
          await evaluate(`window.dispatchEvent(new Event('resize'));true`)
          await until(`document.querySelector('.remote-stream-app')?.dataset.layout==='bars'`,3000)
          await settledLayout()
          await tap(await centreOf(button('Keyboard')))
          await until(`document.activeElement?.classList.contains('remote-stream-typing-field')`,3000)
          {
            const from=await atShack('__shack.received.control.length')
            const typed=()=>atShack(`__shack.received.control.slice(${from}).filter(m=>['text','key','pointer','wheel'].includes(m.type)).map(m=>m.type==='text'?'text '+m.text:m.type+' '+(m.action??'')+' '+(m.key??''))`)
            await browser.call('Input.insertText',{text:'W1AW'},session)
            await untilShack(`__shack.received.control.slice(${from}).some(m=>m.type==='text')`)
            await browser.call('Input.imeSetComposition',{text:'ka',selectionStart:2,selectionEnd:2},session)
            await sleep(300)
            const composing=await typed()
            await browser.call('Input.insertText',{text:'か'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
            await untilShack(`__shack.received.control.slice(${from}).some(m=>m.type==='key'&&m.key==='Enter'&&m.action==='up')`)
            await sleep(300)
            const sent=await typed()
            console.log('Phone typing record: '+JSON.stringify({composing,sent}))
            assert.deepEqual(composing,['text W1AW'],'a word still being composed sends nothing')
            assert.deepEqual(sent,['text W1AW','text か','key down Enter','key up Enter'],'exactly the committed text, then Enter')
            assert.equal(await evaluate(`document.querySelector('.remote-stream-typing-field').value`),'','Enter empties the box')
            if(artifacts){await browser.call('Input.insertText',{text:'CQ'},session);await settledLayout();const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'phone-412x915-typing.png'),Buffer.from(shot.data,'base64'))}
            phoneRecord.push({typing:sent})
          }
          // THE INSTALLABLE APP, under the Worker's real policy: Chrome reads the page's own manifest (`manifest-src 'self'`) with no
          // error and no policy refusal. CONTROL: the same page pointed at another origin's manifest is refused by the policy, and
          // that refusal is seen, so the empty list above means none was refused.
          {
            const manifest=await browser.call('Page.getAppManifest',{},session)
            const installable=await browser.call('Page.getInstallabilityErrors',{},session).catch(error=>({error:String(error.message)}))
            console.log('Phone manifest: '+JSON.stringify({url:manifest.url,errors:manifest.errors,installable}))
            assert.ok(manifest.url.endsWith('/manifest.webmanifest')&&manifest.errors.length===0&&JSON.parse(manifest.data).name==='Nexus Remote',`Chrome read the page's manifest: ${JSON.stringify({url:manifest.url,errors:manifest.errors})}`)
            assert.deepEqual({events:(await evaluate('window.__csp')).filter(v=>v.startsWith('manifest-src')),console:refused.filter(text=>/manifest/i.test(text))},{events:[],console:[]},'no policy refusal of the manifest')
            await evaluate(`document.querySelector('link[rel=manifest]').href='https://example.invalid/app.webmanifest';true`)
            await browser.call('Page.getAppManifest',{},session).catch(()=>null)
            for(let i=0;i<30&&!(await evaluate(`window.__csp.some(v=>v.startsWith('manifest-src'))`));i++)await sleep(100)
            assert.ok((await evaluate('window.__csp')).some(v=>v.startsWith('manifest-src https://example.invalid')),'control: another origin\'s manifest is refused by the policy: '+JSON.stringify(await evaluate('window.__csp')))
            await evaluate(`document.querySelector('link[rel=manifest]').href='/manifest.webmanifest';true`)
            phoneRecord.push({manifest:{url:manifest.url,errors:manifest.errors,installable}})
          }
          // AN iPAD-SIZE WINDOW, 1024 x 768 and upright 768 x 1024, has the header row (the operator's pick, 2026-10-05). With a mouse
          // it is the row as it was, with no Keyboard. On a touch screen Keyboard follows Mic, and the typing box it opens takes a line
          // of its own after the buttons, whole and on screen with Stop TX first and uncovered, and what is typed goes to the shack as
          // on a phone.
          {
            const headerAt=async(touch,height,width=1024)=>{
              await browser.call('Emulation.setTouchEmulationEnabled',touch?{enabled:true,maxTouchPoints:5}:{enabled:false},session)
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:2,mobile:touch},session)
              await evaluate(`window.dispatchEvent(new Event('resize'));true`)
              await until(`(s=>s.getPropertyValue('--vw-eff')==='${width}px'&&s.getPropertyValue('--vh-eff')==='${height}px')(getComputedStyle(document.documentElement))`,3000)
              await settledLayout()
              return evaluate(`(()=>{const app=document.querySelector('.remote-stream-app'),header=app.querySelector('.remote-stream-header'),buttons=[...header.querySelectorAll('button')].filter(e=>e.getClientRects().length>0),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {layout:app.dataset.layout,coarse:matchMedia('(pointer: coarse)').matches,names:buttons.map(e=>e.textContent),first:buttons[0]===${button('Stop TX')},cut:buttons.filter(e=>{const r=e.getBoundingClientRect();return !(r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth&&hit(e))}).map(e=>e.textContent),scrolls:header.scrollHeight>header.clientHeight+1}})()`)
            }
            const shoot=async name=>{if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,name),Buffer.from(shot.data,'base64'))}}
            const row=['Stop TX','Listen','Hold PTT','Mic off','End the stream','Full screen','Disconnect and return to stations','Sign out']
            const mouse=await headerAt(false,768)
            console.log('iPad-size header, a mouse: '+JSON.stringify(mouse))
            assert.deepEqual(mouse,{layout:'header',coarse:false,names:row,first:true,cut:[],scrolls:false},'an iPad-size window with a mouse: the header row as it was, with no Keyboard')
            await shoot('ipad-1024x768-mouse.png')
            // A size change on the way, so the page reads the pointer again: a real tablet never changes its pointer.
            await headerAt(true,767)
            const touch=await headerAt(true,768)
            console.log('iPad-size header, a touch screen: '+JSON.stringify(touch))
            assert.deepEqual(touch,{layout:'header',coarse:true,names:[...row.slice(0,4),'Keyboard',...row.slice(4)],first:true,cut:[],scrolls:false},'an iPad-size touch screen: Keyboard follows Mic in the header row')
            await shoot('ipad-1024x768-touch.png')
            await tap(await centreOf(button('Keyboard')))
            await until(`document.activeElement?.classList.contains('remote-stream-typing-field')`,3000)
            const placedNow=()=>evaluate(`(()=>{const f=document.querySelector('.remote-stream-typing-field'),form=f.form,controls=document.querySelector('.remote-stream-controls'),r=form.getBoundingClientRect(),c=controls.getBoundingClientRect(),buttons=[...controls.querySelectorAll('button')].filter(e=>e.getClientRects().length>0),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {after:buttons.every(e=>e.getBoundingClientRect().bottom<=r.top+0.5),line:Math.abs(r.left-c.left)<=1&&Math.abs(r.right-c.right)<=1,inside:r.top>=0&&r.bottom<=innerHeight,field:hit(f),stop:hit(${button('Stop TX')}),scrolls:document.querySelector('.remote-stream-header').scrollHeight>document.querySelector('.remote-stream-header').clientHeight+1,w:Math.round(r.width),h:Math.round(r.height)}})()`)
            const placed=await placedNow()
            console.log('iPad-size header, the typing box: '+JSON.stringify(placed))
            assert.deepEqual({...placed,w:undefined,h:undefined},{after:true,line:true,inside:true,field:true,stop:true,scrolls:false,w:undefined,h:undefined},'the typing box on a line of its own after the header\'s buttons, whole and on screen, and Stop TX uncovered')
            const from=await atShack('__shack.received.control.length')
            await browser.call('Input.insertText',{text:'W1AW'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r'},session)
            await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
            await untilShack(`__shack.received.control.slice(${from}).some(m=>m.type==='key'&&m.key==='Enter'&&m.action==='up')`)
            await sleep(300)
            const sent=await atShack(`__shack.received.control.slice(${from}).filter(m=>['text','key','pointer','wheel'].includes(m.type)).map(m=>m.type==='text'?'text '+m.text:m.type+' '+(m.action??'')+' '+(m.key??''))`)
            assert.deepEqual(sent,['text W1AW','key down Enter','key up Enter'],'on a tablet, exactly the committed text, then Enter')
            if(artifacts){await browser.call('Input.insertText',{text:'CQ'},session);await settledLayout()}
            await shoot('ipad-1024x768-touch-typing.png')
            // The tablet's own keyboard up takes height and never width: an iPad's page is left about 340 px tall, a small tablet's
            // on its side about 250. The row holds (the rail's shape is not taken while the operator types), the browser brings the
            // field into view, and Stop TX is still on screen and takes its taps.
            const raised=[]
            for(const height of [340,250]){
              await browser.call('Emulation.setDeviceMetricsOverride',{width:1024,height,deviceScaleFactor:2,mobile:true},session)
              await evaluate(`window.dispatchEvent(new Event('resize'));true`)
              await until(`getComputedStyle(document.documentElement).getPropertyValue('--vh-eff')==='${height}px'`,3000)
              await settledLayout()
              const up=await evaluate(`(()=>{const f=document.activeElement;f.scrollIntoView({block:'nearest'});const r=f.getBoundingClientRect(),stop=${button('Stop TX')},s=stop.getBoundingClientRect(),hit=e=>{const b=e.getBoundingClientRect();return e.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))};return {height:${height},layout:document.querySelector('.remote-stream-app').dataset.layout,typing:f.classList.contains('remote-stream-typing-field'),field:r.top>=0&&r.bottom<=innerHeight&&hit(f),stop:s.top>=0&&s.bottom<=innerHeight&&hit(stop),stopTop:Math.round(s.top),fieldTop:Math.round(r.top),header:Math.round(document.querySelector('.remote-stream-header').getBoundingClientRect().height)}})()`)
              console.log(`iPad-size header, its keyboard up (1024 x ${height}): `+JSON.stringify(up))
              await shoot(`ipad-1024x${height}-touch-typing.png`)
              raised.push(up)
            }
            assert.deepEqual(raised.map(({height,layout,typing,field,stop})=>({height,layout,typing,field,stop})),[340,250].map(height=>({height,layout:'header',typing:true,field:true,stop:true})),'with the tablet\'s keyboard up the row holds, the field is in view, and Stop TX is on screen and takes its taps: '+JSON.stringify(raised))
            // Its keyboard down again, Keyboard closes the box. Then the same tablet upright, 768 x 1024: the header row too (768 is
            // wider than a phone), Keyboard after Mic, and the box on a line of its own after the buttons.
            await headerAt(true,768)
            await tap(await centreOf(button('Keyboard')))
            await until(`!document.querySelector('.remote-stream-typing-field')`,3000)
            const upright=await headerAt(true,1024,768)
            console.log('iPad-size header upright, a touch screen: '+JSON.stringify(upright))
            assert.deepEqual(upright,{layout:'header',coarse:true,names:[...row.slice(0,4),'Keyboard',...row.slice(4)],first:true,cut:[],scrolls:false},'an iPad upright, a touch screen: Keyboard follows Mic in the header row')
            await shoot('ipad-768x1024-touch.png')
            await tap(await centreOf(button('Keyboard')))
            await until(`document.activeElement?.classList.contains('remote-stream-typing-field')`,3000)
            const placedUpright=await placedNow()
            console.log('iPad-size header upright, the typing box: '+JSON.stringify(placedUpright))
            assert.deepEqual({...placedUpright,w:undefined,h:undefined},{after:true,line:true,inside:true,field:true,stop:true,scrolls:false,w:undefined,h:undefined},'upright too, the typing box on a line of its own after the header\'s buttons, whole and on screen, and Stop TX uncovered')
            if(artifacts){await browser.call('Input.insertText',{text:'CQ'},session);await settledLayout()}
            await shoot('ipad-768x1024-touch-typing.png')
            phoneRecord.push({tablet:{mouse,touch,placed,sent,raised,upright,placedUpright}})
          }
          console.log('Phone input record: '+JSON.stringify(phoneRecord))
          assert.equal(exceptions,0,'the stream view raised no runtime exception')
          assert.equal(unexpectedMessages,0,'only reviewed messages left the browser socket')
          return
        }
        // A5 in real Chrome: the offer carries the key this browser made and registered with its device,
        // and a signature over the offer's own DTLS fingerprint and the ids the relay stamps. The station's
        // check is Rust's (ring); this proves the page's half end to end, against the signed bytes built
        // again here from the contract's README. The private half is a non-extractable key in IndexedDB.
        const signed=offers[0].payload
        assert.equal(signed.publicKey,(await app.db.prepare('SELECT public_key FROM devices WHERE id=?').bind(device.id).first()).public_key,'A5: the offer carries the key this browser registered with its device')
        const fingerprint=signed.sdp.split('\r\n').find(line=>line.startsWith('a=fingerprint:sha-256 ')).slice(22).split(':').map(pair=>parseInt(pair,16))
        const hashed=new Uint8Array(await crypto.subtle.digest('SHA-256',Uint8Array.from(fingerprint)))
        const bound=session=>Uint8Array.from([...new TextEncoder().encode('nexus-stream-offer/1'),...hashed,...new TextEncoder().encode(pair.stationId+device.id+session)])
        const verifyKey=await crypto.subtle.importKey('spki',Buffer.from(signed.publicKey,'hex'),{name:'ECDSA',namedCurve:'P-256'},false,['verify'])
        const verifies=session=>crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},verifyKey,Buffer.from(signed.signature,'hex'),bound(session))
        assert.equal(await verifies(offers[0].sessionId),true,'A5: the signature verifies for this station, device and session')
        assert.equal(await verifies(crypto.randomUUID()),false,'control: it does not verify for another session')
        assert.deepEqual(await evaluate(`new Promise(done=>{const open=indexedDB.open('nexus-remote-device-keys');open.onsuccess=()=>{const got=open.result.transaction('keys').objectStore('keys').get(${JSON.stringify(pair.stationId)});got.onsuccess=()=>done({type:got.result?.privateKey?.type,extractable:got.result?.privateKey?.extractable})}})`),{type:'private',extractable:false},'A5: the private half is a non-extractable key in IndexedDB')
        // S9: the heartbeat names the frame the page actually presented, in the station's RTP clock.
        await untilShack(`__shack.received.control.some(m=>m.type==='heartbeat'&&Number.isInteger(m.decodedFrameAt))`)
        assert.deepEqual(await atShack(`Object.keys(__shack.received.control.find(m=>m.type==='heartbeat')).sort()`),['decodedFrameAt','leaseId','requestId','type'])
        // W3: the whole stream ran under the Worker's policy without one violation.
        assert.deepEqual({events:await evaluate('window.__csp'),console:refused},{events:[],console:[]},'no CSP violation while negotiating and streaming')
        if(artifacts)for(const theme of ['dark','light']){await evaluate(`document.documentElement.dataset.theme='${theme}';true`);await settledLayout();const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`stream-live-${theme}.png`),Buffer.from(shot.data,'base64'))}
        await evaluate(`document.documentElement.dataset.theme='dark';true`)
        // FIT TO THE WINDOW (the operator's report, 2026-10-16: "the stream doesn't fit the window"). The station stands in as
        // the real one does: its window is a 1920 x 1080 or a 2560 x 1440 shack screen, sent scaled down to the view the page
        // asks for, by the internet's pixel budget (tempo-stream video/picture.rs, encoded_size). Each window is W x H device
        // pixels at this Chrome's own pixel ratio: run it with --force-device-scale-factor=1.25 (or 0.8, 1.5) for a browser zoom
        // or a Windows display scale, which reach the page the same way. Chrome's emulated deviceScaleFactor is no stand-in for
        // either: it leaves devicePixelContentBoxSize in CSS pixels while devicePixelRatio says otherwise (measured 2026-10-16),
        // a mismatch no real browser has. At every desktop size the page has no scrollbar, the
        // whole picture is on screen at the shack's shape and as large as the stage allows, Stop TX is whole and takes its
        // click, and a click on the picture lands at the shack where it was made.
        {
          const restore=await evaluate(`({width:innerWidth,height:innerHeight})`)
          await browser.call('Emulation.setDeviceMetricsOverride',{...restore,deviceScaleFactor:0,mobile:false},session)
          const zoom=await evaluate('devicePixelRatio')
          await atShack(`(()=>{const c=document.querySelector('body > canvas');__shack.window=[c.width,c.height];const size=([W,H],v)=>{const w=W&~1,h=H&~1;let sw=w,sh=h;if(v&&!(w<=v.width&&h<=v.height)){if(v.width*h<=v.height*w){sw=v.width;sh=Math.floor(h*v.width/w)}else{sw=Math.floor(w*v.height/h);sh=v.height}}const P=2560*1600;if(sw*sh>P){const k=Math.sqrt(P/(sw*sh));sw=Math.floor(sw*k);sh=Math.floor(sh*k);while(sw*sh>P)sw--}const side=(n,whole)=>Math.min(Math.max(n&~1,16),whole);return [side(sw,w),side(sh,h)]};window.__fit=setInterval(()=>{const [w,h]=size(__shack.window,__shack.received.control.filter(m=>m.type==='view').at(-1));if(c.width!==w||c.height!==h){c.width=w;c.height=h}},50);return true})()`)
          const shown=`(()=>{const v=document.querySelector('.remote-stream-video'),r=v.getBoundingClientRect(),k=Math.min(r.width/v.videoWidth,r.height/v.videoHeight),w=v.videoWidth*k,h=v.videoHeight*k;return {x:r.x+(r.width-w)/2,y:r.y+(r.height-h)/2,w,h,frame:[v.videoWidth,v.videoHeight],exact:v.hasAttribute('data-exact')}})()`
          // THE RAIL ON A SHORT OR ZOOMED WINDOW (the operator's pick, 2026-10-16). Where the header row leaves a 16:9 shack's
          // picture small, the page takes the rail beside it; the picture is then wider than the header row left it at
          // 1.19.0, measured by this sweep on the unchanged page (the width after each). At 100, 125 and 150 % every other
          // window keeps the layout it had: the header row, or the rail a window under 500 px already took.
          const railNow={'1280x720 100':1062,'1280x720 125':713.6,'1440x900 150':757.3,'1920x1080 150':1062.7,'1024x768 150':556},railBefore=['1280x720 150']
          const fitRecord=[],misfits=[],before=await atShack(`__shack.received.control.length`)
          // A ratio of 2 or more is a Mac's or an iPad's own screen: its windows are given in CSS pixels, and the iPad's on its side.
          const screen=zoom>=2?2:1,windows=screen===2?[[1280,800],[1440,900],[1512,982],[1728,1117],[1024,768],[1180,820],[1366,1024]].map(([w,h])=>[w*2,h*2]):[[1280,720],[1440,900],[1920,1080],[2560,1440],[1024,768]]
          for(const shack of [[1920,1080],[2560,1440],[3440,1440]]){
            await atShack(`__shack.window=${JSON.stringify(shack)};true`)
            for(const [W,H] of windows){
              const width=Math.round(W/zoom),height=Math.round(H/zoom),at=`${shack.join('x')} shack, ${W}x${H} window at ${Math.round(zoom*100)}%`
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:0,mobile:false},session)
              await evaluate(`window.dispatchEvent(new Event('resize'));true`)
              // The page's size is the window's, less nothing: a scrollbar would take its width from it, which is a fault, not a wait.
              let sized=false
              for(let i=0;i<30&&!sized;i++){sized=await evaluate(`(s=>Math.abs(parseFloat(s.getPropertyValue('--vw-eff'))-${width})<1&&Math.abs(parseFloat(s.getPropertyValue('--vh-eff'))-${height})<1)(getComputedStyle(document.documentElement))`);if(!sized)await sleep(100)}
              // Settled: the frame the page shows is the one the station sends for the last view it heard, twice running.
              let last=''
              for(let i=0;i<60;i++){await sleep(150);const now=JSON.stringify([await atShack(`(c=>[c.width,c.height])(document.querySelector('body > canvas'))`),await evaluate(`(v=>[v.videoWidth,v.videoHeight])(document.querySelector('.remote-stream-video'))`)]);const [a,b]=JSON.parse(now);if(a[0]===b[0]&&a[1]===b[1]&&now===last)break;last=now}
              await settledLayout()
              const m=await evaluate(`(()=>{const se=document.scrollingElement,s=document.querySelector('.remote-stream-stage').getBoundingClientRect(),stop=${button('Stop TX')},b=stop.getBoundingClientRect(),p=${shown};return {scroll:[se.scrollWidth-se.clientWidth,se.scrollHeight-se.clientHeight],gutter:[innerWidth-se.clientWidth,innerHeight-se.clientHeight],stage:{x:s.x,y:s.y,w:s.width,h:s.height},picture:p,stop:{inside:b.top>=0&&b.left>=0&&b.bottom<=innerHeight&&b.right<=innerWidth,hit:stop.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2))},layout:document.querySelector('.remote-stream-app').dataset.layout}})()`)
              const p=m.picture,s=m.stage,slack=3
              const faults=sized?[]:['viewport']
              if(m.scroll[0]>0||m.scroll[1]>0||m.gutter[0]>0||m.gutter[1]>0)faults.push('scrolls')
              if(p.x<s.x-1||p.y<s.y-1||p.x+p.w>s.x+s.w+1||p.y+p.h>s.y+s.h+1||p.x+p.w>width+1||p.y+p.h>height+1)faults.push('clipped')
              if(!(Math.abs(p.w-s.w)<=slack||Math.abs(p.h-s.h)<=slack))faults.push('small')
              // The shack's shape, within the station's cut to even sides; the page itself draws the frame at exactly its own.
              if(Math.abs(p.w/p.h-p.frame[0]/p.frame[1])>0.002*p.frame[0]/p.frame[1]||Math.abs(p.frame[0]/p.frame[1]/(shack[0]/shack[1])-1)>0.01)faults.push('shape')
              if(!(m.stop.inside&&m.stop.hit))faults.push('stop')
              const pct=Math.round(zoom*100),named=`${W}x${H} ${pct}`,wide=shack[0]/shack[1]<2
              if([100,125,150].includes(pct)&&m.layout!==((wide&&named in railNow)||railBefore.includes(named)?'rail':'header'))faults.push('layout')
              if(m.layout==='rail'&&Math.abs(s.w-(width-120))>1)faults.push('rail')
              if(wide&&named in railNow&&m.layout==='rail'&&!(p.w>railNow[named]+1))faults.push('smaller')
              // A click at a quarter across and three quarters down the picture, at the shack.
              const mark=await atShack(`__shack.received.control.length`),point={x:p.x+p.w*0.25,y:p.y+p.h*0.75}
              await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1},session)
              await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1},session)
              let landed=[]
              for(let i=0;i<30&&landed.length<2;i++){await sleep(100);landed=await atShack(`__shack.received.control.slice(${mark}).filter(m=>m.type==='pointer'&&(m.action==='down'||m.action==='up')).map(m=>({action:m.action,x:m.x,y:m.y}))`)}
              if(landed.length!==2||!landed.every(l=>Math.abs(l.x-0.25)<=0.01&&Math.abs(l.y-0.75)<=0.01))faults.push('click')
              const entry={at,layout:m.layout,frame:p.frame,exact:p.exact,stage:[s.w,s.h].map(n=>+n.toFixed(1)),picture:[p.w,p.h].map(n=>+n.toFixed(1)),scroll:m.scroll,gutter:m.gutter,stop:m.stop,landed,faults}
              fitRecord.push(entry);if(faults.length)misfits.push(entry)
              if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`fit-${shack.join('x')}-${W}x${H}-${Math.round(zoom*100)}.png`),Buffer.from(shot.data,'base64'))}
            }
          }
          console.log('Stream fit record: '+JSON.stringify(fitRecord))
          // The sweep's clicks leave the record, so what follows judges its own.
          await atShack(`(()=>{clearInterval(window.__fit);const c=document.querySelector('body > canvas');c.width=640;c.height=360;const r=__shack.received.control;r.splice(0,r.length,...r.filter((m,i)=>i<${before}||m.type!=='pointer'));return true})()`)
          await browser.call('Emulation.setDeviceMetricsOverride',{...restore,deviceScaleFactor:0,mobile:false},session)
          await evaluate(`window.dispatchEvent(new Event('resize'));true`)
          await settledLayout()
          assert.deepEqual(misfits,[],'the stream fits the window at every size and zoom, with Stop TX whole and clicks landing where they were made')
        }
        // LISTEN, on the stream, under the same policy. The player's AudioWorklet is a module script, and
        // the Worker's `script-src 'self'` refuses one made from a blob: - so Listen said "no audio"
        // against a station that was sending it (the operator's rc2 test, 2026-10-02), and nothing here
        // pressed it. The worklet has to load as a file of the page's own origin, and the station's
        // audio has to play through it. Both browser APIs are watched, never replaced: each call
        // goes through to Chrome's own.
        await evaluate(`window.__worklets=[];const add=AudioWorklet.prototype.addModule;AudioWorklet.prototype.addModule=function(url,...rest){const w={url:String(url)};window.__worklets.push(w);return add.call(this,url,...rest).then(v=>{w.ok=true;return v},e=>{w.ok=false;w.error=String(e);throw e})};window.__audioReports=[];const Node=AudioWorkletNode;window.AudioWorkletNode=class extends Node{constructor(...a){super(...a);this.port.addEventListener('message',e=>window.__audioReports.push(e.data));this.port.start()}};true`)
        const audioButton=label=>`[...document.querySelectorAll('.remote-stream-app button')].find(e=>e.textContent===${JSON.stringify(label)})`
        // The station has been sending since the channel opened. Nobody has asked to listen, so the
        // control still offers Listen, with no state line: its "audio started" turns nothing on.
        assert.equal(await atShack('__shack.feeds'),1,'the station started its audio when the channel opened')
        // The word crosses the link after the stand-in counts its feed, and one Chrome dropped is said again
        // (the feed above): wait for it to reach the page before asserting what it did.
        await until(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c)return !!c.stream.audioState;f=f.return}return false})()`,10000)
        const said=await atShack('__shack.said')
        if(said>1)console.log(`Station stand-in: Chrome did not send "audio started" until it was said ${said} times`)
        // Listen is drawn only while the station's operation state offers it and the stream's control channel is open. This
        // went red once (2026-10-20, under load) with the station's word read and no Listen drawn, and did not come back in 14
        // runs, quiet, pinned or loaded; so a mismatch says what the page held at that moment, and the next one names its cause.
        const listenRead=await evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c){const a=document.querySelector('.remote-stream-app .remote-audio');return {heard:c.stream.audioState,button:a?.querySelector('button')?.textContent,state:a?.querySelector('.remote-audio-state')?.textContent??null}}f=f.return}return null})()`)
        try{assert.deepEqual(listenRead,{heard:{type:'audioState',listening:true},button:'Listen',state:null},'nobody asked to listen: the station\'s word arrived and turned nothing on')}catch(error){console.log('Listen diagnostic',JSON.stringify({read:listenRead,page:await evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c){const o=c.operations.getSnapshot(),s=c.stream.getSnapshot();return {at:performance.now(),hidden:document.hidden,operations:{connected:o.connected,error:o.error,busy:o.busy,fresh:o.fresh,phase:o.state?.phase??null,capabilities:o.state?.controls?.capabilities??null},stream:{phase:s.phase,control:s.control,reason:s.reason}}}f=f.return}return null})()`).catch(e=>String(e)),closures:await evaluate('window.__socketClosures').catch(()=>null),wire:operationWire.slice(-12)}));throw error}
        await click(audioButton('Listen'))
        await until(`window.__worklets.length>0&&window.__worklets.every(w=>'ok' in w)`,10000)
        const worklets=await evaluate(`window.__worklets.map(w=>{const u=new URL(w.url,location.href);return {ok:w.ok,error:w.error??null,scheme:u.protocol,sameOrigin:u.origin===location.origin,asset:u.pathname.startsWith('/assets/')&&u.pathname.endsWith('.js')}})`)
        assert.deepEqual({worklets,events:await evaluate('window.__csp'),console:refused},{worklets:[{ok:true,error:null,scheme:'http:',sameOrigin:true,asset:true}],events:[],console:[]},'Listen: the audio worklet loads as a file of the page\'s own origin, under the Worker\'s policy')
        await until(`!!document.querySelector('.remote-stream-app .remote-audio-state--live')||!!document.querySelector('.remote-stream-app .remote-audio-state--ended')`,10000)
        assert.equal(await evaluate(`document.querySelector('.remote-stream-app .remote-audio-state')?.textContent`),'Listening to the station','Listen: the station\'s audio is playing')
        // Through the worklet, in the audio thread: its own report says the buffer filled and it plays.
        await until(`window.__audioReports.some(r=>r.playing&&r.held>0)`,10000)
        assert.ok(await atShack('__shack.largestBundle')<=1024,'every bundle within the contract\'s 1024 bytes')
        assert.deepEqual({events:await evaluate('window.__csp'),console:refused},{events:[],console:[]},'no CSP violation while listening')
        await click(audioButton('Stop listening'))
        await until(`!!${audioButton('Listen')}`,5000)
        // THE MICROPHONE, under the Worker's real Permissions-Policy: the page's own origin may use it.
        // The browser still asks (granted here as the operator would; this Chrome's microphone is its
        // fake device), and the station's peer receives its audio. What the policy still refuses: the
        // camera and geolocation everywhere, and the microphone to any other origin, the sign-in
        // frame's included.
        await browser.call('Browser.grantPermissions',{origin:app.origin,permissions:['audioCapture']})
        await evaluate(`(()=>{const real=MediaDevices.prototype.getUserMedia;window.__media=[];MediaDevices.prototype.getUserMedia=function(c){const p=real.apply(this,arguments);p.then(()=>__media.push({audio:!!c?.audio,video:!!c?.video,ok:true}),e=>__media.push({audio:!!c?.audio,video:!!c?.video,ok:false,error:e.name}));return p};return true})()`)
        await click(`document.querySelector('.remote-stream-mic')`)
        await until(`document.querySelector('.remote-stream-mic')?.getAttribute('aria-pressed')==='true'||window.__media.length>0&&!window.__media[0].ok`,10000)
        assert.deepEqual(await evaluate('window.__media'),[{audio:true,video:false,ok:true}],'the stream page obtained a microphone under the real policy')
        assert.equal(await evaluate(`document.querySelector('.remote-stream-mic')?.textContent`),'Mic on')
        await untilShack(`(async()=>{for(const r of (await __pc.getStats()).values())if(r.type==='inbound-rtp'&&r.kind==='audio'&&r.packetsReceived>0)return true;return false})()`)
        // THE MIC LEVEL, under the same policy: its worklet is a file of the page's own origin, the line carries the
        // microphone through it (the meter reads Chrome's fake microphone), and the control is on screen beside Mic at
        // today's level, 0 dB.
        await until(`window.__worklets.some(w=>w.url.includes('mic-level')&&'ok' in w)`,10000)
        assert.deepEqual(await evaluate(`window.__worklets.filter(w=>w.url.includes('mic-level')).map(w=>{const u=new URL(w.url,location.href);return {ok:w.ok,error:w.error??null,sameOrigin:u.origin===location.origin,asset:u.pathname.startsWith('/assets/')&&u.pathname.endsWith('.js')}})`),
          [{ok:true,error:null,sameOrigin:true,asset:true}],'the Mic level worklet loads as a file of the page\'s own origin, under the Worker\'s policy')
        await until(`Number(document.querySelector('.remote-stream-micmeter')?.getAttribute('aria-valuenow'))>-48`,10000)
        assert.deepEqual(await evaluate(`(()=>{const s=document.querySelector('.remote-stream-miclevel input[type=range]'),r=s.getBoundingClientRect(),next=document.querySelector('.remote-stream-mic').nextElementSibling;return {value:s.value,label:s.closest('label')?.textContent,onScreen:r.width>0&&r.height>0&&r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,besideMic:!!next?.contains(s)}})()`),
          {value:'0',label:'Mic level',onScreen:true,besideMic:true},'the Mic level is on screen beside Mic, at today\'s level')
        assert.deepEqual({events:await evaluate('window.__csp'),console:refused},{events:[],console:[]},'no CSP violation with the Mic level running')
        assert.deepEqual(await evaluate(`(p=>({microphone:p.allowsFeature('microphone'),elsewhere:p.allowsFeature('microphone','https://identity.remote-test.invalid'),camera:p.allowsFeature('camera'),geolocation:p.allowsFeature('geolocation')}))(document.featurePolicy)`),{microphone:true,elsewhere:false,camera:false,geolocation:false},'the microphone for this origin alone, and nothing else the policy names')
        assert.equal(await evaluate(`navigator.mediaDevices.getUserMedia({video:true}).then(()=>'granted',e=>e.name)`),'NotAllowedError','control: the camera is still refused')
        await click(`document.querySelector('.remote-stream-mic')`)
        await until(`document.querySelector('.remote-stream-mic')?.getAttribute('aria-pressed')==='false'`,5000)
        // The microphone blocked for this site, as the browser's own Block leaves it: Chrome refuses with
        // NotAllowedError and reports the site's permission as denied, and the page names that place, not
        // the computer's settings or a question left unanswered, in a note on screen under the picture.
        await browser.call('Browser.setPermission',{origin:app.origin,permission:{name:'microphone'},setting:'denied'})
        const asked=await evaluate('window.__media.length')
        await click(`document.querySelector('.remote-stream-mic')`)
        await until(`window.__media.length>${asked}`,10000)
        assert.deepEqual(await evaluate(`window.__media.slice(${asked})`),[{audio:true,video:false,ok:false,error:'NotAllowedError'}],'Chrome refuses the blocked site')
        const blocked=JSON.stringify("The microphone is blocked for this site. Allow it in the browser's settings for this site (the icon at the left of the address bar), then turn the microphone on again.")
        await until(`[...document.querySelectorAll('.remote-stream-note')].some(e=>e.textContent===${blocked})`,5000)
        assert.deepEqual(await evaluate(`(()=>{const e=[...document.querySelectorAll('.remote-stream-note')].find(e=>e.textContent===${blocked}),r=e.getBoundingClientRect();return {role:e.getAttribute('role'),onScreen:r.width>0&&r.height>0&&r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,notes:document.querySelectorAll('.remote-stream-note').length,mic:document.querySelector('.remote-stream-mic')?.textContent}})()`),
          {role:'alert',onScreen:true,notes:1,mic:'Mic off'},'the site block is said, alone and on screen, and the Mic button is off')
        // S11 / A2, the page's half: a click on the picture arrives as a pointer at the frame's own
        // coordinates; a key typed on the focused picture arrives; Tab stays here and leaves the picture.
        await press(await center('.remote-stream-video'))
        await untilShack(`__shack.received.control.some(m=>m.type==='pointer'&&m.action==='up')`)
        const [down]=await atShack(`__shack.received.control.filter(m=>m.type==='pointer'&&m.action==='down')`)
        assert.ok(Math.abs(down.x-0.5)<0.02&&Math.abs(down.y-0.5)<0.02,'the press lands at the centre of the frame: '+JSON.stringify(down))
        // A click is a click at the shack: Chrome's pointer events carry no click count (detail 0), and a
        // press and release sent with none would reach Nexus as a release that clicks nothing.
        const [up]=await atShack(`__shack.received.control.filter(m=>m.type==='pointer'&&m.action==='up')`)
        assert.deepEqual([down.clicks,up.clicks],[1,1],'one click, counted on the press and its release')
        assert.equal(await evaluate(`document.activeElement?.classList.contains('remote-stream-video')`),true,'pressing the picture focuses it')
        await typeKey('a','KeyA','a')
        await untilShack(`__shack.received.control.some(m=>m.type==='key'&&m.key==='a'&&m.action==='up')`)
        await typeKey('Tab','Tab')
        assert.equal(await atShack(`__shack.received.control.some(m=>m.type==='key'&&m.key==='Tab')`),false,'Tab is never sent: it is the way out of the picture')
        // Keys typed on the page's own controls stay on the page.
        const before=await atShack('__shack.received.control.filter(m=>m.type==="key").length')
        await evaluate(`[...document.querySelectorAll('button')].find(b=>b.textContent==='Stop TX').focus();true`)
        await typeKey('b','KeyB','b')
        assert.equal(await atShack('__shack.received.control.filter(m=>m.type==="key").length'),before,'a key on Stop TX never reaches the station')
        // S7: a held PTT is re-asserted under one hold id until let go, then released at once. Judged on the
        // page's own clock, from what it sent and when the button was pressed and let go, so the count follows
        // how long the page held it, whatever this runner slept: the station ends an over that goes PTT_GAP_MS
        // without a hold, so no gap may pass that (the press to the first hold, hold to hold, the last to the
        // let-go), and the release goes when the button is let go, not before. The button's own disabled and
        // focus changes are logged beside them: a press the page ends early names what ended it. So is the page's
        // operations state going stale and fresh again. It lapses for a round trip now and then, and a press on the
        // PTT greyed out by a lapse starts nothing (the operator's pick, 2026-10-04), so each press goes just after
        // a read lands, never into a lapse; held, the PTT stays lit through one (the operator's pick, the same day).
        await evaluate(`(()=>{const log=window.__ptt=[],at=()=>performance.now(),b=document.querySelector('.remote-stream-ptt'),send=RTCDataChannel.prototype.send
          RTCDataChannel.prototype.send=function(data){if(this.label==='ptt')log.push({...JSON.parse(data),at:at()});return send.call(this,data)}
          for(const type of ['pointerdown','pointerup','focusout'])b.addEventListener(type,()=>log.push({type,at:at()}),true)
          for(const type of ['keydown','keyup'])addEventListener(type,e=>log.push({type,on:e.target===b?'PTT':e.target.tagName,at:at()}),true)
          new MutationObserver(()=>log.push({type:b.disabled||b.getAttribute('aria-disabled')==='true'?'greyed':'lit',aria:b.getAttribute('aria-disabled'),at:at()})).observe(b,{attributeFilter:['disabled','aria-disabled']})
          const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f&&!f.memoizedProps?.connection)f=f.return
          const ops=f.memoizedProps.connection.operations,reads=[];let {fresh,state}=ops.getSnapshot()
          ops.subscribe(()=>{const s=ops.getSnapshot();if(s.fresh!==fresh){fresh=s.fresh;log.push({type:fresh?'fresh':'stale',at:at()})}if(s.state!==state){state=s.state;if(s.fresh)for(const read of reads.splice(0))read(true)}})
          window.__afterRead=()=>new Promise((resolve,reject)=>{reads.push(resolve);setTimeout(()=>reject(new Error('the station state was not read again within 5 s')),5000)});return true})()`)
        // One press of the PTT, held `hold` ms, by the mouse or by `key` on the focused button; `meanwhile` runs from the press.
        // `focus` false presses the key on whatever already has the focus.
        const heldPress=async(hold,meanwhile,key,focus=true)=>{
          const from=await evaluate('window.__ptt.length'),got=(await atShack('__shack.received.ptt')).length,point=await center('.remote-stream-ptt')
          // Pressed only once the read lands: a lapse greys out a PTT that is not held, and a press then starts nothing.
          await evaluate(`window.__afterRead().then(()=>{${key&&focus?"document.querySelector('.remote-stream-ptt').focus();":''}return true})`)
          await Promise.all([key?(async()=>{await browser.call('Input.dispatchKeyEvent',{type:'keyDown',text:key.key,...key},session);await sleep(hold);await browser.call('Input.dispatchKeyEvent',{type:'keyUp',...key},session);await sleep(50)})():press(point,hold),meanwhile?.()])
          // The page's own release, waited for no longer than it could take: a press the page never lets go is the red.
          const released=`window.__ptt.slice(${from}).some(m=>m.type==='pttRelease')`
          for(let i=0;i<20&&!await evaluate(released);i++)await sleep(100)
          if(await evaluate(released))await untilShack(`__shack.received.ptt.slice(${got}).some(m=>m.type==='pttRelease')`)
          const sent=(await evaluate('window.__ptt')).slice(from).filter(m=>m.type!=='held'),ptt=(await atShack('__shack.received.ptt')).slice(got).filter(m=>m.type!=='held')
          const pressed=sent.find(m=>m.type===(key?'keydown':'pointerdown'))?.at,letGo=sent.find(m=>m.type===(key?'keyup':'pointerup'))?.at
          const holds=sent.filter(m=>m.type==='pttHold'),pttReleases=sent.filter(m=>m.type==='pttRelease')
          const told=` (ms after the press: ${sent.map(m=>`${m.type}${m.on?' '+m.on:''}${m.seq===undefined?'':' '+m.seq} ${Math.round(m.at-pressed)}`).join(', ')}; the station got ${ptt.map(m=>m.type+' '+m.seq).join(', ')})`
          assert.ok(pressed!==undefined&&letGo!==undefined&&pttReleases.length===1,'the page saw one press and one let-go, and sent one release'+told)
          const gaps=[pressed,...holds.map(m=>m.at),letGo].map((at,i,all)=>at-all[i-1]).slice(1)
          assert.ok(Math.max(...gaps)<=PTT_GAP_MS&&holds.length>=Math.ceil((letGo-pressed)/PTT_GAP_MS),`held ${Math.round(letGo-pressed)} ms: ${holds.length} holds, never ${PTT_GAP_MS} ms without one`+told)
          assert.ok(pttReleases[0].at>=letGo&&pttReleases[0].at-letGo<50,'released when let go, not before and not after'+told)
          assert.equal(new Set([...holds,...pttReleases,...ptt].map(m=>m.holdId)).size,1,'one press, one hold id')
          assert.deepEqual(holds.map(m=>m.seq),holds.map((_,i)=>i),'the sequence counts up from 0')
          assert.ok(!sent.some(m=>m.type==='greyed'&&m.at>pressed&&m.at<letGo),'held, the PTT is never greyed out'+told)
          return {holds,lapsed:sent.some(m=>m.type==='stale'&&m.at>pressed&&m.at<letGo),told}
        }
        const {holds}=await heldPress(450)
        // Held through a lapse: heartbeats answered 400 ms late from just after the press, so the state's 1.2 s runs
        // out between replies while the PTT is held 1.5 s. Chrome 154 blurs a focused button it disables, and a held
        // PTT greyed out by the lapse was let go through its blur, mid-over.
        const throughLapse=await heldPress(1500,async()=>{await sleep(20);heartbeatReplyDelayMs=400})
        heartbeatReplyDelayMs=0
        assert.ok(throughLapse.lapsed,'the state lapsed while the PTT was held'+throughLapse.told)
        console.log('Stream PTT held through a lapse of the state'+throughLapse.told)
        // Space held on the focused PTT through a lapse, let go by its key-up, which reaches the button only while it
        // keeps its focus: a browser that blurs a button it disables sends that key-up to the page instead.
        const spaceThroughLapse=await heldPress(1500,async()=>{await sleep(20);heartbeatReplyDelayMs=400},{key:' ',code:'Space',windowsVirtualKeyCode:32})
        heartbeatReplyDelayMs=0
        assert.ok(spaceThroughLapse.lapsed,'the state lapsed while Space held the PTT'+spaceThroughLapse.told)
        console.log('Stream PTT held by Space through a lapse of the state'+spaceThroughLapse.told)
        // An idle PTT the operator tabbed to keeps its focus through a lapse (the operator's pick "Stay focusable while
        // greyed", 2026-10-04): greyed out it is aria-disabled, never disabled, which a browser blurs, and Space then went
        // to the page until the PTT was focused again. Once it is lit again, Space on it holds, nothing focusing it first.
        const idleFrom=await evaluate('window.__ptt.length')
        await evaluate(`window.__afterRead().then(()=>{document.querySelector('.remote-stream-ptt').focus();return true})`)
        heartbeatReplyDelayMs=400
        await until(`window.__ptt.slice(${idleFrom}).some(m=>m.type==='greyed')`,8000)
        heartbeatReplyDelayMs=0
        await evaluate('window.__afterRead()')
        const idle=(await evaluate('window.__ptt')).slice(idleFrom),idleTold=` (${idle.map(m=>m.type+(m.aria===undefined?'':` aria-disabled=${m.aria}`)).join(', ')})`
        assert.ok(!idle.some(m=>m.type==='focusout')&&await evaluate(`document.activeElement===document.querySelector('.remote-stream-ptt')`),'greyed out by a lapse, the idle PTT keeps its focus'+idleTold)
        assert.ok(idle.some(m=>m.type==='greyed'&&m.aria==='true'),'greyed out, it is aria-disabled: unavailable to a screen reader'+idleTold)
        const keptFocus=await heldPress(450,undefined,{key:' ',code:'Space',windowsVirtualKeyCode:32},false)
        console.log('Stream PTT kept its focus through a lapse'+idleTold+', and Space on it held once lit'+keptFocus.told)
        await evaluate('window.__afterRead()')
        // THE DEAD-MAN, the page's half: a key held on the picture goes as itself and is re-asserted on
        // ptt at once and every 100 ms while held, and not after; a button held on the picture likewise. Judged on
        // the page's own clock as S7 is: the shack's window lets go of whatever is not re-asserted within the held
        // PTT's own gap (STREAM_HELD_GAP_MS is STREAM_PTT_GAP_MS), so none may pass PTT_GAP_MS from the press to the
        // first `held`, `held` to `held` and the last to the let-go, and none is sent once let go.
        const helds=async()=>(await atShack('__shack.received.ptt')).filter(m=>m.type==='held')
        const consecutive=list=>{const seqs=list.map(m=>m.seq).sort((a,b)=>a-b);return seqs.every((seq,i)=>seq===seqs[0]+i)}
        await evaluate(`(()=>{const v=document.querySelector('.remote-stream-video');for(const type of ['keydown','keyup','pointerdown','pointerup'])v.addEventListener(type,()=>window.__ptt.push({type:'picture '+type,at:performance.now()}),true);return true})()`)
        const heldOnPicture=(what,log,down,up,station,itself)=>{
          const pressed=log.find(m=>m.type===down)?.at,letGo=log.find(m=>m.type===up)?.at,sent=log.filter(m=>m.type==='held'),during=sent.filter(m=>m.at<=letGo)
          const told=` (ms after the press: ${log.map(m=>`${m.type}${m.seq===undefined?'':' '+m.seq} ${Math.round(m.at-pressed)}`).join(', ')}; the station got ${station.map(m=>m.seq).join(', ')})`
          assert.ok(pressed!==undefined&&letGo!==undefined,`${what}: the page saw the press and the let-go`+told)
          const gaps=[pressed,...during.map(m=>m.at),letGo].map((at,i,all)=>at-all[i-1]).slice(1)
          assert.ok(Math.max(...gaps)<=PTT_GAP_MS&&during.length>=Math.ceil((letGo-pressed)/PTT_GAP_MS),`${what} ${Math.round(letGo-pressed)} ms: ${during.length} re-assertions, never ${PTT_GAP_MS} ms without one`+told)
          assert.deepEqual(sent.filter(m=>m.at>letGo),[],`${what}: nothing re-asserted once let go`+told)
          assert.ok(station.length>0&&station.every(itself)&&consecutive(station),`${what}: re-asserted as itself, in order`+told)
        }
        await evaluate(`document.querySelector('.remote-stream-video').focus();true`)
        let heldBefore=(await helds()).length,logged=await evaluate('window.__ptt.length')
        const space={key:' ',code:'Space',windowsVirtualKeyCode:32}
        await browser.call('Input.dispatchKeyEvent',{type:'keyDown',text:' ',...space},session)
        await sleep(450)
        await browser.call('Input.dispatchKeyEvent',{type:'keyUp',...space},session)
        await untilShack(`__shack.received.control.some(m=>m.type==='key'&&m.code==='Space'&&m.action==='up')`)
        await sleep(300)
        const keyHeld=(await helds()).slice(heldBefore)
        heldOnPicture('Space held on the picture',(await evaluate('window.__ptt')).slice(logged),'picture keydown','picture keyup',keyHeld,m=>m.keys.join()==='Space'&&m.buttons===0)
        heldBefore=(await helds()).length;logged=await evaluate('window.__ptt.length')
        const point=await center('.remote-stream-video'),ups=await atShack(`__shack.received.control.filter(m=>m.type==='pointer'&&m.action==='up').length`)
        await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1},session)
        await sleep(450)
        await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1},session)
        await untilShack(`__shack.received.control.filter(m=>m.type==='pointer'&&m.action==='up').length>${ups}`)
        await sleep(300)
        const buttonHeld=(await helds()).slice(heldBefore)
        heldOnPicture('A button held on the picture',(await evaluate('window.__ptt')).slice(logged),'picture pointerdown','picture pointerup',buttonHeld,m=>m.keys.length===0&&m.buttons===1)
        // THE STOP LINE, in real layout: Stop TX is on screen and nothing covers it, at every size.
        for(const [w,h] of [[360,640],[390,844],[844,390],[1024,768],[1280,800],[1920,1080]])for(const theme of ['dark','light']){
          await browser.call('Emulation.setDeviceMetricsOverride',{width:w,height:h,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'));true`)
          await settledLayout()
          const stop=await evaluate(`(()=>{const e=[...document.querySelectorAll('button')].find(b=>b.textContent==='Stop TX'),r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return {inside:r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,hit:e.contains(document.elementFromPoint(x,y)),docW:document.documentElement.scrollWidth,w:innerWidth}})()`)
          assert.ok(stop.inside&&stop.hit&&stop.docW<=stop.w+1,`Stop TX reachable at ${w}x${h} ${theme}: ${JSON.stringify(stop)}`)
        }
        await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.dataset.theme='dark';window.dispatchEvent(new Event('resize'));true`)
        await settledLayout()
        // Stop reaches the station both ways: on the stream's control channel and on the socket.
        const socketStops=stopRequests.length
        await click(button('Stop TX'))
        await untilShack(`__shack.received.control.some(m=>m.type==='stopTransmit')`)
        assert.equal((await atShack(`__shack.received.control.find(m=>m.type==='stopTransmit')`)).leaseId,heldLease)
        await sleep(300)
        assert.equal(stopRequests.length,socketStops+1,'and the same Stop over the socket, at the same moment')
        // ESC IS A STOP ANYWHERE ON THE PAGE, in real Chrome: from wherever the keyboard is the Stop goes both ways at once,
        // and from the picture the key still goes on to Nexus at the shack. CONTROL: another key is no Stop. Each Esc waits
        // for the page to hold the station's next transmit epoch, as an operator's second press would find it, and past the
        // relay's own limit of two Stops a second for a session (a press inside it is answered remoteBusy, by design).
        let stoppedAt=Date.now()
        const pageEpoch=()=>evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c){const o=c.operations.getSnapshot();return o.stopSending?null:o.state?.transmitEpoch??null}f=f.return}return null})()`)
        const ready=async()=>{await sleep(Math.max(0,stoppedAt+1100-Date.now()));for(let i=0;i<80&&await pageEpoch()!==transmitEpoch;i++)await sleep(100);assert.equal(await pageEpoch(),transmitEpoch,'the page holds the station\'s current transmit epoch')}
        const stopsNow=async()=>({stream:await atShack(`__shack.received.control.filter(m=>m.type==='stopTransmit').length`),socket:stopRequests.length})
        const oneStop=async(before,what)=>{stoppedAt=Date.now();for(let i=0;i<50;i++){const now=await stopsNow();if(now.stream>before.stream&&now.socket>before.socket)break;await sleep(100)}await sleep(300);assert.deepEqual(await stopsNow(),{stream:before.stream+1,socket:before.socket+1},what)}
        const esc=async()=>{for(const type of ['rawKeyDown','keyUp'])await browser.call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27,nativeVirtualKeyCode:27},session)}
        for(const [where,focus] of [['nothing focused','document.activeElement?.blur()'],['Stop TX',`${button('Stop TX')}.focus()`],['Hold PTT',`document.querySelector('.remote-stream-ptt').focus()`],['the microphone',`document.querySelector('.remote-stream-mic').focus()`],['the picture',`document.querySelector('.remote-stream-video').focus()`]]){
          await ready()
          await evaluate(`${focus};true`)
          const before=await stopsNow()
          await esc()
          await oneStop(before,`Esc with ${where} sends one Stop, both ways`)
        }
        await untilShack(`__shack.received.control.some(m=>m.type==='key'&&m.key==='Escape'&&m.action==='down')`)
        {await ready();const before=await stopsNow();await evaluate('document.activeElement?.blur();true');await typeKey('q','KeyQ','q');await sleep(600);assert.deepEqual(await stopsNow(),before,'control: another key is no Stop')}
        // FULL SCREEN, with Esc still a Stop. Chrome and Edge: the whole page, Stop TX with it, and Esc locked to the page
        // (Keyboard Lock), so Esc stops and full screen stays; the page's own Exit leaves it with no Stop.
        await evaluate(`window.__locks=[];const l=navigator.keyboard.lock.bind(navigator.keyboard);navigator.keyboard.lock=k=>{__locks.push(String(k));return l(k).then(()=>{__locks.push('locked')},e=>{__locks.push('refused '+e.name);throw e})};true`)
        await ready()
        await click(button('Full screen'))
        await until(`document.fullscreenElement===document.documentElement&&window.__locks.includes('locked')`,5000)
        assert.deepEqual(await evaluate('window.__locks'),['Escape','locked'],'Keyboard Lock holds Esc for the page')
        {const stop=await evaluate(`(()=>{const e=${button('Stop TX')},r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return {inside:r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,hit:e.contains(document.elementFromPoint(x,y))}})()`);assert.ok(stop.inside&&stop.hit,'Stop TX on screen in full screen: '+JSON.stringify(stop))}
        {const before=await stopsNow();await esc();await oneStop(before,'Esc in full screen is a Stop');assert.equal(await evaluate('document.fullscreenElement===document.documentElement'),true,'and full screen stays')}
        {await ready();const before=await stopsNow();await click(button('Exit full screen'));await until('!document.fullscreenElement',5000);await sleep(600);assert.deepEqual(await stopsNow(),before,'the page\'s own Exit sends no Stop')}
        // A lock may resolve without holding Esc (WebView2 may). Chrome then hands the page no Esc, as below, and Esc leaves
        // full screen unheard. The page cannot tell, so that exit is a Stop, as is every exit it did not ask for.
        await evaluate(`navigator.keyboard.lock=k=>{__locks.push('unheld '+k);return Promise.resolve()};true`)
        {await ready();await click(button('Full screen'));await until(`document.fullscreenElement===document.documentElement&&window.__locks.includes('unheld Escape')`,5000);const before=await stopsNow();await esc();await until('!document.fullscreenElement',5000);await oneStop(before,'a lock that resolved without holding Esc: Esc leaving full screen is a Stop')}
        // Firefox and Safari have no Keyboard Lock, and without it Chrome hands the page no Esc at all in full screen: Esc
        // leaves full screen first (measured, Chrome 140). There any exit the page did not ask for is a Stop. CONTROL: its
        // own Exit is not.
        await evaluate(`Object.defineProperty(navigator,'keyboard',{configurable:true,value:undefined});true`)
        {await ready();await click(button('Full screen'));await until('document.fullscreenElement===document.documentElement',5000);const before=await stopsNow();await esc();await until('!document.fullscreenElement',5000);await oneStop(before,'without Keyboard Lock, Esc leaving full screen is a Stop')}
        {await ready();await click(button('Full screen'));await until('document.fullscreenElement===document.documentElement',5000);const before=await stopsNow();await click(button('Exit full screen'));await until('!document.fullscreenElement',5000);await sleep(600);assert.deepEqual(await stopsNow(),before,'control: without Keyboard Lock, the page\'s own Exit sends no Stop')}
        await evaluate(`delete navigator.keyboard;true`)
        // Ending it is the operator's to ask; the station is told, and the picture goes.
        await click(button('End the stream'))
        await until(`document.querySelector('.remote-stream-app')?.dataset.streamPhase==='idle'&&!document.querySelector('.remote-stream-video')?.srcObject`)
        for(let i=0;i<30&&!closes.length;i++)await sleep(100)
        assert.equal(closes.length,1,'the station is told the stream ended')
        // A3: the card's ask was spent by that start, so an ended stream is never re-offered on its own.
        await until(`!!${button('Start the stream')}`)
        await sleep(1500)
        assert.equal(offers.length,1,'A3: nothing is offered again until the operator asks again')
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'stream-ready.png'),Buffer.from(shot.data,'base64'))}
        // "STILL THERE?" (the operator's picks "15 min + prompt" and "Only clicks, keys, PTT"), in real
        // layout and real hit-testing. The stream starts again and this TEST puts a clock of its own on
        // the page's idle watch; nothing in the product changes the fifteen minutes. At 15:00 the prompt
        // is up with Stop TX where it was and reachable at every size; the prompt's own press reaches
        // nothing at the shack; and unanswered at 16:00 the stream ends as End the stream ends it.
        await click(button('Start the stream'))
        await until(`document.querySelector('.remote-stream-app')?.dataset.streamPhase==='live'&&document.querySelector('.remote-stream-video')?.videoWidth>0`,20000)
        assert.equal(offers.length,2,'a second offer, for the stream started again')
        assert.equal(await evaluate(`(()=>{const e=document.querySelector('.app');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))];while(f){const c=f.memoizedProps?.connection;if(c){window.__idleSkew=0;c.stream.idle.now=()=>performance.now()+window.__idleSkew;return true}f=f.return}return false})()`),true,'the test clock is on the idle watch')
        const stopBox=()=>evaluate(`JSON.stringify([...document.querySelectorAll('button')].find(b=>b.textContent==='Stop TX').getBoundingClientRect())`)
        const stopBefore=await stopBox()
        await evaluate(`window.__idleSkew+=15*60000+1000;true`)
        await until(`!!document.querySelector('.remote-stream-idle')`,5000)
        assert.equal(await stopBox(),stopBefore,'the prompt moved Stop TX')
        if(artifacts)for(const theme of ['dark','light']){await evaluate(`document.documentElement.dataset.theme='${theme}';true`);await settledLayout();const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`stream-still-there-${theme}.png`),Buffer.from(shot.data,'base64'))}
        // THE STOP LINE while the prompt is up: Stop TX on screen and uncovered, and the prompt's button too.
        for(const [w,h] of [[360,640],[390,844],[844,390],[1024,768],[1280,800],[1920,1080]])for(const theme of ['dark','light']){
          await browser.call('Emulation.setDeviceMetricsOverride',{width:w,height:h,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'));true`)
          await settledLayout()
          const reach=await evaluate(`(()=>{const at=e=>{const r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return {inside:r.top>=0&&r.left>=0&&r.bottom<=innerHeight&&r.right<=innerWidth,hit:e.contains(document.elementFromPoint(x,y))}},keep=document.querySelector('.remote-stream-idle button');return {stop:at([...document.querySelectorAll('button')].find(b=>b.textContent==='Stop TX')),keep:keep&&at(keep),docW:document.documentElement.scrollWidth,w:innerWidth}})()`)
          assert.ok(reach.stop.inside&&reach.stop.hit&&reach.docW<=reach.w+1,`Stop TX reachable with the prompt up at ${w}x${h} ${theme}: ${JSON.stringify(reach)}`)
          assert.ok(reach.keep?.inside&&reach.keep.hit,`the prompt's button reachable at ${w}x${h} ${theme}: ${JSON.stringify(reach)}`)
        }
        await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.dataset.theme='dark';window.dispatchEvent(new Event('resize'));true`)
        await settledLayout()
        assert.equal(await evaluate(`!!document.querySelector('.remote-stream-idle')`),true,'the sweep ran with the prompt up')
        // The prompt's own press, dragged off it onto the picture before it is let go: none of it is a
        // press, a release or a drag at the shack, and it answers the prompt.
        const keep=await center('.remote-stream-idle button'),below={x:keep.x,y:keep.y+140}
        assert.equal(await evaluate(`document.querySelector('.remote-stream-video').contains(document.elementFromPoint(${below.x},${below.y}))`),true,'the drag ends over the picture')
        const [controlBefore,pttBefore]=await atShack('[__shack.received.control.length,__shack.received.ptt.length]')
        await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...keep,button:'left',buttons:1,clickCount:1},session)
        await sleep(100)
        await browser.call('Input.dispatchMouseEvent',{type:'mouseMoved',...below,button:'left',buttons:1},session)
        await sleep(100)
        await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...below,button:'left',buttons:0,clickCount:1},session)
        await until(`!document.querySelector('.remote-stream-idle')`,3000)
        await sleep(400)
        const leaked=await atShack(`[...__shack.received.control.slice(${controlBefore}).filter(m=>m.type==='pointer'&&(m.action!=='move'||m.buttons!==0)),...__shack.received.ptt.slice(${pttBefore}).filter(m=>m.type==='held'&&m.buttons!==0)]`)
        assert.deepEqual(leaked,[],'the prompt\'s own press reached nothing at the shack')
        assert.equal(await evaluate(`document.querySelector('.remote-stream-app')?.dataset.streamPhase`),'live','answered: the stream carries on')
        // The same press held until the prompt has gone. The press is the answer, so the page's next look (once a
        // second) takes the prompt down while the button is still held, its pointer capture goes with it, and Chrome
        // hands the rest of the press to the picture under it. The picture never sends a press it did not take. Held
        // for 100 ms as above, the press outlived that look now and then, and its move reached the shack marked pressed.
        await evaluate(`window.__idleSkew+=15*60000+1000;true`)
        await until(`!!document.querySelector('.remote-stream-idle')`,5000)
        {
          const keep=await center('.remote-stream-idle button'),below={x:keep.x,y:keep.y+140}
          const [controlBefore,pttBefore]=await atShack('[__shack.received.control.length,__shack.received.ptt.length]')
          await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...keep,button:'left',buttons:1,clickCount:1},session)
          await until(`!document.querySelector('.remote-stream-idle')`,3000)
          await browser.call('Input.dispatchMouseEvent',{type:'mouseMoved',...below,button:'left',buttons:1},session)
          await sleep(100)
          await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...below,button:'left',buttons:0,clickCount:1},session)
          await sleep(400)
          const leaked=await atShack(`[...__shack.received.control.slice(${controlBefore}).filter(m=>m.type==='pointer'&&(m.action!=='move'||m.buttons!==0)),...__shack.received.ptt.slice(${pttBefore}).filter(m=>m.type==='held'&&m.buttons!==0)]`)
          assert.deepEqual(leaked,[],'a press on the prompt, held while the prompt went, reached nothing at the shack')
          // CONTROL: a drag that began on the picture reaches the shack as a press, pressed moves and a release.
          const from=await center('.remote-stream-video'),to={x:from.x+30,y:from.y+20},since=await atShack('__shack.received.control.length')
          await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...from,button:'left',buttons:1,clickCount:1},session)
          await sleep(100)
          await browser.call('Input.dispatchMouseEvent',{type:'mouseMoved',...to,button:'left',buttons:1},session)
          await sleep(100)
          await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...to,button:'left',buttons:0,clickCount:1},session)
          await untilShack(`__shack.received.control.slice(${since}).some(m=>m.type==='pointer'&&m.action==='up')`)
          const dragged=await atShack(`__shack.received.control.slice(${since}).filter(m=>m.type==='pointer'&&(m.action!=='move'||m.buttons!==0)).map(m=>m.action+' '+m.buttons)`)
          assert.ok(dragged[0]==='down 1'&&dragged.at(-1)==='up 0'&&dragged.length>2&&dragged.slice(1,-1).every(m=>m==='move 1'),'control: a drag that began on the picture is sent: '+JSON.stringify(dragged))
        }
        // Unanswered: fifteen minutes on the prompt is back, and a minute after that the stream ends -
        // the close on the signalling lane and control released, which is what End the stream sends.
        const releases=()=>operationWire.filter(v=>v.direction==='out'&&v.type==='release').length
        const releasesBefore=releases()
        await evaluate(`window.__idleSkew+=15*60000+1000;true`)
        await until(`!!document.querySelector('.remote-stream-idle')`,5000)
        assert.equal(closes.length,1,'the prompt alone ends nothing')
        await evaluate(`window.__idleSkew+=60000;true`)
        await until(`document.querySelector('.remote-stream-app')?.dataset.streamPhase==='idle'&&!document.querySelector('.remote-stream-video')?.srcObject`,5000)
        for(let i=0;i<30&&closes.length<2;i++)await sleep(100)
        assert.equal(closes.length,2,'unanswered, the station is told the stream ended')
        for(let i=0;i<30&&releases()===releasesBefore;i++)await sleep(100)
        assert.equal(releases(),releasesBefore+1,'and control is released')
        // S1-M1, in the built page: everything it sent the older lane carried its proof, but Stop and state.
        const sentOperations=operationWire.filter(v=>v.direction==='out')
        assert.ok(sentOperations.some(v=>v.type==='acquire')&&sentOperations.some(v=>v.type==='release'),'premise: the page took and released control')
        assert.deepEqual(sentOperations.filter(v=>v.signed!==!['state','stopTransmit'].includes(v.type)).map(v=>v.type),[],'a proof on every request but state and Stop, and none on those')
        assert.equal(loggingLease,null,'the station holds no lease for this browser')
        assert.equal(await evaluate(`document.querySelector('.remote-stream-placeholder')?.textContent.includes('Nobody answered “Still there?”, so the stream ended.')`),true,'the page says why it ended')
        // A REAL STATION'S ANSWER. The stand-in above answers with Chrome's own SDP; the shack answers with
        // str0m's. fixtures/stream/station-answer.json holds the page's offer and the answer the station's
        // own code wrote for it, and the page takes that answer through the relay, its parser, secureAnswer
        // and Chrome's setRemoteDescription, then waits for the connection as it would for the shack.
        const recorded=JSON.parse(await readFile(new URL('./fixtures/stream/station-answer.json',import.meta.url),'utf8'))
        await evaluate(`(()=>{const real=RTCPeerConnection.prototype.setRemoteDescription;window.__descriptions=[];RTCPeerConnection.prototype.setRemoteDescription=function(d){const p=real.apply(this,arguments);p.then(()=>__descriptions.push({type:d?.type,ok:true}),e=>__descriptions.push({type:d?.type,ok:false,error:String(e)}));return p};return true})()`)
        const offersBefore=offers.length,closesBefore=closes.length
        realAnswer=recorded.answer
        await click(button('Start the stream'))
        await until(`window.__descriptions.length>0||document.querySelector('.remote-stream-app')?.dataset.streamPhase==='ended'`,15000)
        assert.equal(offers.length,offersBefore+1,'the page offered again')
        // The recording still answers this page: every media line, codec and header extension the
        // station's answer names is in the offer the page makes today.
        const sections=sdp=>sdp.split(/\r\n(?=m=)/).map(section=>section.split('\r\n'))
        const offered=sections(offers.at(-1).payload.sdp),answered=sections(recorded.answer)
        assert.deepEqual(offered.map(lines=>lines[0].split(' ')[0]),answered.map(lines=>lines[0].split(' ')[0]),'the page offers the media the recorded answer answers')
        for(const [i,lines] of answered.entries())for(const line of lines.filter(line=>/^a=(mid|rtpmap|extmap):/.test(line)))assert.ok(offered[i].includes(line),`the page's offer no longer carries "${line}", which the recorded station answer uses: record the pair again (fixtures/stream/README.md)`)
        assert.deepEqual(await evaluate('window.__descriptions'),[{type:'answer',ok:true}],'Chrome took the station\'s own answer')
        await sleep(1000)
        assert.equal(closes.length,closesBefore,'the page refused nothing: no close went to the station')
        assert.equal(await evaluate(`document.querySelector('.remote-stream-app')?.dataset.streamPhase`),'connecting','the page waits for the connection')
        // Nothing can connect to a recording, so the station ends the attempt as it ends one that never connects.
        realAnswer=null
        station.send({type:'streamState',sessionId:streamSession,streaming:false,reason:'connectionFailed'})
        await until(`document.querySelector('.remote-stream-app')?.dataset.streamPhase==='ended'`,5000)
        // POSITIVE CONTROL for the CSP listener above: a request the policy forbids is reported, so the
        // empty list really meant "nothing violated", not "nothing was listening".
        await evaluate(`fetch('https://example.invalid/').catch(()=>{});true`)
        for(let i=0;i<30&&!(await evaluate('window.__csp.length'));i++)await sleep(100)
        assert.ok((await evaluate('window.__csp')).some(v=>v.startsWith('connect-src')),'control: a forbidden fetch is reported as a connect-src violation')
        // And the console half sees what that event cannot: a worklet module made from a blob:, refused.
        const seen=refused.length,blobRefused=()=>refused.slice(seen).some(text=>text.includes("'blob:"))
        await evaluate(`new AudioContext().audioWorklet.addModule(URL.createObjectURL(new Blob([''],{type:'text/javascript'}))).catch(()=>{});true`)
        for(let i=0;i<30&&!blobRefused();i++)await sleep(100)
        assert.ok(blobRefused(),'control: a blob: worklet module is reported as refused: '+JSON.stringify(refused.slice(seen)))
        assert.equal(exceptions,0,'the stream view raised no runtime exception')
        assert.equal(unexpectedMessages,0,'only reviewed messages left the browser socket')
        console.log(`Compiled browser stream: offer under the held lease, live VP8 picture, heartbeat with decodedFrameAt, pointer and key input, held PTT (${holds.length} holds, and through a lapse of the state ${throughLapse.holds.length} by the mouse and ${spaceThroughLapse.holds.length} by Space), a held key and a held button re-asserted (${keyHeld.length} and ${buttonHeld.length}), Stop both ways, Stop TX reachable at 12 layouts, "Still there?" at 15:00 with Stop TX unmoved and reachable at 12 layouts and its press reaching nothing at the shack, the idle end at 16:00 as close and release, no CSP violation`)
      }finally{
        shackLive=false
        await Promise.allSettled([signalling,trickle])
      }
    }
  } catch(error) {
    console.error('Compiled browser primary failure',error)
    throw error
  } finally {
    await stop()
  }
})
