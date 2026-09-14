import { useContext, useRef, useState, useSyncExternalStore } from 'react'
import { RemoteOperationsContext } from '../stationAccess'
import type { Settings } from '../types'
import type { SettingsConfiguration } from './configuration'
import { WRITABLE_CONTROL_SETTINGS_KEYS, WRITABLE_LOGGING_SETTINGS_KEYS } from './configuration-schema'
import { OperationFailure } from './operation-client'
import { OPERATION_REQUEST_BYTES, type LogCapability } from './operation-protocol'

const idleSubscribe = () => () => {}
const idleView = () => null
const LOGGING: readonly string[] = WRITABLE_LOGGING_SETTINGS_KEYS
const WRITABLE: readonly string[] = [...WRITABLE_CONTROL_SETTINGS_KEYS, ...WRITABLE_LOGGING_SETTINGS_KEYS]
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
/** What a preference save came to. `notSent`: nothing left this browser. */
export type PreferenceResult = 'saved' | 'contextChanged' | 'refused' | 'unconfirmed' | 'tooLarge' | 'notSent'
/** The largest identifiers a change envelope can carry, to measure a change before it is sent. */
const ID = '00000000-0000-4000-8000-000000000000'

/** The operating preferences this browser may change on the station's Settings page. A field is
 * editable only when it is on the allow-list AND the station offers the grant it needs (station
 * control, or the logging grant for logging preferences), which an older desktop never does; every
 * other field stays read-only. Save sends only what changed, against the revision of the Settings
 * document the form came from, and the station decides again. */
export function useRemotePreferences(doc: SettingsConfiguration | null, refresh: () => void) {
  const client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleView)
  // Drawn from the retained state too, so a heartbeat gap does not disable a field mid-edit.
  const shown = view?.state ?? view?.retainedState
  const offered = (capability: LogCapability) => !!(client && client.operationVersion >= 4 && view?.connected &&
    shown?.phase === 'controlling' && shown.controls?.capabilities.includes(capability))
  const logging = offered('settingsLogging'), control = offered('settingsControl')
  const [saving, setSaving] = useState(false)
  const [result, setResult] = useState<PreferenceResult | null>(null)
  const base = useRef<SettingsConfiguration | null>(null)
  const refreshRef = useRef(refresh)
  refreshRef.current = refresh
  const canEdit = (key: string) => !saving && !view?.unresolved && WRITABLE.includes(key) && (LOGGING.includes(key) ? logging : control)
  const changes = (form: Settings | null): Record<string, unknown> => {
    if (!doc || !form) return {}
    const fields = form as unknown as Record<string, unknown>
    return Object.fromEntries(WRITABLE.filter(key => !same(fields[key], doc.settings[key])).map(key => [key, fields[key]]))
  }
  const canSave = (form: Settings | null) => {
    const keys = Object.keys(changes(form))
    return keys.length > 0 && keys.every(canEdit)
  }
  /** Seed the form from a newer document without losing unsaved preference edits. An edit survives
   * unless the station changed that same setting meanwhile, which is reported as contextChanged. */
  const rebase = (form: Settings | null, next: Settings | null, nextDoc: SettingsConfiguration | null): Settings | null => {
    const previous = base.current
    base.current = nextDoc
    if (!form || !next || !previous || !nextDoc) return next
    const fields = form as unknown as Record<string, unknown>, merged = { ...next } as unknown as Record<string, unknown>
    let lost = false
    for (const key of WRITABLE) {
      if (same(fields[key], previous.settings[key])) continue
      if (!same(previous.settings[key], nextDoc.settings[key])) lost = true
      else merged[key] = fields[key]
    }
    if (lost) setResult('contextChanged')
    return merged as unknown as Settings
  }
  const save = async (form: Settings | null): Promise<PreferenceResult | null> => {
    const values = changes(form)
    if (!client || !doc || saving || !Object.keys(values).length) return null
    const change = { kind: 'settings' as const, revision: doc.revision, values }
    const bytes = new TextEncoder().encode(JSON.stringify({ type: 'logChange', requestId: ID, stationBootId: ID, leaseId: ID,
      expectedRevision: Number.MAX_SAFE_INTEGER, commandWindowId: ID, clientSequence: Number.MAX_SAFE_INTEGER, change })).length
    // The station refuses a request over this size, so say so rather than send one it cannot take.
    if (bytes > OPERATION_REQUEST_BYTES - 2048) {
      setResult('tooLarge')
      return 'tooLarge'
    }
    setSaving(true)
    setResult(null)
    let outcome: PreferenceResult
    try {
      await client.awaitCurrent()
      const reply = await client.change(change)
      outcome = reply.outcome === 'applied' ? 'saved' : reply.outcome === 'unknown' ? 'unconfirmed'
        : reply.reason === 'contextChanged' ? 'contextChanged' : 'refused'
    } catch (error) {
      outcome = error instanceof OperationFailure && !error.sent ? 'notSent' : 'unconfirmed'
    } finally {
      setSaving(false)
    }
    setResult(outcome)
    // A saved change, or a station that moved on, is read again; the next document seeds the form.
    if (outcome === 'saved' || outcome === 'contextChanged') {
      base.current = null
      refreshRef.current()
    }
    return outcome
  }
  /** Ask the station for the receipt of a save whose outcome never arrived. */
  const check = () => void client?.resolve().then(receipt => {
    const next: PreferenceResult = receipt.outcome === 'applied' ? 'saved' : receipt.outcome === 'rejected' ? 'refused' : 'unconfirmed'
    setResult(next)
    if (next === 'saved') refreshRef.current()
  }).catch(() => {})
  /** The operator looked at the settings at the station, which releases the pending receipt. */
  const acknowledge = () => void client?.acknowledgeAfterCheckingLog().then(() => {
    setResult(null)
    refreshRef.current()
  }).catch(() => {})
  return { supported: logging || control, canEdit, canSave, changes, rebase, save, saving, result, check, acknowledge }
}
