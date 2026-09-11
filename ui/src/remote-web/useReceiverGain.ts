import { useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { RemoteOperationsContext, useStationCapability } from '../stationAccess'
import type { RadioStatus } from '../types'
import { pushToast } from '../toast'
import { t } from '../i18n'
import type { SettingsConfiguration } from './configuration'
import { useRemoteStation } from './amplifier-observation'

type Draft = { radioId: number; revision: string; expected: number; gain: number; canceled: boolean }
const idleSubscribe = () => () => {}
const idleSnapshot = () => null
const validGain = (value: number) => Number.isFinite(value) && value >= 1 && value <= 8

/** Keep the native release-to-apply slider. A drag belongs to the configuration
 * where it began; losing that context cancels it even if permission returns
 * before the pointer/key is released. Only station reads confirm a new value. */
export function useReceiverGain(doc: SettingsConfiguration | null, radioId: number | undefined,
  radio: RadioStatus | undefined, refresh: () => void) {
  const client = useContext(RemoteOperationsContext), allowed = useStationCapability('receiverGain')
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  const observation = useRemoteStation(radioId), measured = observation.station?.radio
  const documentRadio = typeof doc?.settings.activeRadio === 'number' ? doc.settings.activeRadio : undefined
  const expected = typeof doc?.settings.rxGain === 'number' && Number.isFinite(doc.settings.rxGain) ? doc.settings.rxGain : null
  const [draft, setDraft] = useState<Draft | null>(null), edit = useRef<Draft | null>(null)
  const [saving, setSaving] = useState(false), inFlight = useRef(false)
  const [confirmed, setConfirmed] = useState<Draft | null>(null)
  const refreshRef = useRef(refresh)
  refreshRef.current = refresh
  const supported = (client?.operationVersion ?? 0) >= 3 && !!view?.state?.controls?.capabilities.includes('receiverGain')
  const canEdit = !!(allowed && client && doc && expected !== null && documentRadio === radioId &&
    radio?.source === 'native' && radio.catOk === true && !radio.txEnabled && !radio.transmitting &&
    !radio.rigKeyed && !radio.tuning && !radio.txBusyReason && observation.context &&
    measured?.catConnected && measured.rigKeyed === false && !measured.nexusBusy &&
    measured.readings.ptt && measured.readings.ptt.ageMs < 1000 && !saving && !confirmed)
  const same = (value: Draft) => value.radioId === documentRadio && value.revision === doc?.revision && value.expected === expected
  const current = draft && !draft.canceled && same(draft) && canEdit ? draft : null

  useEffect(() => {
    if (edit.current && (!canEdit || !same(edit.current))) {
      edit.current.canceled = true
      setDraft(null)
    }
    if (confirmed && (confirmed.radioId !== documentRadio || confirmed.revision !== doc?.revision)) {
      if (confirmed.radioId !== documentRadio || confirmed.gain !== expected)
        pushToast(t('remote.controlRequestFailed'), 'error')
      setConfirmed(null)
    }
  }, [canEdit, documentRadio, doc?.revision, expected, confirmed])

  // Result recovery is a fresh read, never another slider command.
  useEffect(() => {
    const result = view?.controlResult
    if (result?.outcome === 'applied' && result.evidence === 'settingsSaved' ||
      result?.outcome === 'rejected' && result.reason === 'contextChanged') refreshRef.current()
  }, [view?.controlResult])

  const begin = () => {
    if (edit.current || !canEdit || !doc || documentRadio === undefined || expected === null) return
    edit.current = { radioId: documentRadio, revision: doc.revision, expected, gain: expected, canceled: false }
    setDraft({ ...edit.current })
  }
  const change = (gain: number) => {
    const value = edit.current
    if (!value || value.canceled || !canEdit || !same(value) || !validGain(gain)) return
    value.gain = gain
    setDraft({ ...value })
  }
  const cancel = () => { edit.current = null; setDraft(null) }
  const commit = async () => {
    const value = edit.current
    cancel()
    if (!value || value.canceled || !canEdit || !same(value) || !validGain(value.gain) ||
      value.gain === value.expected || !client || !observation.context || inFlight.current) return
    inFlight.current = true
    setSaving(true)
    try {
      const result = await client.control({ action: 'receiver.rxGain', radioId: value.radioId,
        expectedSettingsRevision: value.revision, expectedGain: value.expected, gain: value.gain }, observation.context)
      if (result.outcome !== 'applied' || result.evidence !== 'settingsSaved') throw Error('gainUnconfirmed')
      setConfirmed(value)
    } catch { pushToast(t('settings.audio.rxGain.failed'), 'error'); refreshRef.current() }
    finally { inFlight.current = false; setSaving(false) }
  }
  return { supported, canEdit, gain: current?.gain ?? expected, begin, change, commit, cancel }
}
