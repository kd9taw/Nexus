import { useContext } from 'react'
import { setSidebandOverride } from '../api'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import { useRemoteStation } from './amplifier-observation'
import { PHONE_MODES, type PhoneMode } from './station-operation'

type Pick = 'USB' | 'LSB' | 'FM' | 'AM' | null
export function usePhoneMode(snap: AppSnapshot, phoneMode?: string) {
  const local = useStationControl(), capability = useStationCapability('phoneMode'), fm = useStationCapability('fmTuning')
  const operations = useContext(RemoteOperationsContext), { context } = useRemoteStation(snap.activeRadioId)
  const current = operations?.getSnapshot().state?.controls?.context, radio = snap.radio
  const expected = radio.sidebandOverride ?? 'auto'
  const allowed = !!(capability && operations && context && current && context.radioId === current.radioId &&
    context.radioConnection !== null && context.radioConnection === current.radioConnection && context.ampConnection === current.ampConnection &&
    radio.source === 'native' && radio.operatingMode === 'phone' && radio.catOk === true && radio.rigKeyed === false &&
    !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason &&
    PHONE_MODES.includes(expected as PhoneMode) && typeof radio.rigMode === 'string' && radio.rigMode.length > 0 &&
    (fm || (radio.rigMode !== 'FM' && radio.rigMode !== 'PKTFM' && expected !== 'FM')))
  const canPick = (mode: Pick) => local || !!(allowed && PHONE_MODES.includes((mode ?? 'auto') as PhoneMode) &&
    (fm || (mode !== 'FM' && !(mode === null && phoneMode?.toLowerCase() === 'fm' && radio.dialMhz >= 29))))
  const pick = async (mode: Pick): Promise<AppSnapshot | undefined> => {
    if (local) return setSidebandOverride(mode)
    if (!canPick(mode) || !operations || !context) throw Error('notController')
    const result = await operations.control({ action: 'radio.phoneMode', expectedMode: expected as PhoneMode, mode: (mode ?? 'auto') as PhoneMode }, context)
    if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
    return undefined
  }
  return { canPick, pick }
}
