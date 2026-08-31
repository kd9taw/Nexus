// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT, and must not: the band-plan channel labels and
// their dial frequencies, the HF/VHF/UHF group names, the band chip, and the USB/FM mode names —
// `MODES` is both the button text and the value handed to `onSet`, so it is a token twice over.
// The channel select's `title` is the plan's own note when it has one; only the fallback is prose.
import { useMemo } from 'react'
import type { BandChannel, RadioMode } from '../types'
import { bandLabelForMhz } from '../band'
import { bandColor } from '../bandColors'
import { t } from '../i18n'
import { FrequencyReadout } from './FrequencyReadout'

interface Props {
  channels: BandChannel[]
  dialMhz: number
  band: string
  /** current phone mode (USB / FM) as a free string from the snapshot/settings */
  mode: string
  /** compact = TopBar inline; full = Settings field block */
  variant?: 'compact' | 'full'
  /** Render the big MHz readout (default). Set false when a parent (the shared CockpitHeader)
   * already owns the readout and this control only supplies the band-plan channel select + chip. */
  showReadout?: boolean
  /** Render the USB/FM mode toggle (default). Off for FT8/FT4, whose "mode" is the tier. */
  showModeToggle?: boolean
  onSet: (dialMhz: number, band: string, mode: string) => void
}

const GROUP_ORDER: BandChannel['group'][] = ['HF', 'VHF', 'UHF']
const MODES: RadioMode[] = ['USB', 'FM']
// Dial-match tolerance for highlighting the active channel (Hz-ish in MHz).
const MATCH_EPS = 0.0005

/** Stable key for a channel (band id is unique in the plan). */
function chanKey(c: BandChannel): string {
  return c.band
}

function findActive(channels: BandChannel[], dialMhz: number, mode: string): BandChannel | null {
  return (
    channels.find(
      (c) => Math.abs(c.dialMhz - dialMhz) < MATCH_EPS && c.mode === mode,
    ) ?? null
  )
}

export function FrequencyControl({
  channels,
  dialMhz,
  band,
  mode,
  variant = 'compact',
  showReadout = true,
  showModeToggle = true,
  onSet,
}: Props) {
  const active = useMemo(
    () => findActive(channels, dialMhz, mode),
    [channels, dialMhz, mode],
  )

  const grouped = useMemo(() => {
    const out: { group: BandChannel['group']; items: BandChannel[] }[] = []
    for (const g of GROUP_ORDER) {
      const items = channels.filter((c) => c.group === g)
      if (items.length) out.push({ group: g, items })
    }
    return out
  }, [channels])

  const selectChannel = (key: string) => {
    const c = channels.find((x) => chanKey(x) === key)
    if (c) onSet(c.dialMhz, c.band, c.mode)
  }

  const setMode = (next: RadioMode) => {
    if (next === mode) return
    onSet(dialMhz, band, next)
  }

  const selectValue = active ? chanKey(active) : ''

  // The band is a primary operating fact — color the control with the active
  // band's color (shared with the map spot dots + the CW/Phone BandPicker) so
  // FT8/FT4 and Tempo read the band the same way CW/Phone do.
  const col = bandColor(band || bandLabelForMhz(dialMhz) || '')

  return (
    <div className={`freq-control ${variant}`} role="group" aria-label={t('freq.control.aria')}>
      <span className="band-picker-dot" style={{ background: col }} aria-hidden="true" />
      <label className="freq-channel-wrap">
        {variant === 'full' && <span className="settings-label">{t('freq.channel.label')}</span>}
        <select
          className="freq-channel"
          value={selectValue}
          onChange={(e) => selectChannel(e.target.value)}
          title={active ? active.note : t('freq.channel.title')}
          aria-label={t('freq.channel.aria')}
          style={{ color: col, borderColor: col, boxShadow: `0 0 0 1px ${col}55, 0 0 10px ${col}33` }}
        >
          <option value="">
            {active
              ? t('freq.channel.presets')
              : t('freq.channel.custom', { band: band || '—' })}
          </option>
          {grouped.map((g) => (
            <optgroup key={g.group} label={g.group}>
              {g.items.map((c) => (
                <option
                  key={chanKey(c)}
                  value={chanKey(c)}
                  // Receive-only bands stay SELECTABLE — you may listen anywhere, and the
                  // rig tunes there. The suffix says why you will not be able to key it;
                  // the transmit gate is what actually refuses.
                  title={c.tx === false ? t('freq.channel.rxOnly.title') : c.note}
                >
                  {c.label} · {c.dialMhz.toFixed(4)} · {c.mode}
                  {c.tx === false ? ` · ${t('freq.channel.rxOnly')}` : ''}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>

      {showReadout && (
        <div className="freq-manual-wrap">
          {variant === 'full' && <span className="settings-label">{t('freq.dial.label')}</span>}
          <FrequencyReadout
            dialMhz={dialMhz}
            size="hero"
            editable
            commitOnBlur
            onCommit={(v) => onSet(v, bandLabelForMhz(v), mode)}
          />
        </div>
      )}

      <div className="freq-band-tag" title={active ? active.note : t('freq.band.title')}>
        <span className={`band-chip${active ? ' active' : ''}`}>{band || bandLabelForMhz(dialMhz) || '—'}</span>
      </div>

      {showModeToggle && (
        <div className="freq-mode-toggle" role="group" aria-label={t('freq.mode.aria')}>
          {MODES.map((md) => (
            <button
              key={md}
              type="button"
              className={`freq-mode-btn${mode === md ? ' active' : ''}`}
              aria-pressed={mode === md}
              onClick={() => setMode(md)}
            >
              {md}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
