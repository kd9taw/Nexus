// Settings ▸ Digital ▸ JS8 ▸ Composing: the compose box as JS8Call 3.0.3 keeps it — the CQ message
// (`CQMessage`, default `CQ CQ CQ <MYGRID4>`), the reply message (`Reply`, `HW CPY?`), the saved
// messages (its Saved Messages tab: a list to add to, edit, order and delete from; one, `TNX 73
// GL`, out of the box), "send at once" (its "Immediately transmit CQ, Reply, Info, Saved, and
// Directed messages from the menu", on), pills (on) and word suggestions (on), with the 13 macros
// a message may carry listed under the saved messages. Rendered inside the JS8 section with the
// panel's own form, so a Save writes these with every other field; the panel's Save refuses a CQ
// message JS8Call's would (js8CqTextRefused).
//
// Migrated from birth (i18n/hardcoded-strings.test.ts): the CQ forms and the macros are the mode's
// vocabulary; every sentence is from the catalog.
import { useState } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'
import { JS8_CQS } from '../js8Vocab'
import { JS8_DEFAULT_CQ, JS8_DEFAULT_REPLY, JS8_DEFAULT_SAVED, JS8_MACRO_TOKENS, JS8_MYGRID4 } from '../js8Macros'
import { upperTyped } from '../typedInput'

type Key = 'js8CqText' | 'js8ReplyText' | 'js8SavedMessages' | 'js8SendAtOnce' | 'js8Pills' | 'js8WordSuggestions'
export type Js8ComposePatch = Partial<Pick<Settings, Key>>

export function Js8ComposeSettings({
  form,
  onChange,
}: {
  form: Settings
  onChange: (patch: Js8ComposePatch) => void
}) {
  const saved = form.js8SavedMessages ?? JS8_DEFAULT_SAVED
  const [adding, setAdding] = useState('')
  const setSaved = (next: string[]) => onChange({ js8SavedMessages: next })
  const move = (i: number, by: number) => {
    const j = i + by
    if (j < 0 || j >= saved.length) return
    const next = [...saved]
    ;[next[i], next[j]] = [next[j], next[i]]
    setSaved(next)
  }
  const add = () => {
    const m = adding.trim()
    if (m === '') return
    setSaved([...saved, m])
    setAdding('')
  }
  // The three switches, each on by default as JS8Call ships them (`!== false`: a form from before
  // the key reads it on).
  const switches: ['js8SendAtOnce' | 'js8Pills' | 'js8WordSuggestions', boolean, string, string][] = [
    ['js8SendAtOnce', form.js8SendAtOnce !== false, t('settings.js8.sendAtOnce.label'), t('settings.js8.sendAtOnce.hint')],
    ['js8Pills', form.js8Pills !== false, t('settings.js8.pills.label'), t('settings.js8.pills.hint')],
    ['js8WordSuggestions', form.js8WordSuggestions !== false, t('settings.js8.wordSuggestions.label'), t('settings.js8.wordSuggestions.hint')],
  ]
  return (
    <div className="settings-featgroup js8-compose-settings">
      <span className="settings-featgroup-title">{t('settings.js8.compose.title')}</span>
      <label className="settings-field">
        <span className="settings-label">{t('settings.js8.cqText.label')}</span>
        <input
          className="settings-input"
          list="js8-cq-forms"
          value={form.js8CqText ?? JS8_DEFAULT_CQ}
          onChange={(e) => onChange({ js8CqText: upperTyped(e.target) })}
          autoComplete="off"
          spellCheck={false}
        />
        {/* JS8Call's eight one-frame CQ forms, each with the square. */}
        <datalist id="js8-cq-forms">
          {JS8_CQS.map((c) => (
            <option key={c} value={`${c} ${JS8_MYGRID4}`} />
          ))}
        </datalist>
        <span className="settings-hint">{t('settings.js8.cqText.hint', { default: JS8_DEFAULT_CQ })}</span>
      </label>
      <label className="settings-field">
        <span className="settings-label">{t('settings.js8.replyText.label')}</span>
        <input
          className="settings-input"
          value={form.js8ReplyText ?? JS8_DEFAULT_REPLY}
          onChange={(e) => onChange({ js8ReplyText: upperTyped(e.target) })}
          autoComplete="off"
          spellCheck={false}
        />
        <span className="settings-hint">{t('settings.js8.replyText.hint', { default: JS8_DEFAULT_REPLY })}</span>
      </label>
      <div className="settings-field">
        <span className="settings-label">{t('settings.js8.saved.label')}</span>
        {saved.length === 0 ? (
          <span className="settings-hint">{t('settings.js8.saved.empty')}</span>
        ) : (
          <ol className="js8-saved-list">
            {saved.map((m, i) => (
              <li key={i} className="js8-saved-row">
                <input
                  className="settings-input"
                  value={m}
                  onChange={(e) => setSaved(saved.map((x, k) => (k === i ? upperTyped(e.target) : x)))}
                  aria-label={t('settings.js8.saved.row.aria', { n: i + 1 })}
                  autoComplete="off"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="cw-macro"
                  disabled={i === 0}
                  onClick={() => move(i, -1)}
                  aria-label={t('settings.js8.saved.up.aria', { n: i + 1 })}
                  title={t('settings.js8.saved.up.aria', { n: i + 1 })}
                >
                  ↑
                </button>
                <button
                  type="button"
                  className="cw-macro"
                  disabled={i + 1 === saved.length}
                  onClick={() => move(i, 1)}
                  aria-label={t('settings.js8.saved.down.aria', { n: i + 1 })}
                  title={t('settings.js8.saved.down.aria', { n: i + 1 })}
                >
                  ↓
                </button>
                <button
                  type="button"
                  className="cw-macro"
                  onClick={() => setSaved(saved.filter((_, k) => k !== i))}
                  aria-label={t('settings.js8.saved.remove.aria', { n: i + 1 })}
                  title={t('settings.js8.saved.remove.aria', { n: i + 1 })}
                >
                  ✕
                </button>
              </li>
            ))}
          </ol>
        )}
        <div className="js8-saved-add">
          <input
            className="settings-input"
            value={adding}
            onChange={(e) => setAdding(upperTyped(e.target))}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return
              e.preventDefault()
              add()
            }}
            placeholder={t('settings.js8.saved.add.placeholder')}
            aria-label={t('settings.js8.saved.add.placeholder')}
            autoComplete="off"
            spellCheck={false}
          />
          <button type="button" className="cw-macro" disabled={adding.trim() === ''} onClick={add}>
            {t('settings.js8.saved.add')}
          </button>
        </div>
        <span className="settings-hint">{t('settings.js8.saved.hint')}</span>
        <span className="settings-hint js8-macro-list">
          {t('settings.js8.macros.label')} <code>{JS8_MACRO_TOKENS.join(' ')}</code>
        </span>
      </div>
      {switches.map(([key, on, label, hint]) => (
        <div className="settings-field" key={key}>
          <label className="settings-toggle">
            <span className="settings-label">{label}</span>
            <button
              type="button"
              role="switch"
              aria-checked={on}
              className={`toggle${on ? ' on' : ''}`}
              onClick={() => onChange({ [key]: !on })}
            >
              <span className="toggle-knob" />
            </button>
          </label>
          <span className="settings-hint">{hint}</span>
        </div>
      ))}
    </div>
  )
}
