// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does not: the F-key names, the macro tokens ({MYCALL} and
// the rest — the expander matches them literally) and whatever the operator saved as a caption
// or a message, which is their own words and never translated.
//
// THE RTTY DOCK'S MACRO SURFACE — RTTY's half of the shared surface in `MacroEditor.tsx`: this
// file is where RTTY's catalog keys live, and it holds nothing else. The components, the
// validation and the DOM are shared with PSK (#316), so the two docks cannot drift; the wording
// is per mode, because the modes do not always mean the same thing (see `unknownToken`).
//
// ⚠️ THE CATALOG KEYS ARE WRITTEN OUT LITERALLY, one `t('rtty.…')` per caption, and must stay
// that way: the orphan/placeholder guards read literal call sites, so a key composed from a
// prefix would report every `rtty.macroEditor.*` entry as unused.

import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import {
  MacroButton,
  MacroEditor as SharedMacroEditor,
  MacroSetSwitch,
  type MacroEditorLabels,
  type MacroSetSwitchLabels,
} from './MacroEditor'
import { isEmptyMacroSlot, type MacroSetId } from '../features/macroSets'
import { RTTY_TOKENS, unknownRttyTokens, type RttyMacroSlot, type RttySetId } from '../features/rttyMacros'

/** The dock attribute RTTY's cockpit finds its own slots by. */
export const RTTY_SLOT_ATTR = 'data-rtty-macro'

/** The editor's token list: each token and what it fills, in the order `RTTY_TOKENS` names them.
 *  A `hintKey` key table, which is the shape the catalog guard's extractor reads — a lookup
 *  under any other property name is a computed call site and reports the whole namespace as
 *  orphaned. */
const TOKEN_HINTS: { token: (typeof RTTY_TOKENS)[number]; hintKey: MessageKey }[] = [
  { token: '{MYCALL}', hintKey: 'rtty.macroEditor.token.mycall' },
  { token: '{CALL}', hintKey: 'rtty.macroEditor.token.call' },
  { token: '{RST}', hintKey: 'rtty.macroEditor.token.rst' },
  { token: '{EXCH}', hintKey: 'rtty.macroEditor.token.exch' },
]

/** A slot with neither caption nor message: clicking it opens the editor instead of sending. */
export const isEmptyRttySlot = isEmptyMacroSlot

interface ButtonProps {
  slot: RttyMacroSlot
  control: boolean
  editing: boolean
  title: string
  onSend: () => void
  onEdit: () => void
}

/** One F-key macro, captioned from RTTY's catalog. */
export function RttyMacroButton({ slot, control, editing, title, onSend, onEdit }: ButtonProps) {
  return (
    <MacroButton
      slot={slot}
      slotAttr={RTTY_SLOT_ATTR}
      control={control}
      editing={editing}
      title={title}
      emptyLabel={t('rtty.macro.empty.label')}
      emptyTitle={t('rtty.macro.empty.title', { key: slot.key })}
      editAria={t('rtty.macro.edit.aria', { key: slot.key })}
      onSend={onSend}
      onEdit={onEdit}
    />
  )
}

interface EditorProps {
  slot: RttyMacroSlot
  left: number
  saving: boolean
  onSave: (label: string, text: string) => void
  onCancel: () => void
  onReset: () => void
}

/** The inline editor for one RTTY key. `unknownToken` is RTTY's own wording and the reason this
 *  is not one shared sentence: ITA2 has no braces, so an unknown token loses them and a bare
 *  word goes on the air. */
export function RttyMacroEditor({ slot, left, saving, onSave, onCancel, onReset }: EditorProps) {
  const labels: MacroEditorLabels = {
    aria: t('rtty.macroEditor.aria', { key: slot.key }),
    titleLabel: t('rtty.macroEditor.title.label'),
    messageLabel: t('rtty.macroEditor.message.label'),
    tokensAria: t('rtty.macroEditor.tokens.aria'),
    tokens: TOKEN_HINTS.map(({ token, hintKey }) => ({ token, hint: t(hintKey) })),
    stopTitle: t('rtty.macroEditor.stopTitle'),
    unknownToken: (token) => t('rtty.macroEditor.unknownToken', { token }),
    save: t('rtty.macroEditor.save'),
    cancel: t('rtty.macroEditor.cancel'),
    resetButton: t('rtty.macroEditor.resetButton'),
  }
  return (
    <SharedMacroEditor
      slot={slot}
      left={left}
      saving={saving}
      labels={labels}
      unknownTokens={unknownRttyTokens}
      onSave={onSave}
      onCancel={onCancel}
      onReset={onReset}
    />
  )
}

interface SetSwitchProps {
  set: RttySetId
  control: boolean
  customized: boolean
  confirming: boolean
  onConfirming: (on: boolean) => void
  onSwitch: (set: RttySetId) => void
  onReset: () => void
}

/** Everyday / Contest and "Reset set", captioned from RTTY's catalog. */
export function RttyMacroSetSwitch({
  set,
  control,
  customized,
  confirming,
  onConfirming,
  onSwitch,
  onReset,
}: SetSwitchProps) {
  const labels: MacroSetSwitchLabels = {
    aria: t('rtty.macroSet.aria'),
    name: (id: MacroSetId) => (id === 'contest' ? t('rtty.macroSet.contest') : t('rtty.macroSet.everyday')),
    resetLabel: t('rtty.macroSet.reset.label'),
    resetTitle: (set) => t('rtty.macroSet.reset.title', { set }),
    resetConfirm: (set) => t('rtty.macroSet.reset.confirm', { set }),
    resetYes: t('rtty.macroSet.reset.yes'),
    resetNo: t('rtty.macroSet.reset.no'),
  }
  return (
    <MacroSetSwitch
      set={set}
      control={control}
      customized={customized}
      confirming={confirming}
      labels={labels}
      onConfirming={onConfirming}
      onSwitch={onSwitch}
      onReset={onReset}
    />
  )
}
