// @vitest-environment jsdom
// Settings must not poll the OS keychain (#154).
//
// M0LHJ, Fedora 44: `gnome-keyring-daemon` aborted in `service_method_open_session` and was
// restarted by systemd roughly every 35 seconds, for as long as Nexus was open, and only once a
// connector with stored credentials had been configured. The cause was ours — Settings read
// per-connector credential status on a 5-second timer, and each read opens a Secret Service
// session per connector.
//
// The answer to "is a password stored?" changes only when the operator saves or clears one. So
// it is an event now, raised centrally at the IPC boundary rather than at each of the ten call
// sites, because a missed site would leave a stale badge that nothing re-reads to correct.
//
// These tests pin the CLASSIFIER, which is the part that can silently rot: a new connector's
// setter must raise the event the day it is added, and an ordinary command must never raise it
// (an event on every invoke would put the keychain read back on a timer by the back door — the
// exact bug, wearing a different hat).
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { CREDENTIALS_CHANGED } from './api'
import * as api from './api'

let fired = 0
const count = () => {
  fired += 1
}

// The real seam: `bridge()` resolves `window.__TAURI_INTERNALS__.invoke`. Stubbing that rather
// than inventing a test-only hook keeps this exercising the SAME path the app uses — a hook
// would let the classifier drift from what actually ships.
beforeEach(() => {
  fired = 0
  window.addEventListener(CREDENTIALS_CHANGED, count)
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async () => undefined,
  }
})
afterEach(() => {
  window.removeEventListener(CREDENTIALS_CHANGED, count)
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('#154 — storing a secret is news; reading one is not', () => {
  it('raises the event when a secret is SAVED', async () => {
    await api.setLotwPassword('x')
    expect(fired).toBe(1)
  })

  it('raises it when a secret is CLEARED', async () => {
    await api.clearLotwPassword()
    expect(fired).toBe(1)
  })

  it('covers every connector, not a hand-kept list', async () => {
    // Each of these is a different command shape; all must be recognised. A list would have to
    // be extended per connector, and the one somebody forgets is the one whose badge lies.
    await api.setEqslPassword('x')
    await api.setQrzLogbookKey('x')
    await api.setHrdlogCode('x')
    await api.setRepeaterbookToken('x')
    expect(fired).toBe(4)
  })

  it('does NOT raise it for an ordinary command — the control that matters', async () => {
    // Without this the classifier could be matching everything, which would re-create the
    // 5-second keychain read through the event instead of the timer.
    await api.getConnectionLog()
    await api.getCredentialsStatus()
    expect(fired).toBe(0)
  })
})
