// "STORE MESSAGE…" — JS8Call's entry on a heard station (UI_Constructor.cpp:1170-1191): the operator
// leaves a message at this station for that call. It is held in the Inbox, from the operator's own
// call, and handed over when the station asks for it (QUERY MSGS, then QUERY MSG n), through every
// automatic-reply gate; storing it keys nothing. As JS8Call's box does, the text is upper-cased as
// it is typed (MessageReplyDialog.cpp:47-56). The station refuses what could never be handed over
// (no text, no callsign of yours, a message too long for one transmission at this speed) and the
// dialog shows why, keeping the text.
import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import { js8StoreMessage } from '../api'
import type { Js8State } from '../types'
import { pushToast } from '../toast'
import { upperTyped } from '../typedInput'

export function Js8StoreMessage({
  call,
  onClose,
  onStored,
}: {
  /** The heard station the message is for; `null` is closed. */
  call: string | null
  onClose: () => void
  /** The station's state once the message is held. */
  onStored: (s: Js8State) => void
}) {
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Every opening starts empty: a message begun for one station is not another's.
  useEffect(() => {
    setText('')
    setError(null)
  }, [call])
  if (!call) return null

  const store = async () => {
    if (busy || !text.trim()) return
    setBusy(true)
    setError(null)
    try {
      onStored(await js8StoreMessage(call, text))
      pushToast(t('js8.store.done', { call }), 'success')
      onClose()
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose()
      }}
      title={t('js8.store.title')}
      description={t('js8.store.label', { call })}
    >
      <input
        className="settings-input js8-store-input"
        value={text}
        maxLength={512}
        onChange={(e) => setText(upperTyped(e.target))}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            void store()
          }
        }}
        aria-label={t('js8.store.aria', { call })}
        autoComplete="off"
        spellCheck={false}
        autoFocus
        disabled={busy}
      />
      {error && (
        <p className="settings-error" role="alert">
          {error}
        </p>
      )}
      <div className="confirm-actions">
        <button type="button" className="settings-refresh" onClick={onClose} disabled={busy}>
          {t('js8.store.cancel')}
        </button>
        <button type="button" className="settings-save" onClick={() => void store()} disabled={busy || !text.trim()}>
          {t('js8.store.save')}
        </button>
      </div>
    </Dialog>
  )
}
