// The station's working channel list as a CHIRP or spreadsheet CSV, read in bounded chunks and
// saved by this browser. The rows are already on this page (the `programming` collection) — what
// only the station has is the WRITER, so the file is built there by the same code the desktop's own
// Export CHIRP button calls, and a second CHIRP writer never exists to drift from it.
//
// ⛔ It carries no attribution line. The desktop stamps the directory the rows were fetched from
// because the search result is still on its screen; `radioprog.json` does not record where a
// channel came from, so naming either directory here would be a claim nothing checked.
import { fetchChunkedFile, type ChunkReply } from './chunked-file'
import type { ProgramExportFormat } from './operation-protocol'
import type { OperationClient } from './operation-client'

export async function downloadProgramExport(client: OperationClient, format: ProgramExportFormat,
  nameCap: number, wait?: (ms: number) => Promise<void>): Promise<Blob> {
  return fetchChunkedFile((index): Promise<ChunkReply> => client.programExport(format, nameCap, index),
    'invalidProgramFile', wait)
}

/** The two export file names, used by the desktop path and the browser path alike so a file
 * exported from a browser is named the file the operator would have got at the shack. The stamp is
 * the UTC day, which is what `toISOString` gives and what the desktop has always written. */
export function programExportName(format: ProgramExportFormat, now = new Date()): string {
  const stamp = now.toISOString().slice(0, 10)
  return format === 'chirp' ? `nexus-chirp-${stamp}.csv` : `nexus-channels-${stamp}.csv`
}
