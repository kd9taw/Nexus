// @vitest-environment jsdom
// A stored password, key, token or code is saved or forgotten at the station's own screen only.
// The station's window may be streamed to a browser, and a press made there reaches this page as a
// DOM event (`isStreamInput`); each credential command it starts is refused before it is sent, in
// words its caller shows. Everything else a stream does stands: the stream is the shack.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as api from './api'
import { t } from './i18n'

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

/** Every credential the Settings panel saves or forgets, with the command each sends. */
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
]

describe('credentials are saved and forgotten at the station only', () => {
  it.each(CREDENTIALS)('%s through a stream is refused and never sent', async (_cmd, call) => {
    stream.on = true
    await expect(call()).rejects.toBe(t('remote.lan.atShackOnly'))
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
})
