// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does not: the F-key names, the macro tokens ({MYCALL} and
// the rest — the expander matches them literally) and whatever the operator saved as a caption
// or a message, which is their own words and never translated.
//
// THE RTTY DOCK'S MACRO SURFACE — one F-key button, the inline editor, and the Everyday/Contest
// switch. The cockpit owns everything that TRANSMITS or STOPS: `send()` (the one path a click and
// an F-key share), the Esc key, the continuous-TX latch and the Esc/Stop macro. Nothing here keys
// the rig except by calling back into that `send()`, and nothing here is a stop control.
//
// ⚠️ NON-MODAL, AND NO RADIX LAYER, BY DESIGN. A modal would put an overlay between the operator
// and Stop TX. And a Radix dismissable layer closes on Escape WITHOUT stopping the event, so the
// cockpit's window Esc handler would also run and turn TX off on a key the operator meant as
// "cancel". Esc is decided in ONE place — the cockpit — against the Esc/Stop macro's own
// predicate: an over on the air or a latched transmitter stops, an idle one only closes.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import {
  RTTY_TOKENS,
  isStopLikeLabel,
  unknownRttyTokens,
  type RttyMacroSlot,
  type RttySetId,
} from '../features/rttyMacros'

/** The editor's token list: each token and what it fills. A key table the catalog guard reads
 *  (`hintKey`), in the order `RTTY_TOKENS` names them. */
const TOKEN_HINTS: { token: (typeof RTTY_TOKENS)[number]; hintKey: MessageKey }[] = [
  { token: '{MYCALL}', hintKey: 'rtty.macroEditor.token.mycall' },
  { token: '{CALL}', hintKey: 'rtty.macroEditor.token.call' },
  { token: '{RST}', hintKey: 'rtty.macroEditor.token.rst' },
  { token: '{EXCH}', hintKey: 'rtty.macroEditor.token.exch' },
]

/** A slot with neither caption nor message: clicking it opens the editor instead of sending. */
export function isEmptyRttySlot(slot: RttyMacroSlot): boolean {
  return !slot.label.trim() && !slot.text.trim()
}

interface ButtonProps {
  slot: RttyMacroSlot
  /** The station may be operated from here (a Remote observer may not). */
  control: boolean
  /** This key's editor is open. */
  editing: boolean
  /** Hover text for a filled key: the message as it will go out. */
  title: string
  onSend: () => void
  onEdit: () => void
}

/** One F-key macro. The ✎ beside it shows on hover or keyboard focus; an empty key opens the
 *  editor when clicked, because there is nothing to send. */
export function RttyMacroButton({ slot, control, editing, title, onSend, onEdit }: ButtonProps) {
  const empty = isEmptyRttySlot(slot)
  return (
    <div className="rtty-macro-slot" data-rtty-macro={slot.key}>
      <button
        type="button"
        className={`cw-macro${empty ? ' rtty-macro-empty' : ''}`}
        disabled={!control}
        onClick={empty ? onEdit : onSend}
        title={empty ? t('rtty.macro.empty.title', { key: slot.key }) : title}
      >
        <span className="cw-macro-key">{slot.key}</span>
        <span className="cw-macro-label">
          {empty ? t('rtty.macro.empty.label') : slot.label.trim() || slot.text}
        </span>
      </button>
      {control && !empty && (
        <button
          type="button"
          className="rtty-macro-edit"
          aria-label={t('rtty.macro.edit.aria', { key: slot.key })}
          title={t('rtty.macro.edit.aria', { key: slot.key })}
          aria-expanded={editing}
          onClick={onEdit}
        >
          ✎
        </button>
      )}
    </div>
  )
}

interface EditorProps {
  slot: RttyMacroSlot
  /** Horizontal offset inside the dock, so the editor sits over the key it edits. */
  left: number
  /** A save is in flight: nothing more may be committed. */
  saving: boolean
  onSave: (label: string, text: string) => void
  onCancel: () => void
  /** Put the built-in back for this key. Offered only when the key holds the operator's own. */
  onReset: () => void
}

/** The inline editor for one key: Title, Message, the tokens, Save / Cancel / Reset this button.
 *  It refuses a caption that reads as a stop and a token the expander does not know — the one
 *  would dress a transmit key as the operator's way out, the other would put a bare word on the
 *  air (RTTY cannot send braces). */
