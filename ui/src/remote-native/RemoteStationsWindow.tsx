// Settings ▸ Station ▸ Remote access, on the PC an operator works FROM: open the Remote page in a
// Nexus window of its own (src-tauri/src/remote_window.rs) — the browser tab, without the browser.
// Beside it, the same kind of window for stations paired over the shack's own network, with no
// internet (the operator, 2026-10-04: "Window only in v1"). Offered on Windows only; the caller
// decides, and says what to do elsewhere.
import { useState } from 'react'
import { openLanStationsWindow, openRemoteStationsWindow } from '../api'
import { getLocale, t } from '../i18n'
import '../remote-web/remote.css'

export function RemoteStationsWindow() {
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const [lanFailed, setLanFailed] = useState(false)
  const open = async () => {
    setBusy(true)
    setFailed(false)
    // The window asks the Remote service for its sign-in address before it opens, so the one
    // failure is not reaching the service (or, rarely, the window itself not opening).
    try { await openRemoteStationsWindow() } catch { setFailed(true) } finally { setBusy(false) }
  }
  // Asks no service for anything, so it opens with no internet; it fails only if the window itself
  // or its loopback origin cannot start.
  const openLan = async () => {
    setBusy(true)
    setLanFailed(false)
    try { await openLanStationsWindow(getLocale()) } catch { setLanFailed(true) } finally { setBusy(false) }
  }
  return <>
    <div className="settings-field">
      <span className="settings-label">{t('settings.remoteStations.label')}</span>
      <button type="button" className="remote-button" disabled={busy} onClick={() => void open()}>{t('settings.remoteStations.open')}</button>
      <span className="settings-hint">{t('settings.remoteStations.hint')}</span>
    </div>
    {failed && <p className="settings-note" role="alert">{t('settings.remoteStations.failed')}</p>}
    <div className="settings-field">
      <span className="settings-label">{t('settings.lanStations.label')}</span>
      <button type="button" className="remote-button" disabled={busy} onClick={() => void openLan()}>{t('settings.lanStations.open')}</button>
      <span className="settings-hint">{t('settings.lanStations.hint')}</span>
    </div>
    {lanFailed && <p className="settings-note" role="alert">{t('settings.lanStations.failed')}</p>}
  </>
}
