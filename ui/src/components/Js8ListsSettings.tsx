// Settings ▸ Digital ▸ JS8: what JS8Call 3.0.3 keeps for its lists and sounds, at its defaults —
// the RX block list ("Hide messages from these callsigns in the RX pane", empty), the primary and
// secondary highlight lists (words or callsigns, empty), the end-of-transmission and missing-frame
// marks (♢ and ……), the notification sounds (all off) and JS8Call's startup reset of the lists (off).
// Rendered inside the JS8 section with the panel's own form and setters, so a Save writes them with
// every other field. The call list's "Block this station" writes the block list too, on the click.
//
// Nothing here transmits.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): the marks' defaults are glyphs; every word
// is in the catalog.
import { useState } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'
import { JS8_SOUND_EVENTS, playJs8Sound, type Js8SoundEvent } from '../features/js8Sounds'

type ListKey = 'js8RxBlocklist' | 'js8HighlightPrimary' | 'js8HighlightSecondary'
type MarkKey = 'js8Eot' | 'js8Mfi'

/** JS8Call's defaults, for a form from before these settings. */
export const JS8_EOT_DEFAULT = '♢'
export const JS8_MFI_DEFAULT = '……'

/** A comma-separated list, upper-cased and trimmed as JS8Call's `splitWords` reads it. */
export const parseJs8Words = (raw: string): string[] =>
  raw
    .split(',')
    .map((w) => w.trim().toUpperCase())
    .filter(Boolean)

/** One list as one text field: what is typed stays as typed while the field has the focus. */
function WordsInput({ value, onChange, label }: { value: readonly string[]; onChange: (list: string[]) => void; label: string }) {
  const [draft, setDraft] = useState<string | null>(null)
  return (
    <input
      className="settings-input"
      aria-label={label}
      value={draft ?? value.join(', ')}
      onChange={(e) => {
        setDraft(e.target.value)
        onChange(parseJs8Words(e.target.value))
      }}
      onBlur={() => setDraft(null)}
      autoComplete="off"
      spellCheck={false}
    />
  )
}

/** Literal keys per sound, so the orphan guard sees each referenced. */
function soundLabel(ev: Js8SoundEvent): string {
  switch (ev) {
    case 'cq':
      return t('settings.js8.sound.cq')
    case 'hb':
      return t('settings.js8.sound.hb')
    case 'ack':
      return t('settings.js8.sound.ack')
    case 'directed':
      return t('settings.js8.sound.directed')
    case 'inbox':
      return t('settings.js8.sound.inbox')
    case 'callNew':
      return t('settings.js8.sound.callNew')
    case 'callOld':
      return t('settings.js8.sound.callOld')
  }
}

export function Js8ListsSettings({
  form,
  setList,
  setMark,
  setSounds,
  setReset,
}: {
  form: Settings
  setList: (key: ListKey, list: string[]) => void
  setMark: (key: MarkKey, value: string) => void
  setSounds: (events: string[]) => void
  /** JS8Call's "Reset the Band Activity, Call Activity, and RX history at startup". */
  setReset: (on: boolean) => void
}) {
  const lists: [ListKey, string, string][] = [
    ['js8RxBlocklist', t('settings.js8.rxBlocklist.label'), t('settings.js8.rxBlocklist.hint')],
    ['js8HighlightPrimary', t('settings.js8.highlightPrimary.label'), t('settings.js8.highlightPrimary.hint')],
    ['js8HighlightSecondary', t('settings.js8.highlightSecondary.label'), t('settings.js8.highlightSecondary.hint')],
  ]
  const marks: [MarkKey, string, string, string, number][] = [
    ['js8Eot', form.js8Eot ?? JS8_EOT_DEFAULT, t('settings.js8.eot.label'), t('settings.js8.eot.hint'), 2],
    ['js8Mfi', form.js8Mfi ?? JS8_MFI_DEFAULT, t('settings.js8.mfi.label'), t('settings.js8.mfi.hint'), 8],
  ]
  const sounds = form.js8Sounds ?? []
  const reset = form.js8ResetActivity === true
  return (
    <>
      <div className="settings-featgroup">
        <span className="settings-featgroup-title">{t('settings.js8.lists.title')}</span>
        {/* JS8Call's General tab (Configuration.ui:429-433), off by default (Configuration.cpp:2303). */}
        <div className="settings-field">
          <label className="settings-toggle">
            <span className="settings-label">{t('settings.js8.resetActivity.label')}</span>
            <button
              type="button"
              role="switch"
              aria-checked={reset}
              className={`toggle${reset ? ' on' : ''}`}
              onClick={() => setReset(!reset)}
            >
              <span className="toggle-knob" />
            </button>
          </label>
          <span className="settings-hint">{t('settings.js8.resetActivity.hint')}</span>
        </div>
        {lists.map(([key, label, hint]) => (
          <label className="settings-field" key={key}>
            <span className="settings-label">{label}</span>
            <WordsInput value={form[key] ?? []} onChange={(list) => setList(key, list)} label={label} />
            <span className="settings-hint">{hint}</span>
          </label>
        ))}
        {marks.map(([key, value, label, hint, max]) => (
          <label className="settings-field" key={key}>
            <span className="settings-label">{label}</span>
            <input
              className="settings-input js8-mark-input"
              aria-label={label}
              value={value}
              maxLength={max}
              onChange={(e) => setMark(key, e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
            <span className="settings-hint">{hint}</span>
          </label>
        ))}
      </div>
      <div className="settings-featgroup">
        <span className="settings-featgroup-title">{t('settings.js8.sounds.title')}</span>
        <span className="settings-hint">{t('settings.js8.sounds.hint')}</span>
        {JS8_SOUND_EVENTS.map((ev) => {
          const on = sounds.includes(ev)
          const label = soundLabel(ev)
          return (
            <div className="settings-field js8-sound" key={ev}>
              <label className="settings-toggle">
                <span className="settings-label">{label}</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={on}
                  className={`toggle${on ? ' on' : ''}`}
                  onClick={() => setSounds(on ? sounds.filter((s) => s !== ev) : JS8_SOUND_EVENTS.filter((s) => s === ev || sounds.includes(s)))}
                >
                  <span className="toggle-knob" />
                </button>
              </label>
              <button
                type="button"
                className="settings-refresh js8-sound-test"
                onClick={() => playJs8Sound(ev)}
                aria-label={t('settings.js8.sound.test.aria', { event: label })}
                title={t('settings.js8.sound.test.title')}
              >
                {t('settings.js8.sound.test.label')}
              </button>
            </div>
          )
        })}
      </div>
    </>
  )
}
