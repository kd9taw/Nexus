// Settings ▸ Digital ▸ JS8 ▸ Colours and fonts: JS8Call 3.0.3's UI settings for its RX pane, its two
// lists and its compose box (Configuration ▸ UI: the Messages, Band && Call Activity and Composition
// tabs, Configuration.ui:3250-4060) — a free colour picker for each of JS8Call's colours and a font
// for each pane. Each colour starts as the theme's; what is picked here is kept for the theme the page
// paints now, and the other theme keeps its own, so a choice made readable on one theme never paints
// the other (features/js8PaneStyle). A sample of each pane, drawn by the pane's own rules, shows the
// result; a line under it says when a text would be hard to read on its background; and each pane has
// a Reset. Rendered inside the JS8 section with the panel's form, so a Save writes it with every other
// field.
//
// Nothing here transmits.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): the samples' calls, offsets and reports are
// data; every word is in the catalog.
import { useState, type CSSProperties } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'
import {
  JS8_COLOR_TOKEN,
  JS8_FONT_PT_MAX,
  JS8_FONT_PT_MIN,
  JS8_PANES,
  JS8_PANE_COLORS,
  JS8_PANE_FONTS,
  js8LowContrast,
  js8PaneVars,
  parseJs8PaneStyle,
  resetJs8Pane,
  withJs8Color,
  withJs8Font,
  type Js8ColorKey,
  type Js8FontKey,
  type Js8Pane,
  type Js8PaneStyle,
  type Js8Theme,
} from '../features/js8PaneStyle'

/** The samples' data: a received line, my own, a list row's offset and report, a compose line. */
const SAMPLE_TIME = '12:34:56'
const SAMPLE_OFFSET = '1500'
const SAMPLE_SNR = '-05'
const SAMPLE_RX = 'K1ABC: N0CALL SNR? ♢'
const SAMPLE_TX = 'N0CALL: K1ABC SNR -05 ♢'
const SAMPLE_COMPOSE = 'K1ABC SNR?'
/** CSS's generic families, offered beside a typed name. */
const GENERIC_FAMILIES = ['monospace', 'sans-serif', 'serif', 'system-ui']

/** Literal keys per colour and pane, so the orphan guard sees each referenced. */
function colorLabel(k: Js8ColorKey): string {
  switch (k) {
    case 'rxBg':
      return t('settings.js8.style.color.rxBg')
    case 'rxFg':
      return t('settings.js8.style.color.rxFg')
    case 'txFg':
      return t('settings.js8.style.color.txFg')
    case 'tableBg':
      return t('settings.js8.style.color.tableBg')
    case 'tableFg':
      return t('settings.js8.style.color.tableFg')
    case 'tableSelected':
      return t('settings.js8.style.color.tableSelected')
    case 'directed':
      return t('settings.js8.style.color.directed')
    case 'cq':
      return t('settings.js8.style.color.cq')
    case 'primary':
      return t('settings.js8.style.color.primary')
    case 'secondary':
      return t('settings.js8.style.color.secondary')
    case 'composeBg':
      return t('settings.js8.style.color.composeBg')
    case 'composeFg':
      return t('settings.js8.style.color.composeFg')
  }
}
function fontLabel(k: Js8FontKey): string {
  switch (k) {
    case 'rx':
      return t('settings.js8.style.font.rx')
    case 'tx':
      return t('settings.js8.style.font.tx')
    case 'table':
      return t('settings.js8.style.font.table')
    case 'compose':
      return t('settings.js8.style.font.compose')
  }
}
function paneTitle(p: Js8Pane): string {
  switch (p) {
    case 'rx':
      return t('settings.js8.style.pane.rx')
    case 'lists':
      return t('settings.js8.style.pane.lists')
    case 'compose':
      return t('settings.js8.style.pane.compose')
  }
}

/** A theme token's value as the page paints it now, if it is a #rrggbb (styles.css writes them so). */
function themeColor(name: string): string | null {
  try {
    const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
    return /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : null
  } catch {
    return null
  }
}

