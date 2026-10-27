// JS8Call's "Add New Station or Group…" (UI_Constructor.cpp:1059-1097): one callsign or @GROUP. A
// callsign joins the call list with nothing heard yet; a @GROUP is joined (Settings ▸ JS8's groups,
// so the station answers it from then on, as joining it there does); @ALLCALL takes part in @ALLCALL
// again. A name that is neither is refused with JS8Call's words.
//
// Nothing here keys.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts).
import { useEffect, useState } from 'react'
import { t } from '../i18n'
import { Dialog } from './ui/Dialog'
import { ALLCALL } from '../js8Vocab'
import { upperTyped } from '../typedInput'

/** What a name typed into the dialog is (JS8Call's checks, upper-cased and trimmed). A callsign is
 *  JS8Call's shape: letters and digits with at least one of each, any '/' parts, 3 to 12 long. */
export function js8AddKind(raw: string): { kind: 'allcall' | 'group' | 'call'; name: string } | null {
  const name = raw.trim().toUpperCase()
  if (name === ALLCALL) return { kind: 'allcall', name }
  if (name.startsWith('@')) return /^@[A-Z0-9/]{1,9}$/u.test(name) ? { kind: 'group', name } : null
  const call = /^(?=.*[0-9])(?=.*[A-Z])[A-Z0-9]+(\/[A-Z0-9]+)*$/u.test(name) && name.length >= 3 && name.length <= 12
  return call ? { kind: 'call', name } : null
}

export function Js8AddStation({
  open,
  onOpenChange,
  onAdd,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The checked name: the cockpit adds the call, joins the group or takes part in @ALLCALL. */
  onAdd: (what: { kind: 'allcall' | 'group' | 'call'; name: string }) => void
}) {
  const [text, setText] = useState('')
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!open) return
    setText('')
    setError(null)
  }, [open])
  const submit = () => {
    if (!text.trim()) return
    const what = js8AddKind(text)
    if (!what) {
      setError(
        text.trim().startsWith('@')
          ? t('js8.calls.add.notGroup', { name: text.trim().toUpperCase() })
          : t('js8.calls.add.notCall', { name: text.trim().toUpperCase() }),
      )
      return
    }
    onAdd(what)
    onOpenChange(false)
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('js8.calls.add.title')} className="js8-add-dialog">
      <form
        className="js8-add-form"
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <label className="settings-field">
          <span className="settings-label">{t('js8.calls.add.label')}</span>
          <input
            className="settings-input"
            value={text}
            onChange={(e) => {
              setText(upperTyped(e.target))
              setError(null)
            }}
            autoComplete="off"
            spellCheck={false}
            autoFocus
          />
        </label>
        {error && (
          <div className="settings-error" role="alert">
            {error}
          </div>
        )}
        <div className="confirm-actions">
          <button type="button" className="settings-refresh" onClick={() => onOpenChange(false)}>
            {t('js8.calls.add.cancel')}
          </button>
          <button type="submit" className="settings-save" disabled={!text.trim()}>
            {t('js8.calls.add.ok')}
          </button>
        </div>
      </form>
    </Dialog>
  )
}
