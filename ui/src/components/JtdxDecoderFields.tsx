// Settings ▸ Digital ▸ Decoder: which FT8 decoder runs the full pass, and the JTDX-style decoder's
// preset and knobs.
//
// The JTDX-style decoder is WSJT-X 3.0's multithreaded FT8 decoder, its port of JTDX's. Its presets
// are the engine's (`crates/tempo-app/src/settings.rs`, `JtdxPreset::knobs`); PRESET_KNOBS below
// mirrors them so a knob turned under a preset starts Custom from what that preset runs. Turning
// any knob makes the preset Custom. Keen and Ludicrous say, plainly, how many cores they may take
// against how many this computer has.
import { t } from '../i18n'
import type { JtdxKnobs, JtdxPreset, Settings } from '../types'

/** ft8var's thread ceiling (WSJT-X `decoder.f90`: 1..12). */
export const JTDX_MAX_THREADS = 12

/** What each preset runs: the engine's `JtdxPreset::knobs`. */
export const PRESET_KNOBS: Record<Exclude<JtdxPreset, 'custom'>, JtdxKnobs> = {
  easy: { cycles: 2, sensitivity: 2, rxFreqSensitivity: 3, threads: 1, wideDxSearch: true, candThin: 100, swl: false },
  keen: { cycles: 3, sensitivity: 3, rxFreqSensitivity: 3, threads: 0, wideDxSearch: true, candThin: 100, swl: false },
  ludicrous: { cycles: 3, sensitivity: 3, rxFreqSensitivity: 3, threads: 12, wideDxSearch: true, candThin: 100, swl: true },
}

/** The knobs a preset runs; Custom's are the form's own. */
export function knobsOf(preset: JtdxPreset, custom: JtdxKnobs | undefined): JtdxKnobs {
  return preset === 'custom' ? (custom ?? PRESET_KNOBS.easy) : PRESET_KNOBS[preset]
}

/** WSJT-X's automatic thread count for `cores` logical cores (`decoder.f90`, ft8var_threads): Keen's. */
export function autoThreads(cores: number): number {
  if (cores <= 1) return 1
  if (cores < 5) return cores - 1
  if (cores < 9) return cores - 2
  if (cores < 16) return cores - 3
  return JTDX_MAX_THREADS
}

/** The logical cores this computer reports (what WSJT-X's thread count is measured against). */
export function logicalCores(): number {
  const n = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency : 0
  return n && n > 0 ? n : 1
}

export interface JtdxDecoderFieldsProps {
  form: Settings
  setForm: (f: (prev: Settings | null) => Settings | null) => void
  markDirty: () => void
  locked: (key: keyof Settings) => boolean
  /** The computer's logical cores; tests pass their own. */
  cores?: number
}

