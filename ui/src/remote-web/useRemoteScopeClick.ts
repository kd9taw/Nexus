import { useContext, useRef } from 'react'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationControl } from '../stationAccess'
import { useRemoteWheelTuning } from './wheel-tuning-context'

/** Reuse the station's frequency owner; PhoneScope retains signal detection,
 * CW pitch and sideband math. Authority belongs to the original pointer press. */
export function useRemoteScopeClick(snap: AppSnapshot) {
  const local = useStationControl(), { controller, allowed: frequency } = useRemoteWheelTuning()
  const operations = useContext(RemoteOperationsContext), owner = useRef({})
  const radio = snap.radio, context = operations?.getSnapshot().state?.controls?.context
  const allowed = !!(!local && frequency && controller && context && snap.activeRadioId === context.radioId &&
    radio.source === 'native' && ['cw', 'phone'].includes(radio.operatingMode ?? '') && radio.catOk === true &&
    radio.rigKeyed === false && !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason)
  const begin = () => allowed && controller ? controller.captureTarget({
    dialMhz: radio.dialMhz, sideband: radio.sideband, owner: owner.current,
  }) : null
  return { allowed, begin }
}
