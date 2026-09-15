// A file read from the station in bounded base64 chunks, checked against its digest, and handed to
// the browser's own download. Two operations travel this way — a POTA/SOTA activation's ADIF and the
// Program view's CHIRP/CSV — and they share this reader so neither can grow a looser check than the
// other. An operation reply is small; a file is not, so nothing arrives whole.
//
// ⛔ The station keeps NOTHING between chunks: it rebuilds the text for each one. So a file that
// changed half way down arrives as a different description, and the stitch below refuses it rather
// than splicing two files together and calling the result an export.
import { EXPORT_CHUNK_BYTES, type ExportFile } from './operation-protocol'

/** Between chunks the page yields, so its once-a-second heartbeat keeps the lease alive. */
const GAP_MS = 250
const BUSY_MS = 300
const BUSY_RETRIES = 20
const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

/** One chunk as either end describes it: the whole file, which index this is, and its bytes. */
export type ChunkReply = { file: ExportFile; index: number; base64: string } | { refused: string }
export type ReadChunk = (index: number) => Promise<ChunkReply>

async function read(request: ReadChunk, index: number, wait: (ms: number) => Promise<void>) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await request(index)
    } catch (error) {
      // Busy means nothing was read: the station's Engine was held, or this browser's own request
      // budget (heartbeats included) was full. A read changes nothing, so asking again is safe.
      const code = error instanceof Error ? error.message : ''
      if (attempt >= BUSY_RETRIES || (code !== 'remoteBusy' && code !== 'stationBusy')) throw error
      await wait(BUSY_MS)
    }
  }
}

/**
 * Every chunk of one file, stitched and verified. `invalid` names the error a caller throws when the
 * chunks do not describe one file — the station's own refusal words are thrown verbatim instead, so
 * a caller can tell "too large" from "did not arrive intact".
 */
export async function fetchChunkedFile(request: ReadChunk, invalid: string,
  wait: (ms: number) => Promise<void> = sleep): Promise<Blob> {
  const chunks: Uint8Array<ArrayBuffer>[] = []
  let file: ExportFile | null = null, length = 0
  for (let index = 0; !file || index < file.chunks; index++) {
    if (index) await wait(GAP_MS)
    const value = await read(request, index, wait)
    if ('refused' in value) throw Error(value.refused)
    // Every chunk must describe the same file: a list that changed mid-download is a different file.
    if (!('file' in value) || value.index !== index || (file && JSON.stringify(file) !== JSON.stringify(value.file)))
      throw Error(invalid)
    file = value.file
    const data = Uint8Array.from(atob(value.base64), c => c.charCodeAt(0))
    if (!data.length || data.length > EXPORT_CHUNK_BYTES ||
      (index + 1 < file.chunks && data.length !== EXPORT_CHUNK_BYTES)) throw Error(invalid)
    length += data.length
    if (length > file.byteLength) throw Error(invalid)
    chunks.push(data)
  }
  if (!file || length !== file.byteLength) throw Error(invalid)
  const bytes = new Uint8Array(length)
  let at = 0
  for (const chunk of chunks) {
    bytes.set(chunk, at)
    at += chunk.length
  }
  const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
    .map(b => b.toString(16).padStart(2, '0')).join('')
  if (digest !== file.sha256) throw Error(invalid)
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
