// Settings ▸ Licenses, in the panel header beside the build stamp: the license texts of the npm
// packages built into this app's interface, then those of the Rust crates the app is built from.
// The desktop build derives the first from its own bundle and emits it as THIRD-PARTY.txt, and
// emits the second, scripts/gen-rust-licenses.py's licenses/rust/THIRD-PARTY.txt, beside it as
// rust/THIRD-PARTY.txt (ui/vite.config.ts). The installers carry the same texts as
// resources/ui/THIRD-PARTY.txt and resources/rust/THIRD-PARTY.txt, beside COPYING and NOTICE.
// Read when first opened, never at start, and shown only whole: half of them would read as all.
import { useEffect, useState } from 'react'
import { useFocusReturn } from '../focusReturn'
import { t } from '../i18n'
import { Dialog } from './ui/Dialog'

const FILES = ['THIRD-PARTY.txt', 'rust/THIRD-PARTY.txt']

export function SettingsLicenses() {
  const [open, setOpen] = useState(false)
  const [text, setText] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const returnFocus = useFocusReturn(open)
  useEffect(() => {
    if (!open || text !== null || failed) return
    let live = true
    // A dev server answers a path it lacks with the page itself, so only a text file is the text.
    Promise.all(FILES.map(file => fetch(file)
      .then(response => response.ok && response.headers.get('content-type')?.startsWith('text/plain')
        ? response.text() : Promise.reject(new Error(`${file}: ${response.status}`)))))
      .then(bodies => { if (live) setText(bodies.join('\n\n')) }, () => { if (live) setFailed(true) })
    return () => { live = false }
  }, [open, text, failed])
  return <>
    <button type="button" className="settings-update-btn" onClick={() => setOpen(true)}>
      {t('settings.licenses.button')}
    </button>
    <Dialog open={open} onOpenChange={setOpen} title={t('settings.licenses.title')} className="licenses-dialog" onCloseAutoFocus={returnFocus}>
      {text !== null
        ? <pre className="licenses-text">{text}</pre>
        : <p className="settings-note" role="status">{failed ? t('settings.licenses.unavailable') : t('settings.licenses.loading')}</p>}
    </Dialog>
  </>
}
