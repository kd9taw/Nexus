// Settings ▸ Licenses, in the panel header beside the build stamp: the license texts of the npm
// packages built into this app's interface. The desktop build derives them from its own bundle and
// emits them as THIRD-PARTY.txt (ui/vite.config.ts); the installers carry the same text as
// resources/ui/THIRD-PARTY.txt, beside COPYING and NOTICE. Read when first opened, never at start.
import { useEffect, useState } from 'react'
import { useFocusReturn } from '../focusReturn'
import { t } from '../i18n'
import { Dialog } from './ui/Dialog'

export function SettingsLicenses() {
  const [open, setOpen] = useState(false)
  const [text, setText] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const returnFocus = useFocusReturn(open)
  useEffect(() => {
    if (!open || text !== null || failed) return
    let live = true
    // A dev server answers a path it lacks with the page itself, so only a text file is the text.
    fetch('THIRD-PARTY.txt')
      .then(response => response.ok && response.headers.get('content-type')?.startsWith('text/plain')
        ? response.text() : Promise.reject(new Error(`THIRD-PARTY.txt: ${response.status}`)))
      .then(body => { if (live) setText(body) }, () => { if (live) setFailed(true) })
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
