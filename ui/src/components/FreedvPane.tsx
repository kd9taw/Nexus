// The Phone screen's FreeDV pane — FreeDV's own app beside Nexus, RECEIVE ONLY.
//
// It shows what the station's FreeDV listeners report (`snap.freedv`, `tempo_app::freedv`):
// whether FreeDV has been heard from on the contact port, the last contact it logged into the
// Nexus log, the last one refused and why, and the calls FreeDV decoded this session. A click on
// a heard call puts it in the log strip's call field, and that is all: NOTHING HERE LOGS, KEYS OR
// TUNES. It is a fill pane under the pane contract (cockpit-panes.css) and holds no transmit
// control, so hiding it ends nothing.
//
// Callsigns, mode names (RADEV1), SNRs, frequencies, the address and the port are invariant
// tokens: they are the same characters in every language, and numbers go through
// `invariantNumber`.

import { getSettings, setSettings } from '../api'
import { invariantNumber, t } from '../i18n'
import type { FreedvState } from '../types'

export interface FreedvPaneProps {
  /** The station's FreeDV state; absent while neither listener is wanted, and on the Remote. */
  freedv: FreedvState | null | undefined
  /** Put a heard call in the log strip's call field. */
  onPickCall: (call: string) => void
  /** May this screen change the station's settings (false on the Remote). */
  control: boolean
  /** Seconds since the epoch now, for "connected" (a test passes a fixed one). */
  nowUnix?: number
}

/** FreeDV counts as connected for this long after its last datagram: its heartbeat is periodic. */
const ALIVE_SECS = 120

/** Turn on the heard-call listener: the settings as stored now, with that one field changed. */
async function listenForHeardCalls(): Promise<void> {
  const s = await getSettings()
  await setSettings({ ...s, freedvHeard: true })
}

export function FreedvPane({ freedv, onPickCall, control, nowUnix }: FreedvPaneProps) {
  const now = nowUnix ?? Math.floor(Date.now() / 1000)
  const connected = freedv?.aliveUnix != null && now - freedv.aliveUnix < ALIVE_SECS
  const heardOn = freedv?.heardPort != null
  return (
    <div className="freedv-pane" data-testid="freedv-pane">
      <p className="dim" role="status">
        {freedv == null
          ? t('phone.freedv.off')
          : freedv.logError
            ? freedv.logError
            : freedv.logPort == null
              ? t('phone.freedv.logOff')
              : connected
                ? t('phone.freedv.connected')
                : t('phone.freedv.waiting', { address: `127.0.0.1:${freedv.logPort}` })}
      </p>
      {freedv?.lastContact && (
        <p className="dim">{t('phone.freedv.lastContact', { contact: freedv.lastContact })}</p>
      )}
      {freedv?.notice && (
        <p className="freedv-notice" role="alert">
          {freedv.notice}
        </p>
      )}
      {freedv?.heardError ? (
        <p className="freedv-notice" role="alert">
          {freedv.heardError}
        </p>
      ) : !heardOn ? (
        <p className="dim">
          {t('phone.freedv.heard.off')}{' '}
          {control && (
            <button type="button" className="settings-linkbtn" onClick={() => void listenForHeardCalls().catch(() => {})}>
              {t('phone.freedv.heard.listen')}
            </button>
          )}
        </p>
      ) : freedv && freedv.heard.length === 0 ? (
        <p className="dim">{t('phone.freedv.heard.empty')}</p>
      ) : (
        <ul className="freedv-heard" aria-label={t('phone.freedv.heard.label')}>
          {freedv?.heard.map((h) => (
            <li key={h.call}>
              <button
                type="button"
                className="freedv-heard-call mono"
                title={t('phone.freedv.heard.pick', { call: h.call })}
                onClick={() => onPickCall(h.call)}
              >
                {h.call}
              </button>
              <span className="dim mono">
                {[
                  h.mode,
                  h.snr != null ? `${invariantNumber(h.snr)} dB` : '',
                  h.frequencyHz != null ? `${(h.frequencyHz / 1e6).toFixed(3)} MHz` : '',
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
