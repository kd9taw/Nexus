import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, LoggedQso } from '../types'
import { Waterfall } from './Waterfall'
import { logQso, sendCw, setCwKeyer, setCwWpm, stopCw } from '../api'
import { pushToast, withErrorToast } from '../toast'

interface Props {
  snap: AppSnapshot
  theme: string
}

/** Default CASUAL/ragchew macro set (no contest serial/exchange), per
 * `tasks/specs/cw-operating.md`. The engine expands the tokens ({MYCALL}/{NAME}/
 * {RST}/! = worked call) with the live QSO context, so we just send the template. */
const MACROS: { key: string; label: string; text: string }[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', label: 'Answer', text: '! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? !' },
  { key: 'F3', label: '73', text: '! 73 ES TU DE {MYCALL} SK' },
  { key: 'F4', label: 'My Call', text: '{MYCALL}' },
  { key: 'F5', label: 'His Call', text: '! ' },
  { key: 'F6', label: 'AGN', text: 'AGN AGN' },
  { key: 'F7', label: 'RR FB', text: 'RR FB' },
  { key: 'F8', label: '?', text: '? ' },
]

const WPM_MIN = 5
const WPM_MAX = 50

/**
 * CW operating cockpit — casual/ragchew. Keyboard + F-key macros key the rig via the
 * CAT keyer (the engine's send_cw path); the waterfall is the CW spectrum; a compact
 * strip logs the QSO into the multi-mode logbook (RST 599). Entering the section forces
 * the rig to CW (the rig-mode policy, wired in App). No contest scoring — by design.
 */
