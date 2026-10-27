// The JS8 panes' colours and fonts (features/js8PaneStyle), as JS8Call 3.0.3's UI settings hold them:
// what is read back from the settings, the custom properties the cockpit sets (only what was picked,
// for the theme in use), a pane's Reset, and the contrast check.
import { describe, expect, it } from 'vitest'
import {
  JS8_COLOR_TOKEN,
  JS8_PANE_COLORS,
  emptyJs8PaneStyle,
  js8Contrast,
  js8LowContrast,
  js8PaneVars,
  js8Rgb,
  parseJs8PaneStyle,
  resetJs8Pane,
  withJs8Color,
  withJs8Font,
} from './js8PaneStyle'

describe('what the settings hold', () => {
  it('reads each theme’s colours and the fonts back by value, and leaves out anything that is not one', () => {
    const s = parseJs8PaneStyle({
      light: { rxBg: '#FFEAA7', rxFg: 'red', cq: '#66ff66', wobble: '#000000', txFg: '#12345' },
      dark: { tableBg: '#101820' },
      fonts: {
        rx: { family: ' DejaVu Sans Mono ', sizePt: 11 },
        tx: { family: 'Evil", serif', sizePt: 10 },
        table: { family: '', sizePt: 200 },
        compose: { family: 'Consolas', sizePt: 9.5 },
      },
    })
    expect(s).toEqual({
      light: { rxBg: '#ffeaa7', cq: '#66ff66' },
      dark: { tableBg: '#101820' },
      fonts: { rx: { family: 'DejaVu Sans Mono', sizePt: 11 }, tx: { family: '', sizePt: 10 }, compose: { family: 'Consolas', sizePt: 0 } },
    })
    for (const junk of [null, undefined, 'x', [], { light: 'x', fonts: [] }]) expect(parseJs8PaneStyle(junk)).toEqual(emptyJs8PaneStyle())
  })
})

describe('the custom properties the cockpit sets', () => {
  const style = {
    light: { rxBg: '#ffeaa7', txFg: '#ff0000' },
    dark: { rxBg: '#1a1a1a', tableSelected: '#3498db' },
    fonts: { rx: { family: 'DejaVu Sans Mono', sizePt: 11 }, table: { family: 'monospace', sizePt: 0 }, compose: { family: '', sizePt: 12 } },
  }

  it('are only what was picked, for the theme the page paints', () => {
    expect(js8PaneVars(style, 'light')).toEqual({
      '--js8-rx-bg': '#ffeaa7',
      '--js8-tx-fg': '#ff0000',
      '--js8-rx-font-family': '"DejaVu Sans Mono", var(--font-mono)',
      '--js8-rx-font-size': 'calc(11pt * var(--text-scale, 1))',
      '--js8-table-font-family': 'monospace, var(--font-mono)',
      '--js8-compose-font-size': 'calc(12pt * var(--text-scale, 1))',
    })
    const dark = js8PaneVars(style, 'dark')
    expect([dark['--js8-rx-bg'], dark['--js8-table-selected'], dark['--js8-tx-fg']], 'a Light pick never paints Dark').toEqual(['#1a1a1a', '#3498db', undefined])
    expect(js8PaneVars(undefined, 'dark'), 'nothing picked: every pane follows the theme').toEqual({})
  })
})

describe('changing the style', () => {
  it('picks a colour for the theme being edited, gives one back, and sets or clears a font', () => {
    let s = withJs8Color(emptyJs8PaneStyle(), 'dark', 'cq', '#00AA00')
    expect(s).toEqual({ light: {}, dark: { cq: '#00aa00' }, fonts: {} })
    expect(withJs8Color(s, 'dark', 'cq', null).dark).toEqual({})
    expect(withJs8Color(s, 'dark', 'cq', 'green').dark, 'not a colour: given back').toEqual({})
    s = withJs8Font(s, 'compose', { family: 'Consolas', sizePt: 12 })
    expect(s.fonts).toEqual({ compose: { family: 'Consolas', sizePt: 12 } })
    expect(withJs8Font(s, 'compose', { family: '', sizePt: 0 }).fonts, 'a font that changes nothing is the theme’s').toEqual({})
  })

  it('Reset gives one pane back to the theme: its colours under the theme being edited, and its fonts', () => {
    const s = {
      light: { rxBg: '#ffeaa7', tableBg: '#ffffff' },
      dark: { rxBg: '#000000', txFg: '#ff0000', composeBg: '#202020' },
      fonts: { rx: { family: 'Consolas', sizePt: 0 }, tx: { family: '', sizePt: 12 }, compose: { family: '', sizePt: 9 } },
    }
    expect(resetJs8Pane(s, 'rx', 'dark')).toEqual({
      light: { rxBg: '#ffeaa7', tableBg: '#ffffff' },
      dark: { composeBg: '#202020' },
      fonts: { compose: { family: '', sizePt: 9 } },
    })
    expect(JS8_PANE_COLORS.rx, 'premise: the RX pane’s colours').toEqual(['rxBg', 'rxFg', 'txFg'])
  })
})

