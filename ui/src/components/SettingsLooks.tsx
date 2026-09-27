// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word comes from the
// catalog. The look ids are code (features/looks.ts).
//
// Settings ▸ Appearance ▸ Workspace ▸ Look — the one-tap looks. The row NAMES the look on screen by
// reading the five settings back (`lookOf`), so there is nothing stored to disagree with the screen:
// change a setting by hand and the row says Custom. A look is applied by a tap and by nothing else.
//
// UNDO is the answer to "a tap replaced my own mix": it remembers, in this component only, what
// the last tap replaced, and puts back exactly the settings that look owns — never one changed
// after it. It is offered only while that look is still the one on screen, so it can never undo
// anything but the tap. Nothing about it is stored; leaving Settings forgets it.
//
// Each chip's tooltip lists what it sets, built from the rows' own labels ("Density: Compact"),
// so the list cannot drift from the controls it names. The hint says what a look never touches.
import { useState } from 'react'
import { t } from '../i18n'
import type { Density } from '../useDensity'
import type { TextSize } from '../useTextSize'
import {
  LOOKS,
  lookAxesOf,
  lookOf,
  setAxes,
  type Look,
  type LookAxes,
  type LookId,
  type LookSetters,
} from '../features/looks'

/** "Field mode: Off · Night: Off · …" — the settings a look writes, in the rows' own words, so a
 *  tooltip says exactly what the rows below would show. Literal keys throughout: every one is
 *  checked by the catalog guards like any other call site. */
function summary(look: Look): string {
  const s = look.sets
  const parts: string[] = []
  if (s.fieldMode !== undefined)
    parts.push(
      `${t('settings.workspace.field.label')}: ${s.fieldMode ? t('settings.workspace.field.on') : t('settings.workspace.field.off')}`,
    )
  if (s.night !== undefined)
    parts.push(
      `${t('settings.workspace.night.label')}: ${s.night === 'on' ? t('settings.workspace.night.on') : t('settings.workspace.night.off')}`,
    )
  if (s.highContrast !== undefined)
    parts.push(
      `${t('settings.workspace.contrast.label')}: ${s.highContrast ? t('settings.workspace.contrast.on') : t('settings.workspace.contrast.off')}`,
    )
  if (s.textSize !== undefined) parts.push(`${t('settings.workspace.textSize.label')}: ${textSizeWord(s.textSize)}`)
  if (s.density !== undefined) parts.push(`${t('settings.workspace.density.label')}: ${densityWord(s.density)}`)
  return parts.join(' · ')
}

function textSizeWord(v: TextSize): string {
  switch (v) {
    case 'normal':
      return t('settings.workspace.textSize.normal')
    case 'large':
      return t('settings.workspace.textSize.large')
    case 'larger':
      return t('settings.workspace.textSize.larger')
  }
}

function densityWord(v: Density): string {
  switch (v) {
    case 'guided':
      return t('settings.workspace.density.guided')
    case 'standard':
      return t('settings.workspace.density.standard')
    case 'dense':
      return t('settings.workspace.density.dense')
    case 'touch':
      return t('settings.workspace.density.touch')
  }
}

interface Props {
  /** The five settings as they are now — the host's own state, read back on every render. */
  now: LookAxes
  set: LookSetters
  disabled?: boolean
}

export function SettingsLooks({ now, set, disabled }: Props) {
  const on = lookOf(now)
  // What the last tap replaced. Undo writes only the settings that look owns.
  const [undo, setUndo] = useState<{ look: LookId; to: LookAxes } | null>(null)
  const undoLook = undo && on === undo.look ? LOOKS.find((l) => l.id === undo.look) : undefined
  const undoTo = undoLook && undo ? backTo(undoLook, undo.to, now) : null
  const canUndo = !!undoTo && !sameAxes(undoTo, now)

  const tap = (look: Look) => {
    const next = lookAxesOf(look, now)
    if (sameAxes(next, now)) return
    setUndo({ look: look.id, to: now })
    setAxes(next, now, set)
  }

  return (
    <div className="settings-field settings-looks">
      <span className="settings-label">{t('settings.workspace.looks.label')}</span>
      <div className="settings-looks-row">
        <div className="theme-switcher" role="group" aria-label={t('settings.workspace.looks.label')}>
          {LOOKS.map((l) => (
            <button
              key={l.id}
              type="button"
              disabled={disabled}
              className={`theme-chip${on === l.id ? ' active' : ''}`}
              aria-pressed={on === l.id}
              title={summary(l)}
              onClick={() => tap(l)}
            >
              {t(l.labelKey)}
            </button>
          ))}
          {on === 'custom' && (
            <span className="theme-chip active" aria-disabled="true" title={t('settings.workspace.looks.custom.title')}>
              {t('settings.workspace.looks.custom')}
            </span>
          )}
        </div>
        {canUndo && (
          <button
            type="button"
            className="settings-linkbtn"
            disabled={disabled}
            title={t('settings.workspace.looks.undo.title')}
            onClick={() => {
              setAxes(undoTo!, now, set)
              setUndo(null)
            }}
          >
            {t('settings.workspace.looks.undo')}
          </button>
        )}
      </div>
      <span className="settings-hint">{t('settings.workspace.looks.hint')}</span>
    </div>
  )
}

/** The screen with `look`'s own settings put back as they were in `was`; every other setting as it
 *  is `now`, so an Undo never reverts a change made after the tap. */
function backTo(look: Look, was: LookAxes, now: LookAxes): LookAxes {
  const s = look.sets
  return {
    fieldMode: s.fieldMode !== undefined ? was.fieldMode : now.fieldMode,
    night: s.night !== undefined ? was.night : now.night,
    highContrast: s.highContrast !== undefined ? was.highContrast : now.highContrast,
    textSize: s.textSize !== undefined ? was.textSize : now.textSize,
    density: s.density !== undefined ? was.density : now.density,
  }
}

const sameAxes = (a: LookAxes, b: LookAxes) =>
  a.fieldMode === b.fieldMode &&
  a.night === b.night &&
  a.highContrast === b.highContrast &&
  a.textSize === b.textSize &&
  a.density === b.density
