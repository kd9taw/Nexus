// Private headless Chrome profiles for compiled Remote verification only.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import WebSocket from 'ws'

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

// Node cancels a timed-out test without awaiting its body's finally block.
// Register the same cleanup with the runner so the next case cannot start with
// this case's browser/Worker still alive. Both paths share one completion.
export function cleanupAfterTest(context, cleanup) {
  let stopping
  const stop = () => stopping ??= Promise.resolve().then(cleanup)
  context.after(stop)
  return stop
}

export async function chrome() {
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
