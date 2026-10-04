// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every sentence is in the catalog under
// `scope.strip.*`. The window widths (Hz) and the averaging times (ms, s) are measurements and stay here.
//
// The scope's ⚙ strip: the analysis window (the FFT size), log-recursive averaging and the detector, for
// one cockpit's scale record (scaleSettings.ts). A controlled component: the host binds it to
// `useScaleSettings` and hides or shows it behind its ⚙ button. G and Z stay where the operator already
// finds them, beside the S-meter; they read and write the same record.
//
// The window is a request to the station (the backend computes the scope's row with it), so a window
// that does not drive the station's scope shows it disabled, as the rig scope's button does today. The
// averaging and the detector are this window's own drawing and are never disabled.

import { t } from '../i18n'
import type { ScopeWindow } from '../api'
import { AVERAGE_STEPS_MS } from './scaleAverage'
import type { ScaleSettings } from './scaleSettings'
import type { Detector } from './types'

/** The window choices, coarsest first. The label is the Hann main lobe's width, the thing the operator
 *  is choosing between (the rig scope's button carries the same three). */
const WINDOWS: ReadonlyArray<{ id: ScopeWindow; label: string }> = [
  { id: 'fast', label: '47 Hz' },
  { id: 'balanced', label: '23 Hz' },
  { id: 'sharp', label: '12 Hz' },
]

const DETECTORS: readonly Detector[] = ['peak', 'average']

// Each sentence is looked up by a literal key, not through a table of keys, so the catalog guards
// (i18n/placeholders.test.ts) read every call site.
function windowTitle(id: ScopeWindow): string {
  switch (id) {
    case 'fast':
      return t('scope.strip.window.fast.title')
    case 'balanced':
      return t('scope.strip.window.balanced.title')
    case 'sharp':
      return t('scope.strip.window.sharp.title')
  }
}

function detectorLabel(d: Detector): string {
  return d === 'peak' ? t('scope.strip.detector.peak') : t('scope.strip.detector.average')
}

function detectorTitle(d: Detector): string {
  return d === 'peak' ? t('scope.strip.detector.peak.title') : t('scope.strip.detector.average.title')
}

const MS = 'ms'
const S = 's'

/** An averaging time as the control shows it: `50 ms`, `1 s`. */
function averageLabel(ms: number): string {
  return ms < 1000 ? `${ms} ${MS}` : `${ms / 1000} ${S}`
}

interface Props {
  settings: ScaleSettings
  onChange: (patch: Partial<ScaleSettings>) => void
  /** Offer the analysis window. The rig scope's row has one; a waterfall drawing the wide audio row,
   *  whose window is fixed, must not offer a control that changes nothing. */
  windowControl: boolean
  /** Whether this window drives the station's scope. Without it the window buttons are disabled. */
  control: boolean
}

export function ScaleStrip({ settings, onChange, windowControl, control }: Props) {
  return (
    <div className="ph-scope-strip" role="group" aria-label={t('scope.strip.aria')}>
      {windowControl && (
        <span role="group" aria-label={t('scope.strip.window.aria')}>
          {WINDOWS.map((w) => (
            <button
              key={w.id}
              type="button"
              className={`ph-scope-btn${settings.window === w.id ? ' on' : ''}`}
              aria-pressed={settings.window === w.id}
              disabled={!control}
              title={control ? windowTitle(w.id) : t('remote.scopeFollowsStation')}
              onClick={() => onChange({ window: w.id })}
            >
              {w.label}
            </button>
          ))}
        </span>
      )}
      <label className="ph-scope-gz" title={t('scope.strip.average.title')}>
        {t('scope.strip.average.label')}
        <select value={settings.averageMs} onChange={(e) => onChange({ averageMs: Number(e.target.value) })}>
          {AVERAGE_STEPS_MS.map((ms) => (
            <option key={ms} value={ms}>
              {ms === 0 ? t('scope.strip.average.off') : averageLabel(ms)}
            </option>
          ))}
        </select>
      </label>
      <span role="group" aria-label={t('scope.strip.detector.aria')}>
        {DETECTORS.map((d) => (
          <button
            key={d}
            type="button"
            className={`ph-scope-btn${settings.detector === d ? ' on' : ''}`}
            aria-pressed={settings.detector === d}
            title={detectorTitle(d)}
            onClick={() => onChange({ detector: d })}
          >
            {detectorLabel(d)}
          </button>
        ))}
      </span>
    </div>
  )
}
