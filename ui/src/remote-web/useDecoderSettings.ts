import { useContext } from 'react'
import { RemoteOperationsContext, useStationCapability } from '../stationAccess'
import type { AppSnapshot, Tier } from '../types'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { useRemoteStation } from './amplifier-observation'
import type { StationAction } from './station-operation'

type DecoderSetting = Extract<StationAction, { action: 'decoder.js8Speed' | 'decoder.msk144Period' }>

/** The existing cockpit widgets keep displaying station samples. A receipt
 * confirms the saved choice; it never manufactures a decoder state locally. */
export function useDecoderSettings(snap: AppSnapshot | null, tier: Tier) {
  const client = useContext(RemoteOperationsContext), capability = useStationCapability('decoderSettings')
  const observation = useRemoteStation(snap?.activeRadioId), radio = observation.station?.radio
  const allowed = !!(capability && client && observation.context && snap?.link.tier === tier &&
    snap.radio.operatingMode?.toLowerCase() === 'digital' && snap.radio.source === 'native' && snap.radio.catOk === true &&
    !snap.radio.txEnabled && !snap.radio.transmitting && !snap.radio.rigKeyed && !snap.radio.tuning && !snap.radio.txBusyReason &&
    radio?.catConnected && radio.rigKeyed === false && !radio.nexusBusy && radio.readings.ptt && radio.readings.ptt.ageMs < 1000)
  const change = (action: DecoderSetting) => {
    if (!allowed || !client || !observation.context) return
    void client.control(action, observation.context).then(result => {
      if (result.outcome !== 'applied' || result.evidence !== 'settingsSaved') throw Error('operationUnconfirmed')
    }).catch(() => pushToast(t('remote.controlRequestFailed'), 'error'))
  }
  return { allowed, change }
}
