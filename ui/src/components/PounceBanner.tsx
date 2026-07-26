// The Pounce banner — a rare one just appeared, and you have seconds, not minutes.
//
// Shown across the top of whatever you are doing, because that is the point: it fires the moment
// a needed spot lands in the RBN/cluster firehose, before the needed board's 30 s poll would have
// caught it. The earcon has already played by the time this renders (see usePounce) — the banner
// is the "what and where", and the button is the "go".
//
// It does NOT auto-dismiss. A DX alert that vanishes while you are turning the rotator is worse
// than no alert; the operator closes it, or works it.

import type { PounceAlert } from '../usePounce'
import { NEED_VISUALS } from '../features/needVisuals'
import type { NeedCat } from '../features/needVisuals'

/** Backend NeedTag → the shared chip vocabulary, so the banner reads like the rest of the app. */
const TAG_TO_CAT: Record<string, NeedCat> = {
  NewEntity: 'entity',
  NewZone: 'zone',
  NewState: 'state',
  NewGrid: 'grid',
  NewBand: 'band',
  NewMode: 'mode',
  Wanted: 'wanted',
}

interface Props {
  alert: PounceAlert | null
  onDismiss: () => void
  /** Work it: QSY to the spot and set up the QSO. */
  onWork: (a: PounceAlert) => void
  /** Why working it is refused right now (in a QSO, transmitting), or null when it's allowed.
   *
   * This banner appears UNBIDDEN over whatever the operator is doing, with a prominent button.
   * `work_spot` has no in-QSO guard of its own, so without this a stray click QSYs away from a
   * live contact — turning an operator-initiated action into an accident. Alerts notify; they
   * must not become a way to lose the contact you were already in. */
  blockReason?: string | null
}

export function PounceBanner({ alert, onDismiss, onWork, blockReason }: Props) {
  if (!alert) return null
  const cat = TAG_TO_CAT[alert.tags[0] ?? '']
  const vis = cat ? NEED_VISUALS[cat] : null
  const where = alert.freqMhz ? `${alert.freqMhz.toFixed(3)} MHz` : alert.band

  return (
    // `alert` (not `status`): this is assertive by design — it interrupts, which is the feature.
    <div className="pounce-banner" role="alert" aria-live="assertive">
      <span className="pounce-tag">{vis?.label ?? 'NEW'}</span>
      <span className="pounce-what">
        <strong>{alert.call}</strong>
        {alert.entity ? <span className="pounce-entity">{alert.entity}</span> : null}
      </span>
      <span className="pounce-where">
        {where} · {alert.mode} · {alert.band}
      </span>
      <button
        type="button"
        className="pounce-work"
        onClick={() => onWork(alert)}
        disabled={!!blockReason}
        title={blockReason ?? `QSY to ${alert.call} and start the QSO`}
      >
        {blockReason ?? 'Work it'}
      </button>
      <button
        type="button"
        className="pounce-close"
        onClick={onDismiss}
        aria-label="Dismiss alert"
        title="Dismiss"
      >
        ✕
      </button>
    </div>
  )
}
