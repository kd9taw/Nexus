import { useContext } from 'react'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import { useRemoteStation } from './amplifier-observation'
import { CONTROL_TIERS, type StationAction } from './station-operation'
import { setOperatingMode } from '../api'
import type { AppSnapshot } from '../types'
import { t } from '../i18n'
import { pushToast } from '../toast'

export type OperatingSection = Parameters<typeof setOperatingMode>[0]
export type OperatingWorkspace = Extract<StationAction, { action: 'radio.workspace' }>['workspace']

/** Browsing a cockpit is passive. This explicit station gesture reuses the
 * native section-entry verb, beneath the existing operation/receipt transport. */
export function ModeEntry({ snap, mode, workspace, onSnap }: {
  snap: AppSnapshot
  mode?: OperatingSection
  workspace?: OperatingWorkspace
  onSnap?: (snap: AppSnapshot) => void
}) {
  const local = useStationControl(), allowed = useStationCapability(workspace ? 'workspace' : 'mode')
  const operations = useContext(RemoteOperationsContext), observation = useRemoteStation(snap.activeRadioId)
  const tier = snap.link?.tier, digital = snap.radio.operatingMode?.toLowerCase() === 'digital'
  const chatTier = tier === 'TempoFast' || tier === 'TempoDeep'
  const matches = workspace ? digital && (workspace === 'tempo' ? chatTier && snap.mode === 'chat'
    : workspace === 'js8' ? tier === 'JS8' : tier != null && CONTROL_TIERS.includes(tier) && !chatTier && tier !== 'JS8' && snap.mode !== 'chat')
    : snap.radio.operatingMode?.toLowerCase() === mode
  if (local || (!mode && !workspace) || matches) return null
  const radio = observation.station?.radio
  const idle = !snap.radio.txEnabled && !snap.radio.txBusyReason && !snap.radio.transmitting && !snap.radio.rigKeyed && !snap.radio.tuning &&
    (!workspace || !!(radio?.catConnected && radio.rigKeyed === false && !radio.nexusBusy && radio.readings.ptt && radio.readings.ptt.ageMs < 1000))
  return <button type="button" className="op-btn remote-mode-entry" data-remote-workspace={workspace} disabled={!allowed || !idle || snap.radio.catOk !== true}
    title={workspace ? t('remote.workspaceEntry.title') : t('remote.modeEntry.title')}
    onClick={() => {
      if (!allowed || !idle) return
      if (workspace) {
        if (!operations || !observation.context) return
        void operations.control({ action: 'radio.workspace', workspace }, observation.context).then(result => {
          if (result.outcome !== 'applied') throw Error('operationUnconfirmed')
          // The subscribed native snapshot supplies the actual resulting state.
        }).catch(() => pushToast(t('remote.controlRequestFailed'), 'error'))
      } else if (mode) {
        void setOperatingMode(mode, true).then(s => onSnap?.(s)).catch(() => pushToast(t('remote.controlRequestFailed'), 'error'))
      }
    }}>
    {t('remote.modeEntry.label')}
  </button>
}
