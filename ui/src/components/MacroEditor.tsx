// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). It holds NO operator
// string of its own: every caption arrives already resolved, in the `labels` a cockpit's own
// wrapper builds from its own catalog keys. That is deliberate and not a style — the catalog
// guard reads LITERAL `t('key')` call sites, so a shared component that composed its keys from
// a mode prefix would make every entry it uses read as an orphan.
//
// A KEYBOARD COCKPIT'S MACRO SURFACE — one F-key button, the inline editor, and the
// Everyday/Contest switch. Written for RTTY (2026-09-17) and generalised when PSK got the same
// editor (#316), because one feature with two dialects is this project's most-found defect. The
// COCKPIT still owns everything that TRANSMITS or STOPS: its own `send()` (the one path a click
// and an F-key share), the Esc key, the continuous-TX latch and the Esc/Stop macro. Nothing here
// keys the rig except by calling back into that `send()`, and nothing here is a stop control.
//
// ⚠️ NON-MODAL, AND NO RADIX LAYER, BY DESIGN. A modal would put an overlay between the operator
// and Stop TX. And a Radix dismissable layer closes on Escape WITHOUT stopping the event, so the
// cockpit's window Esc handler would also run and turn TX off on a key the operator meant as
// "cancel". Esc is decided in ONE place — the cockpit — against the Esc/Stop macro's own
// predicate: an over on the air or a latched transmitter stops, an idle one only closes.
//
// ⚠️ THE `rtty-macro-*` CLASS NAMES ARE SHARED, NOT COPIED. They are the keyboard dock's
// vocabulary, measured in a real browser for RTTY's eight-key row (styles.css
// `.rtty-dock-keys`); PSK's dock already reuses `rtty-hiscall`, `rtty-tx-latch`, `rtty-stop` and
// `rtty-arm` the same way. Renaming them would be a stylesheet-wide change whose result jsdom
// cannot see.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { isEmptyMacroSlot, isStopLikeLabel, type MacroSetId, type MacroSlot } from '../features/macroSets'

interface ButtonProps {
  slot: MacroSlot
  /** The `data-*` attribute this dock finds its own slots by (`data-rtty-macro`,
   *  `data-psk-macro`) — each cockpit queries only its own, so two mounted docks never find
   *  each other's keys. */
  slotAttr: string
  /** The station may be operated from here (a Remote observer may not). */
  control: boolean
  /** This key's editor is open. */
  editing: boolean
  /** Hover text for a filled key: the message as it will go out. */
  title: string
  /** Already-resolved captions for an empty key and for the ✎. */
  emptyLabel: string
  emptyTitle: string
  editAria: string
  onSend: () => void
  onEdit: () => void
}

/** One F-key macro. The ✎ beside it shows on hover or keyboard focus; an empty key opens the
 *  editor when clicked, because there is nothing to send. */
export function MacroButton({
  slot,
  slotAttr,
  control,
  editing,
  title,
  emptyLabel,
  emptyTitle,
  editAria,
  onSend,
  onEdit,
}: ButtonProps) {
  const empty = isEmptyMacroSlot(slot)
  return (
    <div className="rtty-macro-slot" {...{ [slotAttr]: slot.key }}>
      <button
        type="button"
        className={`cw-macro${empty ? ' rtty-macro-empty' : ''}`}
        disabled={!control}
        onClick={empty ? onEdit : onSend}
        title={empty ? emptyTitle : title}
      >
        <span className="cw-macro-key">{slot.key}</span>
        <span className="cw-macro-label">
          {empty ? emptyLabel : slot.label.trim() || slot.text}
        </span>
      </button>
      {control && !empty && (
        <button
          type="button"
          className="rtty-macro-edit"
          aria-label={editAria}
          title={editAria}
          aria-expanded={editing}
          onClick={onEdit}
        >
          ✎
        </button>
      )}
    </div>
  )
}

/** The editor's resolved captions. Every one comes from the calling cockpit's own catalog
 *  namespace, so the two modes can say different things where they mean different things —
 *  `unknownToken` above all, whose REASON differs (RTTY cannot encode a brace at all; PSK sends
 *  it perfectly and would put the token on the air as written). */
export interface MacroEditorLabels {
  aria: string
  titleLabel: string
  messageLabel: string
  tokensAria: string
  /** The token buttons, in the order the mode names them, each with its hint. */
  tokens: { token: string; hint: string }[]
  stopTitle: string
  unknownToken: (tokens: string) => string
  save: string
  cancel: string
  resetButton: string
}

