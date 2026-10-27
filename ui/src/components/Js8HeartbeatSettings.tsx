// Settings ▸ Digital ▸ JS8: the heartbeat-and-presence switches JS8Call 3.0.3 has, each at its
// default — heartbeat networking (its Mode menu, off), heartbeats outside 500-1000 Hz (off), the
// QSO pause (ON), the 55-minute acknowledgement rule (off) and not taking part in @ALLCALL (off).
// Rendered inside the section's "Automatic transmissions" group, with the panel's own form and
// setter, so a Save writes them with every other field. Heartbeat networking is also the dock's
// HB NET chip, as HB ACK is both a chip and a switch here.
//
// Migrated from birth (i18n/hardcoded-strings.test.ts).
import { t } from '../i18n'
import type { Settings } from '../types'

type Key = 'js8HbNetworking' | 'js8HbAnywhere' | 'js8HbQsoPause' | 'js8HbAckRateLimit' | 'js8AvoidAllcall'

export function Js8HeartbeatSettings({
  form,
  update,
}: {
  form: Settings
  update: (key: Key, value: boolean) => void
}) {
  // The QSO pause is ON by default (`!== false`: a form from before the key reads it on); the
  // rest are off by default.
  const rows: [Key, boolean, string, string][] = [
    ['js8HbNetworking', form.js8HbNetworking === true, t('settings.js8.hbNetworking.label'), t('settings.js8.hbNetworking.hint')],
    ['js8HbAnywhere', form.js8HbAnywhere === true, t('settings.js8.hbAnywhere.label'), t('settings.js8.hbAnywhere.hint')],
    ['js8HbQsoPause', form.js8HbQsoPause !== false, t('settings.js8.hbQsoPause.label'), t('settings.js8.hbQsoPause.hint')],
    ['js8HbAckRateLimit', form.js8HbAckRateLimit === true, t('settings.js8.hbAckRateLimit.label'), t('settings.js8.hbAckRateLimit.hint')],
    ['js8AvoidAllcall', form.js8AvoidAllcall === true, t('settings.js8.avoidAllcall.label'), t('settings.js8.avoidAllcall.hint')],
  ]
  return (
    <>
      {rows.map(([key, on, label, hint]) => (
        <div className="settings-field" key={key}>
          <label className="settings-toggle">
            <span className="settings-label">{label}</span>
            <button
              type="button"
              role="switch"
              aria-checked={on}
              className={`toggle${on ? ' on' : ''}`}
              onClick={() => update(key, !on)}
            >
              <span className="toggle-knob" />
            </button>
          </label>
          <span className="settings-hint">{hint}</span>
        </div>
      ))}
    </>
  )
}