export function RttyMacroEditor({ slot, left, saving, onSave, onCancel, onReset }: EditorProps) {
  const [label, setLabel] = useState(slot.label)
  const [text, setText] = useState(slot.text)
  const boxRef = useRef<HTMLDivElement>(null)
  const titleRef = useRef<HTMLInputElement>(null)
  const messageRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    titleRef.current?.focus({ preventScroll: true })
    titleRef.current?.select()
  }, [])
  // Over F8 at a narrow width the key's own offset would run the editor off the dock's right
  // edge; slide it back in. Layout units throughout (`offset*`), so the app's zoom cancels out.
  useLayoutEffect(() => {
    const box = boxRef.current
    const dock = box?.offsetParent as HTMLElement | null
    if (!box || !dock) return
    box.style.left = `${Math.max(0, Math.min(left, dock.clientWidth - box.offsetWidth))}px`
  }, [left])

  const stopLike = isStopLikeLabel(label)
  const unknown = unknownRttyTokens(text)
  const blocked = stopLike || unknown.length > 0 || saving
  const save = () => {
    if (!blocked) onSave(label.trim(), text.trim())
  }
  const onEnter = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      save()
    }
  }
  /** Put a token where the caret was in Message (or at the end), and give the caret back. */
  const insertToken = (token: string) => {
    const el = messageRef.current
    const start = el?.selectionStart ?? text.length
    const end = el?.selectionEnd ?? start
    const next = text.slice(0, start) + token + text.slice(end)
    setText(next)
    requestAnimationFrame(() => {
      el?.focus({ preventScroll: true })
      el?.setSelectionRange(start + token.length, start + token.length)
    })
  }

  return (
    <div
      className="rtty-macro-editor"
      role="group"
      aria-label={t('rtty.macroEditor.aria', { key: slot.key })}
      ref={boxRef}
    >
      <label className="rtty-macro-editor-field">
        <span className="rtty-macro-editor-cap">{t('rtty.macroEditor.title.label')}</span>
        <input
          ref={titleRef}
          className="settings-input"
          value={label}
          maxLength={16}
          onChange={(e) => setLabel(e.target.value)}
          onKeyDown={onEnter}
          autoComplete="off"
          spellCheck={false}
        />
      </label>
      <label className="rtty-macro-editor-field rtty-macro-editor-message">
        <span className="rtty-macro-editor-cap">{t('rtty.macroEditor.message.label')}</span>
        <input
          ref={messageRef}
          className="settings-input mono"
          value={text}
          maxLength={500}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onEnter}
          autoComplete="off"
          spellCheck={false}
        />
      </label>
      <div className="rtty-macro-editor-tokens" role="group" aria-label={t('rtty.macroEditor.tokens.aria')}>
        {TOKEN_HINTS.map(({ token, hintKey }) => (
          <button
            key={token}
            type="button"
            className="rtty-macro-token mono"
            title={t(hintKey)}
            onClick={() => insertToken(token)}
          >
            {token}
          </button>
        ))}
      </div>
      {stopLike && (
        <p className="rtty-macro-editor-error" role="alert">
          {t('rtty.macroEditor.stopTitle')}
        </p>
      )}
      {unknown.length > 0 && (
        <p className="rtty-macro-editor-error" role="alert">
          {t('rtty.macroEditor.unknownToken', { token: unknown.join(' ') })}
        </p>
      )}
      <div className="rtty-macro-editor-actions">
        <button type="button" className="le-log-btn" disabled={blocked} onClick={save}>
          {t('rtty.macroEditor.save')}
        </button>
        <button type="button" className="le-qrz" onClick={onCancel}>
          {t('rtty.macroEditor.cancel')}
        </button>
        <button type="button" className="le-qrz" disabled={!slot.custom || saving} onClick={onReset}>
          {t('rtty.macroEditor.resetButton')}
        </button>
      </div>
    </div>
  )
}

interface SetSwitchProps {
  set: RttySetId
  control: boolean
  /** The shown set holds any key of the operator's own — otherwise there is nothing to reset. */
  customized: boolean
  /** The inline "reset this set?" question is showing. Owned by the cockpit, because Esc must
   *  close it exactly as it closes the editor. */
  confirming: boolean
  onConfirming: (on: boolean) => void
  onSwitch: (set: RttySetId) => void
  onReset: () => void
}

/** Everyday / Contest, and "Reset set to defaults" beside it — asked inline, never through a
 *  dialog (a modal would stand between the operator and Stop TX, and `window.confirm` does
 *  nothing in this webview). */
export function RttyMacroSetSwitch({
  set,
  control,
  customized,
  confirming,
  onConfirming,
  onSwitch,
  onReset,
}: SetSwitchProps) {
  const name = (id: RttySetId) => (id === 'contest' ? t('rtty.macroSet.contest') : t('rtty.macroSet.everyday'))
  return (
    <div className="rtty-macro-set" role="group" aria-label={t('rtty.macroSet.aria')}>
      {(['everyday', 'contest'] as const).map((id) => (
        <button
          key={id}
          type="button"
          className={`rtty-arm${set === id ? ' on' : ''}`}
          aria-pressed={set === id}
          disabled={!control}
          onClick={() => {
            if (set !== id) onSwitch(id)
          }}
        >
          {name(id)}
        </button>
      ))}
      {confirming ? (
        <span className="rtty-macro-set-confirm" role="group" aria-label={t('rtty.macroSet.reset.label')}>
          <span role="status">{t('rtty.macroSet.reset.confirm', { set: name(set) })}</span>
          <button
            type="button"
            className="le-qrz"
            onClick={() => {
              onConfirming(false)
              onReset()
            }}
          >
            {t('rtty.macroSet.reset.yes')}
          </button>
          <button type="button" className="le-qrz" onClick={() => onConfirming(false)}>
            {t('rtty.macroSet.reset.no')}
          </button>
        </span>
      ) : (
        <button
          type="button"
          className="le-qrz"
          disabled={!control || !customized}
          title={t('rtty.macroSet.reset.title', { set: name(set) })}
          onClick={() => onConfirming(true)}
        >
          {t('rtty.macroSet.reset.label')}
        </button>
      )}
    </div>
  )
}