/** One pane's sample, drawn by the pane's own rules (styles.css) with what is picked. */
function Sample({ pane, vars }: { pane: Js8Pane; vars: CSSProperties }) {
  if (pane === 'rx') {
    return (
      <div className="js8-activity js8-rx js8-style-sample" style={vars} aria-hidden="true">
        <div className="js8-row js8-rx-line">
          <span className="js8-cell js8-time">{SAMPLE_TIME}</span>
          <span className="js8-cell js8-freq">{SAMPLE_OFFSET}</span>
          <span className="js8-cell js8-text">{SAMPLE_RX}</span>
        </div>
        <div className="js8-row js8-rx-line mine">
          <span className="js8-cell js8-time">{SAMPLE_TIME}</span>
          <span className="js8-cell js8-freq">{SAMPLE_OFFSET}</span>
          <span className="js8-cell js8-text">{SAMPLE_TX}</span>
        </div>
      </div>
    )
  }
  if (pane === 'lists') {
    // A row for the lists' text and one for each row colour, each named by its control.
    const rows: [string, Js8ColorKey][] = [
      ['', 'tableFg'],
      ['selected', 'tableSelected'],
      ['directed', 'directed'],
      ['cq', 'cq'],
      ['hl-primary', 'primary'],
      ['hl-secondary', 'secondary'],
    ]
    return (
      <div className="js8-offsets js8-style-sample" style={vars} aria-hidden="true">
        <table className="js8-table js8-band-table">
          <tbody>
            {rows.map(([cls, key]) => (
              <tr key={key} className={`js8-offset-row${cls ? ` ${cls}` : ''}`}>
                <td className="js8-cell js8-freq">{SAMPLE_OFFSET}</td>
                <td className="js8-cell js8-snr">{SAMPLE_SNR}</td>
                <td className="js8-cell js8-text">{colorLabel(key)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    )
  }
  return (
    <span className="js8-compose-wrap js8-style-sample" style={vars} aria-hidden="true">
      <input className="settings-input cw-type js8-compose" value={SAMPLE_COMPOSE} readOnly disabled tabIndex={-1} />
    </span>
  )
}

/** One font: a family (typed, or one of CSS's generic ones) and a size in points; empty is the theme's. */
function FontRow({
  pane,
  k,
  font,
  onFont,
}: {
  pane: string
  k: Js8FontKey
  font: { family: string; sizePt: number }
  onFont: (f: { family: string; sizePt: number }) => void
}) {
  const label = fontLabel(k)
  const [family, setFamily] = useState<string | null>(null)
  const [size, setSize] = useState<string | null>(null)
  return (
    <div className="settings-field js8-style-font">
      <span className="settings-label">{label}</span>
      <input
        className="settings-input js8-style-family"
        aria-label={t('settings.js8.style.family.aria', { pane, font: label })}
        placeholder={t('settings.js8.style.family.placeholder')}
        list="js8-style-families"
        value={family ?? font.family}
        onChange={(e) => {
          setFamily(e.target.value)
          onFont({ ...font, family: e.target.value })
        }}
        onBlur={() => setFamily(null)}
        autoComplete="off"
        spellCheck={false}
      />
      <input
        className="settings-input js8-style-size"
        type="number"
        min={JS8_FONT_PT_MIN}
        max={JS8_FONT_PT_MAX}
        step={1}
        aria-label={t('settings.js8.style.size.aria', { pane, font: label })}
        placeholder={t('settings.js8.style.size.placeholder')}
        value={size ?? (font.sizePt ? String(font.sizePt) : '')}
        onChange={(e) => {
          setSize(e.target.value)
          const n = Number(e.target.value)
          onFont({ ...font, sizePt: e.target.value.trim() === '' || !Number.isInteger(n) ? 0 : n })
        }}
        onBlur={() => setSize(null)}
      />
    </div>
  )
}

export function Js8PaneStyleSettings({
  form,
  theme,
  onChange,
}: {
  form: Settings
  /** The theme the page paints now: the one whose colours are edited here. */
  theme: Js8Theme
  onChange: (style: Js8PaneStyle) => void
}) {
  const style = parseJs8PaneStyle(form.js8PaneStyle)
  const picked = style[theme]
  const vars = js8PaneVars(style, theme) as CSSProperties
  const themeName = theme === 'light' ? t('theme.light.label') : t('theme.dark.label')
  const low = js8LowContrast(style, theme, themeColor)
  return (
    <div className="settings-featgroup js8-style">
      <span className="settings-featgroup-title">{t('settings.js8.style.title')}</span>
      <span className="settings-hint">{t('settings.js8.style.hint', { theme: themeName })}</span>
      <datalist id="js8-style-families">
        {GENERIC_FAMILIES.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
      {JS8_PANES.map((pane) => (
        <div className="js8-style-pane" key={pane} data-pane-style={pane}>
          <div className="js8-style-head">
            <span className="settings-label js8-style-title">{paneTitle(pane)}</span>
            <button
              type="button"
              className="settings-refresh js8-style-reset"
              title={t('settings.js8.style.reset.title', { theme: themeName })}
              aria-label={t('settings.js8.style.reset.aria', { pane: paneTitle(pane) })}
              onClick={() => onChange(resetJs8Pane(style, pane, theme))}
            >
              {t('settings.js8.style.reset.label')}
            </button>
          </div>
          <Sample pane={pane} vars={vars} />
          <div className="js8-style-colors">
            {JS8_PANE_COLORS[pane].map((k) => (
              <label className="settings-field js8-style-color" key={k}>
                <input
                  type="color"
                  aria-label={t('settings.js8.style.color.aria', { pane: paneTitle(pane), color: colorLabel(k) })}
                  value={picked[k] ?? themeColor(JS8_COLOR_TOKEN[k]) ?? ''}
                  onChange={(e) => onChange(withJs8Color(style, theme, k, e.target.value))}
                />
                <span className="settings-label">{colorLabel(k)}</span>
              </label>
            ))}
          </div>
          {JS8_PANE_FONTS[pane].map((k) => (
            <FontRow
              key={k}
              pane={paneTitle(pane)}
              k={k}
              font={style.fonts[k] ?? { family: '', sizePt: 0 }}
              onFont={(f) => onChange(withJs8Font(style, k, f))}
            />
          ))}
          {low
            .filter((w) => w.pane === pane)
            .map((w) => (
              <span className="settings-hint js8-style-low" role="status" key={`${w.fg}-${w.bg}`}>
                {t('settings.js8.style.lowContrast', { text: colorLabel(w.fg), background: colorLabel(w.bg), ratio: w.ratio.toFixed(1) })}
              </span>
            ))}
        </div>
      ))}
    </div>
  )
}
