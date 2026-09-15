import { useEffect, useState, useSyncExternalStore } from 'react'
import type { LoggedActivation } from '../types'
import { downloadActivation, saveDownload } from './activation-export'
import { OperationFailure, type OperationClient } from './operation-client'

const idleSubscribe = () => () => {}
const idleView = () => null
const busy = (error: unknown) => error instanceof OperationFailure && (error.message === 'remoteBusy' || error.message === 'stationBusy')

/** The station's activations for the Logbook's one-activation download. Offered only while this
 * browser holds logging control and the station offers the export, which an older desktop does not.
 * Drawn from the retained station state too, so a heartbeat gap does not flicker the picker away. */
export function useRemoteActivations(client: OperationClient | null, logSize: number) {
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleView)
  const shown = view?.state ?? view?.retainedState
  const available = !!(client && client.operationVersion >= 4 && view?.connected && shown?.phase === 'controlling' &&
    shown.controls?.capabilities.includes('activationExport'))
  const [activations, setActivations] = useState<LoggedActivation[]>([])
  const [downloading, setDownloading] = useState(false)
  useEffect(() => {
    if (!client || !available) {
      setActivations(previous => previous.length ? [] : previous)
      return
    }
    let current = true
    void (async () => {
      for (let attempt = 0; attempt < 10 && current; attempt++) {
        try {
          const value = await client.activationExport(null)
          if (current && 'activations' in value) setActivations(value.activations)
          return
        } catch (error) {
          if (!busy(error)) return
          await new Promise(resolve => setTimeout(resolve, 400))
        }
      }
    })()
    return () => { current = false }
  }, [client, available, logSize])
  /** Saves the checked file under `name`. Resolves with that name, with the reason nothing was saved
   * (`tooLarge`, `notFound`, or anything else for a file that did not arrive intact), or with null
   * when a download is already running. */
  const download = async (activation: LoggedActivation, name: string): Promise<{ saved: string } | { failed: string } | null> => {
    if (!client || downloading) return null
    setDownloading(true)
    try {
      const blob = await downloadActivation(client,
        { reference: activation.reference, dayStartUnix: activation.dayStartUnix, callsign: activation.callsign ?? null })
      saveDownload(name, blob)
      return { saved: name }
    } catch (error) {
      return { failed: error instanceof Error ? error.message : '' }
    } finally {
      setDownloading(false)
    }
  }
  return { available, activations, downloading, download }
}
