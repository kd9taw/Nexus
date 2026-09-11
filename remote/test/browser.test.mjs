// Compiled browser + actual Auth0 SDK + workerd. The simulated issuer grants one
// ephemeral authorization code only after checking the SDK's PKCE verifier.
// It is a test provider, not evidence of a live Auth0 tenant or physical phone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, writeFile, mkdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash, randomBytes } from 'node:crypto'
import WebSocket from 'ws'
import { runtime } from './runtime.mjs'
import { applicationFixture, collectionFixture, recallFixture } from './application-fixture.mjs'
import { stationModesFixture } from './station-modes-fixture.mjs'
import { navigationFixture } from './navigation-fixture.mjs'
import { tempoConversations } from './tempo-fixture.mjs'

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
async function chrome() {
  const profile = await mkdtemp(join(tmpdir(), 'nexus-remote-browser-'))
  const child = spawn(process.env.CHROME_BIN || 'google-chrome', [
    '--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage',
    '--disable-background-networking', '--no-first-run', '--no-default-browser-check',
    '--no-proxy-server', '--disable-extensions', '--disable-default-apps',
    // Isolate synthetic browser cookies from a locked desktop credential store.
    '--password-store=basic', '--use-mock-keychain',
    '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank',
  ], { stdio: 'ignore', detached: true })
  let exited = false
  child.once('error', () => { exited = true })
  const exit = new Promise(resolve => child.once('exit', () => { exited = true; resolve() }))
  let ws
  try {
    let endpoint
    // A cold hosted runner can take longer than five seconds to launch Chrome.
    // Bound process startup separately from the browser's application assertions.
    const deadline = performance.now() + 30_000
    while (performance.now() < deadline) {
      if (exited) throw new Error('Chrome could not start')
      try { const [port,path] = (await readFile(join(profile,'DevToolsActivePort'),'utf8')).split('\n'); endpoint=`ws://127.0.0.1:${port}${path}`; break } catch {}
      await sleep(50)
    }
    assert.ok(endpoint, 'Chrome debugging endpoint must start within 30 seconds')
    ws = new WebSocket(endpoint)
    await new Promise((resolve,reject) => { ws.once('open',resolve); ws.once('error',()=>reject(new Error('Chrome debugging connection failed'))) })
    const pending = new Map(), listeners = new Map()
    let next=0
    ws.on('message', bytes => {
      const value=JSON.parse(bytes)
      if (value.id) { const promise=pending.get(value.id); if(promise){pending.delete(value.id);clearTimeout(promise.timer);value.error?promise.reject(new Error(`Browser protocol command failed: ${promise.method} (${value.error.code}): ${value.error.message}`)):promise.resolve(value.result)} }
      else listeners.get(value.method)?.(value.params, value.sessionId)
    })
    const call = (method,params={},sessionId) => new Promise((resolve,reject) => {
      const id=++next, timer=setTimeout(()=>{pending.delete(id);reject(new Error(`Browser command timed out: ${method}`))},10000)
      pending.set(id,{resolve,reject,timer,method});ws.send(JSON.stringify({id,method,params,sessionId}))
    })
    return { call, on: (method, fn) => listeners.set(method,fn), async stop() {
      await call('Browser.close').catch(()=>{}); ws.close()
      await Promise.race([exit,sleep(2000)])
      if(!exited){process.kill(-child.pid,'SIGTERM');await Promise.race([exit,sleep(2000)])}
      if(!exited)process.kill(-child.pid,'SIGKILL')
      await rm(profile,{recursive:true,force:true,maxRetries:8,retryDelay:100})
    } }
  } catch(error) { ws?.close(); if(!exited)process.kill(-child.pid,'SIGTERM'); await rm(profile,{recursive:true,force:true,maxRetries:8,retryDelay:100}); throw error }
}

