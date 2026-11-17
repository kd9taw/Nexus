// @vitest-environment jsdom
// A stored password, key, token or code is saved or forgotten at the station's own screen only.
// The station's window may be streamed to a browser, and a press made there reaches this page as a
// DOM event (`isStreamInput`); each credential command it starts is refused before it is sent, in
// words its caller shows. Everything else a stream does stands: the stream is the shack.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as api from './api'
import { t } from './i18n'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const stream = vi.hoisted(() => ({ on: false }))
vi.mock('./remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))

let sent: string[] = []
beforeEach(() => {
  sent = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string) => {
      sent.push(cmd)
      return undefined
    },
  }
})
afterEach(() => {
  stream.on = false
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

/** Every credential the Settings panel saves or forgets, the saved-key notice's Use and Remove,
 *  and the question a login save asks first, with the command each sends. */
const CREDENTIALS: [string, () => Promise<unknown>][] = [
  ['set_lotw_password', () => api.setLotwPassword('fake-pw')],
  ['clear_lotw_password', () => api.clearLotwPassword()],
  ['set_icom_lan_password', () => api.setIcomLanPassword(1, 'fake-pw')],
  ['clear_icom_lan_password', () => api.clearIcomLanPassword(1)],
  ['set_eqsl_password', () => api.setEqslPassword('fake-pw', false)],
  ['clear_eqsl_password', () => api.clearEqslPassword()],
  ['set_qrz_password', () => api.setQrzPassword('fake-pw')],
  ['clear_qrz_password', () => api.clearQrzPassword()],
  ['set_hamqth_password', () => api.setHamqthPassword('fake-pw')],
  ['clear_hamqth_password', () => api.clearHamqthPassword()],
  ['set_qrz_logbook_key', () => api.setQrzLogbookKey('FAKE-0000-0000-0000', false)],
  ['clear_qrz_logbook_key', () => api.clearQrzLogbookKey()],
  ['set_clublog_password', () => api.setClublogPassword('fake-pw', false)],
  ['clear_clublog_password', () => api.clearClublogPassword()],
  ['set_hrdlog_code', () => api.setHrdlogCode('fake-code')],
  ['clear_hrdlog_code', () => api.clearHrdlogCode()],
  ['set_wrl_key', () => api.setWrlKey('fake-key')],
  ['clear_wrl_key', () => api.clearWrlKey()],
  ['set_cloudlog_key', () => api.setCloudlogKey('fake-key')],
  ['clear_cloudlog_key', () => api.clearCloudlogKey()],
  ['set_repeaterbook_token', () => api.setRepeaterbookToken('fake-token')],
  ['answer_saved_key', () => api.answerSavedKey(1, true, '192.0.2.7')],
  ['answer_saved_key', () => api.answerSavedKey(null, false, 'https://log.example.invalid')],
  ['upload_backlog', () => api.uploadBacklog('qrz')],
]

describe('credentials are saved and forgotten at the station only', () => {
  it.each(CREDENTIALS)('%s through a stream is refused and never sent', async (_cmd, call) => {
    stream.on = true
    await expect(call()).rejects.toMatchObject({ code: 'atShackOnly', message: t('remote.lan.keysLockedWhileStreamed') })
    expect(sent).toEqual([])
  })

  it.each(CREDENTIALS)('%s at the station is sent', async (cmd, call) => {
    await call()
    expect(sent).toEqual([cmd])
  })

  it('CONTROL: what is no credential still runs through a stream', async () => {
    stream.on = true
    await api.setSettings({ mycall: 'KD9TAW' } as never)
    await api.clearHuntTarget()
    expect(sent).toEqual(['set_settings', 'clear_hunt_target'])
  })

  it('CONTROL: the saved-key notice still asks which keys wait through a stream', async () => {
    stream.on = true
    await api.keysToConfirm()
    expect(sent).toEqual(['keys_to_confirm'])
  })
})

// The list above is kept by hand; the one below is not. The remote client's class table
// (crates/tempo-client/src/classes.rs) already names every command that only the shack's own screen
// may make, with its reason; a streamed screen is not the shack's own screen either, so every row
// there that is refused for a credential's sake is refused through a stream too. A new credential
// command gets its row the day it is registered (src-tauri's class tests fail without one), and
// this turns red the same day if the stream would still send it.
const here = dirname(fileURLToPath(import.meta.url))
const CLASSES_RS = readFileSync(resolve(here, '../../crates/tempo-client/src/classes.rs'), 'utf8')
const ROW = /^\s*\("([a-z0-9_]+)",\s*(\w+),\s*"((?:[^"\\]|\\.)*)"\),\s*$/gm
const rows = [...CLASSES_RS.matchAll(ROW)].map(([, cmd, cls, why]) => ({ cmd, cls, why }))
const CREDENTIAL_ROWS = rows
  .filter((r) => r.cls === 'RefusedHere' && /credential/i.test(r.why))
  .map((r) => r.cmd)

/** Credential rows a stream may still send, each with why. Kept short on purpose: an entry here
 *  is a decision, and the test below holds each one to being a row the table really has. */
const STREAM_MAY_SEND: Record<string, string> = {
  // Which saved keys wait, by radio name and address, never the key: the same notice the stream
  // already shows in its picture of the shack's screen. The notice asks it on a timer; a refusal
  // would stop the notice asking at the shack as well, for as long as the app is open.
  keys_to_confirm: 'a read that says no secret, and is on the streamed screen already',
}

describe('every credential command the class table keeps at the shack is refused through a stream', () => {
  it('reads the whole table', () => {
    const lines = CLASSES_RS.split('\n').filter((l) => /^\s*\("[a-z0-9_]+",/.test(l)).length
    expect(rows.length).toBe(lines)
    expect(rows.length).toBeGreaterThan(500)
    // The rows the hand-kept list names are among them, so the filter is reading the right text.
    for (const [cmd] of CREDENTIALS) expect(CREDENTIAL_ROWS).toContain(cmd)
  })

  it('names no exemption the table does not have', () => {
    for (const cmd of Object.keys(STREAM_MAY_SEND)) expect(CREDENTIAL_ROWS).toContain(cmd)
  })

  it.each(CREDENTIAL_ROWS.filter((cmd) => !(cmd in STREAM_MAY_SEND)))(
    '%s is refused through a stream',
    (cmd) => {
      expect(api.refusedThroughStream(cmd)).toBe(true)
    },
  )
})

describe('the station refuses credentials while a stream watches its screen', () => {
  it("its refusal reaches the caller as the same typed refusal, in the same words", async () => {
    ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
      invoke: async () => {
        throw 'nexusClient:atShackOnly'
      },
    }
    await expect(api.answerSavedKey(null, true, 'https://log.example.invalid')).rejects.toMatchObject({
      code: api.AT_SHACK_ONLY,
      message: t('remote.lan.keysLockedWhileStreamed'),
    })
  })
})

describe('the saved-key notice tells Settings when a key is forgotten', () => {
  it('Use or Remove fires the credentials-changed event, as a clear does', async () => {
    let told = 0
    const tell = () => { told += 1 }
    window.addEventListener(api.CREDENTIALS_CHANGED, tell)
    try {
      await api.answerSavedKey(1, false, '192.0.2.7')
      expect(told).toBe(1)
      await api.keysToConfirm()
      expect(told, 'CONTROL: a read changes nothing').toBe(1)
    } finally {
      window.removeEventListener(api.CREDENTIALS_CHANGED, tell)
    }
  })
})
