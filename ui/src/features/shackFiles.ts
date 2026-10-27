// The station's own files, from a computer that drives it from afar (client mode).
//
// The SSTV gallery names its pictures by their path on the STATION's disk, and the asset
// protocol reads THIS computer's disk, so from afar every thumbnail came out blank. Here the
// picture is asked of the station instead (`shack_gallery_image`, confined there to the gallery
// folder) and shown from a blob URL. Recordings, ALL.TXT and SSTV pictures were reachable only
// by Reveal, which opens the station's own file manager and is refused from afar; here they are
// read from the station in bounded pieces and written piece by piece into this computer's
// Downloads folder, so a big ALL.TXT never sits whole in this window.
import { useEffect, useState } from 'react'
import { shackFileRead, shackFileSave, shackGalleryImage, type ShackFolder } from '../api'
import { t } from '../i18n'

function bytesOf(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
}

const pictureType = (path: string) => (/\.bmp$/i.test(path) ? 'image/bmp' : 'image/png')

/** The kinds the station shares (shack_files.rs `Folder::admits`), and the only ones a save writes
 *  here: a recording, an SSTV picture, ALL.TXT. */
const savesKind = (name: string) => name === 'ALL.TXT' || /\.(wav|png|bmp)$/i.test(name)

/** One received picture's bytes, asked of the station. */
export async function stationPictureBytes(path: string): Promise<ArrayBuffer> {
  return bytesOf(await shackGalleryImage(path)).buffer as ArrayBuffer
}

/** The station's picture at `path` as a blob URL on this computer, while `wanted`; null until it
 *  arrives, or when the station cannot give it. */
export function useStationPicture(path: string, wanted: boolean): string | null {
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!wanted) return
    let alive = true
    let made: string | null = null
    shackGalleryImage(path)
      .then((base64) => {
        if (!alive) return
        made = URL.createObjectURL(new Blob([bytesOf(base64) as BlobPart], { type: pictureType(path) }))
        setUrl(made)
      })
      .catch(() => {})
    return () => {
      alive = false
      if (made) URL.revokeObjectURL(made)
      setUrl(null)
    }
  }, [path, wanted])
  return url
}

/** Copy one file of a station folder into this computer's Downloads folder, piece by piece, and
 *  return where it landed. `onProgress` hears the share done so far, 0 to 1. The size is the one
 *  the first piece reported, so a file still growing at the station (ALL.TXT) is saved as it was
 *  when the save began. A file already in Downloads is never written over: the first piece takes
 *  the next free name, and every later piece goes to that file. */
export async function saveStationFile(
  folder: ShackFolder,
  name: string,
  onProgress?: (done: number) => void,
): Promise<string> {
  // Any other name is refused here, before a byte of it is read from the station (and the save
  // refuses it again on this computer's side).
  if (!savesKind(name)) throw new Error(t('remote.stationFiles.save.notShared', { name }))
  let offset = 0
  let size = Infinity
  let target = name
  let saved = ''
  do {
    const piece = await shackFileRead(folder, name, offset)
    if (size === Infinity) size = piece.size
    const length = atob(piece.data).length
    if (length === 0 && offset > 0) break
    const here = await shackFileSave(target, piece.data, offset)
    target = here.name
    saved = here.path
    offset += length
    onProgress?.(size > 0 ? Math.min(1, offset / size) : 1)
  } while (offset < size)
  return saved
}