for (const {applicationVersion,operating,sessionLayout,quickLayout,quickMode='phone',contactContinuity} of [...[1,2,3,4,5,6,7,8,9,10,11,12,13,14].map(applicationVersion=>({applicationVersion,operating:false})),{applicationVersion:14,operating:true},{applicationVersion:14,operating:true,sessionLayout:true},{applicationVersion:14,operating:true,quickLayout:true},{applicationVersion:14,operating:true,quickLayout:true,quickMode:'cw'},{applicationVersion:14,operating:true,contactContinuity:true}]) test(`compiled hosted browser ${contactContinuity?'contact continuity':quickLayout?`quick layout${quickMode==='cw'?' CW':''}`:sessionLayout?'session layout':operating?'operations':`v${applicationVersion}`} completes PKCE, local device approval, observation and viewport checks`, { timeout: applicationVersion >= 14 ? 540000 : applicationVersion >= 13 ? 420000 : 180000 }, async () => {
  const app=await runtime(), artifacts=process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS ? join(process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS, contactContinuity?'contact-continuity':quickLayout?`quick-layout${quickMode==='cw'?'-cw':''}`:sessionLayout?'session-layout':operating?'operations':`v${applicationVersion}`) : undefined
  let browser, station, producing=true, pauseObservations=false, producer, applicationProducer
  const results=[]
  try {
    browser=await chrome()
    if(artifacts){await mkdir(artifacts,{recursive:true});await writeFile(join(artifacts,'chrome-version.json'),JSON.stringify(await browser.call('Browser.getVersion'),null,2)+'\n')}
    const pair=await app.paired(), subject=JSON.parse(Buffer.from(pair.browser.jwt.split('.')[1],'base64url')).sub
    const shell = await fetch(app.origin,{signal:AbortSignal.timeout(3000)})
    assert.equal(shell.status,200)
    assert.match(await shell.text(), /Nexus Remote/)
    const stationHeaders = { 'x-nexus-application-version': '1',...(operating?{'x-nexus-operation-version':'2','x-nexus-operation-max-version':'3'}:{}), ...(applicationVersion >= 2 ? { 'x-nexus-application-stream-version': '2' } : {}), ...(applicationVersion >= 3 ? { 'x-nexus-application-query-version': '1' } : {}), ...(applicationVersion >= 4 ? { 'x-nexus-application-recall-version': '1' } : {}), ...(applicationVersion >= 5 ? { 'x-nexus-application-keyboard-version': '1' } : {}), ...(applicationVersion >= 6 ? { 'x-nexus-application-insights-version': '1' } : {}), ...(applicationVersion >= 7 ? { 'x-nexus-application-dxpeditions-version': '1' } : {}), ...(applicationVersion >= 8 ? { 'x-nexus-application-memories-version': '1' } : {}), ...(applicationVersion >= 9 ? { 'x-nexus-application-ota-version': '1' } : {}), ...(applicationVersion >= 10 ? { 'x-nexus-application-field-day-version': '1' } : {}), ...(applicationVersion >= 11 ? { 'x-nexus-application-js8-version': '1' } : {}), ...(applicationVersion >= 12 ? {'x-nexus-application-station-modes-version':'1'} : {}), ...(applicationVersion >= 13 ? {'x-nexus-application-navigation-version':'1'} : {}), ...(applicationVersion >= 14 ? {'x-nexus-application-configuration-version':'1'} : {}) }
    station=await pair.native.open(pair.stationId, undefined, 101, stationHeaders)
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
    let receiverRead=null
    browser.on('Network.webSocketFrameReceived',event=>{try{const v=JSON.parse(event.response.payloadData);if(v.type==='observation')receiverRead={at:performance.now(),radio:v.frame?.station?.radio};if(v.type==='operationResponse')operationWire.push({at:performance.now(),direction:'in',requestId:v.requestId,error:v.error,phase:v.value?.phase})}catch{}})
    browser.on('Network.webSocketFrameSent', event => {
      if (event.response.opcode !== 1) return
      try {
        const message = JSON.parse(event.response.payloadData)
        if (message.type === 'ack' && Object.keys(message).sort().join(',') === 'epoch,sequence,type') acknowledgements++
        else if (message.type === 'applicationHello' && (Object.keys(message).length === 1 || (Object.keys(message).length === 2 && [2,3,4,5,6,7,8,9,10,11,12,13,14].includes(message.version)))) {}
        else if(message.type==='operationRequest'&&((Object.keys(message).length===2)||Object.keys(message).length===3&&[2,3].includes(message.operationVersion))&&['state','acquire','heartbeat','release','result','logManual','stationControl'].includes(message.request?.type)){if(!operating)assert.equal(message.request.type,'state');operationWire.push({at:performance.now(),direction:'out',type:message.request.type,requestId:message.request.requestId})}
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
      window.__socketClosures=[];window.__protocolTrace=[];const originalWebSocket=window.WebSocket;
      window.WebSocket=class extends originalWebSocket{
        constructor(...args){super(...args);this.addEventListener('close',e=>window.__socketClosures.push({at:performance.now(),code:e.code,reason:e.reason}));this.addEventListener('message',e=>{try{const v=JSON.parse(e.data);if(v.type==='applicationFrame'){window.__protocolTrace.push({at:performance.now(),updates:v.updates.map(u=>({command:u.command,type:u.type,age:u.ageMs,revision:u.revision}))});window.__protocolTrace=window.__protocolTrace.slice(-40)}}catch{}})}
        send(raw){try{const v=JSON.parse(raw);window.__protocolTrace.push({at:performance.now(),out:v.type,requestId:v.requestId,topics:v.topics,buffered:this.bufferedAmount});window.__protocolTrace=window.__protocolTrace.slice(-80)}catch{}return super.send(raw)}
        close(...args){window.__socketClosures.push({at:performance.now(),requested:true,code:args[0],reason:args[1],buffered:this.bufferedAmount});return super.close(...args)}
      };
      window.__imagePolicyViolations=[];window.addEventListener('securitypolicyviolation',e=>{if(e.violatedDirective.startsWith('img-src'))window.__imagePolicyViolations.push({directive:e.violatedDirective,scheme:e.blockedURI.split(':')[0]})});
      window.__frameDelay=0;
      const originalRaf=window.requestAnimationFrame.bind(window),originalCancel=window.cancelAnimationFrame.bind(window),delayedFrames=new Map();
      window.requestAnimationFrame=callback=>{const id=originalRaf(time=>{if(!window.__frameDelay){callback(time);return}delayedFrames.set(id,setTimeout(()=>{delayedFrames.delete(id);callback(performance.now())},window.__frameDelay))});return id};
      window.cancelAnimationFrame=id=>{originalCancel(id);clearTimeout(delayedFrames.get(id));delayedFrames.delete(id)};
      window.__keyboardInset=0;const actual=visualViewport;const area=new EventTarget();
      Object.defineProperties(area,{width:{get:()=>actual.width},height:{get:()=>actual.height-window.__keyboardInset}});
      actual.addEventListener('resize',()=>area.dispatchEvent(new Event('resize')));
      Object.defineProperty(window,'visualViewport',{value:area});
      window.__waterfallDraws=0;const draw=CanvasRenderingContext2D.prototype.putImageData;
      CanvasRenderingContext2D.prototype.putImageData=function(...args){if(this.canvas.classList.contains('waterfall-canvas'))window.__waterfallDraws++;return draw.apply(this,args)};
    }`},session)
    const evaluate=async expression=>{
      const value=await browser.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},session)
      if(value.exceptionDetails)throw new Error('Browser evaluation failed: '+String(value.exceptionDetails.exception?.description??value.exceptionDetails.text).split('\n')[0])
      return value.result?.value
    }
    // useViewport publishes dimensions on an animation frame. A fixed sleep can
    // measure the previous viewport on a busy renderer. Wait through the update
    // and its layout frame; all pixel/overflow assertions below stay unchanged.
    const settledLayout=()=>evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))')
    const sessionDiagnostic=()=>evaluate(`(()=>{const e=document.querySelector('.app');let fiber=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))],client;while(fiber){client=fiber.memoizedProps?.connection?.application;if(client)break;fiber=fiber.return}return {now:performance.now(),stale:e?.dataset.remoteStale,phase:client?.getPhase(),snapshotAge:client?.age('get_snapshot'),topics:client?.stream?.topics,waiting:client?.stream?.waiting?[...client.stream.waiting.keys()]:null,interests:client?.stream?.interests?[...client.stream.interests].map(([name,at])=>({name,age:performance.now()-at})):null,closures:window.__socketClosures,trace:window.__protocolTrace}})()`)
    async function until(expression,timeout=12000) { for(let i=0;i<Math.ceil(timeout/100);i++){ if(providerFailure)throw new Error('Simulated provider failed'); if(await evaluate(expression))return;await sleep(100) } if(operating)console.log('Operation diagnostic',expression,operationWire.slice(-30),loggedRequests.map(r=>({call:r.record.call,mode:r.record.mode})),await evaluate(`({status:document.querySelector('.remote-application-status')?.textContent,entries:[...document.querySelectorAll('.remote-log-entry')].map(e=>({text:e.textContent,error:e.dataset.operationError}))})`));throw new Error('Expected browser state did not appear') }
    const button=name=>`[...document.querySelectorAll('button')].find(e=>e.textContent===${JSON.stringify(name)})`
    async function click(expression) {
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
      catch(error){console.log('Application session diagnostic',JSON.stringify(await sessionDiagnostic()),{...applicationTraffic,nativeSnapshotAge:performance.now()-(sentAt.get('get_snapshot')??0),stationClosed:station.closed});console.log('Click diagnostic',expression,await evaluate(`(()=>{const e=${expression},r=e?.getBoundingClientRect();return {stale:document.querySelector('.app')?.dataset.remoteStale,status:document.querySelector('.remote-application-status')?.textContent,rect:r?.toJSON(),hit:r?document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.outerHTML.slice(0,600):null}})()`));if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-click-failure.png'),Buffer.from(shot.data,'base64'))}throw error}
      await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1},session)
      await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1},session)
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
    await until(`!!${button('Sign in or create an account')}`)
    await geometry(390,844)
    await browser.call('Fetch.enable',{patterns:[{urlPattern:'https://identity.remote-test.invalid/*'}]},session)
    await click(button('Sign in or create an account'))
    await until(`document.body.textContent.includes(${JSON.stringify(pair.browser.accountId)})`)
    assert.equal(exchanges,1,'the actual SDK must exchange a PKCE authorization code')
    await until(`!!document.querySelector('input')`)
    await click(`document.querySelector('input')`)
    await browser.call('Input.insertText',{text:'Synthetic browser'},session)
    await click(button('Request local approval'))
    await until(`document.body.textContent.includes('Approve this browser') || document.body.textContent.includes('Waiting for approval') || !document.querySelector('input')`)
    const device=await app.db.prepare('SELECT id FROM devices WHERE station_id=?').bind(pair.stationId).first()
    assert.ok(device,'the browser must enroll through its own HTTP-only cookie')
    // Deterministically exercise a frame that arrives after the former sleep.
    await evaluate('window.__frameDelay=120')
    await geometry(360,740,1.75,'light',260)
    await evaluate('window.__frameDelay=0')
    await pair.native.post(`stations/${pair.stationId}/native/approve-device`,{deviceId:device.id})
    await until(`!!${button('Observe station')}`)
    for(const [w,h] of [[360,740],[390,844],[844,390],[1024,768],[1280,800],[1366,768],[3440,1440]])for(const theme of ['dark','light'])await geometry(w,h,1,theme)
    if(artifacts){await mkdir(artifacts,{recursive:true});await geometry(390,844);const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-account.png'),Buffer.from(shot.data,'base64'))}
    const fixture=JSON.parse(await readFile(new URL('../../ui/src/remote-monitor/fixtures.v2.json',import.meta.url),'utf8')).spe
    let sequence=0
    producer=(async()=>{while(producing){const source=station;let watch;try{watch=await source.take(value=>value.type==='watch'&&value.enabled,1000)}catch{continue}await sleep(200);while(producing&&pauseObservations)await sleep(50);if(!producing)break;if(source!==station||source.closed)continue;source.send({type:'publication',requestId:watch.requestId,frame:{...fixture,source:'native',sequence:++sequence}})}})()
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
    applicationData.get_remote_satellite_state=navigation.live
    const stationModes=await stationModesFixture()
    applicationData.get_sstv_state=stationModes.sstv;applicationData.get_remote_aprs_state=stationModes.aprs
    collections.aprs=stationModes.roster
    const js8Fixture=JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/js8.json',import.meta.url),'utf8'))
    const js8Start=Date.now(), js8Delta=js8Start-js8Fixture.capturedAtMs
    for(const row of js8Fixture.state.activity)row.atMs+=js8Delta
    for(const row of js8Fixture.state.stations)row.lastMs+=js8Delta
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
    let loggingLeaseUntil=0
    let loggingAllowed=false,loggingLease=null,loggingRevision=1,loggingSequence=0,loseLogReply=false
    let stationControls=false
    const stationRequests=[]
    let lastDxTier='FT8',lastMsgTier='TempoFast'
    if(operating)applicationData.get_snapshot.mode='qso'
    const loggingBoot=crypto.randomUUID(),loggingWindow=crypto.randomUUID(),loggedRequests=[],loggingReceipts=new Map()
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
        if(r.type==='logManual'){
          assert.ok(loggingAllowed&&loggingLease&&r.leaseId===loggingLease)
          assert.equal(r.expectedRevision,loggingRevision);assert.equal(r.clientSequence,loggingSequence+1);loggingSequence++
          loggedRequests.push(r);value={outcome:'applied',evidence:'fileSynced',uploads:'stationPipeline',operationId:r.requestId};loggingReceipts.set(r.requestId,value);loggingRevision++
          if(loseLogReply)continue
        }else if(r.type==='stationControl'){
          assert.equal(request.operationVersion,3);assert.ok(stationControls&&loggingLease&&r.leaseId===loggingLease)
          assert.equal(r.expectedRevision,loggingRevision);assert.equal(r.clientSequence,loggingSequence+1);loggingSequence++
          assert.deepEqual(r.context,{radioId:1,radioConnection:1,ampConnection:1,ampReadSequence:1})
          stationRequests.push(r);const a=r.action
          if(a.action==='decoder.arm'){
            if(a.receiver==='sstv'){applicationData.get_sstv_state.state.armed=a.on;applicationData.get_sstv_state.state.health.armed=a.on}
            else if(a.receiver==='aprs')applicationData.get_remote_aprs_state.health.arm=a.on?'auto':'off'
            else applicationData[`get_${a.receiver}_state`].armed=a.on
          }else if(a.action==='radio.frequency'){
            assert.ok(a.dialMhz>0);assert.equal(a.band,a.dialMhz===10?'':'40m');assert.ok(['USB','LSB'].includes(a.sideband))
            applicationData.get_snapshot.radio.dialMhz=a.dialMhz;applicationData.get_snapshot.radio.band=a.band;applicationData.get_snapshot.radio.sideband=a.sideband
          }else if(a.action==='radio.phoneMode'){
            const radio=applicationData.get_snapshot.radio
            assert.equal(radio.operatingMode,'phone');assert.equal(a.expectedMode,radio.sidebandOverride??'auto')
            assert.ok(['auto','USB','LSB','AM'].includes(a.mode));assert.equal(radio.txEnabled,false);assert.equal(radio.rigKeyed,false)
            radio.sidebandOverride=a.mode==='auto'?null:a.mode
            radio.rigMode=a.mode==='auto'?(radio.dialMhz<10?'LSB':'USB'):a.mode
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
            assert.equal(snap.radio.txEnabled,false)
          }else if(a.action==='radio.tier'){
            assert.ok(['FT8','FT4','FT2','WSPR','Q65','MSK144','JT65','FST4','FST4W','TempoFast','TempoDeep'].includes(a.tier))
            assert.equal(applicationData.get_snapshot.radio.operatingMode,'digital')
            // The provider proves the real UI/relay gesture and later snapshot.
            // Native engine/worker tests separately prove channel/source policy.
            applicationData.get_snapshot.link.tier=a.tier
            if(a.tier==='MSK144')applicationData.get_snapshot.link.periodSecs=15
            assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
          }else if(a.action==='decoder.js8Speed'){
            const speeds=['slow','normal','fast','turbo'];assert.equal(applicationData.get_snapshot.link.tier,'JS8')
            assert.equal(a.expectedSpeed,speeds.indexOf(js8Fixture.state.speed));assert.notEqual(a.expectedSpeed,a.speed)
            js8Fixture.state.speed=speeds[a.speed];applicationData.get_snapshot.link.periodSecs=[30,15,10,6][a.speed]
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
          value={operation:'stationControl',operationId:r.requestId,outcome:'applied',evidence:['amplifier.followBand','decoder.js8Speed','decoder.msk144Period','decoder.depth','receiver.rxOffset','receiver.rxGain'].includes(a.action)?'settingsSaved':a.action.startsWith('radio.')?'radioReadback':a.action.startsWith('amplifier.')?'amplifierReadback':'receiverState'}
          loggingReceipts.set(r.requestId,value);loggingRevision++;applicationRevision++
        }else if(r.type==='result'){value=loggingReceipts.get(r.operationId);if(!value)error='resultExpired'}
        else value={stationBootId:loggingBoot,allowed:loggingAllowed||stationControls,phase:loggingLease?'controlling':loggingAllowed||stationControls?'available':'localPermissionRequired',leaseId:loggingLease,revision:loggingRevision,commandWindowId:loggingLease?loggingWindow:null,nextSequence:loggingLease?loggingSequence+1:null,leaseRemainingMs:loggingLease?5000:null,actions:loggingAllowed?['log.manual']:[],txArmed:false,...(stationControls?{controls:{context:{radioId:1,radioConnection:1,ampConnection:1,ampReadSequence:1},capabilities:request.operationVersion>=3?['decoder','amplifier','frequency','mode','tier','ampFollowBand','workspace','decoderSettings','receiverSettings','receiverGain','bandSelection','receiverFilter','receiverDsp','phoneMode']:['decoder','amplifier']}}:{})}
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
    await geometry(390,844)
    await click(button('Observe station'))
    await until(`document.querySelector('.rm-frequency')?.textContent.includes('14.074000')`)
    await click(`document.querySelector('.rm-amp-strip')`)
    assert.ok(await evaluate(`document.querySelector('.rm-amp-details').textContent.includes('41°')`))
    // Positive control against the rendered artifact: an actual over-wide child
    // must make the same geometry check fail before it is restored.
    await evaluate(`document.querySelector('.rm-amplifier').style.minWidth='2000px'`)
    await assert.rejects(geometry(390,844),/viewport/)
    await evaluate(`document.querySelector('.rm-amplifier').style.removeProperty('min-width')`)
    for(const [w,h] of [[360,740],[390,844],[430,932],[844,390],[768,1024],[1024,768],[1280,800],[1366,768],[1200,1390],[3440,1440]])for(const zoom of [1,1.75])for(const theme of ['dark','light'])await geometry(w,h,zoom,theme)
    await geometry(390,844,1.75,'light',300)
    await click(button('Disconnect and return to stations'))
    await until(`!!${button('Observe station')}`)
    await geometry(390,844)
    await click(button('Observe station'))
    await until(`document.querySelector('.rm-frequency')?.textContent.includes('14.074000')`)
    await click(`document.querySelector('.rm-amp-strip')`)
    if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-observer.png'),Buffer.from(shot.data,'base64'))}
    await click(button('Disconnect and return to stations'))
    await until(`!!${button('Open Nexus')}`)
    await geometry(1280,800)
    await click(button('Open Nexus'))
    await until(`!!document.querySelector('.operate-host:not([hidden]) .amp-strip')`)
    await until(`document.querySelector('.amp-strip')?.textContent.includes(${JSON.stringify(fixture.station.amplifier.bandLabel)})`)
    assert.equal(await evaluate(`!!window.__TAURI_INTERNALS__ || !!window.__TAURI__`),false,'the browser must use its explicit adapter')
    assert.equal(await evaluate(`document.querySelectorAll('.app').length`),1,'the real workspace has one app root')
    assert.ok(await evaluate(`document.querySelector('.amp-strip')?.textContent.includes(${JSON.stringify(fixture.station.amplifier.bandLabel)})`),'the existing amp strip must show the current station observation')
    assert.ok(await evaluate(`[...document.querySelectorAll('.amp-strip button')].every(button=>button.disabled)`),'observer amp controls must visibly refuse operating authority')
    await until(`window.__waterfallDraws > 2`)
    assert.ok(await evaluate(`[...document.querySelectorAll('.cockpit-qso button, .tuning-nudge, .cockpit-mode, .tier-btn, .cs-opt, .ph-split button')].every(button=>button.disabled)`),'station controls in the existing workspace must show observer authority')
    await click(button('Tempo'))
    await until(`!!document.querySelector('.empty-conv .cq-btn')`)
    assert.ok(await evaluate(`[...document.querySelectorAll('.empty-conv button')].length>3 && [...document.querySelectorAll('.empty-conv button')].every(e=>e.disabled)`))
    await click(button('FT'))
    if (applicationVersion < 9) {
      await click(button('POTA/SOTA'))
      await until(`!!document.querySelector('.remote-view-unavailable')`)
      assert.equal(otaQueries.length,0)
      await click(button('FT'))
    }
    if (applicationVersion < 10) {
      await click(button('Field Day'))
      await until(`!!document.querySelector('.remote-view-unavailable')`)
      assert.equal(await evaluate(`document.body.textContent.includes('Could not switch mode')`), false)
      assert.equal(fieldDayQueries.length,0)
      await click(button('FT'))
    }
    if(quickLayout){
      loggingAllowed=true;stationControls=true
      await until(`!!${button('Take station control')}`);await click(button('Take station control'))
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('Station control active')`)
      await click(button(quickMode==='cw'?'CW':'Phone'))
      const call=`.${quickMode}-cockpit .remote-log-entry .le-call`
      await until(`!!document.querySelector('${call}')`);await click(`document.querySelector('${call}')`)
      await browser.call('Input.insertText',{text:'N2QUICK'},session)
      const lease=loggingLease
      await evaluate(`void(window.__quickNodes={app:document.querySelector('.app'),cockpit:document.querySelector('.${quickMode}-cockpit'),call:document.querySelector('${call}')})`)
      assert.equal(await evaluate(`document.querySelector('.app').dataset.remotePresentation??'full'`),'full','a new browser starts with the full Nexus interface')
      await click(`document.querySelector('.remote-session-toggle')`)
      await until(`!!${button('Quick Operate')}`)
      await click(button('Quick Operate'))
      await until(`document.querySelector('.app')?.dataset.remotePresentation==='quick'`)
      const navButton=label=>`[...document.querySelectorAll('.remote-quick-nav button')].find(e=>e.textContent===${JSON.stringify(label)})`
      const checks=[]
      for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
        await evaluate(`document.querySelector('.app').scrollTop=0;document.querySelector('.shell').scrollTop=0;document.querySelector('.${quickMode}-cockpit').scrollTop=0`);await settledLayout()
        const shape=await evaluate(`(()=>{const e=document.querySelector('${call}'),r=e.getBoundingClientRect(),nav=document.querySelector('.remote-quick-nav'),buttons=[...nav.querySelectorAll('button')];return {call:r.toJSON(),nav:nav.getBoundingClientRect().toJSON(),buttons:buttons.map(b=>{const r=b.getBoundingClientRect();return {name:b.getAttribute('aria-label')||b.textContent,visible:r.width>0&&r.height>0&&b.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))}}),value:e.value,sameApp:document.querySelector('.app')===window.__quickNodes.app,sameCockpit:document.querySelector('.${quickMode}-cockpit')===window.__quickNodes.cockpit,sameInput:e===window.__quickNodes.call,horizontal:[...document.querySelectorAll('.${quickMode}-cockpit, .${quickMode}-cockpit *')].filter(e=>e.clientWidth>0&&e.scrollWidth>e.clientWidth+1&&/auto|scroll/.test(getComputedStyle(e).overflowX)).map(e=>({class:e.className,width:e.clientWidth,scrollWidth:e.scrollWidth})),vertical:[...document.querySelectorAll('.${quickMode}-cockpit, .${quickMode}-cockpit *')].filter(e=>e.clientHeight>0&&e.scrollHeight>e.clientHeight+1&&/auto|scroll/.test(getComputedStyle(e).overflowY)).map(e=>({class:e.className,height:e.clientHeight,scrollHeight:e.scrollHeight})),header:[...document.querySelector('.${quickMode}-cockpit .cockpit-header').children].map(e=>({class:e.className,text:e.textContent,rect:e.getBoundingClientRect().toJSON()})),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight}})()`)
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`quick-${width}-${zoom}-${theme}.png`),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,`quick-${width}-${zoom}-${theme}.json`),JSON.stringify(shape,null,2))}
        assert.ok(shape.sameApp&&shape.sameCockpit&&shape.sameInput&&shape.value==='N2QUICK','changing presentation must preserve the actual contact form and draft')
        assert.ok(shape.buttons.every(b=>b.visible)&&shape.docW<=width+1&&shape.docH<=height+1,'all Quick destinations and return to Full Nexus must be reachable')
        assert.ok(shape.call.top>=0&&shape.call.bottom<shape.nav.top,'the selected contact must appear before optional radio detail')
        assert.deepEqual(shape.horizontal,[],'Quick contact controls and recall must fit without sideways scrolling')
        assert.equal(loggingLease,lease);assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
        checks.push({width,height,zoom,theme,shape})
      }
      // Receiver detail changes the presentation of the SAME scopes/forms.
      // Start and stop its actual native topic before testing further gestures.
      const waitScope=async present=>{
        for(let attempt=0;attempt<100;attempt++){
          if((streamWatch?.topics.includes('get_scope_snapshot')??false)===present)return
          await sleep(100)
        }
        assert.fail(`Quick scope demand did not become ${present}`)
      }
      await waitScope(false)
      await click(`document.querySelector('.${quickMode}-cockpit .remote-quick-details')`)
      await until(`document.querySelector('.${quickMode}-cockpit .remote-quick-details')?.getAttribute('aria-expanded')==='true'`)
      await waitScope(true)
      const scope=`document.querySelector('.${quickMode}-cockpit .ph-scope canvas')`
      await evaluate(`${scope}.scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
      assert.equal(await evaluate(`(()=>{const r=${scope}.getBoundingClientRect();return r.width>0&&r.height>0})()`),true)
      await click(`document.querySelector('.${quickMode}-cockpit .remote-quick-details')`)
      await until(`document.querySelector('.${quickMode}-cockpit .remote-quick-details')?.getAttribute('aria-expanded')==='false'`)
      await waitScope(false)
      for(const [destination,view]of [['Hunt','needed'],['Log','logbook']]){
        await click(navButton(destination));await until(`document.querySelector('.app').dataset.remoteView==='${view}'`)
        assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N2QUICK'`),true)
        await click(navButton('Operate'));await until(`document.querySelector('.app').dataset.remoteView==='${quickMode}'`)
        assert.equal(loggingLease,lease);assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
      }
      await click(navButton('Full Nexus'))
      await until(`document.querySelector('.app')?.dataset.remotePresentation==='full'`)
      assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N2QUICK'`),true)
      assert.equal(loggingLease,lease);assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
      await click(`document.querySelector('.remote-session-toggle')`);await click(button('Quick Operate'))
      await until(`document.querySelector('.app')?.dataset.remotePresentation==='quick'`)
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`);await settledLayout()
      const freshWindow=async()=>{
        const after=performance.now()
        for(let attempt=0;attempt<100;attempt++){
          const reply=operationWire.findLast(v=>v.direction==='in'&&v.phase==='controlling')
          if(reply&&reply.at>after&&performance.now()-reply.at<150)return
          await sleep(50)
        }
        assert.fail('Quick gestures need a fresh native control window')
      }
      for(const operate of [true,false]){
        await freshWindow();await click(`document.querySelector('.${quickMode}-cockpit .amp-op')`)
        await until(`document.querySelector('.${quickMode}-cockpit .amp-op')?.classList.contains('on')===${operate}`)
        assert.deepEqual(stationRequests[stationRequests.length-1].action,{action:'amplifier.operate',expectedOperate:!operate,operate})
      }
      assert.equal(stationRequests.length,2);assert.equal(loggedRequests.length,0)
      // A lost append reply must survive Full/Quick switches without replay.
      loseLogReply=true
      await freshWindow();await click(`document.querySelector('.${quickMode}-cockpit .le-log-btn')`)
      await until(`document.querySelector('.${quickMode}-cockpit .remote-log-entry')?.textContent.includes('may already be logged')`,12000)
      assert.equal(loggedRequests.length,1);assert.equal(loggedRequests[0].record.call,'N2QUICK')
      await click(navButton('Full Nexus'));await until(`document.querySelector('.app').dataset.remotePresentation==='full'`)
      assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N2QUICK'`),true)
      await click(`document.querySelector('.remote-session-toggle')`);await click(button('Quick Operate'))
      await until(`document.querySelector('.app').dataset.remotePresentation==='quick'`)
      assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N2QUICK'`),true)
      await sleep(1000);assert.equal(loggedRequests.length,1,'presentation must not replay an unconfirmed QSO')
      loseLogReply=false
      await evaluate(`(()=>{window.__quickResultEvents=[];for(const type of ['pointerdown','pointerup','click'])document.addEventListener(type,e=>{window.__quickResultEvents.push({type,target:e.target?.outerHTML,at:performance.now(),x:e.clientX,y:e.clientY})},{capture:true})})()`)
      await click(`[...document.querySelectorAll('.${quickMode}-cockpit .remote-log-entry button')].find(e=>e.textContent==='Check submitted QSO result')`)
      if(artifacts)await writeFile(join(artifacts,'result-click.json'),JSON.stringify({events:await evaluate('window.__quickResultEvents'),wire:operationWire.slice(-8),session:await sessionDiagnostic()},null,2))
      await until(`document.querySelector('.${quickMode}-cockpit .remote-log-entry')?.textContent.includes('QSO saved to the station log file')`)
      assert.equal(loggedRequests.length,1);assert.equal(await evaluate(`document.querySelector('${call}').value`),'')
      await click(`document.querySelector('${call}')`);await browser.call('Input.insertText',{text:'N3QSO'},session)
      applicationAvailable=false
      await until(`document.querySelector('.app').dataset.remoteStale==='true'`)
      assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N3QSO'&&document.querySelector('.${quickMode}-cockpit .le-log-btn').disabled&&document.querySelector('.${quickMode}-cockpit .amp-op').disabled`),true)
      applicationAvailable=true
      await until(`document.querySelector('.app').dataset.remoteStale!=='true'`)
      await until(`!!${button('Take station control')}`)
      assert.equal(loggingLease,null,'data recovery must not restore station authority')
      assert.equal(await evaluate(`document.querySelector('${call}')===window.__quickNodes.call&&document.querySelector('${call}').value==='N3QSO'`),true)
      assert.equal(loggedRequests.length,1);assert.equal(stationRequests.length,2)
      if(artifacts)await writeFile(join(artifacts,'quick-results.json'),JSON.stringify({mode:quickMode,checks,stationActions:stationRequests.map(r=>r.action),logWrites:loggedRequests.length,presentationActions:0,leasePreservedBeforeLoss:true,reacquireRequired:true,scopeDemand:true,huntLogDrafts:true,unknownResultPreserved:true,explicitResultCheck:true,exceptions,unexpectedMessages},null,2))
      assert.equal(exceptions,0);assert.equal(unexpectedMessages,0)
      console.log('Compiled Quick '+quickMode+' presentation: eight layouts, retained drafts, hidden display retirement, native amp readbacks, one QSO receipt and explicit recovery passed');return
    }
    if(contactContinuity){
      loggingAllowed=true;stationControls=true
      await until(`!!${button('Take station control')}`);await click(button('Take station control'))
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('Station control active')`)
      const lease=loggingLease,checks=[]
      const waitTopics=async (predicate,message)=>{
        for(let attempt=0;attempt<100;attempt++){
          if(predicate(streamWatch?.topics??[]))return
          await sleep(100)
        }
        assert.fail(`${message}: ${JSON.stringify(streamWatch?.topics)}`)
      }
      for(const [label,mode,call]of [['Phone','phone','N2CONTACT'],['CW','cw','N3CONTACT']]){
        await click(button(label))
        const selector=`.${mode}-cockpit .remote-log-entry .le-call`
        await until(`!!document.querySelector('${selector}')`)
        await click(`document.querySelector('${selector}')`)
        await browser.call('Input.insertText',{text:call},session)
        assert.equal(await evaluate(`document.querySelector('${selector}').value`),call)
        await evaluate(`void(window.__contactNodes={...(window.__contactNodes??{}),${mode}:document.querySelector('${selector}')})`)
        await waitTopics(topics=>topics.includes('get_scope_snapshot')&&(mode!=='cw'||topics.includes('get_cw_state')),'the visible cockpit must first receive its actual display topics')
        for(const destination of ['Needed','Logbook']){
          await click(button(destination));await settledLayout()
          const hidden=await evaluate(`(()=>{const e=document.querySelector('${selector}');return {same:e===window.__contactNodes.${mode},value:e?.value,hidden:!!e&&e.getBoundingClientRect().width===0}})()`)
          assert.ok(hidden.same&&hidden.value===call&&hidden.hidden,`${label} must retain its actual contact form and draft while visiting ${destination}: ${JSON.stringify(hidden)}`)
          await waitTopics(topics=>!topics.includes('get_scope_snapshot')&&!topics.includes('get_cw_state'),'hidden Phone and CW must retire display demand')
          const before={scope:applicationTraffic.byCommand.get_scope_snapshot??0,cw:applicationTraffic.byCommand.get_cw_state??0}
          await sleep(750)
          assert.deepEqual({scope:applicationTraffic.byCommand.get_scope_snapshot??0,cw:applicationTraffic.byCommand.get_cw_state??0},before,'retired scopes and decoder views must stay idle')
          await click(button(label));await settledLayout()
          assert.equal(await evaluate(`(()=>{const e=document.querySelector('${selector}');return e===window.__contactNodes.${mode}&&e.value==='${call}'&&e.getBoundingClientRect().width>0})()`),true,'returning must reveal the same draft')
          await waitTopics(topics=>topics.includes('get_scope_snapshot')&&(mode!=='cw'||topics.includes('get_cw_state')),'returning must resume display updates')
          assert.equal(loggingLease,lease);assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
          checks.push({mode,destination,draftRetained:true,hiddenTopicsRetired:true,resumed:true})
        }
      }
      await click(button('Phone'));await settledLayout()
      assert.equal(await evaluate(`document.querySelector('.phone-cockpit .le-call')===window.__contactNodes.phone&&document.querySelector('.phone-cockpit .le-call').value==='N2CONTACT'&&document.querySelector('.cw-cockpit .le-call')===window.__contactNodes.cw&&document.querySelector('.cw-cockpit .le-call').value==='N3CONTACT'`),true,'Phone and CW must retain independent contact drafts')
      assert.equal(loggingLease,lease);assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
      if(artifacts)await writeFile(join(artifacts,'contact-results.json'),JSON.stringify({checks,stationActions:0,logWrites:0,leasePreserved:true,exceptions,unexpectedMessages},null,2))
      assert.equal(exceptions,0);assert.equal(unexpectedMessages,0)
      console.log('Compiled contact continuity: independent Phone/CW drafts, Hunt/Log round trips, hidden display retirement and resumed data passed');return
    }
    if(sessionLayout){
      stationControls=true
      await until(`!!${button('Take station control')}`);await click(button('Take station control'))
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('Station control active')`)
      const lease=loggingLease,checks=[]
      await evaluate(`void(window.__sessionApp=document.querySelector('.app'))`)
      for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
        const shape=await evaluate(`(()=>{const e=document.querySelector('.remote-application-status'),r=e.getBoundingClientRect(),b=[...e.querySelectorAll('button')].find(e=>e.textContent==='Release station control'),q=b.getBoundingClientRect();return {height:r.height,width:r.width,release:q.toJSON(),releaseVisible:b.contains(document.elementFromPoint(q.left+q.width/2,q.top+q.height/2)),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight}})()`)
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`session-${width}-${zoom}-${theme}.png`),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,`session-${width}-${zoom}-${theme}.json`),JSON.stringify(shape,null,2))}
        assert.ok(shape.height<=Math.min(200,height/4),`Session status must leave at least three quarters of the screen for Nexus: ${JSON.stringify({width,height,zoom,theme,shape})}`)
        assert.ok(shape.releaseVisible&&shape.docW<=width+1&&shape.docH<=height+1,'control release must remain visible without scrolling')
        assert.equal(await evaluate(`document.querySelector('.remote-logging-authority')?.closest('.remote-session-info')===null`),true)
        const info=`document.querySelector('.remote-session-toggle')`
        await click(info);await until(`document.querySelector('.remote-session-toggle')?.getAttribute('aria-expanded')==='true'`)
        const disconnect=`document.querySelector('.remote-session-info button')`
        await evaluate(`${disconnect}.scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
        assert.equal(await evaluate(`(()=>{const b=${disconnect},r=b.getBoundingClientRect();return b.textContent==='Disconnect and return to stations'&&b.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))})()`),true)
        assert.equal(await evaluate(`document.querySelector('.remote-session-info')?.textContent.includes('Remote transmission remains unavailable')`),true)
        await click(info);await until(`document.querySelector('.remote-session-toggle')?.getAttribute('aria-expanded')==='false'`)
        assert.equal(loggingLease,lease,'presentation cannot release or replace authority')
        assert.equal(await evaluate(`document.querySelector('.app')===window.__sessionApp`),true,'session details must preserve the Nexus component tree')
        checks.push({width,height,zoom,theme,shape})
      }
      applicationAvailable=false
      await until(`document.querySelector('.app')?.dataset.remoteStale==='true'`)
      const loss=await evaluate(`(()=>{const e=document.querySelector('.remote-application-status'),warning=e.querySelector('.remote-session-unavailable');return {visible:!!warning&&warning.getBoundingClientRect().height>0&&warning.closest('.remote-session-info')===null,text:e.textContent}})()`)
      assert.ok(loss.visible&&loss.text.includes('Station data unavailable'),'loss must remain visible outside the collapsed details')
      applicationAvailable=true
      await until(`document.querySelector('.app')?.dataset.remoteStale!=='true'`)
      assert.equal(await evaluate(`document.querySelector('.app')===window.__sessionApp`),true)
      assert.equal(stationRequests.length,0);assert.equal(loggedRequests.length,0)
      // Loss expires the lease. Recovery preserves the cockpit, not authority;
      // a new explicit acquisition is required before testing explicit release.
      await until(`!!${button('Take station control')}`)
      assert.equal(loggingLease,null)
      await click(button('Take station control'))
      await until(`!!${button('Release station control')}`)
      assert.notEqual(loggingLease,lease)
      await click(button('Release station control'))
      await until(`!!${button('Take station control')}`)
      assert.equal(loggingLease,null)
      if(artifacts)await writeFile(join(artifacts,'session-results.json'),JSON.stringify({checks,stationActions:0,logWrites:0,releaseConfirmed:true,reacquireRequired:true,exceptions,unexpectedMessages},null,2))
      assert.equal(exceptions,0);assert.equal(unexpectedMessages,0)
      console.log('Compiled session layout: eight compact layouts, details, preserved authority and explicit release passed');return
    }
    if(operating){
      await click(button('CW'))
      await until(`!!document.querySelector('.cw-cockpit .remote-log-entry .le-log-btn')`)
      assert.equal(await evaluate(`document.querySelector('.cw-cockpit .le-log-btn').disabled`),true)
      assert.equal(await evaluate(`!!${button('Take logging control')}`),false)
      loggingAllowed=true
      await until(`!!${button('Take logging control')}`);await click(button('Take logging control'))
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('control active')`)
      const freshLoggingWindow=async()=>{
        const after=performance.now()
        for(let attempt=0;attempt<100;attempt++){
          const reply=operationWire.findLast(v=>v.direction==='in'&&v.phase==='controlling')
          if(reply&&reply.at>after&&performance.now()-reply.at<150)return
          await sleep(50)
        }
        assert.fail('A fresh native logging window must precede the positive-control gesture')
      }
      for(const [label,selector,call,mode] of [['CW','.cw-cockpit','K1OPS','CW'],['Phone','.phone-cockpit','K2OPS','SSB'],['RTTY','.rtty-cockpit','K3OPS','RTTY'],['PSK','.psk-cockpit','K4OPS','QPSK31'],['JS8','.js8-cockpit','K5OPS','JS8']]){
        console.log('Logging form',label);await click(button(label));
        if(label==='JS8'){await until(`!!${button('Take logging control')}`);await click(button('Take logging control'))}await until(`!!document.querySelector('${selector} .remote-log-entry .le-call')`)
        await click(`document.querySelector('${selector} .remote-log-entry .le-call')`)
        const focused=await evaluate(`document.activeElement===document.querySelector('${selector} .remote-log-entry .le-call')`)
        assert.equal(focused,true,`${label} call input receives the actual mouse gesture`)
        await browser.call('Input.insertText',{text:call},session)
        const entered=await evaluate(`({value:document.querySelector('${selector} .remote-log-entry .le-call')?.value,active:document.activeElement?.outerHTML.slice(0,300)})`)
        console.log('Contact input',label,entered)
        assert.equal(entered.value,call,`${label} typed contact remains in its own form`)
        // Wait for an actual new native reply, rather than an arbitrary delay
        // that can land the positive control on the old window's last instant.
        // Expiry and burst refusal remain separate client/relay test cases.
        await freshLoggingWindow()
        if(label==='CW'){
          // A separate real browser tab holds the station's draft lock. A Log
          // gesture must retain its form, send nothing, and never run later.
          const auxiliary=(await browser.call('Target.createTarget',{url:app.origin+'/api/remote/config',background:true})).targetId
          const auxiliarySession=(await browser.call('Target.attachToTarget',{targetId:auxiliary,flatten:true})).sessionId
          let auxiliaryReady=false
          for(let attempt=0;attempt<100;attempt++){
            const ready=await browser.call('Runtime.evaluate',{expression:`location.origin===${JSON.stringify(app.origin)}&&!!navigator.locks`,returnByValue:true},auxiliarySession)
            if(ready.result?.value===true){auxiliaryReady=true;break}
            await sleep(50)
          }
          assert.equal(auxiliaryReady,true,'the second tab must share the station storage origin')
          const lockKey=`nexus.remote.pending-log.${pair.stationId}`
          const held=await browser.call('Runtime.evaluate',{expression:`new Promise(resolve=>{void navigator.locks.request(${JSON.stringify(lockKey)},async()=>{resolve(true);await new Promise(release=>window.__releaseReceiptLock=release)})})`,returnByValue:true,awaitPromise:true},auxiliarySession)
          assert.equal(held.result?.value,true)
          await click(`document.querySelector('${selector} .le-log-btn')`)
          await until(`document.querySelector('${selector} .remote-log-entry')?.dataset.operationError==='remoteBusy'`)
          assert.equal(loggedRequests.length,0)
          assert.equal(await evaluate(`document.querySelector('${selector} .le-call').value`),call)
          await browser.call('Runtime.evaluate',{expression:'window.__releaseReceiptLock();true'},auxiliarySession)
          await browser.call('Target.closeTarget',{targetId:auxiliary})
          await freshLoggingWindow();assert.equal(loggedRequests.length,0,'releasing a draft lock must not replay the refused gesture')
        }
        if(label==='PSK')loseLogReply=true
        await click(`document.querySelector('${selector} .le-log-btn')`)
        if(label==='PSK'){
          await until(`document.querySelector('${selector} .remote-log-entry')?.textContent.includes('may already be logged')`,12000)
          assert.equal(await evaluate(`document.querySelector('${selector} .le-call').value`),call)
          await until(`document.querySelector('${selector} .remote-log-entry button')!==null`)
          assert.equal(loggedRequests[loggedRequests.length-1].record.call,call)
          const count=loggedRequests.length;await sleep(1500);assert.equal(loggedRequests.length,count)
          loseLogReply=false;
          await browser.call('Page.reload',{},session);
          await until(`!!${button('Open Nexus')}`,15000);
          await click(button('Open Nexus'));
          await until(`!!${button('PSK')}`);await click(button('PSK'));
          await until(`document.querySelector('.psk-cockpit .remote-log-entry')?.textContent.includes('K4OPS')`);
          assert.equal(loggedRequests.length,count,'page reload must not resend a QSO');
          const retained=await evaluate(`JSON.parse(localStorage.getItem('nexus.remote.pending-log.${pair.stationId}'))`);
          assert.equal(retained.record.call,call);assert.equal(retained.operationId,loggedRequests.at(-1).requestId);
          await click(`[...document.querySelectorAll('${selector} button')].find(e=>e.textContent==='Check submitted QSO result')`);
          console.log('RELOAD RECOVERY',JSON.stringify({storedCall:retained.record.call,operationId:retained.operationId,requestsBeforeReload:count,requestsAfterReload:loggedRequests.length}));
        }
        await until(`document.querySelector('${selector} .remote-log-entry')?.textContent.includes('QSO saved to the station log file')`)
        assert.equal(await evaluate(`document.querySelector('${selector} .le-call').value`),'')
        assert.equal(loggedRequests[loggedRequests.length-1].record.call,call)
        assert.equal(loggedRequests[loggedRequests.length-1].record.mode,mode)
        assert.equal(await evaluate(`document.querySelectorAll('${selector} .amp-strip button:not(:disabled)').length`),0)
      }
      await click(button('PSK'));await until(`!!document.querySelector('.psk-cockpit .remote-log-entry .le-call')`)
      for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
        for(const selector of ['.psk-cockpit .le-call','.psk-cockpit .le-log-btn']){
          await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
          const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2),chain=[];for(let p=e;p;p=p.parentElement){const r=p.getBoundingClientRect(),c=getComputedStyle(p);chain.push({class:p.className,rect:r.toJSON(),scroll:p.scrollHeight,client:p.clientHeight,at:p.scrollTop,y:c.overflowY})}return {rect:r.toJSON(),hit:hit?.outerHTML.slice(0,300),chain,docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
          if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'logging-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'logging-layout-failure.json'),JSON.stringify({selector,width,height,zoom,theme,shape},null,2))}
          assert.equal(shape.good,true,`Logging control reachable ${selector} ${width} ${zoom}: ${JSON.stringify(shape)}`)
        }
      }
      loggingAllowed=false;loggingLease=null;loggingRevision++
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('Allow remote logging')`)
      assert.equal(await evaluate(`document.querySelector('.psk-cockpit .le-log-btn').disabled`),true)
      // A separate local grant enables existing receiver/amp buttons, while
      // the manual log and all TX senders remain unavailable without their grant.
      stationControls=true
      await until(`!!${button('Take station control')}`);await click(button('Take station control'))
      await until(`document.querySelector('.remote-logging-authority')?.textContent.includes('Station control active')`)
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      const gesture=async(selector,expected)=>{
        await until(`!!document.querySelector(${JSON.stringify(selector)})&&!document.querySelector(${JSON.stringify(selector)}).disabled`)
        const before=stationRequests.length;await freshLoggingWindow();await click(`document.querySelector(${JSON.stringify(selector)})`)
        for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
        assert.equal(stationRequests.length,before+1,`one gesture must send one ${expected} action`)
        assert.equal(stationRequests.at(-1).action.action,expected)
        await until(`document.querySelector('.remote-control-result')?.textContent.includes('${['amplifier.followBand','decoder.js8Speed','decoder.msk144Period','decoder.depth','receiver.rxOffset','receiver.rxGain'].includes(expected)?'saved':'confirmed'}')`)
      }
      for(const [tab,root,selector,receiver] of [
        ['RTTY','.rtty-cockpit','.cw-decode-head .rtty-arm','rtty'],
        ['PSK','.psk-cockpit','.cw-decode-head .rtty-arm','psk'],
        ['SSTV','.sstv-view','.sstv-arm','sstv'],
        ['APRS','.aprs-cockpit','.np-chip[aria-pressed]','aprs']
      ]){
        await click(button(tab));await settledLayout()
        const target=receiver==='aprs'?'.aprs-cockpit button[title*="Receive-only"]':root+' '+selector
        // Find the existing monitor toggle by its class/pressed state; no test-only UI.
        const actual=receiver==='aprs'?'.aprs-cockpit .np-chip[aria-pressed]:not(.tx-toggle)':target
        const before=stationRequests.length
        if(receiver==='aprs'){
          await until(`!![...document.querySelectorAll('.aprs-cockpit .np-chip[aria-pressed]')].find(e=>e.textContent.includes('Monitoring')||e.textContent.includes('Monitor'))`)
          const monitor=`[...document.querySelectorAll('.aprs-cockpit .np-chip[aria-pressed]')].find(e=>e.textContent.includes('Monitoring')||e.textContent.includes('Monitor'))`
          await freshLoggingWindow();await click(monitor)
          for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
          assert.equal(stationRequests.length,before+1);assert.equal(stationRequests.at(-1).action.receiver,'aprs')
        }else await gesture(actual,'decoder.arm')
        assert.equal(stationRequests.at(-1).action.receiver,receiver)
      }
      await click(button('RTTY'));await gesture('.rtty-cockpit .cw-decode-clear','decoder.clear')
      await click(button('PSK'));await gesture('.psk-cockpit .cw-decode-clear','decoder.clear')
      await click(button('CW'));await gesture('.cw-cockpit .cw-decode-clear','decoder.clear')
      await gesture('.cw-cockpit .amp-op','amplifier.operate')
      await until(`document.querySelector('.cw-cockpit .amp-op').classList.contains('on')`)
      await gesture('.cw-cockpit .amp-band-step:last-child','amplifier.band')
      let followGeometry=0
      const followInput='#settings-amplifier input[type="checkbox"]',followSave='.settings-actions button[type="submit"]'
      for(const desired of [true,false]){
        await click(button('Settings'));await until(`!!document.querySelector('.settings-form')`);await click(button('Radio'))
        await until(`!!document.querySelector('${followInput}')&&!document.querySelector('${followInput}').disabled`)
        assert.equal(await evaluate(`document.querySelector('${followInput}').checked`),!desired)
        const before=stationRequests.length
        await click(`document.querySelector('${followInput}')`)
        assert.equal(stationRequests.length,before,'editing the checkbox must wait for Save')
        if(desired){
          for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
            await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
            await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
            for(const selector of [followInput,followSave]){
              await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
              const good=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);return r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1})()`)
              assert.equal(good,true,`Follow-band setting reachable ${selector} ${width} ${zoom} ${theme}`);followGeometry++
              if(selector===followInput){
                const shape=await evaluate(`(()=>{const e=document.querySelector('${followInput}'),label=e.closest('label'),field=label.closest('.settings-field'),scroll=e.closest('.settings-scroll'),r=label.getBoundingClientRect(),bounds=scroll.getBoundingClientRect();return{label:r.toJSON(),scroll:bounds.toJSON(),fieldWidth:field.clientWidth,fieldScroll:field.scrollWidth,readable:r.left>=bounds.left-1&&r.right<=bounds.right+1&&field.scrollWidth<=field.clientWidth+1}})()`)
                assert.equal(shape.readable,true,`Follow-band label must fit ${width} ${zoom} ${theme}: ${JSON.stringify(shape)}`)
              }
            }
            if(artifacts&&width===390&&zoom===1.75){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`amplifier-follow-390-175-${theme}.png`),Buffer.from(shot.data,'base64'))}
          }
          await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`);await settledLayout()
        }
        await gesture(followSave,'amplifier.followBand')
        assert.equal(stationRequests.at(-1).action.follow,desired)
        await until(`document.querySelector('.settings-actions .settings-ok')?.textContent==='Saved'`)
        assert.equal(await evaluate(`document.querySelector('${followInput}').checked`),desired)
        await click(button('CW'));await settledLayout()
        await until(`document.querySelector('.cw-cockpit .amp-band-step:last-child').disabled===${desired}`)
      }
      // The ordinary main dial, on each core cockpit. Navigation itself must
      // remain read-only; every submitted frequency is one explicit gesture.
      for(const [tab,root,bandDial] of [['FT','.operate-cockpit',7.075],['Phone','.phone-cockpit',7.076],['CW','.cw-cockpit',7.077],['RTTY','.rtty-cockpit',7.078],['PSK','.psk-cockpit',7.079]]){
        for(const mhz of [10,bandDial]){
        const count=stationRequests.length
        await click(button(tab));await settledLayout()
        assert.equal(stationRequests.length,count,'navigation cannot command the radio')
        const dial=`document.querySelector('${root} .ch-readout .readout[role="button"]')`
        await until(`!!${dial}`);await freshLoggingWindow();await click(dial)
        const input=`document.querySelector('${root} .readout-input')`
        await until(`!!${input}`)
        await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2},session)
        await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2},session)
        await browser.call('Input.insertText',{text:String(mhz)},session)
        await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
        await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
        for(let i=0;i<100&&stationRequests.length===count;i++)await sleep(100)
        assert.equal(stationRequests.length,count+1,`one ${tab} dial gesture`)
        assert.equal(stationRequests.at(-1).action.action,'radio.frequency');assert.equal(stationRequests.at(-1).action.dialMhz,mhz)
        assert.equal(stationRequests.at(-1).action.band,mhz===10?'':'40m')
        await until(`document.querySelector('.remote-control-result')?.textContent.includes('confirmed')`)
        await until(`document.querySelector('${root} .ch-readout .readout-val')?.textContent.includes('${mhz.toFixed(4)}')`)
        }
      }
      let modeGeometry=0,bandGeometry=0,wheelGeometry=0,filterGeometry=0,dspGeometry=0,phoneModeGeometry=0
      const tunedModes=new Set()
      for(const [tab,root,mode,workspace] of [['CW','.cw-cockpit','cw'],['FT','.operate-cockpit','digital','ft'],['Phone','.phone-cockpit','phone'],['RTTY','.rtty-cockpit','rtty'],['PSK','.psk-cockpit','keyboard'],['Tempo','.grid-header','digital','tempo'],['JS8','.js8-cockpit','digital','js8'],['FT','.operate-cockpit','digital','ft']]){
        const before=stationRequests.length,selector=root+' .remote-mode-entry'
        await click(button(tab));await settledLayout()
        await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)
        assert.equal(stationRequests.length,before,'entering a different mode view remains passive')
        for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
          await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
          const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return{rect:r.toJSON(),hit:hit?.outerHTML.slice(0,300),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
          if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'mode-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'mode-layout-failure.json'),JSON.stringify({tab,width,height,zoom,theme,shape},null,2))}
          assert.equal(shape.good,true,`Mode entry reachable ${tab} ${width} ${zoom}: ${JSON.stringify(shape)}`);modeGeometry++
        }
        const expected=workspace?{action:'radio.workspace',workspace}:{action:'radio.mode',mode,followFrequency:true}
        await gesture(selector,expected.action)
        assert.deepEqual(stationRequests.at(-1).action,expected)
        await until(`!document.querySelector('${selector}')`)
        assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
        if(['cw','phone'].includes(mode)){
          const picker=root+' .band-picker-select'
          for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
            await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
            await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
            await until(`!!document.querySelector('${picker}')&&!document.querySelector('${picker}').disabled`)
            await evaluate(`document.querySelector('${picker}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
            const good=await evaluate(`(()=>{const e=document.querySelector('${picker}'),r=e.getBoundingClientRect();return r.width>0&&r.height>0&&e.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1})()`)
            assert.equal(good,true,`Band picker reachable ${mode} ${width} ${zoom} ${theme}`);bandGeometry++
          }
          for(const band of ['20m','40m']){
            await freshLoggingWindow();await until(`!document.querySelector('${picker}').disabled`)
            const count=stationRequests.length
            await click(`document.querySelector('${picker}')`)
            for(const key of ['Home',...(band==='20m'?['ArrowDown']:[]),'Enter']){
              const code={Home:36,ArrowDown:40,Enter:13}[key]
              await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key,code:key,windowsVirtualKeyCode:code},session)
              await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key,code:key,windowsVirtualKeyCode:code},session)
            }
            for(let i=0;i<100&&stationRequests.length===count;i++)await sleep(100)
            assert.equal(stationRequests.length,count+1,`one ${mode} band gesture`)
            assert.deepEqual(stationRequests.at(-1).action,{action:'radio.band',band,mode})
            await until(`document.querySelector('.remote-control-result')?.textContent.includes('confirmed')`)
            const dial=applicationData.get_snapshot.radio.dialMhz.toFixed(4)
            await until(`document.querySelector('${root} .readout-val')?.textContent.includes('${dial}')`)
            assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
          }
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`band-picker-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
        }
        if(mode==='phone'){
          // The original synthetic station omitted actual CAT mode. An app
          // sideband and a working link cannot substitute for that reading.
          assert.equal(applicationData.get_snapshot.radio.rigMode,undefined)
          const first=root+' .ph-mode-pick > button:nth-of-type(2)',count=stationRequests.length
          await freshLoggingWindow();await until(`document.querySelector('${first}')?.disabled===true`)
          // The ordinary positive click helper waits for an enabled target.
          // This negative intentionally presses the disabled control itself.
          await evaluate(`document.querySelector('${first}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
          const inactive=await evaluate(`(()=>{const e=document.querySelector('${first}'),r=e.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2,hit:e.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))}})()`)
          assert.equal(inactive.hit,true,'negative Phone gesture must reach the disabled control')
          for(const type of ['mousePressed','mouseReleased'])await browser.call('Input.dispatchMouseEvent',{type,x:inactive.x,y:inactive.y,button:'left',clickCount:1},session)
          await sleep(200)
          assert.equal(stationRequests.length,count,'missing actual mode cannot command Phone selection')
          // Supply the physical reading through the normal station stream.
          // The following four picks are the fresh positive control.
          applicationData.get_snapshot.radio.rigMode='LSB';applicationRevision++
          const phoneDiagnostic=()=>evaluate(`(()=>{const e=document.querySelector('.phone-cockpit');let f=e?.[Object.keys(e).find(k=>k.startsWith('__reactFiber$'))],snap,phoneMode,operations,observation;while(f){if(f.memoizedProps?.snap){snap=f.memoizedProps.snap;phoneMode=f.memoizedProps.phoneMode}for(let d=f.dependencies?.firstContext;d;d=d.next){const v=d.memoizedValue;if(v?.getSnapshot&&v?.control)operations=v.getSnapshot();if(v?.frame&&v?.status)observation=v}f=f.return}return {radio:snap?.radio,activeRadioId:snap?.activeRadioId,phoneMode,operations,observation,buttons:[...e.querySelectorAll('.ph-mode-btn')].map(b=>({text:b.textContent,disabled:b.disabled}))}})()`)
          for(const [pick,index]of [['USB',2],['LSB',3],['AM',5],['auto',1]]){
            const selector=root+` .ph-mode-pick > button:nth-of-type(${index})`
            for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
              await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
              try{await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)}catch(error){const diagnostic={pick,width,height,zoom,theme,provider:applicationData.get_snapshot.radio,browser:await phoneDiagnostic()};console.log('Phone mode diagnostic',JSON.stringify(diagnostic));if(artifacts)await writeFile(join(artifacts,'phone-mode-readiness-failure.json'),JSON.stringify(diagnostic,null,2));throw error}
              await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
              const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return {rect:r.toJSON(),hit:hit?.outerHTML.slice(0,250),good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
              if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'phone-mode-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'phone-mode-layout-failure.json'),JSON.stringify({pick,width,height,zoom,theme,shape},null,2))}
              assert.equal(shape.good,true,`Phone mode reachable ${pick} ${width} ${zoom} ${theme}: ${JSON.stringify(shape)}`);phoneModeGeometry++
            }
            const before=structuredClone(applicationData.get_snapshot.radio)
            await gesture(selector,'radio.phoneMode')
            assert.deepEqual(stationRequests.at(-1).action,{action:'radio.phoneMode',expectedMode:before.sidebandOverride??'auto',mode:pick})
            assert.deepEqual(applicationData.get_snapshot.radio,{...before,sidebandOverride:pick==='auto'?null:pick,rigMode:pick==='auto'?'LSB':pick})
            await until(`document.querySelector('${selector}')?.getAttribute('aria-pressed')==='true'`)
          }
          assert.equal(await evaluate(`document.querySelector('${root} .ph-mode-pick > button:nth-of-type(4)')?.disabled`),true)
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'phone-mode-1280-175-light.png'),Buffer.from(shot.data,'base64'))}
        }
        if(['cw','phone'].includes(mode)){
          const controls=[...['nb','nr','notch',...(mode==='phone'?['manualNotch']:[])].map((func,index)=>({selector:root+` .ph-dsp > button:nth-of-type(${func==='manualNotch'?6:index+1})`,func})),
            ...['auto','fast','mid','slow','off'].map((speed,index)=>({selector:root+` .ph-agc > button:nth-of-type(${index+1})`,speed}))]
          for(const {selector,func,speed} of controls){
            for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
              await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
              await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)
              await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
              const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return {rect:r.toJSON(),hit:hit?.outerHTML.slice(0,250),good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
              if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'dsp-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'dsp-layout-failure.json'),JSON.stringify({mode,func,speed,width,height,zoom,theme,shape},null,2))}
              assert.equal(shape.good,true,`Receiver DSP reachable ${mode} ${func??speed} ${width} ${zoom} ${theme}: ${JSON.stringify(shape)}`);dspGeometry++
            }
            const before=structuredClone(applicationData.get_snapshot.radio)
            const action=func?{action:'radio.function',mode,func,expectedOn:before[func],on:!before[func]}:{action:'radio.agc',mode,expectedSpeed:before.agc,speed}
            await gesture(selector,action.action)
            assert.deepEqual(stationRequests.at(-1).action,action)
            assert.deepEqual(applicationData.get_snapshot.radio,{...before,[func??'agc']:func?!before[func]:speed})
            await until(`document.querySelector('${selector}')?.getAttribute('aria-pressed')==='${func?String(!before[func]):'true'}'`)
          }
          if(mode==='phone')for(const index of [4,5])assert.equal(await evaluate(`document.querySelector('${root} .ph-dsp > button:nth-of-type(${index})')?.disabled`),true)
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`dsp-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
          for(const direction of [1,2]){
            const selector=root+` .ph-filter-step:nth-of-type(${direction})`
            for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
              await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
              await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)
              await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
              const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return {rect:r.toJSON(),hit:hit?.outerHTML.slice(0,250),good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
              if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'filter-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'filter-layout-failure.json'),JSON.stringify({mode,direction,width,height,zoom,theme,shape},null,2))}
              assert.equal(shape.good,true,`Filter control reachable ${mode} ${direction} ${width} ${zoom} ${theme}: ${JSON.stringify(shape)}`);filterGeometry++
            }
            const before=structuredClone(applicationData.get_snapshot.radio),step=mode==='cw'?50:100,hz=before.filterWidthHz+(direction===1?-step:step)
            await gesture(selector,'radio.filterWidth')
            assert.deepEqual(stationRequests.at(-1).action,{action:'radio.filterWidth',mode,expectedHz:before.filterWidthHz,hz})
            assert.deepEqual(applicationData.get_snapshot.radio,{...before,filterWidthHz:hz})
            const displayed=mode==='cw'?String(hz):`${(hz/1000).toFixed(1)}k`
            await until(`document.querySelector('${root} .ph-filter-val')?.textContent.trim()==='${displayed}'`)
          }
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`filter-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
        }
        if(['cw','phone','rtty','keyboard'].includes(mode)||workspace==='ft'){
          if(!tunedModes.has(mode)){
            tunedModes.add(mode)
            const digit=root+' .readout-digit[data-decade="3"]',scope=root+(mode==='cw'?' .ph-scope-panel canvas':' .ph-scope-wrap canvas')
            const targets=[digit,...(['cw','phone'].includes(mode)?[scope,...[1,2,3,4].map(n=>root+` .tuning-nudge:nth-of-type(${n})`)]:[])]
            for(const target of targets)for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
              await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
              await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout()
              await until(`!!document.querySelector('${target}')`)
              await evaluate(`document.querySelector('${target}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
              const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return{rect:r.toJSON(),hit:hit?.outerHTML.slice(0,250),good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
              if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'wheel-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'wheel-layout-failure.json'),JSON.stringify({mode,target,width,height,zoom,theme,shape},null,2))}
              assert.equal(shape.good,true,`Tuning target reachable ${mode} ${target} ${width} ${zoom}: ${JSON.stringify(shape)}`);wheelGeometry++
              if(target===scope&&width===1280&&zoom===1.75&&theme==='light'&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`scope-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
            }
            for(const kind of ['digit','keyboard',...(['cw','phone'].includes(mode)?['scope','nudge']:[])]){
              await freshLoggingWindow();await until(`!!document.querySelector('${digit}')`)
              const before=stationRequests.length,from=applicationData.get_snapshot.radio.dialMhz
              const target=kind==='scope'?scope:kind==='nudge'?root+' .tuning-nudge:nth-of-type(3)':digit
              await evaluate(`document.querySelector('${target}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
              if(kind==='keyboard'){
                await evaluate(`document.querySelector('${root} .readout[role="button"]').focus()`)
                await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38},session)
                await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38},session)
              }else if(kind==='nudge')await click(`document.querySelector('${target}')`)
              else{
                const point=await evaluate(`(()=>{const r=document.querySelector('${target}').getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2}})()`)
                await browser.call('Input.dispatchMouseEvent',{type:'mouseWheel',...point,deltaX:0,deltaY:-100},session)
              }
              const tuningDiagnostic={mode,kind,target,from,before,wire:operationWire.slice(-8),session:await sessionDiagnostic(),view:await evaluate(`({status:document.querySelector('.remote-application-status')?.textContent,toasts:[...document.querySelectorAll('[role="alert"]')].map(e=>e.textContent),target:(()=>{const e=document.querySelector('${target}'),r=e?.getBoundingClientRect();return {rect:r?.toJSON(),disabled:e?.closest('[aria-disabled]')?.getAttribute('aria-disabled'),hit:r?document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)?.outerHTML:null}})()})`)}
              for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
              if(artifacts)await writeFile(join(artifacts,`tuning-${mode}-${kind}.json`),JSON.stringify({...tuningDiagnostic,after:stationRequests.length,afterWire:operationWire.slice(-8),afterSession:await sessionDiagnostic()},null,2))
              assert.equal(stationRequests.length,before+1,`one ${mode} ${kind} tuning burst`)
              const expected=Math.round(from*1e6+(kind==='digit'?1000:100))/1e6
              assert.equal(stationRequests.at(-1).action.action,'radio.frequency');assert.equal(stationRequests.at(-1).action.dialMhz,expected)
              await until(`document.querySelector('.remote-control-result')?.textContent.includes('confirmed')`)
              await until(`document.querySelector('${root} .readout-val')?.textContent.includes('${expected.toFixed(4)}')`)
              assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
            }
            if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`wheel-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
            if(['cw','phone'].includes(mode)){
              const prepareScopePress=async()=>{
                await freshLoggingWindow()
                for(let attempt=0;attempt<100;attempt++){
                  const age=receiverRead?performance.now()-receiverRead.at+(receiverRead.radio?.readings?.ptt?.ageMs??Infinity):Infinity
                  if(age<250&&receiverRead?.radio?.rigKeyed===false&&await evaluate(`document.querySelector('${scope}')?.classList.contains('tunable')`))break
                  if(attempt===99)assert.fail(`Fresh readings and the actual ${mode} scope must precede its positive-control press`)
                  await sleep(50)
                }
                await evaluate(`document.querySelector('${scope}').scrollIntoView({block:'center',inline:'center',behavior:'instant'})`);await settledLayout()
                const point=await evaluate(`(()=>{const e=document.querySelector('${scope}'),r=e.getBoundingClientRect(),x=r.left+r.width*.7,y=r.top+r.height*.4;return{x,y,edge:r.right-1,hit:e===document.elementFromPoint(x,y),rect:r.toJSON()}})()`)
                assert.equal(point.hit,true,`scope press must hit the refreshed ${mode} canvas: ${JSON.stringify(point)}`)
                return point
              }
              let point=await prepareScopePress()
              const before=stationRequests.length,radio=structuredClone(applicationData.get_snapshot.radio)
              await evaluate(`(()=>{window.__scopePointerEvents=[];for(const type of ['pointerdown','pointerup','pointermove','pointercancel','lostpointercapture'])document.addEventListener(type,e=>{window.__scopePointerEvents.push({type,x:e.clientX,y:e.clientY,button:e.button,pointerId:e.pointerId,target:e.target?.outerHTML?.slice(0,200)});window.__scopePointerEvents=window.__scopePointerEvents.slice(-20)},true)})()`)
              await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',x:point.x,y:point.y,button:'left',clickCount:1},session)
              await browser.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:point.edge,y:point.y,buttons:1},session)
              await sleep(150)
              await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',x:point.edge,y:point.y,button:'left',clickCount:1},session)
              await sleep(150)
              assert.equal(stationRequests.length,before,'scope movement cannot fall through to native drag or edge scanning')
              point=await prepareScopePress()
              await browser.call('Input.dispatchMouseEvent',{type:'mousePressed',x:point.x,y:point.y,button:'left',clickCount:1},session)
              await sleep(1600)
              await browser.call('Input.dispatchMouseEvent',{type:'mouseReleased',x:point.x,y:point.y,button:'left',clickCount:1},session)
              await sleep(150);assert.equal(stationRequests.length,before,'a held scope press cannot borrow a renewed command window')
              point=await prepareScopePress()
              for(const type of ['mousePressed','mouseReleased'])await browser.call('Input.dispatchMouseEvent',{type,x:point.x,y:point.y,button:'left',clickCount:1},session)
              for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
              if(stationRequests.length!==before+1&&artifacts){
                const detail=await evaluate(`(()=>{const e=document.querySelector('${scope}'),r=e?.getBoundingClientRect();return{point:${JSON.stringify(point)},rect:r?.toJSON(),hit:document.elementFromPoint(${point.x},${point.y})?.outerHTML?.slice(0,500),scope:e?.outerHTML,events:window.__scopePointerEvents,authority:document.querySelector('.remote-logging-authority')?.textContent,toasts:[...document.querySelectorAll('[role="status"],[role="alert"]')].map(e=>e.textContent)}})()`)
                const shot=await browser.call('Page.captureScreenshot',{format:'png'},session)
                await writeFile(join(artifacts,'scope-click-failure.png'),Buffer.from(shot.data,'base64'))
                await writeFile(join(artifacts,'scope-click-failure.json'),JSON.stringify({mode,detail,before,after:stationRequests.length,radio,receiverRead,lastOperations:operationWire.slice(-30)},null,2))
              }
              assert.equal(stationRequests.length,before+1,`one ${mode} scope click`)
              const action=stationRequests.at(-1).action
              assert.equal(action.action,'radio.frequency');assert.equal(action.sideband,radio.sideband)
              assert.ok(Math.abs(action.dialMhz-radio.dialMhz)>0&&Math.abs(action.dialMhz-radio.dialMhz)<0.004,'native scope resolves a nearby receive target')
              await until(`document.querySelector('.remote-control-result')?.textContent.includes('confirmed')`)
              await until(`document.querySelector('${root} .readout-val')?.textContent.includes('${action.dialMhz.toFixed(4)}')`)
              assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
              assert.equal(applicationData.get_snapshot.radio.operatingMode,radio.operatingMode)
              if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`scope-click-${mode}-1280-175-light.png`),Buffer.from(shot.data,'base64'))}
            }
          }
        }
      }
      let tierGeometry=0
      for(const [tab,root,index,tier] of [
        ['FT','.operate-cockpit .cockpit-modes',2,'FT4'],['FT','.operate-cockpit .cockpit-modes',3,'FT2'],['FT','.operate-cockpit .cockpit-modes',1,'FT8'],
        ...[[6,'WSPR'],[7,'Q65'],[8,'MSK144'],[9,'JT65'],[10,'FST4'],[11,'FST4W']].map(([index,tier])=>['FT','.topbar-group.tier-toggle:not(.tx-period)',index,tier]),
        ['Tempo','.grid-header .cockpit-modes',2,'TempoDeep'],['Tempo','.grid-header .cockpit-modes',1,'TempoFast']
      ]){
        const before=stationRequests.length,selector=`${root} > button:nth-child(${index})`
        await click(button(tab));await settledLayout()
        assert.equal(stationRequests.length,before,'visiting a tier selector cannot select a decoder')
        for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
          await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)
          await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
          const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return{rect:r.toJSON(),hit:hit?.outerHTML.slice(0,300),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
          if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'tier-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'tier-layout-failure.json'),JSON.stringify({tab,tier,width,height,zoom,theme,shape},null,2))}
          assert.equal(shape.good,true,`Tier selector reachable ${tier} ${width} ${zoom}: ${JSON.stringify(shape)}`);tierGeometry++
          if(artifacts&&width===390&&zoom===1.75&&theme==='dark'&&['FT4','WSPR','TempoDeep'].includes(tier)){
            const shot=await browser.call('Page.captureScreenshot',{format:'png'},session)
            await writeFile(join(artifacts,`tier-${tier}-390-175.png`),Buffer.from(shot.data,'base64'))
          }
        }
        await gesture(selector,'radio.tier')
        assert.deepEqual(stationRequests.at(-1).action,{action:'radio.tier',tier})
        await until(`document.querySelector('${selector}').getAttribute('aria-pressed')==='true'`)
        assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
      }
      let decoderGeometry=0
      const decoderLayout=async(selector,name)=>{
        let count=0
        for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
          await until(`!!document.querySelector('${selector}')&&!document.querySelector('${selector}').disabled`)
          await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
          const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2),legend=e.matches('.waterfall-canvas')?e.parentElement.querySelector('.wf-legend')?.getBoundingClientRect():null,bar=e.matches('.waterfall-canvas')?e.parentElement.querySelector('.wf-legend-bar')?.getBoundingClientRect():null,legendFits=!legend||(legend.top>=r.top-1&&legend.bottom<=r.bottom+1&&!!bar&&bar.height>0);return{rect:r.toJSON(),legend:legend?.toJSON(),hit:hit?.outerHTML.slice(0,300),good:legendFits&&r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
          if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'decoder-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'decoder-layout-failure.json'),JSON.stringify({name,selector,width,height,zoom,theme,shape},null,2))}
          assert.equal(shape.good,true,`Decoder control reachable ${name} ${width} ${zoom}: ${JSON.stringify(shape)}`);count++
          if(artifacts&&width===390&&zoom===1.75){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`decoder-${name}-390-175-${theme}.png`),Buffer.from(shot.data,'base64'))}
        }
        return count
      }
      await click(button('JS8'));await gesture('.js8-cockpit .remote-mode-entry','radio.workspace')
      await until(`!document.querySelector('.js8-cockpit .remote-mode-entry')`)
      decoderGeometry+=await decoderLayout('.js8-cockpit .js8-speed-chip:last-child','js8')
      for(const speed of [0,2,3,1]){
        const expectedSpeed=['slow','normal','fast','turbo'].indexOf(js8Fixture.state.speed)
        const selector=`.js8-cockpit .js8-speed-chip:nth-child(${speed+1})`
        await gesture(selector,'decoder.js8Speed')
        assert.deepEqual(stationRequests.at(-1).action,{action:'decoder.js8Speed',expectedSpeed,speed})
        await until(`document.querySelector('${selector}').getAttribute('aria-pressed')==='true'`)
      }
      await click(button('FT'));await gesture('.topbar-group.tier-toggle:not(.tx-period) > button:nth-child(8)','radio.tier')
      await until(`!!document.querySelector('.operate-cockpit .cm-trperiod')`)
      await until(`document.querySelector('.operate-cockpit .cm-trperiod').value==='15'`)
      const periodSelector='.operate-cockpit .cm-trperiod'
      decoderGeometry+=await decoderLayout(periodSelector,'msk144')
      for(const periodSecs of [5,10,30,15]){
        const expectedPeriodSecs=applicationData.get_snapshot.link.periodSecs,before=stationRequests.length
        await freshLoggingWindow();await click(`document.querySelector('${periodSelector}')`)
        assert.equal(await evaluate(`document.activeElement===document.querySelector('${periodSelector}')`),true)
        const keys=[['Home',36],...Array.from({length:[5,10,15,30].indexOf(periodSecs)},()=>['ArrowDown',40]),['Enter',13]]
        for(const [key,code]of keys)for(const type of ['keyDown','keyUp'])await browser.call('Input.dispatchKeyEvent',{type,key,code:key,windowsVirtualKeyCode:code},session)
        for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
        assert.equal(stationRequests.length,before+1,'one committed MSK144 choice')
        assert.deepEqual(stationRequests.at(-1).action,{action:'decoder.msk144Period',expectedPeriodSecs,periodSecs})
        await until(`document.querySelector('.remote-control-result')?.textContent.includes('saved')`)
        await until(`document.querySelector('${periodSelector}').value==='${periodSecs}'`)
      }
      await gesture('.operate-cockpit .cockpit-modes > button:first-child','radio.tier')
      assert.deepEqual(stationRequests.at(-1).action,{action:'radio.tier',tier:'FT8'})
      await until(`document.querySelector('.operate-cockpit .cockpit-depth-chip:last-child')?.getAttribute('aria-pressed')==='true'`)
      let receiverGeometry=await decoderLayout('.operate-cockpit .cockpit-depth-chip:first-child','depth')
      for(const depth of [1,2,3]){
        const expectedDepth=applicationData.get_snapshot.radio.decodeDepth
        await gesture(`.operate-cockpit .cockpit-depth-chip:nth-child(${depth})`,'decoder.depth')
        assert.deepEqual(stationRequests.at(-1).action,{action:'decoder.depth',expectedTier:'FT8',expectedDepth,depth})
        await until(`document.querySelector('.operate-cockpit .cockpit-depth-chip:nth-child(${depth})').getAttribute('aria-pressed')==='true'`)
      }
      const rxField='.operate-cockpit .df-field:first-child input'
      receiverGeometry+=await decoderLayout(rxField,'rx-offset')
      const rxBefore=stationRequests.length,expectedHz=applicationData.get_snapshot.radio.rxOffsetHz
      await freshLoggingWindow();await click(`document.querySelector('${rxField}')`)
      assert.equal(await evaluate(`document.activeElement===document.querySelector('${rxField}')`),true)
      await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65},session)
      await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65},session)
      await browser.call('Input.insertText',{text:'725'},session)
      assert.equal(await evaluate(`document.querySelector('${rxField}').value`),'725')
      for(const type of ['keyDown','keyUp'])await browser.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13},session)
      for(let i=0;i<100&&stationRequests.length===rxBefore;i++)await sleep(100)
      assert.equal(stationRequests.length,rxBefore+1)
      assert.deepEqual(stationRequests.at(-1).action,{action:'receiver.rxOffset',expectedTier:'FT8',expectedHz,hz:725})
      await until(`document.querySelector('${rxField}').value==='725'`)
      assert.equal(await evaluate(`document.querySelector('.operate-cockpit .df-field:last-child input').disabled`),true)
      for(const [tab,workspace,selector,fraction,hz]of [
        ['FT',null,'.operate-cockpit .waterfall-canvas',0.25,900],
        ['JS8','js8','.js8-cockpit .waterfall-canvas',0.5,1600],
        ['Tempo','tempo','.right-rail .waterfall-canvas',0.75,2300]
      ]){
        await click(button(tab));await settledLayout()
        if(workspace)await gesture(workspace==='js8'?'.js8-cockpit .remote-mode-entry':'.grid-header .remote-mode-entry','radio.workspace')
        await until(`!!document.querySelector('${selector}')`)
        receiverGeometry+=await decoderLayout(selector,`waterfall-${tab.toLowerCase()}`)
        await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`);await settledLayout()
        await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
        const hit=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),x=r.left+r.width*${fraction},y=r.top+r.height/2,hit=document.elementFromPoint(x,y);return{x,y,rect:r.toJSON(),hit:hit?.outerHTML.slice(0,1000),visible:e.contains(hit)}})()`)
        if(!hit.visible&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'receiver-waterfall-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'receiver-waterfall-failure.json'),JSON.stringify({tab,selector,hit},null,2))}
        assert.equal(hit.visible,true,`RX waterfall reachable ${tab}: ${JSON.stringify(hit)}`)
        const point={x:hit.x,y:hit.y}
        const before=stationRequests.length,expectedHz=applicationData.get_snapshot.radio.rxOffsetHz,expectedTier=applicationData.get_snapshot.link.tier
        for(const [button,modifiers]of [['right',0],['left',8],['left',2]])for(const type of ['mousePressed','mouseReleased'])
          await browser.call('Input.dispatchMouseEvent',{type,...point,button,modifiers,clickCount:1},session)
        await sleep(300);assert.equal(stationRequests.length,before,'receive permission must not move TX or both markers')
        if(tab==='FT'){
          // Authority and physical readings have separate clocks. An available
          // controller cannot authorize an RX gesture using an old PTT sample.
          pauseObservations=true
          // The input also disables between short authority windows. Prove the
          // independent hardware clock has expired before refreshing authority;
          // an input's disabled state alone does not establish stale readings.
          for(let attempt=0;attempt<100;attempt++){
            const age=receiverRead?performance.now()-receiverRead.at+(receiverRead.radio?.readings?.ptt?.ageMs??Infinity):Infinity
            if(Number.isFinite(age)&&age>=1250)break
            if(attempt===99)assert.fail('The stale-reading negative control requires an expired, previously received PTT sample')
            await sleep(50)
          }
          await until(`document.querySelector('${rxField}').disabled`)
          await freshLoggingWindow()
          assert.equal(await evaluate(`document.querySelector('.remote-logging-authority')?.textContent.includes('Station control active')`),true)
          const stale=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),x=r.left+r.width*${fraction},y=r.top+r.height/2;return {x,y,visible:e.contains(document.elementFromPoint(x,y)),disabled:document.querySelector('${rxField}').disabled}})()`)
          const age=performance.now()-receiverRead.at+receiverRead.radio.readings.ptt.ageMs
          assert.ok(age>=1000&&stale.disabled&&stale.visible,'expired readings and a real waterfall hit must precede the negative gesture')
          for(const type of ['mousePressed','mouseReleased'])await browser.call('Input.dispatchMouseEvent',{type,x:stale.x,y:stale.y,button:'left',clickCount:1},session)
          await sleep(150);assert.equal(stationRequests.length,before,'fresh control authority cannot replace missing fresh receiver readings')
          if(artifacts)await writeFile(join(artifacts,'receiver-stale-reading.json'),JSON.stringify({age,stale,requestsBefore:before,requestsAfter:stationRequests.length},null,2))
          pauseObservations=false
        }
        await freshLoggingWindow()
        // The positive gesture needs a fresh hardware sample as well as the
        // shared widget's permission; leave room for input/layout dispatch.
        for(let attempt=0;attempt<100;attempt++){
          const age=receiverRead?performance.now()-receiverRead.at+(receiverRead.radio?.readings?.ptt?.ageMs??Infinity):Infinity
          if(age<250&&receiverRead?.radio?.rigKeyed===false&&await evaluate(`!document.querySelector('${rxField}').disabled`))break
          if(attempt===99)assert.fail(`Fresh receive readings must precede ${tab} RX gesture`)
          await sleep(50)
        }
        await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
        const prepared=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),x=r.left+r.width*${fraction},y=r.top+r.height/2,hit=document.elementFromPoint(x,y);return {x,y,rect:r.toJSON(),hit:hit?.outerHTML.slice(0,1000),visible:e.contains(hit),authority:document.querySelector('.remote-logging-authority')?.textContent}})()`)
        assert.equal(prepared.visible,true,`RX gesture must hit the refreshed ${tab} canvas`)
        for(const type of ['mousePressed','mouseReleased'])await browser.call('Input.dispatchMouseEvent',{type,x:prepared.x,y:prepared.y,button:'left',clickCount:1},session)
        for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
        if(stationRequests.length!==before+1){
          const state=await evaluate(`({authority:document.querySelector('.remote-logging-authority')?.textContent,result:document.querySelector('.remote-control-result')?.textContent,toasts:[...document.querySelectorAll('[role="alert"]')].map(e=>e.textContent)})`)
          const failure={tab,selector,point,hit,prepared,state,receiverRead,expectedTier,expectedHz,hz,wire:operationWire.slice(-30)}
          console.log('Receiver gesture failure',JSON.stringify(failure))
          if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'receiver-gesture-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'receiver-gesture-failure.json'),JSON.stringify(failure,null,2))}
        }
        assert.equal(stationRequests.length,before+1,`one ${tab} RX waterfall gesture`)
        assert.deepEqual(stationRequests.at(-1).action,{action:'receiver.rxOffset',expectedTier,expectedHz,hz})
        await until(`document.querySelector('${rxField}').value==='${hz}'`)
        assert.equal(applicationData.get_snapshot.radio.txOffsetHz,1500)
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`receiver-${tab.toLowerCase()}-1280.png`),Buffer.from(shot.data,'base64'))}
      }
      await click(button('Settings'));await until(`!!document.querySelector('.settings-form')`);await click(button('Radio'))
      const gainSelector='#settings-audio input[aria-label="RX capture gain"]'
      const gainGeometry=await decoderLayout(gainSelector,'rx-gain')
      {
        const doc=navigation.documents.settings,expectedGain=doc.settings.rxGain,expectedSettingsRevision=doc.revision,before=stationRequests.length
        await freshLoggingWindow();await click(`document.querySelector('${gainSelector}')`)
        for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
        assert.equal(stationRequests.length,before+1,'one pointer release at the middle of the native gain slider')
        assert.deepEqual(stationRequests.at(-1).action,{action:'receiver.rxGain',radioId:1,expectedSettingsRevision,expectedGain,gain:4.5})
        await until(`document.querySelector('${gainSelector}').value==='4.5'&&!document.querySelector('${gainSelector}').disabled`)
      }
      for(const key of ['ArrowRight','Home']){
        const doc=navigation.documents.settings,expectedGain=doc.settings.rxGain,expectedSettingsRevision=doc.revision,before=stationRequests.length
        const gain=key==='Home'?1:Math.round((expectedGain+0.1)*10)/10
        await freshLoggingWindow()
        // Prepare keyboard focus without a second pointer adjustment; the
        // actual key-down/up events below must perform the native input change.
        await evaluate(`document.querySelector('${gainSelector}').focus({preventScroll:true})`)
        assert.equal(await evaluate(`document.activeElement===document.querySelector('${gainSelector}')`),true)
        assert.equal(stationRequests.length,before)
        await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key,code:key,windowsVirtualKeyCode:key==='Home'?36:39},session)
        assert.equal(stationRequests.length,before,'gain changes wait for release')
        await browser.call('Input.dispatchKeyEvent',{type:'keyUp',key,code:key,windowsVirtualKeyCode:key==='Home'?36:39},session)
        for(let i=0;i<100&&stationRequests.length===before;i++)await sleep(100)
        assert.equal(stationRequests.length,before+1,'one RX gain release')
        assert.deepEqual(stationRequests.at(-1).action,{action:'receiver.rxGain',radioId:1,expectedSettingsRevision,expectedGain,gain})
        await until(`document.querySelector('.remote-control-result')?.textContent.includes('saved')`)
        await until(`document.querySelector('${gainSelector}').value==='${gain}'&&!document.querySelector('${gainSelector}').disabled`)
        assert.equal(await evaluate(`document.querySelector('#settings-audio input[aria-label="Transmit drive level"]').disabled`),true)
      }
      await click(button('CW'));await settledLayout()
      let controlGeometry=0
      for(const [width,height,zoom]of [[390,844,1],[1280,800,1],[390,844,1.75],[1280,800,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`);await settledLayout();await settledLayout()
        for(const selector of ['.cw-cockpit .amp-op','.cw-cockpit .cw-decode-clear']){
          await evaluate(`document.querySelector('${selector}').scrollIntoView({block:'center',behavior:'instant'})`);await settledLayout()
          const shape=await evaluate(`(()=>{const e=document.querySelector('${selector}'),r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return{rect:r.toJSON(),hit:hit?.outerHTML.slice(0,300),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,good:r.width>0&&r.height>0&&e.contains(hit)&&document.documentElement.scrollWidth<=innerWidth+1&&document.documentElement.scrollHeight<=innerHeight+1}})()`)
          if(!shape.good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'control-layout-failure.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'control-layout-failure.json'),JSON.stringify({selector,width,height,zoom,theme,shape},null,2))}
          assert.equal(shape.good,true,`Station control reachable ${selector} ${width} ${zoom}: ${JSON.stringify(shape)}`);controlGeometry++
        }
      }
      assert.equal(applicationData.get_snapshot.radio.txEnabled,false)
      assert.ok(await evaluate(`[...document.querySelectorAll('.cockpit-txdock button')].every(e=>e.disabled)`),'receiver/amp permission cannot enable TX')
      stationControls=false;loggingLease=null
      await until(`document.querySelector('.cw-cockpit .amp-op').disabled`)
      assert.equal(controlGeometry+modeGeometry+bandGeometry+wheelGeometry+filterGeometry+dspGeometry+phoneModeGeometry+tierGeometry+followGeometry+decoderGeometry+receiverGeometry+gainGeometry+16,600)
      assert.equal(loggedRequests.length,5);assert.equal(stationRequests.length,108);assert.equal(unexpectedMessages,0);assert.equal(exceptions,0)
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-manual-logging.png'),Buffer.from(shot.data,'base64'));await writeFile(join(artifacts,'operation-results.json'),JSON.stringify({count:loggedRequests.length,stationActions:stationRequests.map(r=>r.action),controlGeometry,modeGeometry,bandGeometry,wheelGeometry,filterGeometry,dspGeometry,phoneModeGeometry,tierGeometry,followGeometry,decoderGeometry,receiverGeometry,gainGeometry,modes:loggedRequests.map(r=>r.record.mode),lostResultResolved:true,wholePageReloadResolved:true,crossTabLockRefusal:true,geometry:16,exceptions,unexpectedMessages},null,2))}
      console.log('Compiled browser: five logging forms, 108 station gestures, saved receiver choices, native band recall, shared wheel/digit tuning, Phone mode picks, separate grants, recovery and 600 geometry cases passed');return
    }
    const startReads=applicationTraffic.reads, startBytes=applicationTraffic.bytes, started=performance.now()
    for(const [width,height] of [[1024,768],[1280,800],[1366,768],[1200,1390],[3440,1440]])for(const zoom of [1,1.75])for(const theme of ['dark','light']) {
      await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      const shape=await evaluate(`(()=>{const app=document.querySelector('.app'),header=document.querySelector('.remote-application-status'),r=app.getBoundingClientRect(),h=header.getBoundingClientRect();return{width:innerWidth,height:innerHeight,appW:r.width,appH:r.height,docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,headerBottom:h.bottom,crash:!!document.querySelector('.view-crash')}})()`)
      assert.ok(!shape.crash && shape.docW<=shape.width+1 && shape.docH<=shape.height+1 && shape.headerBottom<=shape.height,`Nexus workspace must remain bounded: ${JSON.stringify({width,height,zoom,theme,shape})}`)
      results.push({workspace:true,width,height,zoom,theme,shape})
    }
    applicationTraffic.sample = { seconds: (performance.now()-started)/1000, reads: applicationTraffic.reads-startReads, bytes: applicationTraffic.bytes-startBytes }
    assert.ok(applicationTraffic.sample.reads/applicationTraffic.sample.seconds < 24,'real panel demand must remain below the session rate bound')
    // A resize/rebuild can also call putImageData. Confirm that actual station
    // spectrum values have produced a visible signal in the spectrum canvas.
    await until(`(()=>{const c=document.querySelector('.waterfall-canvas'),a=c.getContext('2d').getImageData(0,0,c.width,c.height).data;let colored=0;for(let i=0;i<a.length;i+=4)if(a[i]>30||a[i+1]>30||a[i+2]>30)colored++;return colored>50})()`)
    if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-workspace.png'),Buffer.from(shot.data,'base64'))}
    if (applicationVersion >= 2) {
      for (const [label, mode] of [['CW','cw'], ['Phone','phone']]) {
        applicationData.get_snapshot.radio.operatingMode=mode
        applicationData.get_snapshot.radio.cwWpm=23
        applicationRevision++
        await click(button(label))
        await until(`!!document.querySelector('.${mode}-cockpit')`)
        if (mode === 'cw') await until(`document.querySelector('.cw-cockpit [role="log"]')?.textContent.includes('W1AW')`)
        if (applicationVersion >= 4) {
          await click(`document.querySelector('.${mode}-cockpit .le-call')`)
          await browser.call('Input.insertText',{text:'W1AW'},session)
          await until(`document.querySelector('.${mode}-cockpit .recall-card')?.textContent.includes('Recall browser note')`)
          assert.ok(await evaluate(`document.querySelector('.${mode}-cockpit .recall-card').textContent.includes('Showing 20 of 2030')`))
        }
        await until(`(()=>{const c=document.querySelector('.${mode}-cockpit .ph-scope canvas'),r=c?.getBoundingClientRect();return !!r&&r.width>0&&r.height>0&&getComputedStyle(c).visibility==='visible'})()`)
        for (const [width,height] of [[390,844],[1024,768],[1280,800],[3440,1440]]) {
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
          await settledLayout()
          const shape=await evaluate(`(()=>{const root=document.querySelector('.${mode}-cockpit'),r=root.getBoundingClientRect();return{width:innerWidth,height:innerHeight,docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,cockpitW:r.width,cockpitH:r.height,crash:!!document.querySelector('.view-crash')}})()`)
          assert.ok(!shape.crash && shape.docW<=width+1 && shape.docH<=height+1, `${mode} cockpit must remain bounded: ${JSON.stringify(shape)}`)
          results.push({workspace:true,mode,width,height,shape})
        }
        const controls=await evaluate(`Array.from(document.querySelectorAll('.${mode}-cockpit ${mode==='cw'?'.cw-macro, .cw-cockpit .cw-send-btn':'.ph-ptt, .phone-cockpit .ph-mode-btn'}')).map(e=>e.disabled)`)
        assert.ok(controls.length>2 && controls.every(Boolean),'actual operating controls must be disabled')
        const scopeRow=applicationData.get_scope_snapshot.row
        applicationData.get_scope_snapshot.row=[];applicationRevision++
        await until(`getComputedStyle(document.querySelector('.${mode}-cockpit .ph-scope canvas')).visibility==='hidden' && document.querySelector('.${mode}-cockpit .ph-scope').textContent.includes('Scope data unavailable.')`)
        const staleScope=await evaluate(`document.querySelector('.app')?.dataset.remoteStale==='true'`)
        if(staleScope)console.log('Scope session diagnostic',JSON.stringify(await sessionDiagnostic()))
        assert.equal(staleScope,false,'scope absence is distinct from station/session loss')
        applicationData.get_scope_snapshot.row=scopeRow;applicationRevision++
        await until(`getComputedStyle(document.querySelector('.${mode}-cockpit .ph-scope canvas')).visibility==='visible'`)
        await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
        await settledLayout()
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${mode}.png`),Buffer.from(shot.data,'base64'))}
      }
      await click(button('FT'))
      await until(`!!document.querySelector('.operate-host:not([hidden])')`)
      assert.ok(applicationTraffic.subscriptions>0 && applicationTraffic.batches>0 && applicationTraffic.acks>0, 'positive control: real subscriptions, native batches and browser ACKs flowed')
      assert.equal(applicationTraffic.reads,0, 'v2 panel polling stays within the browser')
    }
    for (const [label, mode] of [['RTTY', 'rtty'], ['PSK', 'psk']]) {
      applicationData.get_snapshot.radio.operatingMode = mode === 'rtty' ? 'rtty' : 'keyboard'
      applicationRevision++
      await click(button(label))
      if (applicationVersion < 5) {
        await until(`!!document.querySelector('.remote-view-unavailable')`)
        assert.equal(applicationTraffic.byCommand[`get_${mode}_state`] ?? 0, 0, 'older stations keep their closed read contract')
        continue
      }
      try { await until(`document.querySelector('.${mode}-cockpit .cw-decode-text')?.textContent==='CQ W1AW'`) }
      catch (error) {
        console.log('Keyboard observation diagnostic', {applicationVersion,mode,byCommand:applicationTraffic.byCommand},await evaluate(`({body:document.querySelector('.${mode}-cockpit')?.textContent,status:document.querySelector('.remote-application-status')?.textContent,unavailable:document.querySelector('.remote-view-unavailable')?.textContent,closures:window.__socketClosures})`))
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-keyboard-failure.png'),Buffer.from(shot.data,'base64'))}
        throw error
      }
      assert.ok(await evaluate(`document.querySelector('.${mode}-cockpit .amp-strip')?.textContent.includes(${JSON.stringify(fixture.station.amplifier.bandLabel)})`), 'keyboard cockpit amp follows the authenticated observation, not the older application snapshot')
      const controls = await evaluate(`Array.from(document.querySelectorAll('.${mode}-cockpit .rtty-arm, .${mode}-cockpit .cw-macro, .${mode}-cockpit .cw-type, .${mode}-cockpit .cw-decode-clear')).map(e=>e.disabled)`)
      assert.ok(controls.length > 8 && controls.every(Boolean))
      await evaluate(`document.querySelector('.${mode}-cockpit .cw-type').dispatchEvent(new InputEvent('beforeinput',{data:'X',inputType:'insertText',bubbles:true,cancelable:true}));window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))`)
      await click(`document.querySelector('.${mode}-cockpit .le-call')`)
      await browser.call('Input.insertText', { text: 'W1AW' }, session)
      await until(`document.querySelector('.${mode}-cockpit .recall-card')?.textContent.includes('Recall browser note')`)
      for (const [width,height,zoom] of [[390,844,1],[1024,768,1],[1280,800,1],[1366,768,1],
        [1200,750,0.8],[1200,1390,1],[3440,1440,1],[1024,768,1.75],[1280,800,1.75],[3440,1440,1.75]]) {
        for (const theme of ['dark','light']) {
          await browser.call('Emulation.setDeviceMetricsOverride', { width,height,deviceScaleFactor:1,mobile:false }, session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
          await settledLayout()
          const shape = await evaluate(`(()=>{const root=document.querySelector('.${mode}-cockpit');return{docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,crash:!!root.querySelector('.view-crash'),frames:root.querySelectorAll(':scope > .pane-frame').length,docks:root.querySelectorAll(':scope > .cockpit-txdock').length}})()`)
          assert.ok(!shape.crash && shape.docW <= width+1 && shape.docH <= height+1, `${mode} is bounded: ${JSON.stringify({width,height,zoom,shape})}`)
          assert.equal(shape.frames, 2); assert.equal(shape.docks, 1)
          for (const selector of ['.cw-decode-text span', '.le-call', '.cockpit-txdock .rtty-stop']) {
            // Scroll above the sticky TX dock, as an operator would. "Nearest"
            // counts text covered by that dock as already in the viewport.
            const reachable = await evaluate(`(()=>{const e=document.querySelector('.${mode}-cockpit ${selector}');e.scrollIntoView({block:'start',inline:'nearest'});const r=e.getBoundingClientRect();return r.width>0 && r.height>0 && e.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))})()`)
            if (!reachable) {
              console.log('Keyboard geometry diagnostic', await evaluate(`(()=>{const e=document.querySelector('.${mode}-cockpit ${selector}'),r=e.getBoundingClientRect(),chain=[];for(let p=e;p;p=p.parentElement){const c=getComputedStyle(p),b=p.getBoundingClientRect();chain.push({class:p.className,top:b.top,bottom:b.bottom,height:b.height,scroll:p.scrollHeight,client:p.clientHeight,y:c.overflowY})}return{chain,hit:document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)?.className}})()`))
              if (artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${mode}-failure.png`),Buffer.from(shot.data,'base64')) }
            }
            assert.ok(reachable, `${mode} ${selector} is reachable at ${width}x${height}, zoom ${zoom}`)
          }
          results.push({ workspace:true, mode, width, height, zoom, theme, shape })
        }
      }
      const state = applicationData[`get_${mode}_state`]
      state.armed = false; applicationRevision++
      await until(`document.querySelector('.${mode}-cockpit')?.textContent.includes('Start the decoder in Nexus at the shack')`)
      unavailableTopics.add(`get_${mode}_state`)
      await until(`document.querySelector('.${mode}-cockpit')?.textContent.includes('Station decoder data unavailable.')`)
      assert.equal(await evaluate(`document.querySelector('.app')?.dataset.remoteStale==='true'`), false)
      assert.ok(!await evaluate(`document.querySelector('.${mode}-cockpit .cw-decode-text')?.textContent.includes('CQ W1AW')`))
      unavailableTopics.delete(`get_${mode}_state`); state.armed = true; applicationRevision++
      try { await until(`document.querySelector('.${mode}-cockpit .cw-decode-text')?.textContent==='CQ W1AW'`) }
      catch (error) {
        console.log('Keyboard observation diagnostic', {applicationVersion,mode,byCommand:applicationTraffic.byCommand},await evaluate(`({body:document.querySelector('.${mode}-cockpit')?.textContent,status:document.querySelector('.remote-application-status')?.textContent,unavailable:document.querySelector('.remote-view-unavailable')?.textContent,closures:window.__socketClosures})`))
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-keyboard-failure.png'),Buffer.from(shot.data,'base64'))}
        throw error
      }
      await browser.call('Emulation.setDeviceMetricsOverride', { width:1280,height:800,deviceScaleFactor:1,mobile:false }, session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      if (artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${mode}.png`),Buffer.from(shot.data,'base64')) }
    }
    await click(button('FT'))
    if (applicationVersion >= 3) {
      await until(`document.querySelector('.operate-host:not([hidden])')?.textContent.includes('CQ ZL1HIST RF72')`)
      for(const [label,selector,call] of [['Needed','.needed-panel','W1AW'],['Spots','.spots-panel','W1AW'],['Logbook','.logbook','K1T129']]) {
        await click(button(label))
        await until(`document.body.textContent.includes(${JSON.stringify(call)})`)
        assert.equal(await evaluate(`!!document.querySelector('.view-crash')`),false)
        for(const [width,height] of [[390,844],[1024,768],[1280,800],[3440,1440]]) {
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await settledLayout()
          const shape=await evaluate(`({docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight})`)
          assert.ok(shape.docW<=width+1 && shape.docH<=height+1,`${label} collection must fit the shell: ${JSON.stringify(shape)}`)
          results.push({workspace:true,collection:label,width,height,shape})
        }
        if(label==='Logbook') {
          assert.equal(await evaluate(`document.querySelectorAll('.log-rowactions button, .log-actions').length`),0,'read-only log must not mount writes, uploads or exports')
          await click(button('Next'))
          await until(`document.body.textContent.includes('Contacts 129–130 · Matches: 130')`)
          await click(button('Previous'))
          await until(`document.body.textContent.includes('Contacts 1–128 · Matches: 130')`)
        }
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${label.toLowerCase()}.png`),Buffer.from(shot.data,'base64'))}
      }
      assert.ok(applicationTraffic.queries>3,'actual compiled browser issues reviewed collection reads')
      await click(button('FT'))
      await until(`!!document.querySelector('.operate-host:not([hidden])')`)
    }
    if (applicationVersion >= 4) {
      await click(`document.querySelectorAll('.cockpit-layout-toggle button')[1]`)
      await until(`!!document.querySelector('.or-row[aria-selected]')`)
      await click(`document.querySelector('.or-row[aria-selected]')`)
      try { await until(`document.querySelector('.operate-host .recall-card')?.textContent.includes('Recall browser note')`) }
      catch (error) {
        console.log('Recall selection diagnostic', await evaluate(`({rows:[...document.querySelectorAll('.or-row')].map(e=>({selected:e.getAttribute('aria-selected'),call:e.querySelector('.or-call')?.textContent})),cards:[...document.querySelectorAll('.operate-host .recall-card')].map(e=>e.textContent),sides:document.querySelectorAll('.operate-host .cockpit-side').length,status:document.querySelector('.remote-application-status')?.textContent})`))
        throw error
      }
      for (const layout of [0,1]) {
        await click(`document.querySelectorAll('.cockpit-layout-toggle button')[${layout}]`)
        for (const [width,height] of [[1024,768],[1280,800],[1200,1390],[3440,1440]]) for (const zoom of [1,1.75]) {
          await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
          await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');window.dispatchEvent(new Event('resize'))`)
          await settledLayout()
          // At narrow effective widths Nexus stacks its panes in an existing
          // scroller. Check reachability, not an assumption that every pane is
          // above the fold. elementFromPoint also detects clipping/occlusion.
          await evaluate(`document.querySelector('.operate-host .recall-card').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          const shape = await evaluate(`(()=>{const card=document.querySelector('.operate-host .recall-card'),r=card.getBoundingClientRect(),header=document.querySelector('.remote-application-status').getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,cardTop:r.top,cardBottom:r.bottom,cardHeight:r.height,headerTop:header.top,reachable:card.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))}})()`)
          const reachable=shape.docW<=width+1 && shape.docH<=height+1 && shape.cardHeight>0 && shape.cardTop>=0 && shape.cardBottom<=height+1 && shape.headerTop>=0 && shape.reachable
          if(!reachable){console.log('Recall geometry diagnostic',await evaluate(`(()=>{const card=document.querySelector('.operate-host .recall-card'),r=card.getBoundingClientRect(),chain=[];for(let e=card;e;e=e.parentElement){const c=getComputedStyle(e),b=e.getBoundingClientRect();chain.push({class:e.className,top:b.top,bottom:b.bottom,scroll:e.scrollHeight,client:e.clientHeight,y:c.overflowY})}return {chain,hit:document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)?.className}})()`));if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-recall-failure.png'),Buffer.from(shot.data,'base64'))}}
          assert.ok(reachable, `recall remains reachable inside its existing pane: ${JSON.stringify({layout,width,height,zoom,shape})}`)
          results.push({recall:true,layout,width,height,zoom,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-recall.png'),Buffer.from(shot.data,'base64'))}
      await click(`document.querySelector('.operate-host .recall-log-row')`)
      await until(`!!document.querySelector('.logbook')`)
      assert.ok(await evaluate(`[...document.querySelectorAll('.logbook input')].some(e=>e.value==='W1AW')`),'recall navigates to the existing filtered Logbook')
      await click(button('FT'))
    }
    for (const [label, selector, scroller, first, last] of [
      ['Awards', '.awards-journey', '.aj-scroll', '.aw-card', '.aw-achievements'],
      ['Stats', '.stats-view', '.stats-view', '.stats-summary', '.stats-card:last-child'],
    ]) {
      await click(button(label))
      if (applicationVersion < 6) {
        await until(`!!document.querySelector('.remote-view-unavailable')`)
        assert.equal(insightQueries.length, 0)
        continue
      }
      try { await until(`document.querySelector('.remote-insights-status')?.textContent.includes('All 2301 station contacts.') && !!document.querySelector('${selector}')`) }
      catch (error) { console.log('Summary diagnostic', { label, insightQueries }, await evaluate(`({status:document.querySelector('.remote-insights-status')?.textContent,unavailable:document.querySelector('.remote-view-unavailable')?.textContent,root:document.querySelector('.remote-application-status')?.textContent,summary:document.querySelector('${selector}')?.textContent.slice(0,250)})`));throw error }
      assert.equal(await evaluate(`document.querySelectorAll('.conf-btn, .conf-panel').length`), 0, 'observation never mounts confirmation uploads')
      if (label === 'Awards') {
        assert.ok(await evaluate(`[...document.querySelectorAll('.aj-tab')].some(e=>e.disabled && e.textContent==='Journey')`))
        assert.ok(await evaluate(`document.querySelector('.awards-journey').textContent.includes('Japan')`))
      } else {
        assert.ok(await evaluate(`document.querySelector('.stats-summary').textContent.includes('2012')`))
        assert.ok(await evaluate(`document.querySelectorAll('.stats-bar-fill').length>5`))
      }
      for (const [width,height,zoom] of [[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1366,768,1],
        [1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.remote-insights-status button', `${selector} ${first}`, `${selector} ${last}`]) {
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          const shape = await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),s=document.querySelector('${scroller}');return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,height:r.height,scrollerHeight:s.clientHeight,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const reachable = shape.docW<=width+1 && shape.docH<=height+1 && shape.height>0 && shape.scrollerHeight>0 && shape.top<height && shape.bottom>0 && shape.reachable
          if (!reachable) console.log('Summary geometry diagnostic',await evaluate(`(()=>{const chain=[];for(let e=document.querySelector('${target}');e;e=e.parentElement){const c=getComputedStyle(e),r=e.getBoundingClientRect();chain.push({class:e.className,top:r.top,bottom:r.bottom,width:r.width,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,x:c.overflowX,y:c.overflowY})}return chain})()`))
          if (!reachable && artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${label.toLowerCase()}-failure.png`),Buffer.from(shot.data,'base64')) }
          assert.ok(reachable, `summary content remains reachable: ${JSON.stringify({label,target,width,height,zoom,theme,shape})}`)
          results.push({insights:label,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'));document.querySelector('${scroller}').scrollTop=0`)
      await settledLayout()
      if (artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${label.toLowerCase()}.png`),Buffer.from(shot.data,'base64')) }
    }
    if (applicationVersion >= 6) {
      insights.statistics.uniqueCalls = 2013
      await click(button('Refresh summary'))
      await until(`document.querySelector('.stats-summary')?.textContent.includes('2013')`)
      unavailableCollections.add('statistics')
      await click(button('Refresh summary'))
      await until(`!document.querySelector('.stats-view') && document.querySelector('.remote-insights-status')?.textContent.includes('unavailable')`)
      assert.equal(await evaluate(`document.querySelector('.app').dataset.remoteStale==='true'`), false, 'a summary failure must not mark live station readings stale')
      unavailableCollections.delete('statistics')
      await click(button('Refresh summary'))
      await until(`document.querySelector('.stats-summary')?.textContent.includes('2013')`)
    } else await click(button('FT'))
    await click(button('DXped'))
    if (applicationVersion < 7) {
      await until(`!!document.querySelector('.remote-view-unavailable')`)
      assert.equal(dxQueries.length, 0, 'older stations receive no DX board request')
    } else {
      await until(`document.querySelector('.dxped-view')?.textContent.includes('Bouvet Island')`)
      assert.equal(await evaluate(`document.querySelectorAll('.dxped-view .wn-work,.dxped-view .wn-chase,.dxped-view .cal-alarm,.dxped-view .cal-chase,.dx-map-link,.dxped-popout').length`), 0)
      assert.ok(await evaluate(`document.querySelector('.remote-insights-status')?.textContent.includes('Station DX board updated')`))
      for (const [width,height,zoom] of [[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1366,768,1],[1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.remote-insights-status button', '.worknow-card .wn-details', '.dxped-calendar .cal-viewtabs button:last-child']) {
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          const shape = await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,height:r.height,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const reachable = shape.docW<=width+1 && shape.docH<=height+1 && shape.height>0 && shape.top<height && shape.bottom>0 && shape.reachable
          if (!reachable) console.log('DXpeditions geometry diagnostic', await evaluate(`(()=>{const chain=[];for(let e=document.querySelector('${target}');e;e=e.parentElement){const c=getComputedStyle(e),r=e.getBoundingClientRect();chain.push({class:e.className,top:r.top,bottom:r.bottom,width:r.width,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,x:c.overflowX,y:c.overflowY})}return chain})()`))
          if (!reachable && artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-dxpeditions-failure.png'),Buffer.from(shot.data,'base64')) }
          assert.ok(reachable, `DXpeditions content remains reachable: ${JSON.stringify({target,width,height,zoom,theme,shape})}`)
          results.push({dxpeditions:true,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'));document.querySelector('.dxped-view').scrollTop=0`)
      await settledLayout()
      await click(`document.querySelector('.wn-details')`)
      await until(`!!document.querySelector('.worknow-card .heatmap')`)
      if (artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-dxpeditions.png'),Buffer.from(shot.data,'base64')) }
      dxpeditions.dxpeditions.workableNow[0].entity = 'Updated test entity'
      await click(button('Refresh DXpeditions'))
      await until(`document.querySelector('.dxped-view')?.textContent.includes('Updated test entity')`)
      unavailableCollections.add('dxpeditions')
      await click(button('Refresh DXpeditions'))
      await until(`!document.querySelector('.dxped-view') && document.querySelector('.remote-insights-status > span')?.textContent.includes('Station data unavailable') && !${button('Refresh DXpeditions')}?.disabled`)
      assert.equal(await evaluate(`document.querySelector('.app').dataset.remoteStale==='true'`), false)
      unavailableCollections.delete('dxpeditions'); await click(button('Refresh DXpeditions'))
      await until(`!!document.querySelector('.dxped-view')`)
    }
    await click(button('Memories'))
    if (applicationVersion < 8) {
      await until(`!!document.querySelector('.remote-view-unavailable')`)
      assert.equal(memoryQueries.length, 0, 'older stations receive no memory bank request')
    } else {
      await until(`document.querySelector('.memories-view')?.textContent.includes('Evening test net')`)
      assert.equal(await evaluate(`document.querySelectorAll('.memories-view .mv-row-tune,.memories-view .mv-row-edit,.memories-view .mv-row-del,.memories-view .mv-row-move,.memories-view .mv-side-add,.memories-view input[type=file]').length`), 0)
      assert.equal(await evaluate(`localStorage.getItem('nexus.memory.bank.v2')`), null, 'the station bank must not become browser storage')
      for (const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],
        [1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.remote-insights-status button', '.mv-search', '.mv-list .mv-row:last-child']) {
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,height:r.height,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const reachable=shape.docW<=width+1&&shape.docH<=height+1&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable
          if (!reachable) console.log('Memories geometry diagnostic', await evaluate(`(()=>{const chain=[];for(let e=document.querySelector('${target}');e;e=e.parentElement){const c=getComputedStyle(e),r=e.getBoundingClientRect();chain.push({class:e.className,top:r.top,bottom:r.bottom,width:r.width,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,x:c.overflowX,y:c.overflowY})}return chain})()`))
          if (!reachable&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-memories-failure.png'),Buffer.from(shot.data,'base64'))}
          assert.ok(reachable,`Memories content remains reachable: ${JSON.stringify({target,width,height,zoom,theme,shape})}`)
          results.push({memories:true,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-memories.png'),Buffer.from(shot.data,'base64'))}
      await click(`[...document.querySelectorAll('.mv-side-item')].find(b=>b.textContent.includes('Local repeaters'))`)
      await until(`!document.querySelector('.mv-list')?.textContent.includes('Evening test net')`)
      await click(`[...document.querySelectorAll('.mv-toolbar button')].find(b=>b.textContent.includes('Grid'))`)
      await until(`document.querySelector('.mv-grid')?.textContent.includes('P25')`)
      assert.equal(await evaluate(`document.querySelectorAll('.mv-grid input:not([type=checkbox]):not([readonly]),.mv-grid select,.mv-grid .mv-row-actions button').length`),0)
      for (const [width,height,zoom] of [[390,844,1],[844,390,1],[1280,800,1],[390,844,1.75]]) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        await evaluate(`document.querySelector('.mv-grid tbody tr:last-child td:nth-last-child(2)').scrollIntoView({block:'nearest',inline:'nearest'})`)
        await settledLayout()
        const shape=await evaluate(`(()=>{const e=document.querySelector('.mv-grid tbody tr:last-child td:nth-last-child(2)'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,left:r.left,right:r.right,width:r.width,height:r.height,hit:document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)?.className,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))}})()`)
        if (!shape.reachable && artifacts) { const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-memories-grid-failure.png'),Buffer.from(shot.data,'base64')) }
        assert.ok(shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.reachable,`Memory grid cell remains reachable: ${JSON.stringify({width,height,zoom,shape})}`)
        results.push({memories:true,grid:true,width,height,zoom,shape})
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      bank.memories[1].name='Updated station memory'
      await click(button('Refresh memories'))
      await until(`[...document.querySelectorAll('.mv-grid input')].some(e=>e.value==='Updated station memory')`)
      assert.equal(await evaluate(`document.querySelector('.mv-side-item.active')?.textContent.includes('Local repeaters')`),true,'refresh preserves the selected group')
      unavailableCollections.add('memories'); await click(button('Refresh memories'))
      await until(`!document.querySelector('.remote-memory-bank:not([hidden])')&&document.querySelector('.remote-insights-status > span')?.textContent.includes('Station data unavailable')&&!${button('Refresh memories')}?.disabled`)
      unavailableCollections.delete('memories'); await click(button('Refresh memories'))
      await until(`[...document.querySelectorAll('.remote-memory-bank:not([hidden]) .mv-grid input')].some(e=>e.value==='Updated station memory')`)
    }
    if (applicationVersion >= 9) {
      await click(button('POTA/SOTA'))
      await until(`document.querySelector('.pota-view')?.textContent.includes('Logged park')`)
      assert.equal(await evaluate(`document.querySelectorAll('.pota-hunt-btn,.pota-hunt-clear,.pota-act-start,.pota-popout,.pota-view input[type=file]').length`),0)
      const reads=otaQueries.length
      await click(`[...document.querySelectorAll('.pota-controls [role=tab]')].find(b=>b.textContent==='Both')`)
      await until(`document.querySelector('.pota-spot-list')?.textContent.includes('Test summit')`)
      await click(`[...document.querySelectorAll('.pota-filter-row button')].find(b=>b.textContent==='CW')`)
      await until(`document.querySelector('.pota-spot-list')?.children.length===1`)
      assert.ok(await evaluate(`document.querySelector('.pota-spot-list')?.textContent.includes('Imported park')`))
      await click(`[...document.querySelectorAll('.pota-filter-row')].find(e=>e.getAttribute('aria-label')?.toLowerCase().includes('mode'))?.querySelector('button')`)
      await until(`document.querySelector('.pota-spot-list')?.children.length===4`)
      assert.equal(otaQueries.length,reads,'local filtering must not fetch or act on the station')
      // A short feed fits without exercising the list's real scroll owner.
      ota.feeds[0].spots.push(...Array.from({length:40},(_,i)=>({...ota.feeds[0].spots[0],reference:`US-${1000+i}`,name:`Long station park name ${i} with additional location detail`})))
      await click(button('Refresh station data'))
      await until(`document.querySelector('.pota-spot-list')?.children.length===44`)
      for (const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],
        [1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.remote-insights-status button','.pota-sort-pick','.pota-spot-list .pota-spot:last-child .pota-spot-meta']) {
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,width:r.width,height:r.height,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const clipped=await evaluate(`(()=>{const result=[];for(let e=document.querySelector('${target}').parentElement;e;e=e.parentElement){const c=getComputedStyle(e);if(['hidden','clip'].includes(c.overflowY)&&e.scrollHeight>e.clientHeight+1)result.push({class:e.className,scroll:e.scrollHeight,client:e.clientHeight,y:c.overflowY})}return result})()`)
          assert.deepEqual(clipped,[],`POTA/SOTA scrolling must remain available to user input: ${JSON.stringify({target,width,height,zoom,theme})}`)
          const reachable=shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable
          if (!reachable) console.log('OTA geometry diagnostic',await evaluate(`(()=>{const chain=[];for(let e=document.querySelector('${target}');e;e=e.parentElement){const c=getComputedStyle(e),r=e.getBoundingClientRect();chain.push({class:e.className,top:r.top,bottom:r.bottom,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,x:c.overflowX,y:c.overflowY})}return chain})()`))
          if (!reachable&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-ota-failure.png'),Buffer.from(shot.data,'base64'))}
          assert.ok(reachable,`POTA/SOTA content remains reachable: ${JSON.stringify({target,width,height,zoom,theme,shape})}`)
          results.push({ota:true,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      await evaluate(`document.querySelector('.remote-ota-view').scrollTop=0`)
      await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-ota.png'),Buffer.from(shot.data,'base64'))}
      ota.feeds[0].spots.splice(3)
      await click(`[...document.querySelectorAll('.pota-controls [role=tab]')].find(b=>b.textContent==='SOTA')`)
      ota.feeds[1].spots[0].name='Updated test summit'
      await click(button('Refresh station data'))
      await until(`document.querySelector('.pota-spot-list')?.textContent.includes('Updated test summit')`)
      assert.equal(await evaluate(`document.querySelector('.pota-controls [role=tab][aria-selected=true]')?.textContent`),'SOTA')
      const originalSpots=ota.feeds[0].spots
      Object.assign(ota.feeds[0],{status:'expired',sourceAgeMs:900000,spots:[]})
      await click(button('Refresh station data'))
      await until(`document.querySelector('.remote-ota-feeds')?.textContent.includes('expired')`)
      assert.ok(await evaluate(`document.querySelector('.pota-spot-list')?.textContent.includes('Updated test summit')`))
      Object.assign(ota.feeds[0],{status:'ready',sourceAgeMs:0,spots:originalSpots})
      unavailableCollections.add('ota'); await click(button('Refresh station data'))
      await until(`!document.querySelector('.remote-ota-bank:not([hidden])')&&!${button('Refresh station data')}?.disabled`)
      unavailableCollections.delete('ota'); await click(button('Refresh station data'))
      await until(`document.querySelector('.pota-spot-list')?.textContent.includes('Updated test summit')`)
    }
    if (applicationVersion >= 10) {
      await click(button('Field Day'))
      assert.equal(await evaluate(`document.body.textContent.includes('Could not switch mode')`), false)
      await until(`document.querySelector('.fieldday')?.textContent.includes('K1ABC')`)
      await until(`!!document.querySelector('.fd-bonuses-list')`)
      assert.equal(await evaluate(`document.querySelectorAll('.fieldday .export-btn,.fieldday .fd-role-btn:not(:disabled),.fieldday .fd-power-chip:not(:disabled),.fieldday .fd-bonus-row input:not(:disabled)').length`),0)
      assert.equal(await evaluate(`document.querySelector('.fieldday input:not([type=checkbox])')?.readOnly`),true)
      fieldDay.fieldDay.log.push(...Array.from({length:48},(_,i)=>({...fieldDay.fieldDay.log[0],call:`K1FD${i}`})))
      Object.assign(fieldDay.fieldDay,{qsoCount:50,points:99,poweredPoints:198,totalScore:298})
      await click(button('Refresh Field Day'))
      await until(`document.querySelectorAll('.fd-log-row').length===50`)
      assert.notEqual(await evaluate(`getComputedStyle(document.querySelector('.remote-field-day-bank')).display`), 'none', 'the event is visible with current data')
      for (const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],
        [1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.remote-insights-status button','.fd-bonuses-toggle','.fd-bonus-row:last-child .fd-bonus-label','.fd-log-row:last-child .fd-col.call']) {
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest'})`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,width:r.width,height:r.height,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const clipped=await evaluate(`(()=>{const result=[];for(let e=document.querySelector('${target}').parentElement;e;e=e.parentElement){const c=getComputedStyle(e);if(['hidden','clip'].includes(c.overflowY)&&e.scrollHeight>e.clientHeight+1)result.push({class:e.className,scroll:e.scrollHeight,client:e.clientHeight,y:c.overflowY})}return result})()`)
          const reachable=shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable&&clipped.length===0
          if (!reachable&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-field-day-failure.png'),Buffer.from(shot.data,'base64'))}
          assert.ok(reachable,`Field Day content remains reachable by user input: ${JSON.stringify({target,width,height,zoom,theme,shape,clipped})}`)
          results.push({fieldDay:true,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      await evaluate(`document.querySelector('.fieldday').scrollTop=0`)
      await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-field-day.png'),Buffer.from(shot.data,'base64'))}
      await click(`document.querySelector('.fd-bonuses-toggle')`)
      fieldDay.settings.fdOperator='K9TEST'
      fieldDay.fieldDay.event='wfd';fieldDay.ruleset.event='wfd'
      Object.assign(fieldDay.fieldDay,{poweredPoints:99,totalScore:199,eventEndUnix:fieldDay.fieldDay.eventStartUnix+30*3600})
      await click(button('Refresh Field Day'))
      await until(`document.querySelector('.fieldday input:not([type=checkbox])')?.value==='K9TEST'`)
      assert.equal(await evaluate(`!!document.querySelector('.fd-bonuses-list')`),false,'refresh preserves the collapsed disclosure')
      assert.equal(await evaluate(`document.querySelector('.fd-score-math')?.textContent.toLowerCase().includes('power')`),false,'WFD does not display ARRL multiplier math')
      unavailableCollections.add('fieldDay');await click(button('Refresh Field Day'))
      await until(`!document.querySelector('.remote-field-day-bank:not([hidden])')&&!${button('Refresh Field Day')}?.disabled`)
      const hiddenDisplay = await evaluate(`getComputedStyle(document.querySelector('.remote-field-day-bank')).display`)
      if(hiddenDisplay!=='none'&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-field-day-unavailable-failure.png'),Buffer.from(shot.data,'base64'))}
      assert.equal(hiddenDisplay, 'none', 'a failed refresh removes the visible score, not only the hidden attribute')
      unavailableCollections.delete('fieldDay');await click(button('Refresh Field Day'))
      await until(`document.querySelector('.fieldday input:not([type=checkbox])')?.value==='K9TEST'`)
    }
    await click(button('JS8'))
    if(applicationVersion<11){
      await until(`!!document.querySelector('.remote-view-unavailable')`)
      assert.equal(await evaluate(`!!document.querySelector('.js8-cockpit')`),false)
      assert.equal(js8Queries.length,0)
      assert.equal(applicationTraffic.byCommand.get_js8_state??0,0)
    }else{
      js8Fixture.state.activity.push(...Array.from({length:76},(_,i)=>({...js8Fixture.state.activity[i%4],text:`JS8 HISTORY ${i+1}`,atMs:js8Start+i,freqHz:600+i*20})))
      js8Fixture.state.inbox.push(...Array.from({length:39},(_,i)=>({...js8Fixture.state.inbox[0],id:i+2,text:`JS8 MAIL ${i+1}`})))
      js8Fixture.state.stations.push(...Array.from({length:38},(_,i)=>({...js8Fixture.state.stations[0],call:`K1JS${i}`})))
      for(const row of js8Fixture.state.stations)collections.js8Context.meta.history[row.call]??={count:0,lastUnix:null,grid:'',name:'',comment:''}
      js8Fixture.state.queue.push(...Array.from({length:38},(_,i)=>({...js8Fixture.state.queue[1],display:`JS8 QUEUE ${i+1}`})))
      await until(`document.querySelectorAll('.js8-row').length===80 && document.querySelector('.js8-stations')?.textContent.includes('COMPLETE LOG')`)
      assert.equal(await evaluate(`document.querySelectorAll('.js8-cockpit').length`),1)
      assert.equal(await evaluate(`document.querySelectorAll('.js8-cockpit .waterfall-wrap').length`),1)
      assert.equal(await evaluate(`!!document.querySelector('.grid-stations,.grid-center .conversation')`),false)
      assert.ok(await evaluate(`[...document.querySelectorAll('.js8-speed-chip,.js8-query,.js8-inbox-act,.js8-send,.js8-cq,.js8-hb,.js8-arm,.js8-cancel,.js8-drop')].length>15 && [...document.querySelectorAll('.js8-speed-chip,.js8-query,.js8-inbox-act,.js8-send,.js8-cq,.js8-hb,.js8-arm,.js8-cancel,.js8-drop')].every(e=>e.disabled)`))
      await click(`document.querySelector('.js8-station-call')`)
      await until(`document.querySelector('.js8-to')?.value==='W1AW' && document.querySelector('.js8-cockpit .le-call')?.value==='W1AW'`)
      await click(`document.querySelector('.js8-pin')`)
      assert.equal(await evaluate(`document.querySelector('.js8-pin').getAttribute('aria-pressed')`),'true')
      for(const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        // Viewport scaling schedules a second ResizeObserver/RAF pass for the
        // native pane columns. Measurements showed a 2→1 column change AFTER
        // the first layout wait, which invalidated an already-completed scroll.
        await settledLayout()
        for(const target of ['.js8-row:last-child .js8-text','.js8-station:last-child .js8-station-call','.js8-inbox-row:last-child .js8-text','.js8-offset-row:last-child .js8-text','.js8-compose','.js8-queue-item:last-of-type .js8-queue-text']){
          const beforeScroll=await evaluate(`(()=>{window.__js8ScrollNode=document.querySelector('${target}');window.__js8ScrollNode.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});const chain=[];for(let e=window.__js8ScrollNode;e;e=e.parentElement){const r=e.getBoundingClientRect();chain.push({class:e.className,top:r.top,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,at:e.scrollTop,cols:e.dataset.cols,flow:e.dataset.flow})}return chain})()`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),x=Math.max(r.left+1,Math.min(r.right-1,r.left+r.width/2)),y=Math.max(1,Math.min(innerHeight-1,r.top+r.height/2));return {zoom:Number(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,width:r.width,height:r.height,reachable:e.contains(document.elementFromPoint(x,y))}})()`)
          const clipped=await evaluate(`(()=>{const result=[];for(let e=document.querySelector('${target}').parentElement;e;e=e.parentElement){const c=getComputedStyle(e);if(['hidden','clip'].includes(c.overflowY)&&e.scrollHeight>e.clientHeight+1)result.push({class:e.className,scroll:e.scrollHeight,client:e.clientHeight})}return result})()`)
          const reachable=Math.abs(shape.zoom-zoom)<0.001&&shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable&&clipped.length===0
          if(!reachable){console.log('JS8 scroll diagnostic',JSON.stringify({before:beforeScroll,after:await evaluate(`(()=>{const chain=[];for(let e=document.querySelector('${target}');e;e=e.parentElement){const r=e.getBoundingClientRect(),c=getComputedStyle(e);chain.push({class:e.className,top:r.top,height:r.height,scroll:e.scrollHeight,client:e.clientHeight,at:e.scrollTop,y:c.overflowY,cols:e.dataset.cols,flow:e.dataset.flow})}return {sameNode:window.__js8ScrollNode===document.querySelector('${target}'),chain}})()`)}));if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-js8-failure.png'),Buffer.from(shot.data,'base64'))}}
          assert.ok(reachable,`JS8 content remains reachable: ${JSON.stringify({width,height,zoom,theme,target,shape,clipped})}`)
          results.push({js8:true,width,height,zoom,theme,target,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      await click(`document.querySelector('.js8-cockpit .panels-menu-btn')`)
      const inboxCheck=`[...document.querySelectorAll('.js8-cockpit .panels-menu-check')].find(e=>/inbox/i.test(e.textContent)).querySelector('input')`
      await click(inboxCheck)
      assert.equal(await evaluate(`!!document.querySelector('.js8-inbox')`),false)
      assert.equal(await evaluate(`document.querySelectorAll('.js8-queue-item').length`),40)
      await click(inboxCheck)
      await until(`document.querySelectorAll('.js8-inbox-row').length===40`)
      await click(`document.querySelector('.js8-cockpit .panels-menu-btn')`)
      await evaluate(`document.querySelector('.js8-cockpit').scrollTop=0`)
      await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-js8.png'),Buffer.from(shot.data,'base64'))}
      unavailableTopics.add('get_js8_state')
      await until(`document.querySelectorAll('.js8-row,.js8-inbox-row,.js8-pending-row').length===0`)
      unavailableTopics.delete('get_js8_state')
      await until(`document.querySelectorAll('.js8-row').length===80 && document.querySelector('.js8-opcomment')?.textContent==='COMPLETE LOG' && document.querySelector('.js8-history-status button')?.disabled===false`)
      unavailableCollections.add('js8Context');await click(`document.querySelector('.js8-history-status button')`)
      await until(`document.querySelector('.js8-history-status')?.textContent.includes('unavailable')`)
      assert.equal(await evaluate(`!!document.querySelector('.js8-opcomment')`),false)
      assert.ok(await evaluate(`document.querySelector('.js8-b4')?.textContent==='—'`))
      unavailableCollections.delete('js8Context');await click(`document.querySelector('.js8-history-status button')`)
      await until(`document.querySelector('.js8-opcomment')?.textContent==='COMPLETE LOG'`)
      await browser.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape'},session)
      assert.ok(js8Queries.length>0)
    }
    for(const mode of ['SSTV','APRS']) {
      await click(button(mode))
      if(applicationVersion<12){await until(`!!document.querySelector('.remote-view-unavailable')`);continue}
      const sstv=mode==='SSTV',root=sstv?'.sstv-view':'.aprs-cockpit'
      await until(sstv?`document.querySelectorAll('.sstv-thumb').length===40`:`document.querySelector('.aprs-cockpit')?.textContent.includes('K36LAY')`)
      const controls=sstv?'.sstv-arm,.sstv-tx-send,.sstv-tx-stop,.sstv-thumb-del':'.aprs-retune,.aprs-beacon-send'
      assert.ok(await evaluate(`[...document.querySelectorAll('${controls}')].length>1&&[...document.querySelectorAll('${controls}')].every(e=>e.disabled)`))
      if(sstv){
        await settledLayout();await settledLayout()
        await evaluate(`document.querySelector('.sstv-thumb').scrollIntoView({block:'nearest',behavior:'instant'})`)
        await settledLayout();await settledLayout()
        try { await until(`(()=>{const e=document.querySelector('.sstv-thumb-img');return e?.complete&&e.naturalWidth===320&&e.src.startsWith('blob:')})()`) }
        catch(error){console.log('SSTV image diagnostic',await evaluate(`(()=>{const e=document.querySelector('.sstv-thumb'),r=e.getBoundingClientRect();return {html:e.innerHTML,rect:r.toJSON(),hit:document.elementFromPoint(r.x+r.width/2,Math.min(innerHeight-1,r.y+r.height/2))?.className,violations:window.__imagePolicyViolations}})()`));throw error}

        assert.equal(await evaluate(`document.querySelector('.sstv-thumb').getAttribute('title')`),null)
      }
      const targets=sstv?['.sstv-live-caption','.sstv-tx-mode select','.sstv-thumb:last-child .sstv-thumb-caption','.sstv-tx-drop']:
        ['.aprs-health','.aprs-beacon-comment input','.aprs-message-compose input','.aprs-table tbody tr:last-child .aprs-from','.aprs-map']
      for(const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout();await settledLayout()
        for(const target of targets){
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'})`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),x=Math.max(r.left+1,Math.min(r.right-1,r.left+r.width/2)),y=Math.max(1,Math.min(innerHeight-1,r.top+r.height/2));return {zoom:Number(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,width:r.width,height:r.height,reachable:e.contains(document.elementFromPoint(x,y))}})()`)
          const clipped=await evaluate(`(()=>{const result=[];for(let e=document.querySelector('${target}').parentElement;e;e=e.parentElement){const c=getComputedStyle(e);if(['hidden','clip'].includes(c.overflowY)&&e.scrollHeight>e.clientHeight+1)result.push({class:e.className,scroll:e.scrollHeight,client:e.clientHeight})}return result})()`)
          const reachable=Math.abs(shape.zoom-zoom)<0.001&&shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable&&clipped.length===0
          if(!reachable){
            console.log('Station mode geometry diagnostic',JSON.stringify(await evaluate(`(()=>{const target=document.querySelector('${target}'),r=target.getBoundingClientRect(),hit=document.elementFromPoint(Math.max(r.left+1,Math.min(r.right-1,r.left+r.width/2)),Math.max(1,Math.min(innerHeight-1,r.top+r.height/2))),chain=[];for(let e=target;e;e=e.parentElement){const r=e.getBoundingClientRect(),c=getComputedStyle(e);chain.push({class:e.className,rect:r.toJSON(),height:e.scrollHeight,at:e.scrollTop,width:e.scrollWidth,x:c.overflowX,y:c.overflowY,display:c.display})}return {hit:hit?.outerHTML.slice(0,500),chain}})()`)))
            if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-${mode.toLowerCase()}-failure.png`),Buffer.from(shot.data,'base64'))}
          }
          assert.ok(reachable,`${mode} content remains reachable: ${JSON.stringify({width,height,zoom,theme,target,shape,clipped})}`)
          results.push({stationMode:mode,width,height,zoom,theme,target,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'));document.querySelector('.remote-workspace').scrollTop=0;for(const e of document.querySelectorAll('.remote-workspace *'))if(/auto|scroll/.test(getComputedStyle(e).overflowY))e.scrollTop=0`)
      await settledLayout();await settledLayout()
      if(sstv)await until(`document.querySelector('.sstv-thumb-img')?.complete===true&&document.querySelector('.sstv-thumb-img')?.naturalWidth===320`)
      else { const map=await evaluate(`(()=>{const e=document.querySelector('.aprs-map canvas'),r=e.getBoundingClientRect(),d=e.getContext('2d').getImageData(0,0,e.width,e.height).data,colors=new Set();for(let i=0;i<d.length;i+=Math.max(4,Math.floor(d.length/4000/4)*4))colors.add(Array.from(d.slice(i,i+4)).join(','));return {width:e.width,height:e.height,rect:r.toJSON(),colors:colors.size}})()`);assert.ok(map.rect.width>100&&map.rect.height>100&&map.width>300&&map.height>150&&map.colors>8,'APRS map actually painted: '+JSON.stringify(map)) }
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${mode.toLowerCase()}.png`),Buffer.from(shot.data,'base64'))}
      const topic=sstv?'get_sstv_state':'get_remote_aprs_state'
      unavailableTopics.add(topic)
      await until(sstv?`!document.querySelector('.sstv-live-canvas')&&document.querySelectorAll('.sstv-thumb').length===0`:`document.querySelector('.aprs-health')?.textContent.includes('unavailable')`)
      unavailableTopics.delete(topic)
      await until(sstv?`document.querySelectorAll('.sstv-thumb').length===40`:`!document.querySelector('.aprs-health')?.textContent.includes('unavailable')`)
    }

    for(const label of ['Connect','Satellites']){
      await click(button(label))
      if(applicationVersion<13){await until(`!!document.querySelector('.remote-view-unavailable')`);assert.equal(navigationQueries.length,0);continue}
      const connect=label==='Connect',root=connect?'.connect-shell':'.sats-view'
      await until(connect?`document.querySelector('.connect-header')?.textContent.includes('Station data · Read only')`:`document.querySelectorAll('.sat-pick').length===40`)
      if(connect){
        // The 2-D renderer is available even on headless/software-only hardware.
        if(await evaluate(`!!document.querySelector('.connect-header button')&&[...document.querySelectorAll('.connect-header button')].some(e=>e.textContent.includes('3D'))`))await click(`[...document.querySelectorAll('.connect-header button')].find(e=>e.textContent.includes('3D'))`)
        await until(`document.querySelector('.connect-map canvas')?.width>300`)
        await click(`[...document.querySelectorAll('.connect-intent button')][2]`)
        assert.equal(await evaluate(`document.querySelectorAll('.connect-intent button')[2].classList.contains('active')`),true)
      }else{
        await click(`[...document.querySelectorAll('.sat-pick')].find(e=>e.textContent.includes('ISS (ZARYA)'))`)
        await until(`document.querySelector('.sats-arm-id')?.textContent.includes('ISS (ZARYA)')`)
        // Catalog/detail readiness precedes the separate, coherent favorites
        // schedule batch. Measure the populated layout: otherwise its initial
        // arrival can move the same radio panel between scroll and hit-test.
        await until(`document.querySelectorAll('.sats-sched tbody.fav tr:not(.sats-inline-empty)').length>0`)
        console.log('Satellite favorites ready for geometry',JSON.stringify(await evaluate(`({rows:document.querySelectorAll('.sats-sched tbody.fav tr:not(.sats-inline-empty)').length,empty:!!document.querySelector('.sats-sched tbody.fav .sats-inline-empty')})`)))
        const actions=await evaluate(`[...document.querySelectorAll('.sat-track,.sat-work,input[name="sat-transponder"],.sat-chip.quiet')].map(e=>e.disabled)`)
        assert.ok(actions.length>0&&actions.every(Boolean),'station actions remain guarded until control is explicitly supported')
      }
      const targets=connect?['.connect-header','.connect-map canvas','.connect-strip']:['.sats-head','.sats-arm-id','.sats-radio','.sats-search','.sats-favmgr li:last-child .sat-pick']
      for(const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout();await settledLayout()
        for(const target of targets){
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('Missing navigation target',label,target,JSON.stringify(await sessionDiagnostic()));
          await evaluate(`(()=>{const e=document.querySelector('${target}');window.__navTarget=e;e.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});window.__navBefore=e.getBoundingClientRect().toJSON()})()`)
          await settledLayout();await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),x=Math.max(r.left+1,Math.min(r.right-1,r.left+r.width/2)),y=Math.max(1,Math.min(innerHeight-1,r.top+r.height/2)),clipped=[];for(let p=e.parentElement;p;p=p.parentElement){const c=getComputedStyle(p);if(['hidden','clip'].includes(c.overflowY)&&p.scrollHeight>p.clientHeight+1)clipped.push({class:p.className,scroll:p.scrollHeight,client:p.clientHeight})}let exposed=0;if(e.tagName==='CANVAS')for(let py=Math.max(1,r.top+8);py<Math.min(innerHeight-1,r.bottom-8);py+=16)for(let px=Math.max(1,r.left+8);px<Math.min(innerWidth-1,r.right-8);px+=16)if(e.contains(document.elementFromPoint(px,py)))exposed++;return {zoom:Number(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,rect:r.toJSON(),hit:document.elementFromPoint(x,y)?.outerHTML.slice(0,500),reachable:e.tagName==='CANVAS'?exposed>=4:e.contains(document.elementFromPoint(x,y)),exposed,clipped}})()`)
          const good=Math.abs(shape.zoom-zoom)<0.001&&shape.docW<=width+1&&shape.docH<=height+1&&shape.rect.width>0&&shape.rect.height>0&&shape.rect.top<height&&shape.rect.bottom>0&&shape.reachable&&shape.clipped.length===0
          if(!good)console.log('SESSION FAILURE',JSON.stringify(await sessionDiagnostic()));
          if(!good)console.log('Navigation layout trace',JSON.stringify(await evaluate(`(()=>{const e=document.querySelector('${target}'),chain=[];for(let p=e;p;p=p.parentElement){const r=p.getBoundingClientRect(),c=getComputedStyle(p);chain.push({class:p.className,rect:r.toJSON(),at:p.scrollTop,scroll:p.scrollHeight,client:p.clientHeight,y:c.overflowY})}return {same:e===window.__navTarget,before:window.__navBefore,viewport:document.documentElement.dataset.viewport,chain}})()`)));
          if(!good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-${label.toLowerCase()}-failure.png`),Buffer.from(shot.data,'base64'))}
          assert.ok(good,`${label} content remains reachable: ${JSON.stringify({width,height,zoom,theme,target,shape})}`)
          results.push({navigation:label,width,height,zoom,theme,target,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'));document.querySelector('.remote-workspace').scrollTop=0;for(const e of document.querySelectorAll('.remote-workspace *'))if(/auto|scroll/.test(getComputedStyle(e).overflowY))e.scrollTop=0`)
      await settledLayout();await settledLayout()
      if(connect){
        const canvas=await evaluate(`(()=>{const e=document.querySelector('.connect-map canvas'),d=e.getContext('2d').getImageData(0,0,e.width,e.height).data,colors=new Set();for(let i=0;i<d.length;i+=Math.max(4,Math.floor(d.length/4000/4)*4))colors.add(Array.from(d.slice(i,i+4)).join(','));return {width:e.width,height:e.height,colors:colors.size}})()`)
        assert.ok(canvas.width>300&&canvas.height>100&&canvas.colors>8,'Connect renders its actual map: '+JSON.stringify(canvas))
      }
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${label.toLowerCase()}.png`),Buffer.from(shot.data,'base64'))}
      const collection=connect?'connect':'satellites'
      unavailableCollections.add(collection)
      await until(connect?`!document.querySelector('.connect-header')?.textContent.includes('Station data · Read only')`:`document.querySelectorAll('.sat-pick').length===0`,35000)
      unavailableCollections.delete(collection)
      await until(connect?`document.querySelector('.connect-header')?.textContent.includes('Station data · Read only')`:`document.querySelectorAll('.sat-pick').length===40`)
    }
    for(const label of ['Settings','Program']){
      await click(button(label))
      if(applicationVersion<14){await until(`!!document.querySelector('.remote-view-unavailable')`);continue}
      const settings=label==='Settings'
      await until(settings?`!!document.querySelector('.settings-tabs')`:`document.querySelectorAll('.rp-chan-name').length===1200`)
      if(settings){
        const tabs=await evaluate(`[...document.querySelectorAll('.settings-tab')].map(e=>e.textContent)`)
        assert.ok(tabs.length>5)
        for(const name of tabs){await click(`[...document.querySelectorAll('.settings-tab')].find(e=>e.textContent===${JSON.stringify(name)})`);assert.equal(await evaluate(`!!document.querySelector('input[type="password"]')`),false)}
        await click(`[...document.querySelectorAll('.settings-tab')].find(e=>e.textContent==='Station')`)
        await until(`document.querySelector('#settings-operator-radio input')?.value==='W1AW'`)
        assert.ok(await evaluate(`[...document.querySelectorAll('#settings-operator-radio input,#settings-operator-radio select')].every(e=>e.matches(':disabled'))`))
      }else{
        assert.equal(await evaluate(`document.querySelector('.rp-chan-row:last-child .rp-chan-name')?.value`),'CH1199')
        assert.ok(await evaluate(`[...document.querySelectorAll('.radioprog button,.radioprog input,.radioprog select')].length>1000&&[...document.querySelectorAll('.radioprog button,.radioprog input,.radioprog select')].every(e=>e.disabled)`))
      }
      const targets=settings?['.settings-tab:last-child','#settings-operator-radio input','#settings-remote-access']:['.rp-origin','.rp-chan-row:first-child .rp-chan-name','.rp-chan-row:last-child .rp-chan-name','.rp-export-chirp']
      for(const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],[1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]])for(const theme of ['dark','light']){
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout();await settledLayout()
        for(const target of targets){
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('Missing navigation target',label,target,JSON.stringify(await sessionDiagnostic()));
          await evaluate(`(()=>{const e=document.querySelector('${target}');window.__navTarget=e;e.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});window.__navBefore=e.getBoundingClientRect().toJSON()})()`)
          await settledLayout();await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect(),x=Math.max(r.left+1,Math.min(r.right-1,r.left+r.width/2)),y=Math.max(1,Math.min(innerHeight-1,r.top+r.height/2)),clipped=[];for(let p=e.parentElement;p;p=p.parentElement){const c=getComputedStyle(p);if(['hidden','clip'].includes(c.overflowY)&&p.scrollHeight>p.clientHeight+1)clipped.push({class:p.className,scroll:p.scrollHeight,client:p.clientHeight})}let exposed=0;if(e.tagName==='CANVAS')for(let py=Math.max(1,r.top+8);py<Math.min(innerHeight-1,r.bottom-8);py+=16)for(let px=Math.max(1,r.left+8);px<Math.min(innerWidth-1,r.right-8);px+=16)if(e.contains(document.elementFromPoint(px,py)))exposed++;return {zoom:Number(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')),docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,rect:r.toJSON(),hit:document.elementFromPoint(x,y)?.outerHTML.slice(0,500),reachable:e.tagName==='CANVAS'?exposed>=4:e.contains(document.elementFromPoint(x,y)),exposed,clipped}})()`)
          const good=Math.abs(shape.zoom-zoom)<0.001&&shape.docW<=width+1&&shape.docH<=height+1&&shape.rect.width>0&&shape.rect.height>0&&shape.rect.top<height&&shape.rect.bottom>0&&shape.reachable&&shape.clipped.length===0
          if(!good)console.log('SESSION FAILURE',JSON.stringify(await sessionDiagnostic()));
          if(!good)console.log('Navigation layout trace',JSON.stringify(await evaluate(`(()=>{const e=document.querySelector('${target}'),chain=[];for(let p=e;p;p=p.parentElement){const r=p.getBoundingClientRect(),c=getComputedStyle(p);chain.push({class:p.className,rect:r.toJSON(),at:p.scrollTop,scroll:p.scrollHeight,client:p.clientHeight,y:c.overflowY})}return {same:e===window.__navTarget,before:window.__navBefore,viewport:document.documentElement.dataset.viewport,chain}})()`)));
          if(!good&&artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-${label.toLowerCase()}-failure.png`),Buffer.from(shot.data,'base64'))}
          assert.ok(good,`${label} content remains reachable: ${JSON.stringify({width,height,zoom,theme,target,shape})}`)
          results.push({navigation:label,width,height,zoom,theme,target,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'));document.querySelector('.remote-workspace').scrollTop=0;for(const e of document.querySelectorAll('.remote-workspace *'))if(/auto|scroll/.test(getComputedStyle(e).overflowY))e.scrollTop=0`)
      await settledLayout();await settledLayout()
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${label.toLowerCase()}.png`),Buffer.from(shot.data,'base64'))}
      const collection=settings?'settings':'programming'
      unavailableCollections.add(collection)
      await until(settings?`!document.querySelector('.settings-tabs')`:`document.querySelector('.rp-body')?.hidden===true`,35000)
      unavailableCollections.delete(collection)
      await until(settings?`!!document.querySelector('.settings-tabs')`:`document.querySelectorAll('.rp-chan-name').length===1200`)
    }
    const beforeTempo = structuredClone(applicationData.get_snapshot)
    for (const tier of ['TempoFast','TempoDeep']) {
      Object.assign(applicationData.get_snapshot, {mode:'chat',activePeer:'W1AW',chatCq:'paused',
        conversations:tempoConversations(tier,44),
        stations:['TempoFast','TempoDeep','FT8'].map((mode,i)=>({call:['W1AW','K2ABC','K3FT'][i],grid:'FN31',
          tier:mode,snr:-8,lastHeardSlot:beforeTempo.radio.slot,heardCount:1,presence:'active',worked:false}))})
      applicationData.get_snapshot.radio.operatingMode='digital'
      applicationData.get_snapshot.link.tier=tier
      applicationRevision++
      await click(button('Tempo'))
      await until(`document.querySelector('.grid-header .cockpit-mode.active')?.textContent.includes('${tier==='TempoFast'?'Fast':'Deep'}') && document.querySelectorAll('.bubble-row').length===52`)
      for (const stage of ['held','sending','confirmed','delivered','no-ack','abandoned']) assert.ok(await evaluate(`!!document.querySelector('.delivery.${stage}')`),stage)
      assert.ok(await evaluate(`document.querySelector('.bubble-incomplete')?.textContent.includes('2 of 3')`))
      assert.equal(await evaluate(`!!document.querySelector('.bubble.resendable')`),false)
      assert.deepEqual(await evaluate(`[...document.querySelectorAll('.station-call')].map(e=>e.textContent)`),['W1AW','K2ABC'])
      assert.ok(await evaluate(`[...document.querySelectorAll('.conversation button,.cq-run button,.recent-archive,.station-work,.grid-header .cockpit-mode,.composer-input')].length>10 && [...document.querySelectorAll('.conversation button,.cq-run button,.recent-archive,.station-work,.grid-header .cockpit-mode,.composer-input')].every(e=>e.disabled)`))
      if (tier==='TempoFast') for (const [width,height,zoom] of [[360,740,1],[390,844,1],[844,390,1],[1024,768,1],[1280,800,1],
        [1200,1390,1],[3440,1440,1],[1024,768,0.8],[1280,800,1.75],[390,844,1.75]]) for (const theme of ['dark','light']) {
        await browser.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false},session)
        await evaluate(`document.documentElement.style.setProperty('--ui-zoom','${zoom}');document.documentElement.dataset.theme='${theme}';window.dispatchEvent(new Event('resize'))`)
        await settledLayout()
        for (const target of ['.message-scroll .bubble-row:first-child .bubble-text','.message-scroll .bubble-row:last-child .bubble-text','.grid-stations .station-open','.composer-input']) {
          if(target.includes('first-child')) {
            // A real wheel gesture leaves the native "follow newest" state
            // before inspecting old history while live snapshots keep arriving.
            await evaluate(`document.querySelector('.message-scroll').scrollIntoView({block:'center',behavior:'instant'})`)
            await settledLayout()
            const point=await evaluate(`(()=>{const e=document.querySelector('.message-scroll'),r=e.getBoundingClientRect(),x=r.left+r.width/2,ys=[];for(let y=Math.max(1,r.top+1);y<Math.min(innerHeight-1,r.bottom);y+=5)if(e.contains(document.elementFromPoint(x,y)))ys.push(y);return ys.length?{x,y:ys[Math.floor(ys.length/2)]}:null})()`)
            assert.ok(point,'the conversation has a reachable wheel-scroll surface')
            await browser.call('Input.dispatchMouseEvent',{type:'mouseWheel',...point,deltaX:0,deltaY:-180},session)
            try { await until(`(()=>{const e=document.querySelector('.message-scroll');return e.scrollHeight-e.clientHeight-e.scrollTop>50})()`) }
            catch(error){console.log('Wheel diagnostic',JSON.stringify({width,height,zoom,theme,point,session:await sessionDiagnostic(),state:await evaluate(`(()=>{const e=document.querySelector('.message-scroll');return {top:e.scrollTop,client:e.clientHeight,height:e.scrollHeight}})()`)}));throw error}
          }
          // Match the native pin helper's instant positioning. A smooth trip
          // through 52 messages is not settled by a layout-only animation frame.
          await evaluate(`document.querySelector('${target}').scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'})`)
          await settledLayout()
          if(!await evaluate(`!!document.querySelector('${target}')`))console.log('SESSION MISSING',target,JSON.stringify(await sessionDiagnostic()));
          const shape=await evaluate(`(()=>{const e=document.querySelector('${target}'),r=e.getBoundingClientRect();return {docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight,top:r.top,bottom:r.bottom,width:r.width,height:r.height,reachable:e.contains(document.elementFromPoint(r.left+r.width/2,Math.min(r.bottom,innerHeight-1)-Math.min(r.height/2,20)))}})()`)
          const clipped=await evaluate(`(()=>{const result=[];for(let e=document.querySelector('${target}').parentElement;e;e=e.parentElement){const c=getComputedStyle(e);if(['hidden','clip'].includes(c.overflowY)&&e.scrollHeight>e.clientHeight+1)result.push({class:e.className,scroll:e.scrollHeight,client:e.clientHeight,y:c.overflowY})}return result})()`)
          const reachable=shape.docW<=width+1&&shape.docH<=height+1&&shape.width>0&&shape.height>0&&shape.top<height&&shape.bottom>0&&shape.reachable&&clipped.length===0
          if(!reachable){console.log('Tempo observation diagnostic',withheld,{...applicationTraffic,nativeSnapshotAge:performance.now()-(sentAt.get('get_snapshot')??0)},JSON.stringify(await sessionDiagnostic()));if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,'remote-nexus-tempo-failure.png'),Buffer.from(shot.data,'base64'))}}
          assert.ok(reachable,`Tempo content remains reachable by user input: ${JSON.stringify({target,width,height,zoom,theme,shape,clipped})}`)
          results.push({tempo:true,target,width,height,zoom,theme,shape})
        }
      }
      await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
      await evaluate(`document.documentElement.style.setProperty('--ui-zoom','1');window.dispatchEvent(new Event('resize'))`)
      await settledLayout()
      if(tier==='TempoFast') {
        await evaluate(`document.querySelector('.message-scroll').scrollIntoView({block:'center',behavior:'instant'})`)
        const point=await evaluate(`(()=>{const r=document.querySelector('.message-scroll').getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()`)
        await browser.call('Input.dispatchMouseEvent',{type:'mouseWheel',...point,deltaX:0,deltaY:-180},session)
        await until(`(()=>{const e=document.querySelector('.message-scroll');return e.scrollTop>0&&e.scrollHeight-e.clientHeight-e.scrollTop>50})()`)
        await settledLayout()
        const top=await evaluate(`document.querySelector('.message-scroll').scrollTop`)
        const messages=applicationData.get_snapshot.conversations[0].messages
        messages.push({...messages.at(-1),text:'NEW WHILE READING',slot:100});applicationRevision++
        await until(`document.querySelectorAll('.bubble-row').length===53`)
        assert.ok(Math.abs(await evaluate(`document.querySelector('.message-scroll').scrollTop`)-top)<2,'an arriving message does not pull the reader away from history')
      }
      if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${tier}.png`),Buffer.from(shot.data,'base64'))}
    }
    await click(`document.querySelectorAll('.station-open')[1]`)
    await until(`document.querySelector('.conv-peer')?.textContent==='K2ABC'`)
    applicationData.get_snapshot.activePeer='W1AW';applicationRevision++
    await until(`document.querySelector('.bubble-text')?.textContent==='SECOND THREAD'`)
    await click(`document.querySelector('.band-row')`)
    await until(`document.querySelector('.bubble-text')?.textContent==='BAND MESSAGE'`)
    applicationAvailable=false
    await until(`document.querySelector('.app')?.dataset.remoteStale==='true'`)
    assert.equal(await evaluate(`getComputedStyle(document.querySelector('.message-scroll')).visibility`),'hidden')
    applicationAvailable=true;applicationRevision++
    station.close();station=await pair.native.open(pair.stationId,undefined,101,stationHeaders)
    await until(`document.querySelector('.app')?.dataset.remoteStale!=='true' && document.querySelector('.bubble-text')?.textContent==='BAND MESSAGE'`)
    Object.assign(applicationData.get_snapshot,beforeTempo);applicationRevision++
    if(applicationVersion>=10){
      await click(button('Field Day'))
      await until(`!!document.querySelector('.fd-bonuses-list')`)
      // Leaving for Tempo unmounted the native event view. Establish the
      // operator's collapsed choice on this new mount before testing loss.
      await click(`document.querySelector('.fd-bonuses-toggle')`)
      assert.equal(await evaluate(`!!document.querySelector('.fd-bonuses-list')`),false)
    }
    else if(applicationVersion===9)await click(button('POTA/SOTA'))
    else if(applicationVersion===8){
      await click(button('Memories'))
      await until(`document.querySelector('.remote-memory-bank:not([hidden]) .mv-list')?.textContent.includes('Updated station memory')`)
      // Grid is local to the native view mount, like the event disclosure.
      await click(`[...document.querySelectorAll('.mv-toolbar button')].find(b=>b.textContent.includes('Grid'))`)
      await until(`[...document.querySelectorAll('.mv-grid input')].some(e=>e.value==='Updated station memory')`)
    }
    // Each supported version exercises loss/recovery on its newest actual pane.
    if (applicationVersion === 7) { await click(button('DXped')); await until(`!!document.querySelector('.dxped-view')`) }
    else if (applicationVersion === 6) { await click(button('Stats')); await until(`!!document.querySelector('.stats-summary')`) }
    else if (applicationVersion < 6) await click(button('FT'))
    applicationAvailable=false
    await until(`document.querySelector('.app')?.dataset.remoteStale==='true'`)
    assert.equal(await evaluate(`getComputedStyle(document.querySelector('.operate-host')).visibility`),'hidden','stale operating values must be hidden')
    if (applicationVersion === 6) assert.equal(await evaluate(`!!document.querySelector('.stats-summary')`), false, 'lost station data removes the summary')
    if (applicationVersion === 8) assert.equal(await evaluate(`!!document.querySelector('.remote-memory-bank:not([hidden])')`),false,'lost station data removes saved memories')
    if (applicationVersion === 7) assert.equal(await evaluate(`!!document.querySelector('.dxped-view')`), false, 'lost station data removes the DX board')
    if (applicationVersion === 9) assert.equal(await evaluate(`!!document.querySelector('.remote-ota-bank:not([hidden])')`),false,'lost station data removes the OTA view')
    if (applicationVersion >= 10) assert.equal(await evaluate(`!!document.querySelector('.remote-field-day-bank:not([hidden])')`),false,'lost station data removes the event')
    applicationAvailable=true;applicationRevision++
    applicationData.get_snapshot.radio.dialMhz=7.074;applicationData.get_snapshot.radio.band='40m'
    // Withholding a station credit can legitimately expire BOTH sockets. The
    // fixture must reconnect the native endpoint as the real controller does;
    // restoring an in-memory boolean cannot revive an expired WebSocket.
    station.close()
    station=await pair.native.open(pair.stationId, undefined, 101, stationHeaders)
    try { await until(`document.querySelector('.app')?.dataset.remoteStale!=='true' && document.body.textContent.includes('7.074')`) }
    catch (error) { console.log('Station recovery diagnostic',withheld,await evaluate(`({closures:window.__socketClosures,stale:document.querySelector('.app')?.dataset.remoteStale,status:document.querySelector('.remote-application-status')?.textContent})`));throw error }
    if (applicationVersion === 6) await until(`document.querySelector('.stats-summary')?.textContent.includes('2013')`)
    if (applicationVersion === 8) await until(`[...document.querySelectorAll('.remote-memory-bank:not([hidden]) .mv-grid input')].some(e=>e.value==='Updated station memory')`)
    if (applicationVersion === 7) await until(`document.querySelector('.dxped-view')?.textContent.includes('Updated test entity')`)
    if (applicationVersion === 9) {
      await until(`document.querySelector('.pota-spot-list')?.textContent.includes('Updated test summit')`)
      assert.equal(await evaluate(`document.querySelector('.pota-controls [role=tab][aria-selected=true]')?.textContent`),'SOTA')
    }
    if(applicationVersion>=10){await until(`document.querySelector('.fieldday input:not([type=checkbox])')?.value==='K9TEST'`);assert.equal(await evaluate(`!!document.querySelector('.fd-bonuses-list')`),false)}
    assert.equal(await evaluate('window.__imagePolicyViolations.length'),0,'all rendered image sources satisfy the hosted image policy')
    assert.equal(exceptions,0,'actual Nexus must render and reconnect without runtime exceptions')
    await click(`document.querySelector('.remote-session-toggle')`)
    await until(`document.querySelector('.remote-session-toggle')?.getAttribute('aria-expanded')==='true'`)
    await click(button('Disconnect and return to stations'))
    await until(`!!${button('Observe station')}`)
    await click(button('Observe station'))
    await until(`document.querySelector('.rm-frequency')?.textContent.includes('14.074000')`)
    await pair.native.post(`stations/${pair.stationId}/native/revoke-device`,{deviceId:device.id})
    await until(`document.querySelector('.rm-frequency')?.textContent.startsWith('—')`)
    assert.equal(await evaluate('window.__imagePolicyViolations.length'),0,'all rendered image sources satisfy the hosted image policy')
    assert.equal(exceptions,0,'the compiled browser must not raise runtime exceptions')
    assert.ok(acknowledgements >= 2,'real observation ACKs must cross the browser socket')
    assert.equal(unexpectedMessages,0,'only reviewed read messages and observation ACKs may leave the browser socket')
    console.log(`Compiled browser: PKCE exchange, browser approval, live observation, revocation, ${results.length} geometry cases and overflow positive control passed`)
    if(artifacts)await writeFile(join(artifacts,'remote-browser-results.json'),JSON.stringify({exchanges,exceptions,acknowledgements,unexpectedMessages,applicationTraffic,geometry:results},null,2)+'\n')
  } finally {
    producing=false;station?.close();await Promise.all([producer,applicationProducer])
    try { await browser?.stop() } finally { await app.mf.dispose() }
  }
})