export function CwCockpit({ snap, theme }: Props) {
  const [wpm, setWpm] = useState(25)
  const [keyer, setKeyer] = useState<'cat' | 'soundcard'>('cat')
  const [text, setText] = useState('')
  const [logCall, setLogCall] = useState('')
  const [logRst, setLogRst] = useState('599')
  const [logName, setLogName] = useState('')

  const changeWpm = (w: number) => {
    const v = Math.max(WPM_MIN, Math.min(WPM_MAX, Math.round(w)))
    setWpm(v)
    void setCwWpm(v)
  }
  const send = (t: string) => {
    if (t.trim()) void withErrorToast(() => sendCw(t), 'CW send failed')
  }
  const sendTyped = () => {
    send(text)
    setText('')
  }
  const abort = () => {
    void stopCw()
  }
  const changeKeyer = (k: 'cat' | 'soundcard') => {
    setKeyer(k)
    void setCwKeyer(k)
  }

  // Keyboard: F1–F8 fire macros; Esc aborts; PgUp/PgDn nudge speed (±2, Shift ±4).
  // Live ref so the document listener (bound once) always reads current state.
  const stateRef = useRef({ wpm, text })
  stateRef.current = { wpm, text }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const macro = MACROS.find((m) => m.key === e.key)
      if (macro) {
        e.preventDefault()
        send(macro.text)
      } else if (e.key === 'Escape') {
        e.preventDefault()
        abort()
      } else if (e.key === 'PageUp') {
        e.preventDefault()
        changeWpm(stateRef.current.wpm + (e.shiftKey ? 4 : 2))
      } else if (e.key === 'PageDown') {
        e.preventDefault()
        changeWpm(stateRef.current.wpm - (e.shiftKey ? 4 : 2))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const logIt = async () => {
    const call = logCall.trim().toUpperCase()
    if (!call) return
    const rec: LoggedQso = {
      call,
      grid: null,
      band: snap.radio.band,
      freqMhz: snap.radio.dialMhz,
      mode: 'CW',
      rstSent: logRst.trim() || '599',
      rstRcvd: logRst.trim() || '599',
      name: logName.trim() || null,
      whenUnix: Math.floor(Date.now() / 1000),
      confirmed: false,
      awardConfirmed: false,
    }
    const r = await withErrorToast(() => logQso(rec), 'Could not log the QSO')
    if (r) {
      pushToast(`Logged ${call} (CW)`, 'success')
      setLogCall('')
      setLogName('')
      setLogRst('599')
    }
  }

  return (
    <main className="layout single cw-cockpit">
      <div className="cw-bar">
        <span className="cw-mode-badge" title="The rig is set to CW while you're in this section">
          CW
        </span>
        <label className="cw-wpm" title="Keyer speed — PgUp/PgDn to nudge (Shift = ±4)">
          <span>Speed</span>
          <input
            type="range"
            min={WPM_MIN}
            max={WPM_MAX}
            value={wpm}
            onChange={(e) => changeWpm(Number(e.target.value))}
            aria-label="CW keyer speed (WPM)"
          />
          <span className="cw-wpm-val">{wpm} WPM</span>
        </label>
        <div className="cw-keyer" role="group" aria-label="CW keyer back-end">
          <button
            type="button"
            className={`cw-keyer-opt${keyer === 'cat' ? ' active' : ''}`}
            onClick={() => changeKeyer('cat')}
            title="CAT keyer — the rig generates CW (rig in CW). Zero extra hardware."
          >
            CAT
          </button>
          <button
            type="button"
            className={`cw-keyer-opt${keyer === 'soundcard' ? ' active' : ''}`}
            onClick={() => changeKeyer('soundcard')}
            title="Soundcard keyer — a keyed audio tone (rig in USB). Works on any rig."
          >
            Soundcard
          </button>
        </div>
        <span className="cw-spacer" />
        <span className={`cw-tx ${snap.radio.transmitting ? 'on' : ''}`}>
          {snap.radio.transmitting ? '▲ KEYING' : snap.radio.txEnabled ? '▼ RX' : '■ TX off'}
        </span>
        <button type="button" className="cw-abort" onClick={abort} title="Stop sending (Esc)">
          Abort
        </button>
      </div>

      <section className="cw-waterfall panel">
        <Waterfall
          transmitting={snap.radio.transmitting}
          rxOffsetHz={snap.radio.rxOffsetHz}
          txOffsetHz={snap.radio.txOffsetHz}
          theme={theme}
        />
      </section>

      <div className="cw-macros" role="group" aria-label="CW macros">
        {MACROS.map((m) => (
          <button
            key={m.key}
            type="button"
            className="cw-macro"
            onClick={() => send(m.text)}
            title={m.text}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.label}</span>
          </button>
        ))}
      </div>

      <div className="cw-send">
        <input
          className="settings-input cw-type"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              sendTyped()
            }
          }}
          placeholder="Type CW to send… (Enter)"
          autoComplete="off"
          spellCheck={false}
        />
        <button type="button" className="cw-send-btn" onClick={sendTyped} disabled={!text.trim()}>
          Send
        </button>
      </div>

      <div className="cw-log">
        <h2>Log this QSO</h2>
        <div className="cw-log-row">
          <input
            className="settings-input mono"
            value={logCall}
            onChange={(e) => setLogCall(e.target.value.toUpperCase())}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void logIt()
            }}
            placeholder="Call"
            autoComplete="off"
            spellCheck={false}
          />
          <input
            className="settings-input mono cw-log-rst"
            value={logRst}
            onChange={(e) => setLogRst(e.target.value)}
            placeholder="RST"
            autoComplete="off"
          />
          <input
            className="settings-input"
            value={logName}
            onChange={(e) => setLogName(e.target.value)}
            placeholder="Name"
            autoComplete="off"
          />
          <button type="button" className="cw-log-btn" onClick={logIt} disabled={!logCall.trim()}>
            Log
          </button>
        </div>
        <span className="cw-log-hint">
          Logs to the shared logbook as CW · {snap.radio.band} · {snap.radio.dialMhz.toFixed(3)} MHz
        </span>
      </div>
    </main>
  )
}
