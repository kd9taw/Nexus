// THE RECEIVED-PICTURE VIEWER — a picture you just received, big, in its own window.
//
// Clicking a gallery thumbnail used to do nothing at all. The gallery offered delete,
// edit-and-resend and Reveal in folder, so the one thing an operator wants to do with a
// picture that has just come in — look at it — meant leaving Nexus for a file manager.
//
// ITS OWN WINDOW, NOT A MODAL, and that is the operator's decision rather than a detail:
// a modal would have to be dismissed before the next picture arrives, while this can sit
// on a second monitor beside the cockpit all through a session. It is the torn-off-panel
// pattern (`DetachedPanel`, `open_panel_window`), so it is an OS window the operator can
// move, resize and park like any other.
//
// ⚠️ IT CLOSES BY CLOSING THE WINDOW, and that is #263's lesson applied here before it
// could become #263's bug again. That issue was the torn-off waterfall's re-dock setting
// the panel state back to 'docked' and never telling the pop-out to close, leaving the
// operator with two waterfalls. The shape of the mistake is a "close" that changes state
// the window does not read. So every close path here — the ✕, Esc, and the picture being
// deleted from under it — goes through `closePanelWindow`, the command that fix added,
// and nothing pretends a flag is a close.
//
// ONE WINDOW, BY CONSTRUCTION: `open_panel_window` focuses an existing window rather than
// making a second, so clicking a different thumbnail re-points the viewer that is already
// open instead of stacking them up. The picture it shows is carried in localStorage, which
// is how a separate JS realm hears about it — the main window writes the path and this
// window's `storage` listener follows.
//
// DESKTOP ONLY in this batch. A Remote browser fetches gallery images lazily and would
// need the full-size copy on demand with something on screen while it loads; that is its
// own piece of work, so the thumbnail is not clickable there.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { t } from '../i18n'
import { closePanelWindow, getSstvState, revealSstvGallery, savePngToDownloads } from '../api'
import { pushToast, withErrorToast } from '../toast'
import type { SstvGalleryEntry, SstvState } from '../types'
import { assetUrl, fmtUtc, GalleryThumb, sstvDownloadName } from './SstvView'

/** The panel slug — the window label, the `?panel=` route, and the key
 *  `open_panel_window` / `close_panel_window` size and address it by. */
export const SSTV_VIEWER_PANEL = 'sstvviewer'

/** Which picture the viewer is showing. Written by the main window when a thumbnail is
 *  clicked, read here — a torn-off window is a separate JS realm, so localStorage plus the
 *  `storage` event is the only channel between them that needs no round trip through Rust. */
export const SSTV_VIEWER_PATH_KEY = 'nexus.sstv.viewer.path'

/** Point the viewer at `path` (and, when it is not open yet, that is what it will show on
 *  its first paint). Safe where storage is unavailable — a private window, a locked-down
 *  profile — because a viewer with no path says so rather than throwing. */
export function setViewerPicture(path: string): void {
  try {
    localStorage.setItem(SSTV_VIEWER_PATH_KEY, path)
  } catch {
    /* storage blocked — the viewer opens on the newest picture instead */
  }
}

function readViewerPicture(): string | null {
  try {
    return localStorage.getItem(SSTV_VIEWER_PATH_KEY)
  } catch {
    return null
  }
}

/** How often the viewer re-reads the gallery. Slower than the cockpit's 1 Hz on purpose:
 *  nothing here is live, and this window may be parked on a second monitor for an hour. */
const GALLERY_POLL_MS = 4000

