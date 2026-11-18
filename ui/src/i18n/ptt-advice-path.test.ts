// THE REFUSED-PTT ADVICE NAMES THE SETTING AS THE SCREEN DOES, in every catalog. The TX strip's one-line
// alarm tells the operator where to look ("Settings ▸ Radio ▸ Rig & CAT"); a path whose words are not the
// ones on the tabs and the section's title sends them hunting for a tab that is not there. So each step
// is read off the catalog's own labels: the Settings window's title, the Radio tab, the Rig & CAT section.
import { describe, it, expect } from 'vitest'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { IT } from './it'
import { JA } from './ja'
import { RU } from './ru'
import { ZH } from './zh'
import { ZH_HANT } from './zh-hant'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = { en: EN, de: DE, es: ES, fr: FR, it: IT, ja: JA, ru: RU, zh: ZH, 'zh-hant': ZH_HANT }

describe('the refused-PTT advice names Settings ▸ Radio ▸ Rig & CAT in the screen’s own words', () => {
  it.each(Object.entries(CATALOGS))('%s', (_loc, c) => {
    const path = [c['settings.panel.title'], c['settings.tabs.radio'], c['settings.rigControl.legend']]
    for (const step of path) expect(typeof step, 'premise: the label exists').toBe('string')
    expect(c['shell.lane.audio.ptt.brief']).toContain(path.join(' ▸ '))
  })
})
