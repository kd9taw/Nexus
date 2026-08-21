// The wildcard call-hide control for the Band Activity chip bar (F4MQS). A small popover —
// PORTALED for the same reason CountryExcludePicker is (the chip bar's clip would swallow an
// inline popover in the narrow rail) — with one text field. The list itself lives in
// features/hideCalls.ts; every pane subscribes, so a pane and its control never disagree.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The prefixes the
// placeholder offers are technical tokens and live below as HIDE_EXAMPLES; the ones quoted
// inside the note stay in the sentence, as `ADIF OPERATOR` does in the Station hint.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useEffect, useState } from 'react'
import { useHideCalls } from '../features/hideCalls'
import { t } from '../i18n'
import { T } from '../i18n/T'

/** Example entries — a callsign and two prefix wildcards. Syntax, not words. */
const HIDE_EXAMPLES = 'VP8* R0* K1ABC'

export function HideCallsPicker() {
  const { entries, setEntries } = useHideCalls()
  // Local edit buffer so typing is smooth; committed on blur / Enter / menu close.
  const [text, setText] = useState(entries.join(' '))
  useEffect(() => setText(entries.join(' ')), [entries])
  const commit = () => setEntries(text)

  return (
    <RM.Root modal={false} onOpenChange={(open) => !open && commit()}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`od-chip${entries.length > 0 ? ' active' : ''}`}
          title={t('hideCalls.chip.title')}
        >
          {t('hideCalls.chip.label')}
          {entries.length > 0 ? ` · ${entries.length}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">{t('hideCalls.head')}</div>
            <div style={{ padding: '0.4rem 0.6rem' }}>
              <input
                className="settings-input"
                type="text"
                value={text}
                placeholder={t('hideCalls.placeholder', { examples: HIDE_EXAMPLES })}
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setText(e.target.value)}
                onBlur={commit}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commit()
                }}
                // Radix would treat typing as a typeahead over menu items; keep keys local.
                onKeyDownCapture={(e) => e.stopPropagation()}
                style={{ width: '16rem', maxWidth: '70vw' }}
              />
            </div>
            <div className="country-menu-note">
              <T k="hideCalls.note" tags={{ code: <code /> }} />
            </div>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
