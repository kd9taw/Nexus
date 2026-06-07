import { useCallback, useEffect, useState } from 'react'
import { TreePine, Mountain, RefreshCw, MapPin } from 'lucide-react'
import type { Activation, OtaSpot } from '../types'
import { clearActivation, getActivation, getOtaSpots, setActivation } from '../api'
import { pushToast, withErrorToast } from '../toast'

type Program = 'POTA' | 'SOTA'

/** kHz → a "14.074 MHz" label. */
function fmtFreq(khz: number): string {
  return `${(khz / 1000).toFixed(3)} MHz`
}

/**
 * Parks/Summits On The Air — hunt activators on the air now, and tag your own
 * activation so every logged QSO carries the right park/summit reference (MY_SIG /
 * MY_SOTA_REF), exporting cleanly to pota.app / the SOTA database.
 */
export function PotaSotaView() {
  const [program, setProgram] = useState<Program>('POTA')
  const [spots, setSpots] = useState<OtaSpot[]>([])
  const [loading, setLoading] = useState(false)
  const [activation, setAct] = useState<Activation>({ program: null, reference: null, qsoCount: 0 })
  const [refInput, setRefInput] = useState('')
  const [actProgram, setActProgram] = useState<Program>('POTA')

  const loadSpots = useCallback(async (p: Program) => {
    setLoading(true)
    const s = await withErrorToast(() => getOtaSpots(p), `${p} spots failed`)
    setLoading(false)
    if (s) setSpots(s)
  }, [])

  useEffect(() => {
    loadSpots(program)
  }, [program, loadSpots])

  useEffect(() => {
    getActivation()
      .then(setAct)
      .catch(() => {})
  }, [])

  const onStart = async () => {
    const r = refInput.trim()
    if (!r) return
    const a = await withErrorToast(() => setActivation(actProgram, r), 'Could not start activation')
    if (a) {
      setAct(a)
      pushToast(`Activating ${a.program} ${a.reference} — logged QSOs are now tagged`, 'success')
    }
  }

  const onStop = async () => {
    const a = await withErrorToast(() => clearActivation(), 'Could not end activation')
    if (a) {
      setAct(a)
      pushToast('Activation ended', 'info')
    }
  }

  const activating = activation.reference != null

  return (
    <section className="panel pota-view">
      <div className="panel-header">
        <h2>POTA / SOTA</h2>
        <span className="awards-sub">Parks &amp; Summits on the air</span>
      </div>

      <div className="pota-body">
        {/* My activation */}
        <div className="aw-panel pota-activation">
          <h3>
            <MapPin size={14} aria-hidden="true" /> My activation
          </h3>
          {activating ? (
            <div className="pota-active">
              <span className="pota-active-ref">
                {activation.program} {activation.reference}
              </span>
              <span className="pota-active-count">
                {activation.qsoCount} QSO{activation.qsoCount === 1 ? '' : 's'} logged this activation
              </span>
              <button type="button" className="export-btn" onClick={onStop}>
                End activation
              </button>
            </div>
          ) : (
            <div className="pota-start">
              <div className="filter-row" role="tablist" aria-label="Program">
                {(['POTA', 'SOTA'] as Program[]).map((p) => (
                  <button
                    key={p}
                    type="button"
                    role="tab"
                    aria-selected={actProgram === p}
                    className={`filter-chip${actProgram === p ? ' active' : ''}`}
                    onClick={() => setActProgram(p)}
                  >
                    {p}
                  </button>
                ))}
              </div>
              <div className="settings-input-row">
                <input
                  className="settings-input"
                  value={refInput}
                  onChange={(e) => setRefInput(e.target.value)}
                  placeholder={actProgram === 'POTA' ? 'K-1234' : 'W7A/MN-001'}
                  autoComplete="off"
                  spellCheck={false}
                />
                <button type="button" className="settings-save" onClick={onStart} disabled={!refInput.trim()}>
                  Start
                </button>
              </div>
              <span className="settings-hint">
                Every QSO you log while activating is tagged with this reference (exports to pota.app / SOTA).
              </span>
            </div>
          )}
        </div>

        {/* On the air now (hunter) */}
        <div className="aw-panel pota-spots">
          <div className="pota-spots-head">
            <h3>
              {program === 'POTA' ? <TreePine size={14} aria-hidden="true" /> : <Mountain size={14} aria-hidden="true" />}{' '}
              On the air now
            </h3>
            <div className="filter-row" role="tablist" aria-label="Program">
              {(['POTA', 'SOTA'] as Program[]).map((p) => (
                <button
                  key={p}
                  type="button"
                  role="tab"
                  aria-selected={program === p}
                  className={`filter-chip${program === p ? ' active' : ''}`}
                  onClick={() => setProgram(p)}
                >
                  {p}
                </button>
              ))}
              <button
                type="button"
                className="filter-chip"
                onClick={() => loadSpots(program)}
                disabled={loading}
                title="Refresh"
                aria-label="Refresh spots"
              >
                <RefreshCw size={12} aria-hidden="true" />
              </button>
            </div>
          </div>
          {spots.length === 0 ? (
            <p className="aw-empty">{loading ? 'Loading…' : `No ${program} activators spotted right now.`}</p>
          ) : (
            <ul className="pota-spot-list">
              {spots.map((s, i) => (
                <li className="pota-spot" key={`${s.reference}-${s.activator}-${i}`}>
                  <div className="pota-spot-main">
                    <span className="pota-spot-line1">
                      <span className="pota-spot-call">{s.activator}</span>
                      <span className="pota-spot-ref">{s.reference}</span>
                      <span className="pota-spot-mode">{s.mode}</span>
                    </span>
                    <span className="pota-spot-line2">
                      {s.name || '—'}
                      {s.comment ? ` · ${s.comment}` : ''}
                    </span>
                  </div>
                  <span className="pota-spot-freq mono">{fmtFreq(s.freqKhz)}</span>
                </li>
              ))}
            </ul>
          )}
          <span className="settings-hint">
            Live from {program === 'POTA' ? 'pota.app' : 'SOTAwatch'}. Tune to an activator to hunt it.
          </span>
        </div>
      </div>
    </section>
  )
}
