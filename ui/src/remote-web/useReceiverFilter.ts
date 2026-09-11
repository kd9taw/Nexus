import { useContext } from 'react'
import { setFilterWidth } from '../api'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'

/** The native steppers keep their own range/formatting and API. Remote binds
 * the displayed width and cockpit mode and waits for the actual radio owner;
 * the regular station stream supplies the later display, without optimism. */
export function useReceiverFilter(snap: AppSnapshot, mode: 'cw' | 'phone') {
  const local = useStationControl(), capable = useStationCapability('receiverFilter')
  const operations = useContext(RemoteOperationsContext), radio = snap.radio
  const context = operations?.getSnapshot().state?.controls?.context
  const prior = radio.filterWidthHz
  const allowed = local || !!(capable && operations && context && snap.activeRadioId === context.radioId &&
    context.radioConnection !== null && radio.source === 'native' && radio.operatingMode === mode &&
    radio.catOk === true && radio.rigKeyed === false && !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason &&
    typeof prior === 'number' && Number.isInteger(prior) && prior > 0)
  const setWidth = async (hz: number): Promise<AppSnapshot | undefined> => {
    if (local) return setFilterWidth(hz)
    if (!allowed || !operations || !context || prior == null) throw Error('notController')
    const result = await operations.control({ action: 'radio.filterWidth', mode, expectedHz: prior, hz }, context)
    if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
    return undefined
  }
  return { allowed, setWidth }
}