export function SstvViewer() {
  const [gallery, setGallery] = useState<SstvGalleryEntry[] | null>(null)
  const [path, setPath] = useState<string | null>(readViewerPicture)
  const [saving, setSaving] = useState(false)
  // Newest first, the order the cockpit's gallery shows — so ← and → step the way the
  // pictures are laid out over there rather than in the order they were written.
  const entries = useMemo(() => (gallery ? [...gallery].reverse() : []), [gallery])

  useEffect(() => {
    let live = true
    const load = () =>
      getSstvState()
        .then((s: SstvState) => live && setGallery(s.gallery))
        .catch(() => {})
    load()
    const id = setInterval(load, GALLERY_POLL_MS)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])

  // The main window re-points this one by writing the key; `storage` fires in every OTHER
  // document of the origin, which is exactly the relationship here.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === SSTV_VIEWER_PATH_KEY) setPath(e.newValue)
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])

  const index = entries.findIndex((g) => g.path === path)
  // No path yet (opened from the ⊞ rail rather than a thumbnail), or a path that is not in
  // the gallery any more: fall back to the newest picture, which is the one an operator
  // opening this window almost always means.
  const entry = index >= 0 ? entries[index] : entries[0]

  const close = useCallback(() => {
    void closePanelWindow(SSTV_VIEWER_PANEL).catch(() => {})
  }, [])

  const step = useCallback(
    (delta: number) => {
      if (entries.length === 0) return
      const from = index >= 0 ? index : 0
      const next = entries[(from + delta + entries.length) % entries.length]
      if (!next) return
      setPath(next.path)
      // Write it back so the main window and this one agree about which picture is open —
      // and so re-opening the viewer lands on the one that was last looked at.
      setViewerPicture(next.path)
    },
    [entries, index],
  )

  // Esc closes, ← / → step. Bound on the window because this document IS the viewer — there
  // is nothing else in it to take the keys, and an operator who has just moved the mouse to
  // this monitor should not have to click the picture first.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        close()
      } else if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        e.preventDefault()
        step(1)
      } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        e.preventDefault()
        step(-1)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [close, step])

  // Saving a copy: the picture is already a file on the station's disk, so this is "put a
  // copy where I keep things", not an export. It goes through the same
  // `savePngToDownloads` the share card uses — a browser `<a download>` is silently dead in
  // the Tauri webview on macOS, and the gallery's own files are .bmp as often as .png, so
  // the bytes are copied verbatim under a caption-derived name rather than re-encoded.
  const saveRef = useRef<string | null>(null)
  const onSave = () => {
    if (!entry || saving) return
    const src = assetUrl(entry.path)
    if (!src) return
    saveRef.current = entry.path
    setSaving(true)
    void withErrorToast(async () => {
      const buf = await (await fetch(src)).arrayBuffer()
      const bytes = new Uint8Array(buf)
      let bin = ''
      for (const b of bytes) bin += String.fromCharCode(b)
      const saved = await savePngToDownloads(sstvDownloadName(entry), btoa(bin))
      pushToast(t('sstv.viewer.save.done', { path: saved }), 'info')
      return saved
    }, t('sstv.viewer.save.failed')).finally(() => setSaving(false))
  }

  if (!entry) {
    return (
      <div className="sstv-viewer empty">
        <p className="dim">{gallery === null ? t('detached.connecting') : t('sstv.viewer.empty')}</p>
      </div>
    )
  }

  const position = index >= 0 ? index + 1 : 1

  return (
    <div className="sstv-viewer">
      {/* The picture, and it gets the room: everything else in this window is one strip. */}
      <div className="sstv-viewer-stage">
        <GalleryThumb entry={entry} />
      </div>
      <div className="sstv-viewer-bar">
        <div className="sstv-viewer-facts">
          {/* Mode, when, and where — the three things written on the gallery caption, plus
              the FSK callsign when the sender's software sent one and ours read it. */}
          <span className="sstv-viewer-mode">{entry.mode}</span>
          {entry.fskId ? <span className="sstv-viewer-call">{entry.fskId}</span> : null}
          <span className="sstv-viewer-meta">
            {t('sstv.viewer.meta', {
              when: fmtUtc(entry.finishedUtc),
              mhz: entry.freqMhz.toFixed(3),
              lines: entry.lines,
            })}
          </span>
        </div>
        <div className="sstv-viewer-actions">
          <span className="sstv-viewer-count" aria-live="polite">
            {t('sstv.viewer.position', { n: position, total: entries.length })}
          </span>
          <button
            type="button"
            className="cw-macro"
            disabled={entries.length < 2}
            onClick={() => step(-1)}
            title={t('sstv.viewer.prev.title')}
          >
            {t('sstv.viewer.prev.label')}
          </button>
          <button
            type="button"
            className="cw-macro"
            disabled={entries.length < 2}
            onClick={() => step(1)}
            title={t('sstv.viewer.next.title')}
          >
            {t('sstv.viewer.next.label')}
          </button>
          <button type="button" className="cw-macro" disabled={saving} onClick={onSave}>
            {t('sstv.viewer.save.label')}
          </button>
          <button
            type="button"
            className="cw-macro"
            onClick={() =>
              void withErrorToast(() => revealSstvGallery(), t('sstv.gallery.reveal.failed'))
            }
          >
            {t('sstv.gallery.reveal.label')}
          </button>
          {/* ⚠️ A REAL CLOSE, not a flag — see the #263 note at the top of this file. */}
          <button
            type="button"
            className="cw-macro sstv-viewer-close"
            onClick={close}
            title={t('sstv.viewer.close.title')}
          >
            {t('sstv.viewer.close.label')}
          </button>
        </div>
      </div>
    </div>
  )
}
