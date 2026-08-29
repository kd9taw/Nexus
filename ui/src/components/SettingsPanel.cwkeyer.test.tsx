// @vitest-environment jsdom
//
// Field report 2026-08 (N0UMF, IC-7410 behind a Timewave Navigator). Two things in this
// panel sent that operator down a road with no keying at the end of it:
//
//  1. The WinKeyer port box rendered unconditionally while the serial-keyline port and line
//     rendered only for their backend. So the ONLY visible port box under "Keyer" belonged
//     to a backend that was not selected — fill it in, Save, and nothing keys, because
//     `cwKeyer` is still the default `cat`. It is now gated like its siblings.
//  2. The keyline hint offered "US Navigator" as an example of a DTR keying interface. The
//     Navigator keys through a K1EL WinKey micro, which ignores DTR entirely — the hint
//     pointed a Navigator owner at the one backend that cannot drive his hardware.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
    <SettingsPanel
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

/** Open Modes ▸ CW with the given keyer backend selected. */
async function openCw(cwKeyer: string) {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', cwKeyer } as never),
  )
  renderPanel()
  // CW is its own tab since the eleven per-mode fieldsets were split into Phone · CW · Digital;
  // a CW operator no longer scrolls past a ~660-line FT8 fieldset and six weak-signal tiers to
  // reach their keyer.
  fireEvent.click(await screen.findByRole('tab', { name: 'CW' }))
  // Unconditional on this tab — wait for it before asserting anything is absent.
  await screen.findByText('Keyer backend')
}

describe('the port boxes under Keyer belong to the selected backend', () => {
  it('CAT keying offers no port box at all — the rig is the keyer', async () => {
    await openCw('cat')
    expect(screen.queryByText('WinKeyer port')).toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('the soundcard keyer offers none either', async () => {
    await openCw('soundcard')
    expect(screen.queryByText('WinKeyer port')).toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('WinKeyer shows its port and only its port', async () => {
    await openCw('winkeyer')
    expect(screen.queryByText('WinKeyer port')).not.toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('the serial keyline shows its port and line, and not the WinKeyer box', async () => {
    await openCw('serial')
    expect(screen.queryByText('Keyline serial port')).not.toBeNull()
    expect(screen.queryByText('Keying line')).not.toBeNull()
    expect(screen.queryByText('WinKeyer port')).toBeNull()
  })
})

describe('the keyline hint does not send WinKey owners to the DTR backend', () => {
  const keylineHint = () =>
    screen.getByText(/USB-to-serial into your keying interface/).textContent ?? ''

  it('stops offering the Navigator as an example of a DTR keying interface', async () => {
    await openCw('serial')
    const examples = keylineHint().match(/keying interface \(([^)]*)\)/)?.[1] ?? ''
    expect(examples.length).toBeGreaterThan(0) // the list still exists…
    expect(examples).not.toMatch(/Navigator/i) // …and no longer claims this one
  })

  it('names where a WinKey-based interface actually goes', async () => {
    // Deleting the word would leave a Navigator owner exactly as stuck. The hint has to
    // route him, by name, to the backend that can drive the thing.
    await openCw('serial')
    const hint = keylineHint()
    expect(hint).toMatch(/Navigator/i)
    expect(hint).toMatch(/does\s+not\s+key on DTR/i)
    expect(hint).toMatch(/WinKeyer backend/i)
  })
})

/**
 * ⭐ FIELD REPORT 2026-08-28 (Yaesu FTX-1, via KR4FQG): "Touch Tune and TX is ok. Try send a cw,
 * never went to tx." PTT and the CAT link were both alive; CW keying specifically did nothing,
 * and the screen said NOTHING — because it cannot. The FTX-1's own Hamlib backend emits a
 * different `send_morse` wire form from the newcat one every other modern Yaesu uses, and it
 * returns `RPRT 0` whether or not the radio keyed, so Nexus's only evidence says "fine".
 *
 * A detector we cannot honestly build is replaced by a caution the operator can read BEFORE he
 * loses an evening. Three rules, one test each: it appears on the flagged model, it stays away
 * from every other radio, and it never appears for a keyer he did not choose.
 */
describe('CAT keying that cannot report its own failure says so up front', () => {
  /** Open Modes ▸ CW with a keyer AND a rig model, with the backend's flag list stubbed. */
  async function openCwOnRig(cwKeyer: string, rigModel: number, flagged: number[] = [1051]) {
    api.get('getCatCwUnprovenRigModels').mockImplementation(() => Promise.resolve(flagged))
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        mycall: 'KD9TAW',
        mygrid: 'EN52',
        cwKeyer,
        rigModel,
      } as never),
    )
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'CW' }))
    await screen.findByText('Keyer backend')
  }
  const caution = () => screen.queryByText(/unproven on this radio/i)

  it('warns on the FTX-1 when the CAT keyer is selected', async () => {
    await openCwOnRig('cat', 1051)
    await screen.findByText(/unproven on this radio/i)
    // It must ROUTE him, not just worry him — the three keyers that do work, by name.
    const text = caution()?.closest('span')?.textContent ?? ''
    expect(text).toMatch(/Serial keyline/i)
    expect(text).toMatch(/WinKeyer/i)
    expect(text).toMatch(/Soundcard/i)
  })

  it('stays silent on a rig whose CAT keyer works — no crying wolf', async () => {
    await openCwOnRig('cat', 1042) // FTDX10: the newcat two-command form, proven
    expect(caution()).toBeNull()
  })

  it('stays silent when the operator picked a different keyer on the same radio', async () => {
    // An FTX-1 owner running a WinKeyer must not be nagged about a backend he is not using.
    await openCwOnRig('winkeyer', 1051)
    expect(caution()).toBeNull()
  })

  it('shows nothing when the rule could not be read', async () => {
    // Built without the radio feature / the command failed: an unreadable rule must never
    // warn an operator off a keyer that works for him.
    await openCwOnRig('cat', 1051, [])
    expect(caution()).toBeNull()
  })

  /**
   * THE COPY MUST NOT NAME A MANUFACTURER, and this is a catalog check rather than a render
   * check because that is where it would rot. The flag list is keyed on rig MODEL
   * (`rigmodels::cat_cw_unproven_rig_models`); today's single entry happening to be a Yaesu is
   * a fact about the entry, not about the rule. Copy that says "other Yaesus" becomes a lie the
   * moment a non-Yaesu is flagged — and nothing would catch it, because the sentence lives in
   * four TypeScript catalogs and the list lives in Rust, so the two drift in silence.
   */
  it('names no manufacturer — the flag list is by model, not by make', async () => {
    const makes = /yaesu|icom|kenwood|elecraft|flex(radio)?|xiegu|alinco|ten-?tec/i

    // The control, and it must come first: a regex that finds nothing proves nothing until it
    // is shown to find something. The `settings.cw.keyer.cat` option DOES name hardware.
    expect(makes.test('sends a different command from other Yaesus'), 'the detector is dead').toBe(
      true,
    )

    for (const [lang, cat] of [
      ['en', EN],
      ['de', DE],
      ['es', ES],
      ['fr', FR],
    ] as const) {
      const s = (cat as Record<string, unknown>)['settings.cw.keyer.unproven']
      expect(typeof s, `${lang} has no unproven caution at all`).toBe('string')
      // The second control: the sentence must still carry the clause the operator deliberately
      // kept, so this cannot pass by the string having been emptied or gutted.
      expect(s as string, `${lang} lost the Hamlib clause`).toMatch(/Hamlib/)
      expect(s as string, `${lang} names a make in a caution the list applies by model`).not.toMatch(
        makes,
      )
    }

    // …and the rendered surface agrees with the catalog it was read from.
    await openCwOnRig('cat', 1051)
    const text = (await screen.findByText(/unproven on this radio/i)).closest('span')?.textContent ?? ''
    expect(text).toMatch(/Hamlib/)
    expect(text).not.toMatch(makes)
  })
})

