import { useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { RemoteOperationsContext, useStationCapability } from '../stationAccess'
import type { SettingsConfiguration } from './configuration'
import { useRemoteAmplifier } from './amplifier-observation'

type Draft = { radioId: number; revision: string; expected: boolean; follow: boolean }
const idleSubscribe = () => () => {}
const idleSnapshot = () => null
/** The native checkbox-then-Save interaction, with only this one field writable.
 * Refreshes cannot give an unsaved draft a different profile or revision. */
export function useAmplifierFollow(doc: SettingsConfiguration | null, radioId: number | undefined, refresh: () => void) {
  const client = useContext(RemoteOperationsContext), allowed = useStationCapability('ampFollowBand')
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  const observation = useRemoteAmplifier(radioId)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [confirmed, setConfirmed] = useState<Draft | null>(null)
  const [saving, setSaving] = useState(false), [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const refreshRef = useRef(refresh)
  refreshRef.current = refresh
  const documentRadio = typeof doc?.settings.activeRadio === 'number' ? doc.settings.activeRadio : undefined
  const expected = doc?.settings.ampFollowBand === true
  const same = (value: Draft) => value.radioId === documentRadio && value.revision === doc?.revision
  const currentDraft = draft && same(draft) ? draft : null
  const configured = !!doc && documentRadio === radioId && ['spe', 'kpa'].includes(String(doc.settings.ampModel).trim().toLowerCase()) &&
    typeof doc.settings.ampPort === 'string' && doc.settings.ampPort.trim() !== ''
  const supported = (client?.operationVersion ?? 0) >= 3 && !!view?.state?.controls?.capabilities.includes('ampFollowBand')
  const canEdit = allowed && configured && !saving && !confirmed
  const canEnable = observation.context !== null && observation.idle && view?.state?.txArmed === false
  const canSave = !!(canEdit && currentDraft && (!currentDraft.follow || canEnable))

  useEffect(() => {
    if (!doc || saving) return
    if (confirmed) {
      if (same(confirmed)) return // Receipt is saved; await the next configuration capture.
      setConfirmed(null); setDraft(null)
      if (confirmed.radioId === documentRadio && confirmed.follow === expected) { setSaved(true); setError(null) }
      else { setSaved(false); setError('contextChanged') }
    } else if (draft && !same(draft)) {
      setDraft(null); setSaved(false); setError('contextChanged')
    }
  }, [doc?.revision, documentRadio, expected, saving, confirmed, draft])

  // A recovered receipt also triggers a fresh read; no command is resubmitted.
  useEffect(() => {
    const result = view?.controlResult
    if (result?.outcome === 'applied' && result.evidence === 'settingsSaved' || result?.outcome === 'rejected' && result.reason === 'contextChanged') {
      refreshRef.current()
    }
  }, [view?.controlResult])

  const change = (follow: boolean) => {
    if (!canEdit || !doc || documentRadio === undefined) return
    setSaved(false); setError(null)
    setDraft(follow === expected ? null : { radioId: documentRadio, revision: doc.revision, expected, follow })
  }
  const save = async () => {
    if (!canSave || !currentDraft || !client) return
    const intent = currentDraft
    if (intent.follow && !observation.context) return
    setSaving(true); setSaved(false); setError(null)
    try {
      const outcome = await client.control({ action: 'amplifier.followBand', radioId: intent.radioId,
        expectedSettingsRevision: intent.revision, expectedFollow: intent.expected, follow: intent.follow },
      intent.follow ? observation.context ?? undefined : undefined)
      if (outcome.outcome === 'applied' && outcome.evidence === 'settingsSaved') {
        setDraft(null); setConfirmed(intent)
      } else {
        setError(outcome.outcome === 'rejected' || outcome.outcome === 'unknown' ? outcome.reason : 'unconfirmed')
      }
    } catch { setError('unconfirmed'); refreshRef.current() }
    finally { setSaving(false) }
  }
  return { supported, canEdit, canSave, saving, saved, confirming: !!confirmed, error,
    waitingForIdle: !!currentDraft?.follow && !canEnable, changed: !!currentDraft,
    follow: confirmed && confirmed.radioId === documentRadio ? confirmed.follow : currentDraft?.follow ?? expected, change, save }
}
