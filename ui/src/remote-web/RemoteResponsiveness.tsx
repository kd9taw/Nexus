import { useContext, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { RemoteOperationsContext, useStationHeld } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import { RemoteWheelTuningContext } from './wheel-tuning-context'
import { ResponsivenessProbe, runResponsivenessCheck, reportText, type CheckPhase, type CheckReport } from './responsiveness'

/** "Responsiveness → Run check": the operator's own measurement of this link (responsiveness.ts).
 * The probe is attached for exactly as long as this panel is mounted — that is the whole of its
 * cost to anyone who never opens it — and the check itself runs only on the button. */
export function RemoteResponsiveness() {
  const operations = useContext(RemoteOperationsContext), tuning = useContext(RemoteWheelTuningContext)
  const application = useContext(RemoteCollectionsContext)?.client ?? null
  const held = useStationHeld()
  const probe = useRef<ResponsivenessProbe | null>(null)
  const live = useRef(true)
  const [phase, setPhase] = useState<CheckPhase | null>(null)
  const [report, setReport] = useState<CheckReport | null>(null)
  const [copied, setCopied] = useState(false)
  useEffect(() => { live.current = true; return () => { live.current = false } }, [])
  useEffect(() => {
    if (!operations || !tuning || !application) return
    const p = probe.current ??= new ResponsivenessProbe(() => performance.now(),
      typeof requestAnimationFrame === 'function' ? cb => { requestAnimationFrame(cb) } : undefined)
    p.attach({ operations, tuning, application })
    return () => p.detach()
  }, [operations, tuning, application])
  const controlling = !!(operations && tuning && application) && held
  const run = async () => {
    if (!operations || !tuning || !application || !probe.current || phase) return
    setReport(null); setCopied(false); setPhase('tuning')
    // A wait that fails once the panel is gone ends the script at its next step.
    const wait = (ms: number) => new Promise<void>((resolve, reject) => setTimeout(() => live.current ? resolve() : reject(new Error('cancelled')), ms))
    try {
      const result = await runResponsivenessCheck({ operations, tuning, application, probe: probe.current, wait }, next => { if (live.current) setPhase(next) })
      if (live.current) setReport(result)
    } catch {} finally { if (live.current) setPhase(null) }
  }
  const text = report ? reportText(report) : ''
  const copy = async () => {
    try { await navigator.clipboard.writeText(text); setCopied(true) } catch { setCopied(false) }
  }
  const progress = phase === 'tuning' ? t('remote.responsiveness.phase.tuning') : phase === 'band' ? t('remote.responsiveness.phase.band')
    : phase === 'stop' ? t('remote.responsiveness.phase.stop') : phase === 'settling' ? t('remote.responsiveness.phase.settling') : null
  return <section className="remote-responsiveness" aria-labelledby="remote-responsiveness-title">
    <div className="remote-responsiveness-row">
      <h3 id="remote-responsiveness-title">{t('remote.responsiveness.title')}</h3>
      <button type="button" className="le-qrz le-lookup" disabled={!controlling || phase !== null} onClick={() => { void run() }}>{t('remote.responsiveness.run')}</button>
      {report && <button type="button" className="le-qrz le-lookup" onClick={() => { void copy() }}>{copied ? t('remote.responsiveness.copied') : t('remote.responsiveness.copy')}</button>}
    </div>
    <p role="status">{progress ?? (controlling ? t('remote.responsiveness.hint') : t('remote.responsiveness.needControl'))}</p>
    {report && <textarea className="remote-responsiveness-report" readOnly value={text} rows={text.split('\n').length} onFocus={event => event.currentTarget.select()} />}
  </section>
}
