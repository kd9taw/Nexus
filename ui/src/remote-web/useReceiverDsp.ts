import { useContext } from 'react'
import { setAgc, setRigFunc } from '../api'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import { useRemoteStation } from './amplifier-observation'
import { AGC_SPEEDS, RECEIVER_FUNCTIONS, type AgcSpeed, type ReceiverFunction, type StationAction } from './station-operation'

/** Keep the native cockpit controls and native local calls. Remote binds the
 * displayed radio/connection and prior reading; only a later stream sample
 * changes the displayed choice, never the submitted intent or receipt. */
export function useReceiverDsp(snap: AppSnapshot, mode: 'cw' | 'phone') {
  const local = useStationControl(), capability = useStationCapability('receiverDsp')
  const fmCapable = useStationCapability('fmReceiver')
  const operations = useContext(RemoteOperationsContext), { context } = useRemoteStation(snap.activeRadioId)
  const current = operations?.getSnapshot().state?.controls?.context, radio = snap.radio
  const allowed = !!(capability && operations && context && current && context.radioId === current.radioId &&
    context.radioConnection !== null && context.radioConnection === current.radioConnection && context.ampConnection === current.ampConnection &&
    radio.source === 'native' && (!['FM', 'PKTFM'].includes(radio.rigMode ?? '') || fmCapable) && radio.operatingMode === mode && radio.catOk === true && radio.rigKeyed === false &&
    !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason)
  const canFunction = (name: string) => local || !!(allowed && RECEIVER_FUNCTIONS.includes(name as ReceiverFunction) &&
    typeof radio[name as ReceiverFunction] === 'boolean')
  const canAgc = local || !!(allowed && AGC_SPEEDS.includes(radio.agc as AgcSpeed))
  const send = async (action: StationAction): Promise<undefined> => {
    if (!allowed || !operations || !context) throw Error('notController')
    const result = await operations.control(action, context)
    if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
    return undefined
  }
  const changeFunction = (name: Parameters<typeof setRigFunc>[0], on: boolean): Promise<AppSnapshot | undefined> => {
    if (local) return setRigFunc(name, on)
    if (!canFunction(name)) return Promise.reject(Error('notController'))
    const func = name as ReceiverFunction
    return send({ action: 'radio.function', mode, func, expectedOn: radio[func]!, on })
  }
  const changeAgc = (speed: AgcSpeed): Promise<AppSnapshot | undefined> => {
    if (local) return setAgc(speed)
    if (!canAgc) return Promise.reject(Error('notController'))
    return send({ action: 'radio.agc', mode, expectedSpeed: radio.agc as AgcSpeed, speed })
  }
  return { canFunction, canAgc, changeFunction, changeAgc }
}
