// One POTA/SOTA activation file, read from the station in bounded chunks and checked against its
// digest, the way an SSTV image is. The station builds it with the desktop's own per-activation
// export; this browser can name one listed activation and nothing else: no path, no date range, no
// search, never the whole log.
import { ACTIVATION_EXPORT_CHUNK_BYTES, type ActivationSelection } from './operation-protocol'
import type { OperationClient } from './operation-client'

/** Between chunks the page yields, so its once-a-second heartbeat keeps the logging lease. */
const GAP_MS = 250
const BUSY_MS = 300
const BUSY_RETRIES = 20
const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

async function read(client: OperationClient, selection: ActivationSelection, index: number, wait: (ms: number) => Promise<void>) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await client.activationExport(selection, index)
    } catch (error) {
      // Busy means nothing was read: the station's Engine was held, or this browser's own request
      // budget (heartbeats included) was full. A read changes nothing, so asking again is safe.
      const code = error instanceof Error ? error.message : ''
      if (attempt >= BUSY_RETRIES || (code !== 'remoteBusy' && code !== 'stationBusy')) throw error
      await wait(BUSY_MS)
    }
  }
}

export async function downloadActivation(client: OperationClient, selection: ActivationSelection,
  wait: (ms: number) => Promise<void> = sleep): Promise<Blob> {
  const chunks: Uint8Array<ArrayBuffer>[] = []
  let file: { byteLength: number; sha256: string; chunks: number } | null = null, length = 0
  for (let index = 0; !file || index < file.chunks; index++) {
    if (index) await wait(GAP_MS)
    const value = await read(client, selection, index, wait)
    if ('refused' in value) throw Error(value.refused)
    // Every chunk must describe the same file: a log that changed mid-download is a different file.
    if (!('file' in value) || value.index !== index || (file && JSON.stringify(file) !== JSON.stringify(value.file)))
      throw Error('invalidActivationFile')
    file = value.file
    const data = Uint8Array.from(atob(value.base64), c => c.charCodeAt(0))
    if (!data.length || data.length > ACTIVATION_EXPORT_CHUNK_BYTES ||
      (index + 1 < file.chunks && data.length !== ACTIVATION_EXPORT_CHUNK_BYTES)) throw Error('invalidActivationFile')
    length += data.length
    if (length > file.byteLength) throw Error('invalidActivationFile')
    chunks.push(data)
  }
  if (!file || length !== file.byteLength) throw Error('invalidActivationFile')
  const bytes = new Uint8Array(length)
  let at = 0
  for (const chunk of chunks) {
    bytes.set(chunk, at)
    at += chunk.length
  }
  const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
    .map(b => b.toString(16).padStart(2, '0')).join('')
  if (digest !== file.sha256) throw Error('invalidActivationFile')
  return new Blob([bytes], { type: 'application/octet-stream' })
}

/** Hand the checked file to the browser's own download, under the name the desktop gives it. */
export function saveDownload(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob), link = document.createElement('a')
  link.href = url
  link.download = name
  link.rel = 'noopener'
  document.body.append(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}