describe('the contrast check', () => {
  it('reads #rrggbb and the browser’s rgb(), never a see-through colour', () => {
    expect([js8Rgb('#FF0080'), js8Rgb('rgb(1, 2, 3)'), js8Rgb('rgba(1, 2, 3, 1)'), js8Rgb('rgb(1 2 3 / 100%)')]).toEqual([
      [255, 0, 128],
      [1, 2, 3],
      [1, 2, 3],
      [1, 2, 3],
    ])
    expect([js8Rgb('rgba(0, 0, 0, 0)'), js8Rgb('transparent'), js8Rgb('var(--text)'), js8Rgb('')]).toEqual([null, null, null, null])
  })

  it('is WCAG’s ratio', () => {
    expect(js8Contrast('#000000', '#ffffff')).toBeCloseTo(21, 5)
    expect(js8Contrast('#ffffff', '#ffffff')).toBeCloseTo(1, 5)
    expect(js8Contrast('#777777', '#ffffff')).toBeCloseTo(4.48, 2)
    expect(js8Contrast('#000000', 'transparent')).toBeNull()
  })
})

describe('a picked colour that would be hard to read', () => {
  // The theme's own colours, as Dark paints them (styles.css).
  const DARK: Record<string, string> = { '--bg': '#0b0f17', '--text': '#e7edf6', '--accent': '#4cc9f0', '--bg-elev-2': '#1a2231' }
  const token = (name: string) => DARK[name] ?? null

  it('is named with what it is on and the ratio, the theme’s colour standing in for the side not picked', () => {
    const s = { ...emptyJs8PaneStyle(), dark: { rxBg: '#ffeaa7' } }
    expect(js8LowContrast(s, 'dark', token)).toEqual([
      { pane: 'rx', fg: 'rxFg', bg: 'rxBg', ratio: expect.closeTo(1.015, 3) },
      { pane: 'rx', fg: 'txFg', bg: 'rxBg', ratio: expect.closeTo(1.61, 2) },
    ])
  })

  it('is nothing once the text is picked to read, nothing where nothing is picked, and never a pair it cannot read', () => {
    const s = { ...emptyJs8PaneStyle(), dark: { rxBg: '#ffeaa7', rxFg: '#000000', txFg: '#c00000' } }
    expect(js8LowContrast(s, 'dark', token)).toEqual([])
    expect(js8LowContrast(emptyJs8PaneStyle(), 'dark', token), 'the theme’s own pairs read').toEqual([])
    expect(js8LowContrast(emptyJs8PaneStyle(), 'dark', () => '#777777'), 'and are the theme’s business even where they would not').toEqual([])
    expect(js8LowContrast({ ...emptyJs8PaneStyle(), dark: { rxBg: '#ffeaa7' } }, 'dark', () => null), 'no theme colour to read: no guess').toEqual([])
    expect(js8LowContrast({ ...emptyJs8PaneStyle(), light: { rxBg: '#ffeaa7' } }, 'dark', token), 'a Light pick under Dark').toEqual([])
  })

  it('checks the lists’ text on every background it is drawn on that was picked, and the compose box', () => {
    const s = { ...emptyJs8PaneStyle(), dark: { tableFg: '#202020', cq: '#66ff66', primary: '#101010', composeBg: '#e7edf6' } }
    expect(js8LowContrast(s, 'dark', token).map((w) => `${w.fg}/${w.bg}`)).toEqual(['tableFg/tableBg', 'tableFg/tableSelected', 'tableFg/primary', 'composeFg/composeBg'])
  })

  it('never measures a row colour not picked: the theme draws it as a wash over the list’s background', () => {
    // A green text on the theme's dark background reads; the theme's CQ green is only a 16 % wash
    // under it, so measuring the green token itself would cry wolf.
    const withWash = (name: string) => (name === '--snr-strong' ? '#69d98d' : token(name))
    const s = { ...emptyJs8PaneStyle(), dark: { tableFg: '#70d090' } }
    expect(js8LowContrast(s, 'dark', withWash)).toEqual([])
    expect(js8LowContrast({ ...s, dark: { ...s.dark, cq: '#69d98d' } }, 'dark', withWash).map((w) => w.bg), 'control: picked, it is measured').toEqual(['cq'])
  })

  it('starts each colour from a theme token that both themes define', () => {
    expect(Object.keys(JS8_COLOR_TOKEN).sort()).toEqual(Object.values(JS8_PANE_COLORS).flat().sort())
  })
})
