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
      if (value.id) { const promise=pending.get(value.id); if(promise){pending.delete(value.id);clearTimeout(promise.timer);value.error?promise.reject(new Error('Browser protocol command failed')):promise.resolve(value.result)} }
      else listeners.get(value.method)?.(value.params, value.sessionId)
    })
    const call = (method,params={},sessionId) => new Promise((resolve,reject) => {
      const id=++next, timer=setTimeout(()=>{pending.delete(id);reject(new Error(`Browser command timed out: ${method}`))},10000)
      pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params,sessionId}))
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

for (const applicationVersion of [1, 2, 3, 4]) test(`compiled hosted browser v${applicationVersion} completes PKCE, local device approval, observation and viewport checks`, { timeout: 120000 }, async () => {
  const app=await runtime(), artifacts=process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS ? join(process.env.NEXUS_REMOTE_BROWSER_ARTIFACTS, `v${applicationVersion}`) : undefined
  let browser, station, producing=true, producer, applicationProducer
  const results=[]
  try {
    browser=await chrome()
    const pair=await app.paired(), subject=JSON.parse(Buffer.from(pair.browser.jwt.split('.')[1],'base64url')).sub
    const shell = await fetch(app.origin,{signal:AbortSignal.timeout(3000)})
    assert.equal(shell.status,200)
    assert.match(await shell.text(), /Nexus Remote/)
    const stationHeaders = { 'x-nexus-application-version': '1', ...(applicationVersion >= 2 ? { 'x-nexus-application-stream-version': '2' } : {}), ...(applicationVersion >= 3 ? { 'x-nexus-application-query-version': '1' } : {}), ...(applicationVersion === 4 ? { 'x-nexus-application-recall-version': '1' } : {}) }
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
        responseCode=302;headers.push({name:'location',value:redirect.href})
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
    browser.on('Network.webSocketFrameSent', event => {
      if (event.response.opcode !== 1) return
      try {
        const message = JSON.parse(event.response.payloadData)
        if (message.type === 'ack' && Object.keys(message).sort().join(',') === 'epoch,sequence,type') acknowledgements++
        else if (message.type === 'applicationHello' && (Object.keys(message).length === 1 || (Object.keys(message).length === 2 && [2,3,4].includes(message.version)))) {}
        else if (message.type === 'applicationRead' && Object.keys(message).length === 4) applicationTraffic.reads++
        else if (message.type === 'applicationQuery' && Object.keys(message).length === 7) applicationTraffic.queries=(applicationTraffic.queries??0)+1
        else if (message.type === 'applicationQueryAck' && Object.keys(message).length === 2) {}
        else if (message.type === 'applicationAck' && Object.keys(message).length === 2) applicationTraffic.acks++
        else if (message.type === 'applicationSubscribe' && Object.keys(message).length === 3) applicationTraffic.subscriptions++
        else if (message.type === 'applicationFrameAck' && Object.keys(message).length === 3) applicationTraffic.acks++
        else unexpectedMessages++
      } catch { unexpectedMessages++ }
    })
    await browser.call('Page.addScriptToEvaluateOnNewDocument',{source:`
      window.__socketClosures=[];const originalWebSocket=window.WebSocket;
      window.WebSocket=class extends originalWebSocket{constructor(...args){super(...args);this.addEventListener('close',e=>window.__socketClosures.push({code:e.code,reason:e.reason}))}};
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
    `},session)
    const evaluate=async expression=>{
      const value=await browser.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},session)
      if(value.exceptionDetails)throw new Error('Browser evaluation failed: '+String(value.exceptionDetails.exception?.description??value.exceptionDetails.text).split('\n')[0])
      return value.result?.value
    }
    // useViewport publishes dimensions on an animation frame. A fixed sleep can
    // measure the previous viewport on a busy renderer. Wait through the update
    // and its layout frame; all pixel/overflow assertions below stay unchanged.
    const settledLayout=()=>evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))')
    async function until(expression) { for(let i=0;i<120;i++){ if(providerFailure)throw new Error('Simulated provider failed'); if(await evaluate(expression))return;await sleep(100) } throw new Error('Expected browser state did not appear') }
    const button=name=>`[...document.querySelectorAll('button')].find(e=>e.textContent===${JSON.stringify(name)})`
    async function click(expression) {
      const point=await evaluate(`(()=>{const e=${expression};if(!e||e.disabled)throw Error('missingControl');e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;if(!e.contains(document.elementFromPoint(x,y)))throw Error('occludedControl');return{x,y}})()`)
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
    producer=(async()=>{while(producing){const source=station;let watch;try{watch=await source.take(value=>value.type==='watch'&&value.enabled,1000)}catch{continue}await sleep(200);if(!producing)break;if(source!==station||source.closed)continue;source.send({type:'publication',requestId:watch.requestId,frame:{...fixture,source:'native',sequence:++sequence}})}})()
    const applicationData = await applicationFixture()
    const collections = collectionFixture()
    if (applicationVersion === 4) applicationData.get_snapshot.stations = [{ call:'W1AW', grid:'FN31', snr:-8, lastHeardSlot:0,
      heardCount:2, presence:'heard', worked:true, workedBand:false, country:'United States', tier:'FT8', freqHz:1500 }]
    const querySnapshots = new Map()
    let applicationRevision = 1, applicationAvailable = true
    const withheld = []
    let streamWatch = null
    const sentAt = new Map(), bases = new Map()
    const intervals = { get_snapshot:500, get_settings:1000, get_band_plan:1000, get_spectrum_row:100, get_meters:200, get_scope_snapshot:100, get_cw_state:200 }
    applicationProducer=(async()=>{while(producing){const source=station;let request;try{request=await source.take(value=>['applicationRead','applicationWatch','applicationCredit','applicationQuery'].includes(value.type),1000)}catch{continue}
      if(source!==station||source.closed)continue
      if (!applicationAvailable || !producing) { withheld.push({type:request.type,collection:request.collection});continue }
      if (request.type === 'applicationQuery') {
        assert.ok(applicationVersion >= 3)
        if (request.collection === 'recall') assert.equal(applicationVersion, 4)
        const [givenId, offsetText] = (request.cursor??'').split(':')
        const offset = Number(offsetText??0), snapshotId=givenId||crypto.randomUUID()
        if (!givenId) {
          const collection = request.collection === 'recall' ? recallFixture(request.search) : collections[request.collection]
          let rows=collection.rows
          if(request.collection==='log') rows=rows.filter(q=>(!request.unconfirmed||!q.awardConfirmed)&&(!request.search||q.call.toLowerCase().includes(request.search.toLowerCase())))
          if(request.collection==='decodes'&&request.after!==null) rows=rows.filter(q=>q.sequence>request.after)
          querySnapshots.set(snapshotId,{...collection, rows})
          if(querySnapshots.size>16) querySnapshots.delete(querySnapshots.keys().next().value)
        }
        const capture=querySnapshots.get(snapshotId)
        assert.ok(capture,'browser must keep a valid sealed cursor')
        const rows=capture.rows.slice(offset,offset+128), end=offset+rows.length
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
          const data=applicationData[command], delta=bases.get(command)===applicationRevision && !Array.isArray(data)
          updates.push({type:'applicationResult', requestId:request.requestId, command, revision:applicationRevision,
            baseRevision:delta?applicationRevision:null, ageMs:0, data:delta?{}:data, removed:[]})
          sentAt.set(command,now);bases.set(command,applicationRevision)
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
    assert.equal(await evaluate(`!!window.__TAURI_INTERNALS__ || !!window.__TAURI__`),false,'the browser must use its explicit adapter')
    assert.equal(await evaluate(`document.querySelectorAll('.app').length`),1,'the real workspace has one app root')
    assert.ok(await evaluate(`document.querySelector('.amp-strip')?.textContent.includes('80m')`),'the existing amp strip must show station data')
    assert.ok(await evaluate(`[...document.querySelectorAll('.amp-strip button')].every(button=>button.disabled)`),'observer amp controls must visibly refuse operating authority')
    await until(`window.__waterfallDraws > 2`)
    assert.ok(await evaluate(`[...document.querySelectorAll('.cockpit-qso button, .tuning-nudge, .cockpit-mode, .tier-btn, .cs-opt, .ph-split button')].every(button=>button.disabled)`),'station controls in the existing workspace must show observer authority')
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
        if (applicationVersion === 4) {
          await click(`document.querySelector('.${mode}-cockpit .le-call')`)
          await browser.call('Input.insertText',{text:'W1AW'},session)
          await until(`document.querySelector('.${mode}-cockpit .recall-card')?.textContent.includes('Recall browser note')`)
          assert.ok(await evaluate(`document.querySelector('.${mode}-cockpit .recall-card').textContent.includes('Showing 20 of 2030')`))
        }
        await until(`document.querySelector('.ph-scope canvas')?.width>0 || document.querySelector('.ph-scope-canvas')?.width>0`)
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
        await until(`getComputedStyle(document.querySelector('.ph-scope canvas')).visibility==='hidden' && document.querySelector('.ph-scope').textContent.includes('Scope data unavailable.')`)
        assert.equal(await evaluate(`document.querySelector('.app')?.dataset.remoteStale==='true'`),false,'scope absence is distinct from station/session loss')
        applicationData.get_scope_snapshot.row=scopeRow;applicationRevision++
        await until(`getComputedStyle(document.querySelector('.ph-scope canvas')).visibility==='visible'`)
        await browser.call('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false},session)
        await settledLayout()
        if(artifacts){const shot=await browser.call('Page.captureScreenshot',{format:'png'},session);await writeFile(join(artifacts,`remote-nexus-${mode}.png`),Buffer.from(shot.data,'base64'))}
      }
      await click(button('FT'))
      await until(`!!document.querySelector('.operate-host:not([hidden])')`)
      assert.ok(applicationTraffic.subscriptions>0 && applicationTraffic.batches>0 && applicationTraffic.acks>0, 'positive control: real subscriptions, native batches and browser ACKs flowed')
      assert.equal(applicationTraffic.reads,0, 'v2 panel polling stays within the browser')
    }
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
    if (applicationVersion === 4) {
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
    applicationAvailable=false
    await until(`document.querySelector('.app')?.dataset.remoteStale==='true'`)
    assert.equal(await evaluate(`getComputedStyle(document.querySelector('.operate-host')).visibility`),'hidden','stale operating values must be hidden')
    applicationAvailable=true;applicationRevision++
    applicationData.get_snapshot.radio.dialMhz=7.074;applicationData.get_snapshot.radio.band='40m'
    // Withholding a station credit can legitimately expire BOTH sockets. The
    // fixture must reconnect the native endpoint as the real controller does;
    // restoring an in-memory boolean cannot revive an expired WebSocket.
    station.close()
    station=await pair.native.open(pair.stationId, undefined, 101, stationHeaders)
    try { await until(`document.querySelector('.app')?.dataset.remoteStale!=='true' && document.body.textContent.includes('7.074')`) }
    catch (error) { console.log('Station recovery diagnostic',withheld,await evaluate(`({closures:window.__socketClosures,stale:document.querySelector('.app')?.dataset.remoteStale,status:document.querySelector('.remote-application-status')?.textContent})`));throw error }
    assert.equal(exceptions,0,'actual Nexus must render and reconnect without runtime exceptions')
    await click(button('Disconnect and return to stations'))
    await until(`!!${button('Observe station')}`)
    await click(button('Observe station'))
    await until(`document.querySelector('.rm-frequency')?.textContent.includes('14.074000')`)
    await pair.native.post(`stations/${pair.stationId}/native/revoke-device`,{deviceId:device.id})
    await until(`document.querySelector('.rm-frequency')?.textContent.startsWith('—')`)
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
