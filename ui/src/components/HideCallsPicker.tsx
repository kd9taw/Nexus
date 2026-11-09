// The wildcard call-hide control for the Band Activity chip bar (F4MQS). A small popover —
// PORTALED for the same reason CountryExcludePicker is (the chip bar's clip would swallow an
// inline popover in the narrow rail) — with one text field. The list itself lives in
// features/hideCalls.ts; every pane subscribes, so a pane and its control never disagree.
//
// #346 — the calls a row's menu hid more narrowly (on one band, or for a while) are listed under
// the field, each with what it covers, when it ends and a ✕ that shows it again — a call hidden
// from a menu always has a way back, and the chip counts it.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The prefixes quoted
// inside the note stay in the sentence, as `ADIF OPERATOR` does in the Station hint.
import * as RM from '@radix-ui/react-dropdown-menu'
import { MenuContent, MenuRoot } from './ui/MenuParts'
import { useEffect, useState } from 'react'
import { unhideScoped, useHideCalls, type ScopedHide } from '../features/hideCalls'
import { t } from '../i18n'
import { T } from '../i18n/T'

/** When a scoped hide ends, as the operator's clock in UTC reads it: HH:MM, with the date when it
 *  is not today's. */
function untilLabel(h: ScopedHide): string {
  if (h.until === null) return t('hideCalls.scoped.always')
  const end = new Date(h.until)
  const now = new Date()
  const hhmm = end.toISOString().slice(11, 16)
  const sameDay = end.toISOString().slice(0, 10) === now.toISOString().slice(0, 10)
  return t('hideCalls.scoped.until', { time: sameDay ? hhmm : `${end.toISOString().slice(0, 10)} ${hhmm}` })
}

export function HideCallsPicker() {
  const { entries, setEntries, scoped } = useHideCalls()
  const count = entries.length + scoped.length
  // Local edit buffer so typing is smooth; committed on blur / Enter / menu close.
  const [text, setText] = useState(entries.join(' '))
  useEffect(() => setText(entries.join(' ')), [entries])
  const commit = () => setEntries(text)

  return (
    <MenuRoot onOpenChange={(open) => !open && commit()}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`od-chip${count > 0 ? ' active' : ''}`}
          title={t('hideCalls.chip.title')}
        >
          {t('hideCalls.chip.label')}
          {count > 0 ? ` · ${count}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <MenuContent className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">{t('hideCalls.head')}</div>
            <div style={{ padding: '0.4rem 0.6rem' }}>
              <input
                className="settings-input"
                type="text"
                value={text}
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
            {scoped.length > 0 && (
              <>
                <div className="country-menu-head">{t('hideCalls.scoped.head')}</div>
                <ul className="hide-scoped-list">
                  {scoped.map((h) => (
                    <li key={`${h.call} ${h.band ?? ''}`} className="hide-scoped-item">
                      <span className="hide-scoped-what">
                        {h.band
                          ? t('hideCalls.scoped.band', { call: h.call, band: h.band })
                          : t('hideCalls.scoped.allBands', { call: h.call })}
                      </span>
                      <span className="hide-scoped-until">{untilLabel(h)}</span>
                      <button
                        type="button"
                        className="hide-scoped-remove"
                        onClick={() => unhideScoped(h.call, h.band)}
                        aria-label={t('hideCalls.scoped.remove', { call: h.call })}
                        title={t('hideCalls.scoped.remove', { call: h.call })}
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
            <div className="country-menu-note">
              <T k="hideCalls.note" tags={{ code: <code /> }} />
            </div>
          </div>
        </MenuContent>
      </RM.Portal>
    </MenuRoot>
  )
}
