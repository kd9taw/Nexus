// Settings ▸ Station ▸ Remote access, on the PC an operator works FROM: open the Remote page in a
// Nexus window of its own (src-tauri/src/remote_window.rs) — the browser tab, without the browser.
// Offered on Windows only; the caller decides, and says what to do elsewhere.
import { useState } from 'react'
import { openRemoteStationsWindow } from '../api'
import { t } from '../i18n'
import '../remote-web/remote.css'

export function RemoteStationsWindow() {
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const open = async () => {
    setBusy(true)
    setFailed(false)
    // The window asks the Remote service for its sign-in address before it opens, so the one
    // failure is not reaching the service (or, rarely, the window itself not opening).
    try { await openRemoteStationsWindow() } catch { setFailed(true) } finally { setBusy(false) }
  }
  return <>
    <div className="settings-field">
      <span className="settings-label">{t('settings.remoteStations.label')}</span>
      <button type="button" className="remote-button" disabled={busy} onClick={() => void open()}>{t('settings.remoteStations.open')}</button>
      <span className="settings-hint">{t('settings.remoteStations.hint')}</span>
    </div>
    {failed && <p className="settings-note" role="alert">{t('settings.remoteStations.failed')}</p>}
  </>
}