interface EditorProps {
  slot: MacroSlot
  /** Horizontal offset inside the dock, so the editor sits over the key it edits. */
  left: number
  /** A save is in flight: nothing more may be committed. */
  saving: boolean
  labels: MacroEditorLabels
  /** The mode's own scan for tokens its expander cannot fill. */
  unknownTokens: (text: string) => string[]
  onSave: (label: string, text: string) => void
  onCancel: () => void
  /** Put the built-in back for this key. Offered only when the key holds the operator's own. */
  onReset: () => void
}

/** The inline editor for one key: Title, Message, the tokens, Save / Cancel / Reset this button.
 *  It refuses a caption that reads as a stop and a token the expander does not know — the one
 *  would dress a transmit key as the operator's way out, the other would put text on the air
 *  that the operator did not mean to send. */
export function MacroEditor({
  slot,
  left,
  saving,
  labels,
  unknownTokens,
  onSave,
  onCancel,
  onReset,
}: EditorProps) {
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
  const unknown = unknownTokens(text)
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
    <div className="rtty-macro-editor" role="group" aria-label={labels.aria} ref={boxRef}>
      <label className="rtty-macro-editor-field">
        <span className="rtty-macro-editor-cap">{labels.titleLabel}</span>
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
        <span className="rtty-macro-editor-cap">{labels.messageLabel}</span>
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
      <div className="rtty-macro-editor-tokens" role="group" aria-label={labels.tokensAria}>
        {labels.tokens.map(({ token, hint }) => (
          <button
            key={token}
            type="button"
            className="rtty-macro-token mono"
            title={hint}
            onClick={() => insertToken(token)}
          >
            {token}
          </button>
        ))}
      </div>
      {stopLike && (
        <p className="rtty-macro-editor-error" role="alert">
          {labels.stopTitle}
        </p>
      )}
      {unknown.length > 0 && (
        <p className="rtty-macro-editor-error" role="alert">
          {labels.unknownToken(unknown.join(' '))}
        </p>
      )}
      <div className="rtty-macro-editor-actions">
        <button type="button" className="le-log-btn" disabled={blocked} onClick={save}>
          {labels.save}
        </button>
        <button type="button" className="le-qrz" onClick={onCancel}>
          {labels.cancel}
        </button>
        <button type="button" className="le-qrz" disabled={!slot.custom || saving} onClick={onReset}>
          {labels.resetButton}
        </button>
      </div>
    </div>
  )
}

/** The set switch's resolved captions. */
export interface MacroSetSwitchLabels {
  aria: string
  /** Each set's display name, by id — also what the reset question and its tooltip name. */
  name: (id: MacroSetId) => string
  resetLabel: string
  resetTitle: (set: string) => string
  resetConfirm: (set: string) => string
  resetYes: string
  resetNo: string
}

interface SetSwitchProps {
  set: MacroSetId
  control: boolean
  /** The shown set holds any key of the operator's own — otherwise there is nothing to reset. */
  customized: boolean
  /** The inline "reset this set?" question is showing. Owned by the cockpit, because Esc must
   *  close it exactly as it closes the editor. */
  confirming: boolean
  labels: MacroSetSwitchLabels
  onConfirming: (on: boolean) => void
  onSwitch: (set: MacroSetId) => void
  onReset: () => void
}

/** Everyday / Contest, and "Reset set to defaults" beside it — asked inline, never through a
 *  dialog (a modal would stand between the operator and Stop TX, and `window.confirm` does
 *  nothing in this webview). */
export function MacroSetSwitch({
  set,
  control,
  customized,
  confirming,
  labels,
  onConfirming,
  onSwitch,
  onReset,
}: SetSwitchProps) {
  return (
    <div className="rtty-macro-set" role="group" aria-label={labels.aria}>
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
          {labels.name(id)}
        </button>
      ))}
      {confirming ? (
        <span className="rtty-macro-set-confirm" role="group" aria-label={labels.resetLabel}>
          <span role="status">{labels.resetConfirm(labels.name(set))}</span>
          <button
            type="button"
            className="le-qrz"
            onClick={() => {
              onConfirming(false)
              onReset()
            }}
          >
            {labels.resetYes}
          </button>
          <button type="button" className="le-qrz" onClick={() => onConfirming(false)}>
            {labels.resetNo}
          </button>
        </span>
      ) : (
        <button
          type="button"
          className="le-qrz"
          disabled={!control || !customized}
          title={labels.resetTitle(labels.name(set))}
          onClick={() => onConfirming(true)}
        >
          {labels.resetLabel}
        </button>
      )}
    </div>
  )
}
