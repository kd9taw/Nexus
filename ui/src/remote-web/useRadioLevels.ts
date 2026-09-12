import { useContext, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { pushToast } from '../toast'
import { setRfPower, setMicGain, setNrLevel, setCompLevel, setNotchFreq } from '../api'
import type { AppSnapshot } from '../types'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import { useRemoteStation } from './amplifier-observation'
import type { ControlContext, RadioLevel, StationAction } from './station-operation'

const fields = { power: 'rfPower', micGain: 'micGain', nr: 'nrLevel', compression: 'compLevel', notch: 'notchFreqHz' } as const

type Draft = { level: RadioLevel; expected: number; value: number; mode: string;
  context: ControlContext; leaseId: string; revision: number; boot: string; canceled: boolean }
const adjustmentKeys = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'])

/** Keep the existing native controls. Remote displays station samples and never
 * treats the submitted value or its receipt as a replacement hardware reading. */
export function useRadioLevels(snap: AppSnapshot) {
  const local = useStationControl(), capability = useStationCapability('radioLevels')
  const fmCapable = useStationCapability('fmReceiver')
  const operations = useContext(RemoteOperationsContext), { context } = useRemoteStation(snap.activeRadioId)
  const state = operations?.getSnapshot().state, current = state?.controls?.context, radio = snap.radio
  const mode = radio.operatingMode
  const allowed = !!(capability && operations && context && current && context.radioId === current.radioId && snap.activeRadioId === context.radioId &&
    context.radioConnection !== null && context.radioConnection === current.radioConnection && context.ampConnection === current.ampConnection &&
    radio.source === 'native' && (!['FM', 'PKTFM'].includes(radio.rigMode ?? '') || fmCapable) && ['digital', 'phone', 'cw', 'rtty', 'keyboard'].includes(mode ?? '') &&
    radio.catOk === true && radio.rigKeyed === false && !radio.txEnabled && !radio.transmitting && !radio.tuning && !radio.txBusyReason)
  const can = (level: RadioLevel) => local || !!(allowed && typeof radio[fields[level]] === 'number' && Number.isFinite(radio[fields[level]]))
  const edit = useRef<Draft | null>(null), [draft, setDraft] = useState<Draft | null>(null)
  const same = (d: Draft) => can(d.level) && !d.canceled && mode === d.mode && radio[fields[d.level]] === d.expected &&
    state?.stationBootId === d.boot && state.leaseId === d.leaseId && state.revision === d.revision &&
    context?.radioId === d.context.radioId && context.radioConnection === d.context.radioConnection && context.ampConnection === d.context.ampConnection
  const cancel = (level: RadioLevel) => {
    if (edit.current?.level !== level) return
    // Keep a tombstone until release so restored authority cannot revive a drag.
    edit.current.canceled = true
    setDraft(null)
  }
  useEffect(() => {
    const d = edit.current
    if (d && !d.canceled && !same(d)) cancel(d.level)
  }, [allowed, mode, state, context, radio])
  const begin = (level: RadioLevel) => {
    if (local || !can(level) || !state?.leaseId || !context) return
    if (edit.current?.level === level && !edit.current.canceled) return
    edit.current = { level, expected: radio[fields[level]]!, value: radio[fields[level]]!, mode: mode!,
      context: { ...context }, leaseId: state.leaseId, revision: state.revision, boot: state.stationBootId, canceled: false }
    setDraft({ ...edit.current })
  }
  const send = async (level: RadioLevel, value: number): Promise<AppSnapshot | undefined> => {
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
  const change = async (level: RadioLevel, value: number): Promise<AppSnapshot | undefined> => {
    const d = edit.current
    if (!local && d?.level === level) {
      if (!same(d) || !Number.isFinite(value)) { cancel(level); return }
      d.value = value
      setDraft({ ...d })
      return
    }
    return send(level, value)
  }
  const finish = async (level: RadioLevel) => {
    const d = edit.current
    if (d?.level !== level) return
    edit.current = null
    setDraft(null)
    if (!same(d) || d.value === d.expected) return
    try { await send(level, d.value) }
    catch (error) { pushToast(String(error), 'error') }
  }
  const input = (level: RadioLevel) => ({
    onPointerDown: () => begin(level),
    onPointerUp: () => { void finish(level) },
    onPointerCancel: () => cancel(level),
    onBlur: () => cancel(level),
    onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => { if (!e.repeat && adjustmentKeys.has(e.key)) begin(level) },
    onKeyUp: (e: KeyboardEvent<HTMLInputElement>) => { if (adjustmentKeys.has(e.key)) void finish(level) },
  })
  return { can, change, input, draft: (level: RadioLevel) => draft?.level === level && same(draft) ? draft.value : undefined }
}
