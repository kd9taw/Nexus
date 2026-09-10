import { useStationCapability, useStationControl } from '../stationAccess'
import { setOperatingMode } from '../api'
import type { AppSnapshot } from '../types'
import { t } from '../i18n'
import { pushToast } from '../toast'

export type OperatingSection = Parameters<typeof setOperatingMode>[0]

/** Browsing a cockpit is passive. This explicit station gesture reuses the
 * native section-entry verb, beneath the existing operation/receipt transport. */
export function ModeEntry({ snap, mode, onSnap }: {
  snap: AppSnapshot
  mode?: OperatingSection
  onSnap?: (snap: AppSnapshot) => void
}) {
  const local = useStationControl(), allowed = useStationCapability('mode')
  if (local || !mode || snap.radio.operatingMode?.toLowerCase() === mode) return null
  const idle = !snap.radio.txEnabled && !snap.radio.txBusyReason && !snap.radio.transmitting && !snap.radio.rigKeyed && !snap.radio.tuning
  return <button type="button" className="op-btn remote-mode-entry" disabled={!allowed || !idle || snap.radio.catOk !== true}
    title={t('remote.modeEntry.title')}
    onClick={() => {
      if (!allowed || !idle) return
      void setOperatingMode(mode, true).then(s => onSnap?.(s)).catch(() => pushToast(t('remote.controlRequestFailed'), 'error'))
    }}>
    {t('remote.modeEntry.label')}
  </button>
}