/**
 * ⭐ THE SAME FIELD REPORT, third symptom: "Soundcard activated caused radio to switch from CW-U
 * to USB, and had to manually change it back."
 *
 * The mode change is DELIBERATE — a keyed audio tone cannot be sent in CW, so the rig has to be
 * moved to the SSB side (as a data mode, so the tone reaches the transmitter and not the mic
 * jack). What was missing is that nobody told him. The picker offered "audio tone through SSB —
 * a workaround" and never mentioned that his radio would leave CW.
 *
 * Copy only, and it has to say both halves: that it LEAVES CW, and that CW COMES BACK — an
 * operator who thinks the change is permanent will not try the keyer at all.
 */
describe('the soundcard keyer says that it takes the radio out of CW', () => {
  const keyerHint = () => screen.getByText(/How Nexus sends CW/).textContent ?? ''

  it('warns that the radio leaves CW, and says CW comes back', async () => {
    await openCw('soundcard')
    const hint = keyerHint()
    expect(hint).toMatch(/takes your radio out of CW/i)
    expect(hint).toMatch(/data mode/i)
    expect(hint).toMatch(/comes straight back|comes back/i)
  })

  it('the option label no longer promises plain SSB', async () => {
    // "audio tone through SSB" described the old behaviour AND read as harmless. The rig is put
    // into a DATA submode now, which is the thing an operator sees change on his front panel.
    await openCw('cat')
    const option = screen.getByRole('option', { name: /Soundcard/ })
    expect(option.textContent).toMatch(/data mode/i)
  })
})
