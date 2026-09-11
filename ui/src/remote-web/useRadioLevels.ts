import { useContext } from 'react'
import { setRfPower, setMicGain, setNrLevel, setCompLevel, setNotchFreq } from '../api'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import { useRemoteStation } from './amplifier-observation'
import type { RadioLevel, StationAction } from './station-operation'

const fields = { power: 'rfPower', micGain: 'micGain', nr: 'nrLevel', compression: 'compLevel', notch: 'notchFreqHz' } as const

/** Keep the existing native controls. Remote displays station samples and never
 * treats the submitted value or its receipt as a replacement hardware reading. */
export function useRadioLevels(snap: AppSnapshot) {
  const local = useStationControl(), capability = useStationCapability('radioLevels')
  const operations = useContext(RemoteOperationsContext), { context } = useRemoteStation(snap.activeRadioId)
  const current = operations?.getSnapshot().state?.controls?.context, radio = snap.radio
  const mode = radio.operatingMode
  const allowed = !!(capability && operations && context && current && context.radioId === current.radioId && snap.activeRadioId === context.radioId &&
    context.radioConnection !== null && context.radioConnection === current.radioConnection && context.ampConnection === current.ampConnection &&
    radio.source === 'native' && ['digital', 'phone', 'cw', 'rtty', 'keyboard'].includes(mode ?? '') &&
    radio.catOk === true && radio.rigKeyed === false && !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason)
  const can = (level: RadioLevel) => local || !!(allowed && typeof radio[fields[level]] === 'number' && Number.isFinite(radio[fields[level]]))
  const change = async (level: RadioLevel, value: number): Promise<AppSnapshot | undefined> => {
    if (local) {
      switch (level) {
        case 'power': return setRfPower(value)
        case 'micGain': return setMicGain(value)
        case 'nr': return setNrLevel(value)
        case 'compression': return setCompLevel(value)
        case 'notch': return setNotchFreq(value)
      }
    }
    if (!can(level) || !operations || !context || !Number.isFinite(value)) throw Error('notController')
    const action: StationAction = { action: 'radio.level', mode: mode as 'digital' | 'phone' | 'cw' | 'rtty' | 'keyboard',
      level, expected: radio[fields[level]]!, value }
    const result = await operations.control(action, context)
    if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
    return undefined
  }
  return { can, change }
}
