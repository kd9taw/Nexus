// The Pounce banner — a rare one just appeared, and you have seconds, not minutes.
//
// Shown across the top of whatever you are doing, because that is the point: it fires the moment
// a needed spot lands in the RBN/cluster firehose, before the needed board's 30 s poll would have
// caught it. The earcon has already played by the time this renders (see usePounce) — the banner
// is the "what and where", and the button is the "go".
//
// It does NOT auto-dismiss. A DX alert that vanishes while you are turning the rotator is worse
// than no alert; the operator closes it, or works it.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The callsign, the
// entity, the frequency, the band and the mode it shouts are data and stay as they arrive; the
// need word comes from `features/needVisuals.ts` with the rest of that vocabulary.

import type { PounceAlert } from '../usePounce'
import { NEED_VISUALS } from '../features/needVisuals'
import type { NeedCat } from '../features/needVisuals'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { t } from '../i18n'

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
  /** The operator's own square. This banner is the one place in the app where a beam
   * heading is time-critical — "you have seconds, not minutes" is the whole premise,
   * and turning the antenna is what those seconds are spent on. */
  myGrid?: string
}

export function PounceBanner({ alert, onDismiss, onWork, myGrid = '' }: Props) {
  // Before the early return: hooks must run on every render of this component.
  const centroids = useEntityCentroids()
  if (!alert) return null
  const cat = TAG_TO_CAT[alert.tags[0] ?? '']
  const vis = cat ? NEED_VISUALS[cat] : null
  const where = alert.freqMhz ? `${alert.freqMhz.toFixed(3)} MHz` : alert.band

  return (
    // `alert` (not `status`): this is assertive by design — it interrupts, which is the feature.
    <div className="pounce-banner" role="alert" aria-live="assertive">
      <span className="pounce-tag">{vis?.label ?? t('pounce.tag.fallback')}</span>
      <span className="pounce-what">
        <strong>{alert.call}</strong>
        {alert.entity ? <span className="pounce-entity">{alert.entity}</span> : null}
        {(() => {
          const az = azimuthTo(myGrid, null, alert.entity, centroids)
          return az ? (
            <span className="pounce-az" title={azimuthTitle(az, alert.entity)}>
              {azimuthLabel(az)}
            </span>
          ) : null
        })()}
      </span>
      <span className="pounce-where">
        {where} · {alert.mode} · {alert.band}
      </span>
      <button
        type="button"
        className="pounce-work"
        onClick={() => onWork(alert)}
        title={t('pounce.work.title', { call: alert.call })}
      >
        {t('pounce.work.label')}
      </button>
      <button
        type="button"
        className="pounce-close"
        onClick={onDismiss}
        aria-label={t('pounce.dismiss.aria')}
        title={t('common.dismiss')}
      >
        ✕
      </button>
    </div>
  )
}
