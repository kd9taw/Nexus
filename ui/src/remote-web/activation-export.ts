// One POTA/SOTA activation file, read from the station in bounded chunks and checked against its
// digest, the way an SSTV image is. The station builds it with the desktop's own per-activation
// export; this browser can name one listed activation and nothing else: no path, no date range, no
// search, never the whole log.
//
// The chunk stitching itself lives in `chunked-file`, shared with the Program view's CHIRP/CSV
// export — one reader, one set of checks.
import type { ActivationSelection } from './operation-protocol'
import { fetchChunkedFile, type ChunkReply } from './chunked-file'
import type { OperationClient } from './operation-client'

export { saveDownload } from './chunked-file'

export async function downloadActivation(client: OperationClient, selection: ActivationSelection,
  wait?: (ms: number) => Promise<void>): Promise<Blob> {
  return fetchChunkedFile(async (index): Promise<ChunkReply> => {
    const value = await client.activationExport(selection, index)
    // The list reply belongs to a read with no selection; one asked for by index is a crossed wire.
    if ('activations' in value) throw Error('invalidActivationFile')
    return value
  }, 'invalidActivationFile', wait)
}
