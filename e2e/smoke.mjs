#!/usr/bin/env node
// Nexus E2E smoke — launches the REAL app binary through tauri-driver
// (WebDriver) and proves the three things unit tests structurally cannot:
// the app starts, the UI renders, and the panels that crashed in the field
// still open. Plain Node + fetch — no test framework, no npm dependencies —
// so it runs anywhere Node 18+ and tauri-driver exist.
//
//   NEXUS_E2E_BINARY   path to the built app binary (required)
//   TAURI_DRIVER       tauri-driver binary        (default: from PATH)
//   NATIVE_DRIVER      WebKitWebDriver binary     (default: tauri-driver's own lookup)
//   E2E_PORT           tauri-driver port          (default: 4444)
//
// Run headless:  xvfb-run -a dbus-run-session -- node e2e/smoke.mjs
// ALWAYS isolate the profile (HOME/XDG_*) — a run on a real profile would
// mutate the operator's actual app state; see e2e/README.md.

import { spawn } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const BINARY = process.env.NEXUS_E2E_BINARY
if (!BINARY) {
  console.error('NEXUS_E2E_BINARY is required (path to the built Nexus binary)')
  process.exit(2)
}
const PORT = Number(process.env.E2E_PORT ?? 4444)
const BASE = `http://127.0.0.1:${PORT}`

// ── tiny WebDriver client ───────────────────────────────────────────────────
let sessionId = null
async function wd(method, path, body) {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const json = await res.json().catch(() => ({}))
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${JSON.stringify(json.value ?? json).slice(0, 400)}`)
  }
  return json.value
}
// Every probe runs INSIDE the app's webview: the assertions see exactly what
// the operator would see, not a parallel mock of it.
async function exec(script, args = []) {
  return wd('POST', `/session/${sessionId}/execute/sync`, { script, args })
}
async function waitFor(label, script, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs
  let last
  while (Date.now() < deadline) {
    try {
      last = await exec(script)
      if (last) return last
    } catch (e) {
      last = String(e)
    }
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`TIMEOUT waiting for ${label} (last: ${JSON.stringify(last).slice(0, 300)})`)
}

// On failure, save what the operator would have seen.
async function screenshot(name) {
  try {
    const b64 = await wd('GET', `/session/${sessionId}/screenshot`)
    mkdirSync('e2e/artifacts', { recursive: true })
    writeFileSync(`e2e/artifacts/${name}.png`, Buffer.from(b64, 'base64'))
    console.error(`  screenshot: e2e/artifacts/${name}.png`)
  } catch {
    /* screenshot is best-effort */
  }
}

// ── driver lifecycle ────────────────────────────────────────────────────────
const driverBin = process.env.TAURI_DRIVER ?? 'tauri-driver'
const driverArgs = ['--port', String(PORT), '--native-port', String(PORT + 1)]
if (process.env.NATIVE_DRIVER) driverArgs.push('--native-driver', process.env.NATIVE_DRIVER)
const driver = spawn(driverBin, driverArgs, { stdio: ['ignore', 'inherit', 'inherit'] })
const killDriver = () => {
  try {
    driver.kill()
  } catch {
    /* already gone */
  }
}
process.on('exit', killDriver)

let failed = false
try {
  // Session = tauri-driver launches the app under the native WebDriver.
  for (let attempt = 0; ; attempt++) {
    try {
      const v = await wd('POST', '/session', {
        capabilities: { alwaysMatch: { 'tauri:options': { application: resolve(BINARY) } } },
      })
      sessionId = v.sessionId
      break
    } catch (e) {
      if (attempt >= 20) throw e
      await new Promise((r) => setTimeout(r, 500)) // driver still starting
    }
  }
  console.log(`session ${sessionId} — app launched`)

  // Flow 1 — the app starts and the UI actually renders (DOA gate).
  await waitFor(
    'the app to render',
    `return document.readyState === 'complete' && document.getElementById('root')?.children.length > 0`,
  )
  console.log('PASS 1: app launched, #root rendered')

  // Flow 2 — a fresh profile gets the first-run wizard (release-critical:
  // this is the first thing every new operator sees).
  const wizardTitle = await waitFor(
    'the setup wizard',
    `return document.querySelector('.wizard-title')?.textContent ?? ''`,
  )
  if (!/who/i.test(wizardTitle)) throw new Error(`wizard title unexpected: ${JSON.stringify(wizardTitle)}`)
  console.log(`PASS 2: setup wizard shown (${JSON.stringify(wizardTitle.trim())})`)

  // Flow 3 — the main UI mounts once the wizard is marked seen, and the
  // Settings panel OPENS. Settings-open is the exact class of the public
  // crash-on-Settings incident; a regression here kills the session and
  // fails loudly instead of reaching users.
  // ⚠️ THE RELOAD RACE. The wizard is an OVERLAY rendered beside the shell, not
  // instead of it, so `.mode-nav` is ALREADY in the pre-reload document. Waiting on
  // it (or on `readyState`, which is 'complete' for the doomed page until the
  // navigation actually starts) is satisfied INSTANTLY by the old DOM — and the next
  // step then queries a document that is mid-teardown and gets null. That is a flaky
  // failure with no bug behind it, and it cost a red main on 2026-08-02.
  //
  // So: stamp a marker on `window` (dies with the document, unlike localStorage),
  // then wait for it to be GONE — which proves the NEW document — and wait for the
  // BUTTON THIS STEP IS ABOUT TO CLICK rather than a proxy element that also exists
  // on the old page. A wait must assert the next step's own precondition.
  await exec(
    `window.__nexusPreReload = true; localStorage.setItem('nexus.features.wizardSeen', '1'); location.reload()`,
  )
  await waitFor(
    'the reloaded main UI, with the Settings button live',
    `return !window.__nexusPreReload
       && document.readyState === 'complete'
       && !!document.querySelector('.mode-nav')
       && !!document.querySelector('button[aria-label="Settings"]')`,
  )
  console.log('PASS 3a: main UI mounted (.mode-nav + Settings button present)')
  await exec(`document.querySelector('button[aria-label="Settings"]').click()`)
  await waitFor(
    'the Settings panel',
    `return !!document.querySelector('[class*="settings"]')`,
    15_000,
  )
  console.log('PASS 3b: Settings opened without killing the app')

  // The webview must still be alive and sane after all of it.
  const title = await exec('return document.title')
  console.log(`done — webview alive (title ${JSON.stringify(title)})`)
} catch (e) {
  failed = true
  console.error(`FAIL: ${e.message ?? e}`)
  if (sessionId) await screenshot('failure')
} finally {
  if (sessionId) await wd('DELETE', `/session/${sessionId}`).catch(() => {})
  killDriver()
}
process.exit(failed ? 1 : 0)
