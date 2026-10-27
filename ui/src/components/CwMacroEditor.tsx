// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does not: the F-key names, the macro tokens ({MYCALL}, `!`
// and the rest — the expander matches them literally) and whatever the operator saved as a caption
// or a message, which is their own words and never translated.
//
// THE CW DOCK'S MACRO SURFACE — CW's half of the shared surface in `MacroEditor.tsx`, exactly as
// `RttyMacroEditor.tsx` is RTTY's: this file is where CW's catalog keys live, and it holds nothing
// else. What a CW key sends, and where its Save writes, are the cockpit's (`CwCockpit.tsx`) and
// `features/cwMacros.ts`'s.
//
// ⚠️ THE CATALOG KEYS ARE WRITTEN OUT LITERALLY, one `t('cw.…')` per caption, and must stay that
// way: the orphan/placeholder guards read literal call sites, so a key composed from a prefix
// would report every `cw.macroEditor.*` entry as unused.

import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import {
  MacroButton,
  MacroEditor as SharedMacroEditor,
  type MacroEditorFocus,
  type MacroEditorLabels,
} from './MacroEditor'
import { unknownCwTokens } from '../features/cwMacros'
import type { MacroSlot } from '../features/macroSets'

/** The dock attribute the CW cockpit finds its own slots by. */
export const CW_SLOT_ATTR = 'data-cw-macro'

/** The editor's token buttons, each with what it fills: the ones a CW QSO or contest uses most.
 *  The expander knows the rest too (`CW_TOKENS`), so a message using them saves and sends; they
 *  are typed rather than clicked. A `hintKey` key table, the shape the catalog guard reads. */
const TOKEN_HINTS: { token: string; hintKey: MessageKey }[] = [
  { token: '{MYCALL}', hintKey: 'cw.macroEditor.token.mycall' },
  { token: '!', hintKey: 'cw.macroEditor.token.call' },
  { token: '{RST}', hintKey: 'cw.macroEditor.token.rst' },
  { token: '{EXCH}', hintKey: 'cw.macroEditor.token.exch' },
  { token: '{NAME}', hintKey: 'cw.macroEditor.token.name' },
  { token: '{HISNAME}', hintKey: 'cw.macroEditor.token.hisname' },
]

interface ButtonProps {
  slot: MacroSlot
  editing: boolean
  /** The next Enter in the contest strip sends this key (Enter Sends Message). */
  lit?: boolean
  /** The turn cue on this key (`MacroButton`'s `cue`): it lights, and never sends. */
  cue?: { glow: 'now' | 'soon' | 'maybe'; describedBy: string }
  title: string
  onSend: () => void
  onEdit: (focus: MacroEditorFocus) => void
}

/** One F-key macro, captioned from CW's catalog. */
export function CwMacroButton({ slot, editing, lit, cue, title, onSend, onEdit }: ButtonProps) {
  return (
    <MacroButton
      slot={slot}
      lit={lit}
      cue={cue}
      slotAttr={CW_SLOT_ATTR}
      editing={editing}
      title={title}
      emptyLabel={t('cw.macro.empty.label')}
      emptyTitle={t('cw.macro.empty.title', { key: slot.key })}
      editAria={t('cw.macro.edit.aria', { key: slot.key })}
      editTitle={t('cw.macro.edit.title', { key: slot.key })}
      onSend={onSend}
      onEdit={onEdit}
    />
  )
}

interface EditorProps {
  slot: MacroSlot
  left: number
  saving: boolean
  focus: MacroEditorFocus
  onSave: (label: string, text: string) => void
  onSendOnce: (text: string) => void
  onCancel: () => void
  onReset: () => void
}

/** The inline editor for one CW key. `unknownToken` is CW's own wording: the keyer skips a brace
 *  it has no Morse for, so an unknown token goes out as a bare word, as on RTTY. */
export function CwMacroEditor({ slot, left, saving, focus, onSave, onSendOnce, onCancel, onReset }: EditorProps) {
  const labels: MacroEditorLabels = {
    aria: t('cw.macroEditor.aria', { key: slot.key }),
    titleLabel: t('cw.macroEditor.title.label'),
    messageLabel: t('cw.macroEditor.message.label'),
    tokensAria: t('cw.macroEditor.tokens.aria'),
    tokens: TOKEN_HINTS.map(({ token, hintKey }) => ({ token, hint: t(hintKey) })),
    stopTitle: t('cw.macroEditor.stopTitle'),
    unknownToken: (token) => t('cw.macroEditor.unknownToken', { token }),
    save: t('cw.macroEditor.save'),
    sendOnce: t('cw.macroEditor.sendOnce'),
    sendOnceTitle: t('cw.macroEditor.sendOnce.title'),
    cancel: t('cw.macroEditor.cancel'),
    resetButton: t('cw.macroEditor.resetButton'),
  }
  return (
    <SharedMacroEditor
      slot={slot}
      left={left}
      saving={saving}
      labels={labels}
      unknownTokens={unknownCwTokens}
      focus={focus}
      onSave={onSave}
      onSendOnce={onSendOnce}
      onCancel={onCancel}
      onReset={onReset}
    />
  )
}
