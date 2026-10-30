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

// THE COLD START (2026-10-05). Chrome's first start on a machine reads Chrome itself: 197 MB of its own
// files to reach the debugging endpoint, where `google-chrome --version` reads 45 MB (Chrome 140, measured
// from a copy of its install with the page cache dropped). A cold hosted runner reads them slowly. Over 175
// CI compiled-browser jobs `google-chrome --version` took 1.8 s at the median, 3.6 s at the 95th percentile
// and 34.6 s at worst, and on that runner (run 37405801832) the first two starts each ran out of 30 s and
// the third had its endpoint about 100 s after the first began. At the rate that runner's `--version`
// read, the other 152 MB take about 120 s. So the suite starts Chrome once, before any timed case
// (browser-lifecycle.test.mjs), with 240 s, twice that; every later start keeps 30 s. A Chrome that
// never starts still fails, after 240 s once and 30 s each time after.
export const CHROME_COLD_START_MS = 240_000

// `switches` are a scenario's own additions to the launch, such as the stream scenario's fake microphone.
// `startupMs` bounds the wait for the debugging endpoint.
export async function chrome(switches = [], startupMs = 30_000) {
  const profile = await mkdtemp(join(tmpdir(), 'nexus-remote-browser-'))
  // Chrome's temp directory is its profile, so removing the profile removes the temp folders Chrome
  // leaves behind (its single-instance folder even after a clean close, empty url-fetcher folders when it
  // is killed). Its single-instance socket then sits under the profile, and Chrome aborts at start on a
  // socket path over 107 bytes; under a temp directory too long for that it keeps the shared one, and
  // those small folders stay behind there.
  const ownTemp = Buffer.byteLength(join(profile, 'org.chromium.Chromium.XXXXXX', 'SingletonSocket')) <= 107
  const child = spawn(process.env.CHROME_BIN || 'google-chrome', [
    '--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage',
    '--disable-background-networking', '--no-first-run', '--no-default-browser-check',
    '--no-proxy-server', '--disable-extensions', '--disable-default-apps',
    // Isolate synthetic browser cookies from a locked desktop credential store.
    '--password-store=basic', '--use-mock-keychain',
    '--remote-debugging-port=0', `--user-data-dir=${profile}`, ...switches, 'about:blank',
  ], { stdio: 'ignore', detached: true, env: ownTemp ? { ...process.env, TMPDIR: profile } : process.env })
  let exited = false
  child.once('error', () => { exited = true })
  const exit = new Promise(resolve => child.once('exit', () => { exited = true; resolve() }))
  // THE ORPHAN GUARD (2026-10-30). Chrome runs in its own process group (detached), so nothing ended it
  // when this process died without calling stop(): runners killed by `timeout`, Ctrl-C, a closed
  // terminal or SIGKILL left thirty headless Chromes running, each holding a 190-365 MB profile, until
  // /tmp (a RAM disk on the development box) filled and every tool on it failed. This shell, outside
  // both process groups, waits on a pipe only this process writes to. The pipe closes when this process
  // ends, however it ends; the shell then ends Chrome's group and removes the profile. teardown() does
  // the same itself and then ends the guard.
  const guard = spawn('sh', ['-c', 'cat >/dev/null; kill -TERM -"$1" 2>/dev/null && sleep 2; kill -KILL -"$1" 2>/dev/null; rm -rf "$2"', 'sh', String(child.pid), profile],
    { stdio: ['pipe', 'ignore', 'ignore'], detached: true })
  guard.unref(); guard.stdin.unref()
  // Ends Chrome's whole process group and removes its profile; stop() and a failed start share it.
  const teardown = async () => {
    if(!exited){process.kill(-child.pid,'SIGTERM');await Promise.race([exit,sleep(2000)])}
    if(!exited)process.kill(-child.pid,'SIGKILL')
    await rm(profile,{recursive:true,force:true,maxRetries:8,retryDelay:100})
    guard.kill('SIGKILL')
  }
  let ws
  try {
    let endpoint
    // A cold hosted runner can take longer than five seconds to launch Chrome.
    // Bound process startup separately from the browser's application assertions.
    const deadline = performance.now() + startupMs
    while (performance.now() < deadline) {
      if (exited) throw new Error('Chrome could not start')
      // Chrome creates this file before it writes it: a read in between finds it empty or half
      // written, and an endpoint built from that is `ws://127.0.0.1:undefined`. Wait for both lines.
      try { const [port,path] = (await readFile(join(profile,'DevToolsActivePort'),'utf8')).split('\n'); if (/^\d+$/.test(port) && path?.startsWith('/')) { endpoint=`ws://127.0.0.1:${port}${path}`; break } } catch {}
      await sleep(50)
    }
    assert.ok(endpoint, `Chrome debugging endpoint must start within ${startupMs / 1000} seconds`)
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
    return { call, on: (method, fn) => listeners.set(method,fn), profile, async stop() {
      await call('Browser.close').catch(()=>{}); ws.close()
      await Promise.race([exit,sleep(2000)])
      await teardown()
    } }
  } catch(error) { ws?.close(); await teardown(); throw error }
}
