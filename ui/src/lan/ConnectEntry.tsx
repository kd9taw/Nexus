// Settings ▸ Remote access, on the PC an operator works FROM: the one Connect to my station… entry. It
// opens the Stations on this network window (src-tauri/src/remote_window.rs), which looks for the
// station on this network first and, when it is not there, signs in to reach it over the internet
// (LanApp.tsx). It replaced two entries: Remote stations… (the Remote page in a window) and Stations
// on this network…. Offered on Windows only; the caller decides, and says what to do elsewhere.
import { useState } from 'react'
import { openLanStationsWindow } from '../api'
import { getLocale, t } from '../i18n'
import '../remote-web/remote.css'

export function ConnectEntry() {
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  // Asks no service for anything, so it opens with no internet; it fails only if the window itself
  // or its loopback origin cannot start.
  const open = async () => {
    setBusy(true)
    setFailed(false)
    try { await openLanStationsWindow(getLocale()) } catch { setFailed(true) } finally { setBusy(false) }
  }
  return <>
    <div className="settings-field">
      <span className="settings-label">{t('settings.connectStation.label')}</span>
      <button type="button" className="remote-button" disabled={busy} onClick={() => void open()}>{t('settings.connectStation.open')}</button>
      <span className="settings-hint">{t('settings.connectStation.hint')}</span>
    </div>
    {failed && <p className="settings-note" role="alert">{t('settings.connectStation.failed')}</p>}
  </>
}
