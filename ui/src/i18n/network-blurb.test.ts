// THE WIZARD'S NETWORK CHOICE SAYS WHAT SETTINGS' DOES. Settings ▸ Radio ▸ Rig & CAT ▸ Connection calls Network
// "host:port — SDR software, or a remote rig", and its hint names the SDR programs (Thetis, PowerSDR, SmartSDR CAT,
// piHPSDR) that serve CAT over TCP. The setup wizard's Rig step, and the picture of that step in the Getting Started
// guide, still said "FlexRadio / remote rigctld": an operator running Thetis for a Hermes Lite 2 read that Network was
// not for them, or reached for a FlexRadio model, which costs the S-meter.
//
// Read off all five catalogs: each blurb is the catalog's own words for the Network choice, and names no FlexRadio.
import { describe, it, expect } from 'vitest'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = { en: EN, de: DE, es: ES, fr: FR, ja: JA }
/** The wizard's Network blurb, and the guide's picture of it. */
const BLURBS = ['setup.rig.conn.network.blurb', 'gettingStarted.radio.shot.netBlurb'] as const

/** A blurb starts its line, so it capitalises the first letter where the Settings label's parenthesis does not. */
const upperFirst = (s: string) => s.charAt(0).toLocaleUpperCase() + s.slice(1)

describe('the setup wizard’s Network choice', () => {
  it.each(Object.entries(CATALOGS))('%s: is described in the words Settings uses, and names no FlexRadio', (_lang, cat) => {
    // "Network (host:port — SDR software, or a remote rig)" → "SDR software, or a remote rig".
    const label = String(cat['settings.rigControl.conn.network'])
    const words = /—\s*(.+?)\s*[)）]\s*$/.exec(label)?.[1]
    expect(words, label).toBeTruthy()
    for (const key of BLURBS) {
      const blurb = String(cat[key])
      expect(blurb, key).toBe(upperFirst(words!))
      expect(blurb, key).not.toMatch(/flex/i)
    }
  })
})