export function JtdxDecoderFields({ form, setForm, markDirty, locked, cores }: JtdxDecoderFieldsProps) {
  const style = form.ft8Decoder ?? 'jtdx'
  const preset = form.jtdxPreset ?? 'easy'
  const knobs = knobsOf(preset, form.jtdxCustom)
  const have = cores ?? logicalCores()

  const set = (patch: Partial<Settings>) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, ...patch } : prev))
  }
  /** Turn one knob: the preset becomes Custom, starting from what was running. */
  const turn = (patch: Partial<JtdxKnobs>) => set({ jtdxPreset: 'custom', jtdxCustom: { ...knobs, ...patch } })

  const chips = <T extends string | number>(
    label: string,
    options: readonly { value: T; name: string }[],
    current: T,
    pick: (v: T) => void,
    disabled: boolean,
  ) => (
    <div className="theme-switcher" role="group" aria-label={label}>
      {options.map(({ value, name }) => (
        <button
          key={String(value)}
          type="button"
          disabled={disabled}
          className={`theme-chip${current === value ? ' active' : ''}`}
          aria-pressed={current === value}
          onClick={() => pick(value)}
        >
          {name}
        </button>
      ))}
    </div>
  )

  const toggle = (label: string, on: boolean, flip: () => void, disabled: boolean) => (
    <label className="settings-toggle">
      <span className="settings-label">{label}</span>
      <button
        type="button"
        role="switch"
        disabled={disabled}
        aria-checked={on}
        className={`toggle${on ? ' on' : ''}`}
        onClick={flip}
      >
        <span className="toggle-knob" />
      </button>
    </label>
  )

  const lockedJtdx = locked('jtdxPreset') || locked('jtdxCustom')

  return (
    <>
      <div className="settings-field" data-field="ft8-decoder">
        <span className="settings-label">{t('settings.digital.ft8Decoder.label')}</span>
        {chips(
          t('settings.digital.ft8Decoder.label'),
          [
            { value: 'jtdx', name: t('settings.digital.ft8Decoder.jtdx') },
            { value: 'standard', name: t('settings.digital.ft8Decoder.standard') },
          ] as const,
          style,
          (v) => set({ ft8Decoder: v }),
          locked('ft8Decoder'),
        )}
        <span className="settings-hint">{t('settings.digital.ft8Decoder.hint')}</span>
      </div>

      {style === 'jtdx' && (
        <div className="settings-field" data-field="jtdx-preset">
          <span className="settings-label">{t('settings.digital.jtdxPreset.label')}</span>
          {chips(
            t('settings.digital.jtdxPreset.label'),
            [
              { value: 'easy', name: t('settings.digital.jtdxPreset.easy') },
              { value: 'keen', name: t('settings.digital.jtdxPreset.keen') },
              { value: 'ludicrous', name: t('settings.digital.jtdxPreset.ludicrous') },
              { value: 'custom', name: t('settings.digital.jtdxPreset.custom') },
            ] as const,
            preset,
            (v: JtdxPreset) =>
              set(
                v === 'custom'
                  ? { jtdxPreset: 'custom', jtdxCustom: form.jtdxCustom ?? knobs }
                  : { jtdxPreset: v },
              ),
            lockedJtdx,
          )}
          <span className="settings-hint">{t('settings.digital.jtdxPreset.hint')}</span>
          {preset === 'keen' && (
            <span className="settings-hint settings-warn" role="note" data-cpu-warning="keen">
              {t('settings.digital.jtdxPreset.keenWarning', { cores: autoThreads(have), have })}
            </span>
          )}
          {preset === 'ludicrous' && (
            <span className="settings-hint settings-warn" role="note" data-cpu-warning="ludicrous" data-ludicrous-warning>
              {t('settings.digital.jtdxPreset.ludicrousWarning', {
                cores: Math.min(JTDX_MAX_THREADS, PRESET_KNOBS.ludicrous.threads),
                have,
              })}
            </span>
          )}
        </div>
      )}

      {style === 'jtdx' && (
        <details className="settings-field" data-field="jtdx-advanced" open={preset === 'custom'}>
          <summary className="settings-label">{t('settings.digital.jtdx.advanced')}</summary>
          <div className="settings-field">
            <span className="settings-label">{t('settings.digital.jtdx.cycles.label')}</span>
            {chips(
              t('settings.digital.jtdx.cycles.label'),
              [1, 2, 3].map((v) => ({ value: v, name: String(v) })),
              knobs.cycles,
              (v) => turn({ cycles: v }),
              lockedJtdx,
            )}
            <span className="settings-hint">{t('settings.digital.jtdx.cycles.hint')}</span>
          </div>
          <div className="settings-field">
            <span className="settings-label">{t('settings.digital.jtdx.sensitivity.label')}</span>
            {chips(
              t('settings.digital.jtdx.sensitivity.label'),
              [
                { value: 1, name: t('settings.digital.jtdx.sensitivity.minimum') },
                { value: 2, name: t('settings.digital.jtdx.sensitivity.lowThresholds') },
                { value: 3, name: t('settings.digital.jtdx.sensitivity.subPass') },
              ],
              knobs.sensitivity,
              (v) => turn({ sensitivity: v }),
              lockedJtdx,
            )}
          </div>
          <div className="settings-field">
            <span className="settings-label">{t('settings.digital.jtdx.rxSens.label')}</span>
            {chips(
              t('settings.digital.jtdx.rxSens.label'),
              [
                { value: 1, name: t('settings.digital.jtdx.rxSens.low') },
                { value: 2, name: t('settings.digital.jtdx.rxSens.medium') },
                { value: 3, name: t('settings.digital.jtdx.rxSens.high') },
              ],
              knobs.rxFreqSensitivity,
              (v) => turn({ rxFreqSensitivity: v }),
              lockedJtdx,
            )}
            <span className="settings-hint">{t('settings.digital.jtdx.rxSens.hint')}</span>
          </div>
          <div className="settings-field">
            <span className="settings-label">{t('settings.digital.jtdx.threads.label')}</span>
            {chips(
              t('settings.digital.jtdx.threads.label'),
              [
                { value: 0, name: t('settings.digital.jtdx.threads.auto') },
                ...[1, 2, 4, 8, 12].map((v) => ({ value: v, name: String(v) })),
              ],
              knobs.threads,
              (v) => turn({ threads: v }),
              lockedJtdx,
            )}
            <span className="settings-hint">{t('settings.digital.jtdx.threads.hint', { have })}</span>
          </div>
          <div className="settings-field">
            <span className="settings-label">{t('settings.digital.jtdx.candThin.label')}</span>
            {chips(
              t('settings.digital.jtdx.candThin.label'),
              [100, 75, 50].map((v) => ({ value: v, name: `${v}%` })),
              knobs.candThin,
              (v) => turn({ candThin: v }),
              lockedJtdx,
            )}
            <span className="settings-hint">{t('settings.digital.jtdx.candThin.hint')}</span>
          </div>
          <div className="settings-field">
            {toggle(
              t('settings.digital.jtdx.wideDx.label'),
              knobs.wideDxSearch,
              () => turn({ wideDxSearch: !knobs.wideDxSearch }),
              lockedJtdx,
            )}
            <span className="settings-hint">{t('settings.digital.jtdx.wideDx.hint')}</span>
          </div>
          <div className="settings-field">
            {toggle(t('settings.digital.jtdx.swl.label'), knobs.swl, () => turn({ swl: !knobs.swl }), lockedJtdx)}
            <span className="settings-hint">{t('settings.digital.jtdx.swl.hint')}</span>
          </div>
        </details>
      )}
    </>
  )
}
