// Settings ▸ Digital ▸ JS8: what JS8Call 3.0.3 keeps for its lists and sounds, at its defaults —
// the RX block list ("Hide messages from these callsigns in the RX pane", empty), the primary and
// secondary highlight lists (words or callsigns, empty), the end-of-transmission and missing-frame
// marks (♢ and ……), the notification sounds (all off), each with the WAV file it plays, as JS8Call
// lets each play one, or the built-in tone, and JS8Call's startup reset of the lists (off). Rendered
// inside the JS8 section with the panel's own form and setters, so a Save writes them with every other
// field. The call list's "Block this station" writes the block list too, on the click.
//
// Nothing here transmits.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): the marks' defaults are glyphs; every word
// is in the catalog.
import { useState, type ReactNode } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'
import { JS8_SOUND_EVENTS, js8SoundFileName, playJs8Sound, type Js8SoundEvent } from '../features/js8Sounds'

type ListKey = 'js8RxBlocklist' | 'js8HighlightPrimary' | 'js8HighlightSecondary'
type MarkKey = 'js8Eot' | 'js8Mfi'
/** The sounds' settings set at the station only: a save from a computer driving it keeps them. */
type StationKey = 'js8SoundFiles' | 'js8SoundDevice'

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
  pickSoundFile,
  setSoundFile,
  outputs = [],
  outputLabel = (d: string) => d,
  setSoundDevice,
  locked,
  setAtStation,
}: {
  form: Settings
  setList: (key: ListKey, list: string[]) => void
  setMark: (key: MarkKey, value: string) => void
  setSounds: (events: string[]) => void
  /** JS8Call's "Reset the Band Activity, Call Activity, and RX history at startup". */
  setReset: (on: boolean) => void
  /** The OS file picker for a sound's WAV file (null when cancelled). Absent: no file controls. */
  pickSoundFile?: () => Promise<string | null>
  /** A sound's file as it should be; null is the built-in tone. */
  setSoundFile?: (event: Js8SoundEvent, path: string | null) => void
  /** This computer's output devices, by name, for the sounds' output picker. */
  outputs?: readonly string[]
  /** An output's name as the other device pickers show it. */
  outputLabel?: (device: string) => string
  /** The sounds' output as it should be ('' = the system default). Absent: no picker. */
  setSoundDevice?: (device: string) => void
  /** From a computer driving the station, whether a sound setting is set at the station only: its
   *  output shows read-only and its files' Choose… and Reset are not drawn; `setAtStation` says so.
   *  Absent at the station. */
  locked?: (key: StationKey) => boolean
  setAtStation?: (key: StationKey) => ReactNode
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
  const files = form.js8SoundFiles ?? {}
  const device = form.js8SoundDevice ?? ''
  const filesHere = !locked?.('js8SoundFiles')
  // The radio's TX audio devices, offered but not choosable: a sound there could key a rig on VOX.
  // A convenience only: what holds the line is the check at every play (tempo_audio::notify_sound).
  const txDevices = new Set(
    [form.audioOut, ...(form.radios ?? []).map((r) => r.audioOut)].filter((d): d is string => !!d?.trim()).map((d) => d.trim().toLowerCase()),
  )
  const isTx = (d: string) => txDevices.has(d.trim().toLowerCase())
  const deviceOptions = device && !outputs.includes(device) ? [device, ...outputs] : [...outputs]
  // Test plays on the output the form holds, before Save, refused in Rust against the form's TX audio
  // devices as well as the saved ones. Not from a computer driving the station: there the output is
  // the station's, and Test plays on the saved one.
  const testRoute =
    setSoundDevice && !locked?.('js8SoundDevice')
      ? { device, txDevices: [form.audioOut ?? '', ...(form.radios ?? []).map((r) => r.audioOut ?? '')] }
      : undefined
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
        {/* From a computer driving the station a sound plays its built-in tone there (client_mode's
            JS8 tone): the station's files stay at the station, and none is chosen here. */}
        {!filesHere && (
          <span className="settings-hint" data-js8-sounds-here="">
            {t('settings.js8.sounds.thisComputer')}
          </span>
        )}
        {setAtStation?.('js8SoundFiles')}
        {setSoundDevice && (
          <label className="settings-field">
            <span className="settings-label">{t('settings.js8.sound.device.label')}</span>
            <select
              className="settings-input js8-sound-device"
              aria-label={t('settings.js8.sound.device.label')}
              disabled={locked?.('js8SoundDevice')}
              data-setting="js8SoundDevice"
              value={device}
              onChange={(e) => setSoundDevice(e.target.value)}
            >
              <option value="">{t('settings.audio.device.systemDefault')}</option>
              {deviceOptions.map((d) => (
                <option key={d} value={d} disabled={isTx(d) && d !== device}>
                  {isTx(d) ? t('settings.js8.sound.device.tx', { device: outputLabel(d) }) : outputLabel(d)}
                </option>
              ))}
            </select>
            {setAtStation?.('js8SoundDevice')}
            <span className="settings-hint">{t('settings.js8.sound.device.hint')}</span>
          </label>
        )}
        {JS8_SOUND_EVENTS.map((ev) => {
          const on = sounds.includes(ev)
          const label = soundLabel(ev)
          const file = files[ev] || null
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
                onClick={() => void playJs8Sound(ev, file, true, testRoute)}
                aria-label={t('settings.js8.sound.test.aria', { event: label })}
                title={t('settings.js8.sound.test.title')}
              >
                {t('settings.js8.sound.test.label')}
              </button>
              {pickSoundFile && setSoundFile && (
                <span className="js8-sound-file">
                  <span className="js8-sound-file-name" title={file ?? undefined}>
                    {file ? js8SoundFileName(file) : t('settings.js8.sound.file.builtIn')}
                  </span>
                  {filesHere && (
                    <button
                      type="button"
                      className="settings-refresh"
                      data-setting="js8SoundFiles"
                      onClick={() =>
                        void pickSoundFile().then((path) => {
                          if (path) setSoundFile(ev, path)
                        })
                      }
                      aria-label={t('settings.js8.sound.file.choose.aria', { event: label })}
                      title={t('settings.js8.sound.file.choose.title')}
                    >
                      {t('settings.js8.sound.file.choose.label')}
                    </button>
                  )}
                  {filesHere && file && (
                    <button
                      type="button"
                      className="settings-refresh"
                      data-setting="js8SoundFiles"
                      onClick={() => setSoundFile(ev, null)}
                      aria-label={t('settings.js8.sound.file.reset.aria', { event: label })}
                    >
                      {t('settings.js8.sound.file.reset.label')}
                    </button>
                  )}
                </span>
              )}
            </div>
          )
        })}
      </div>
    </>
  )
}
