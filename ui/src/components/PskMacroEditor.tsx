// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does not: the F-key names, the macro tokens ({MYCALL} and
// the rest — the expander matches them literally) and whatever the operator saved as a caption
// or a message, which is their own words and never translated.
//
// THE PSK DOCK'S MACRO SURFACE (#316) — PSK's half of the shared surface in `MacroEditor.tsx`,
// exactly as `RttyMacroEditor.tsx` is RTTY's: this file is where PSK's catalog keys live and it
// holds nothing else. Two docks, one implementation, so the feature cannot grow a second dialect.
//
// ⚠️ THE CATALOG KEYS ARE WRITTEN OUT LITERALLY, one `t('psk.…')` per caption, and must stay
// that way: the orphan/placeholder guards read literal call sites, so a key composed from a
// prefix would report every `psk.macroEditor.*` entry as unused.

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
import { PSK_TOKENS, unknownPskTokens, type PskMacroSlot, type PskSetId } from '../features/pskMacros'

/** The dock attribute PSK's cockpit finds its own slots by — its own, never RTTY's: both
 *  cockpits stay MOUNTED in their keep-alive hosts, so a shared attribute would let one dock's
 *  query reach the other's keys. */
export const PSK_SLOT_ATTR = 'data-psk-macro'

/** The editor's token list: each token and what it fills, in the order `PSK_TOKENS` names them.
 *  A `hintKey` key table, which is the shape the catalog guard's extractor reads — a lookup
 *  under any other property name is a computed call site and reports the whole namespace as
 *  orphaned. */
const TOKEN_HINTS: { token: (typeof PSK_TOKENS)[number]; hintKey: MessageKey }[] = [
  { token: '{MYCALL}', hintKey: 'psk.macroEditor.token.mycall' },
  { token: '{CALL}', hintKey: 'psk.macroEditor.token.call' },
  { token: '{RST}', hintKey: 'psk.macroEditor.token.rst' },
  { token: '{EXCH}', hintKey: 'psk.macroEditor.token.exch' },
]

/** A slot with neither caption nor message: clicking it opens the editor instead of sending. */
export const isEmptyPskSlot = isEmptyMacroSlot

interface ButtonProps {
  slot: PskMacroSlot
  control: boolean
  editing: boolean
  title: string
  onSend: () => void
  onEdit: () => void
}

/** One F-key macro, captioned from PSK's catalog. */
export function PskMacroButton({ slot, control, editing, title, onSend, onEdit }: ButtonProps) {
  return (
    <MacroButton
      slot={slot}
      slotAttr={PSK_SLOT_ATTR}
      control={control}
      editing={editing}
      title={title}
      emptyLabel={t('psk.macro.empty.label')}
      emptyTitle={t('psk.macro.empty.title', { key: slot.key })}
      editAria={t('psk.macro.edit.aria', { key: slot.key })}
      onSend={onSend}
      onEdit={onEdit}
    />
  )
}

interface EditorProps {
  slot: PskMacroSlot
  left: number
  saving: boolean
  onSave: (label: string, text: string) => void
  onCancel: () => void
  onReset: () => void
}

/** The inline editor for one PSK key. `unknownToken` is PSK's own wording and the reason this is
 *  not one shared sentence with RTTY's: PSK31's varicode carries a brace perfectly, so an
 *  unknown token is not mangled — it goes out reading exactly as it was typed, which is never
 *  what the operator meant by writing one. */
export function PskMacroEditor({ slot, left, saving, onSave, onCancel, onReset }: EditorProps) {
  const labels: MacroEditorLabels = {
    aria: t('psk.macroEditor.aria', { key: slot.key }),
    titleLabel: t('psk.macroEditor.title.label'),
    messageLabel: t('psk.macroEditor.message.label'),
    tokensAria: t('psk.macroEditor.tokens.aria'),
    tokens: TOKEN_HINTS.map(({ token, hintKey }) => ({ token, hint: t(hintKey) })),
    stopTitle: t('psk.macroEditor.stopTitle'),
    unknownToken: (token) => t('psk.macroEditor.unknownToken', { token }),
    save: t('psk.macroEditor.save'),
    cancel: t('psk.macroEditor.cancel'),
    resetButton: t('psk.macroEditor.resetButton'),
  }
  return (
    <SharedMacroEditor
      slot={slot}
      left={left}
      saving={saving}
      labels={labels}
      unknownTokens={unknownPskTokens}
      onSave={onSave}
      onCancel={onCancel}
      onReset={onReset}
    />
  )
}

interface SetSwitchProps {
  set: PskSetId
  control: boolean
  customized: boolean
  confirming: boolean
  onConfirming: (on: boolean) => void
  onSwitch: (set: PskSetId) => void
  onReset: () => void
}

/** Everyday / Contest and "Reset set", captioned from PSK's catalog. */
export function PskMacroSetSwitch({
  set,
  control,
  customized,
  confirming,
  onConfirming,
  onSwitch,
  onReset,
}: SetSwitchProps) {
  const labels: MacroSetSwitchLabels = {
    aria: t('psk.macroSet.aria'),
    name: (id: MacroSetId) => (id === 'contest' ? t('psk.macroSet.contest') : t('psk.macroSet.everyday')),
    resetLabel: t('psk.macroSet.reset.label'),
    resetTitle: (set) => t('psk.macroSet.reset.title', { set }),
    resetConfirm: (set) => t('psk.macroSet.reset.confirm', { set }),
    resetYes: t('psk.macroSet.reset.yes'),
    resetNo: t('psk.macroSet.reset.no'),
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
