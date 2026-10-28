import { describe, it, expect, beforeEach } from 'vitest'

// profiles persist through localStorage; this suite runs in the default node
// environment (no jsdom), so install a minimal in-memory localStorage shim.
class MemoryStorage {
  private m = new Map<string, string>()
  get length() { return this.m.size }
  clear() { this.m.clear() }
  getItem(k: string) { return this.m.has(k) ? (this.m.get(k) as string) : null }
  key(i: number) { return [...this.m.keys()][i] ?? null }
  removeItem(k: string) { this.m.delete(k) }
  setItem(k: string, v: string) { this.m.set(k, String(v)) }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore

import {
  loadProfiles,
  mergeProfile,
  saveProfile,
  deleteProfile,
  PROFILE_SCHEMA,
  type Profile,
} from './profiles'
import type { Settings } from './types'

// A minimal Settings stub — profiles snapshot the whole object verbatim, so the test
// only needs a couple of distinguishing fields.
const settings = (over: Partial<Settings>): Settings =>
  ({ mycall: 'KD9TAW', serialPort: 'COM1', phoneMode: 'ssb', ...over }) as Settings

describe('config profiles', () => {
  beforeEach(() => memStore.clear())

  it('starts empty and tolerates corrupt storage', () => {
    expect(loadProfiles()).toEqual([])
    localStorage.setItem('nexus.profiles', '{not json')
    expect(loadProfiles()).toEqual([])
    localStorage.setItem('nexus.profiles', '{"not":"an array"}')
    expect(loadProfiles()).toEqual([])
  })

  it('saves and reloads a named snapshot', () => {
    saveProfile('Portable VHF', settings({ serialPort: 'COM7', phoneMode: 'fm' }))
    const list = loadProfiles()
    expect(list).toHaveLength(1)
    expect(list[0].name).toBe('Portable VHF')
    expect(list[0].settings.serialPort).toBe('COM7')
    expect(list[0].settings.phoneMode).toBe('fm')
  })

  it('upserts by name and keeps the list name-sorted', () => {
    saveProfile('Home HF', settings({ serialPort: 'COM3' }))
    saveProfile('Field Day', settings({ serialPort: 'COM5' }))
    saveProfile('Home HF', settings({ serialPort: 'COM9' })) // overwrite, not duplicate
    const list = loadProfiles()
    expect(list.map((p: Profile) => p.name)).toEqual(['Field Day', 'Home HF'])
    expect(list.find((p) => p.name === 'Home HF')!.settings.serialPort).toBe('COM9')
  })

  it('ignores a blank name and deletes by name', () => {
    saveProfile('  ', settings({}))
    expect(loadProfiles()).toEqual([])
    saveProfile('A', settings({}))
    saveProfile('B', settings({}))
    deleteProfile('A')
    expect(loadProfiles().map((p) => p.name)).toEqual(['B'])
  })

  it('stamps the schema on save', () => {
    saveProfile('Home HF', settings({}))
    expect(loadProfiles()[0].schema).toBe(PROFILE_SCHEMA)
  })
})

// The three-week window that made merge-loading real: profiles saved
// 2026-06-30…07-22 predate the per-mode power ceilings, and the raw replay let
// serde's absent-key-takes-default turn "no key" into "NO CAP" at 100% digital
// duty — the one audit finding with a hardware-damage path.
describe('mergeProfile — the profile-load contract', () => {
  const current = {
    mycall: 'KD9TAW',
    licenseClass: 'general',
    maxPowerDigital: 0.25,
    maxPowerPhone: 0.8,
    band: '20m',
    dialMhz: 14.074,
    activeRadio: 1,
    radios: [{ id: 0 }, { id: 1 }],
    qrzLastSyncUnix: 1_753_000_000,
    lotwLastQsl: '2025-01-15',
  } as unknown as Settings

  it('a key the profile predates keeps the CURRENT value — never the default', () => {
    // An old (pre-ceiling) profile: no maxPower* keys at all.
    const old = { band: '80m', dialMhz: 3.573 } as unknown as Settings
    const merged = mergeProfile(current, old) as unknown as Record<string, unknown>
    expect(merged.maxPowerDigital).toBe(0.25) // the safety cap SURVIVES the load
    expect(merged.maxPowerPhone).toBe(0.8)
    expect(merged.band).toBe('80m') // what the profile does carry applies
    expect(merged.dialMhz).toBe(3.573)
  })

  it('identity, license class, the radio roster and sync cursors never import', () => {
    const foreign = {
      mycall: 'N0CALL',
      licenseClass: 'open',
      radios: [{ id: 9 }],
      activeRadio: 9,
      qrzLastSyncUnix: 0,
      band: '40m',
    } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.mycall).toBe('KD9TAW')
    expect(merged.licenseClass).toBe('general') // the TX-safety gate stays yours
    expect(merged.activeRadio).toBe(1)
    expect((merged.radios as { id: number }[])[0].id).toBe(0)
    expect(merged.qrzLastSyncUnix).toBe(1_753_000_000)
    expect(merged.band).toBe('40m')
  })

  it('the LoTW download cursor never imports either', () => {
    // Same class as `qrzLastSyncUnix` and `eqslLastSync`, and it was the one missing from the
    // list. A cursor says "I have already fetched everything up to here" — true of the machine
    // that wrote the profile, not of this one. Importing a NEWER stamp makes Nexus skip the
    // confirmations between the two, and they never come back on their own: the next sync asks
    // only for records after the borrowed date. Silent, and it costs award credit the operator
    // has already earned.
    const foreign = { lotwLastQsl: '2026-08-01', band: '40m' } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.lotwLastQsl).toBe('2025-01-15')
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  it('the experience answer and the setup wizard record are the operator\'s, never a profile\'s', () => {
    // Who the operator is (how much they have done) and whether this station's setup wizard was
    // done: the licence class's family. A profile from a "new to the hobby" station must not fold
    // this one's Settings, nor bring the first-run wizard back.
    const mine = { ...current, experienceLevel: 'experienced', setupWizardSeen: true } as unknown as Settings
    const foreign = { experienceLevel: 'newToHobby', setupWizardSeen: false, band: '40m' } as unknown as Settings
    const merged = mergeProfile(mine, foreign) as unknown as Record<string, unknown>
    expect(merged.experienceLevel).toBe('experienced')
    expect(merged.setupWizardSeen).toBe(true)
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  it('nor does the mark that says a LoTW download of every call made the cursor', () => {
    // The mark belongs to the cursor beside it. Imported onto an older build's cursor (a download
    // of the Settings call alone made it), it would let the next sync ask from that cursor, and
    // skip for good every confirmation of the operator's other calls matched before it.
    const older = { ...current, lotwCursorEveryCall: false } as unknown as Settings
    const foreign = { lotwCursorEveryCall: true, band: '40m' } as unknown as Settings
    const merged = mergeProfile(older, foreign) as unknown as Record<string, unknown>
    expect(merged.lotwCursorEveryCall).toBe(false)
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  it('a profile value the operator set DOES override the current one', () => {
    const p = { maxPowerDigital: 0.5 } as unknown as Settings
    const merged = mergeProfile(current, p) as unknown as Record<string, unknown>
    expect(merged.maxPowerDigital).toBe(0.5)
  })
})

// Round 2, defect 3. `confirmSatUplink` (features/satVfo.ts) is THE
// uplink-mapping writer — the retire-on-change rule lives there, in both
// languages. `mergeProfile` was a third writer: a profile saved by 0.25.0
// carries `satVfoMap` and no `satUplinkRadios`, so loading it re-pointed the
// mapping while every standing confirmation survived — a radio left listed as
// confirmed for a layout it never confirmed, the exact state the rule exists
// to make unreachable. And a post-0.26 profile carries `satUplinkRadios` (raw
// `RadioProfile.id`s) while `radios`/`activeRadio` — the ids' meaning — are
// declared machine-local and refused. The pair is machine wiring, exactly like
// the roster it describes: neither key rides a profile.
describe('mergeProfile — the satellite uplink pair never imports', () => {
  const current = {
    mycall: 'KD9TAW',
    satVfoMap: 'main-down-sub-up',
    satUplinkRadios: [0],
    band: '20m',
  } as unknown as Settings

  it('a pre-0.26 profile cannot re-point the mapping under standing confirmations', () => {
    const old = { satVfoMap: 'a-up-b-down', band: '70cm' } as unknown as Settings
    const merged = mergeProfile(current, old) as unknown as Record<string, unknown>
    expect(merged.satVfoMap).toBe('main-down-sub-up')
    expect(merged.satUplinkRadios).toEqual([0])
    expect(merged.band).toBe('70cm') // the rest of the profile still applies
  })

  it('a post-0.26 profile cannot import consent ids for a roster it did not bring', () => {
    const foreign = {
      satVfoMap: 'main-down-sub-up',
      satUplinkRadios: [2], // id 2 on the SAVING machine — a different rig here
    } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.satUplinkRadios).toEqual([0])
  })
})

// A config profile is a whole-station snapshot, and the update channel is not part of a
// station: it is a per-machine, per-operator preference about which builds this box installs.
// A beta tester who loads a profile saved before they opted in would be returned to the
// stable channel with no error, no log line and no way to notice — the betas simply stop
// arriving. Same family as `mycall` and `licenseClass`: it describes the operator, not the rig.
describe('mergeProfile — the beta-channel opt-in never imports', () => {
  const current = { mycall: 'KD9TAW', betaUpdates: true, band: '20m' } as unknown as Settings

  it('a profile saved before the operator opted in cannot switch the channel back', () => {
    const old = { betaUpdates: false, band: '40m' } as unknown as Settings
    const merged = mergeProfile(current, old) as unknown as Record<string, unknown>
    expect(merged.betaUpdates).toBe(true)
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  it('nor can a profile opt a stable-channel operator IN', () => {
    const stable = { mycall: 'KD9TAW', betaUpdates: false, band: '20m' } as unknown as Settings
    const beta = { betaUpdates: true, band: '40m' } as unknown as Settings
    const merged = mergeProfile(stable, beta) as unknown as Record<string, unknown>
    expect(merged.betaUpdates).toBe(false)
    expect(merged.band).toBe('40m')
  })
})

// #396, and the operator's ruling: a profile never touches a logbook upload switch. A profile is
// the station (rig, antenna, CAT, bands), and where your contacts are sent is not part of it.
// Left importable, loading a profile saved while QRZ upload was on turned it back on after the
// operator had switched it off and forgotten the key, so every contact went to QRZ before it
// could be corrected. The other direction is as quiet: a profile saved before an upload was set
// up switched it off again, and contacts stopped arriving with no error.
describe('mergeProfile — the logbook upload switches never import', () => {
  it('a profile saved with QRZ upload on leaves the current switch off (#396)', () => {
    const current = { mycall: 'KD9TAW', qrzLogbookUpload: false, band: '20m' } as unknown as Settings
    const saved = { qrzLogbookUpload: true, band: '40m' } as unknown as Settings
    const merged = mergeProfile(current, saved) as unknown as Record<string, unknown>
    expect(merged.qrzLogbookUpload).toBe(false)
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  // Every upload switch under Settings ▸ Logging & Connectors ▸ Confirmations, both ways round.
  it.each([
    'qrzLogbookUpload',
    'clublogUpload',
    'eqslUpload',
    'hrdlogUpload',
    'wrlUpload',
    'cloudlogUpload',
    'lotwAutoUpload',
  ])('%s keeps its current value, on or off', (key) => {
    for (const now of [true, false]) {
      const current = { mycall: 'KD9TAW', [key]: now, band: '20m' } as unknown as Settings
      const saved = { [key]: !now, band: '40m' } as unknown as Settings
      const merged = mergeProfile(current, saved) as unknown as Record<string, unknown>
      expect(merged[key]).toBe(now)
      expect(merged.band).toBe('40m')
    }
  })
})

// The pushes to the other logging programs on your network are the same family, and the
// operator's ruling leaves them alone too. A profile saved before HRD Logbook forwarding was set
// up stopped it with no error; one saved with it on restarted it beside a JTAlert relay into HRD,
// and every contact was logged there twice. DXKeeper's own upload switch, turned back on, sent
// every contact to LoTW, eQSL, ClubLog and QRZ a second time.
describe('mergeProfile — the local logger pushes never import', () => {
  // Every push switch in the HRD (Integrations & Feeds), DXKeeper, N3FJP and N1MM+ sections.
  it.each(['hrdLogging', 'dxkeeperUploads', 'n3fjpUpload', 'n1mmUpload'])(
    '%s keeps its current value, on or off',
    (key) => {
      for (const now of [true, false]) {
        const current = { mycall: 'KD9TAW', [key]: now, band: '20m' } as unknown as Settings
        const saved = { [key]: !now, band: '40m' } as unknown as Settings
        const merged = mergeProfile(current, saved) as unknown as Record<string, unknown>
        expect(merged[key]).toBe(now)
        expect(merged.band).toBe('40m')
      }
    },
  )

  // DXKeeper's push has no switch of its own: an empty host is off, so the host IS its on/off.
  // A profile saved before DXKeeper was set up blanked it and the push stopped with no error; one
  // saved with it set started sending every contact to DXKeeper after the operator had cleared it.
  it("DXKeeper's host keeps its current value, set or blank", () => {
    for (const [now, saved] of [['127.0.0.1', ''], ['', '127.0.0.1']]) {
      const current = { mycall: 'KD9TAW', dxkeeperHost: now, band: '20m' } as unknown as Settings
      const profile = { dxkeeperHost: saved, band: '40m' } as unknown as Settings
      const merged = mergeProfile(current, profile) as unknown as Record<string, unknown>
      expect(merged.dxkeeperHost).toBe(now)
      expect(merged.band).toBe('40m')
    }
  })

  // The rest stay with the profile: N3FJP's and N1MM+'s addresses (a Field Day profile carries the
  // club's master-log address) and the WSJT-X UDP API, a feed other programs listen to.
  it('the other logger addresses and the WSJT-X UDP API still come from a profile', () => {
    const current = {
      mycall: 'KD9TAW',
      n3fjpHost: '',
      n1mmAddr: '',
      wsjtxUdp: false,
    } as unknown as Settings
    const saved = {
      n3fjpHost: '192.168.1.10',
      n1mmAddr: '127.0.0.1:12060',
      wsjtxUdp: true,
    } as unknown as Settings
    const merged = mergeProfile(current, saved) as unknown as Record<string, unknown>
    expect(merged.n3fjpHost).toBe('192.168.1.10')
    expect(merged.n1mmAddr).toBe('127.0.0.1:12060')
    expect(merged.wsjtxUdp).toBe(true)
  })
})

// CONTEST MODE KEPT EVERY STORED KEY. The switch was renamed from "Field Day mode" in words only:
// the setting is still `fdActive`, and the contest's setup keeps its `fd*` and `contest*` keys. So
// a profile saved before the rename loads with the switch and that setup as they were, by value.
// The keys are the wire's own, as `components/__fixtures__/defaultSettings.json` holds them.
describe('a profile saved before Contest mode was named loads unchanged', () => {
  const SAVED_BEFORE = {
    fdActive: true,
    fdEvent: 'ilqp',
    fdClass: '2A',
    fdSection: 'IL',
    fdPowerMult: 5,
    fdBonuses: ['media'],
    fdOperator: 'K9ABC',
    contestQthState: 'IL',
    contestQthCounty: 'COOK',
    contestEsmCw: true,
    contestEsmRtty: true,
    contestEsmPhone: false,
    unassistedMode: true,
  }

  it('the switch and the contest’s setup round-trip through save, load and merge', () => {
    memStore.clear()
    saveProfile('ILQP club', settings(SAVED_BEFORE as unknown as Partial<Settings>))
    const loaded = loadProfiles().find((p) => p.name === 'ILQP club')!
    const current = settings({
      fdActive: false,
      fdEvent: 'arrlfd',
      fdClass: '',
      fdSection: '',
      fdPowerMult: 2,
    } as unknown as Partial<Settings>)
    const merged = mergeProfile(current, loaded.settings) as unknown as Record<string, unknown>
    for (const [key, value] of Object.entries(SAVED_BEFORE)) expect(merged[key], key).toEqual(value)
  })

  it('CONTROL: a value saved under any other name would not load, which is what keeping the keys prevents', () => {
    const renamed = { contestMode: true } as unknown as Settings
    const merged = mergeProfile(settings({ fdActive: false } as unknown as Partial<Settings>), renamed)
    expect((merged as unknown as Record<string, unknown>).fdActive).toBe(false)
  })
})

// JS8Call's two send options travel with a profile as the rest of a station's JS8 settings do: a
// profile that sets them loads them, and one saved before they existed leaves the current choice.
describe('mergeProfile — the JS8 send options travel with a profile', () => {
  it('loads Hold PTT and the TX offset while sending from a profile, and keeps them over an older one', () => {
    const current = settings({ js8HoldPtt: false, js8TxOffsetWhileSending: false })
    const merged = mergeProfile(current, settings({ js8HoldPtt: true, js8TxOffsetWhileSending: true }))
    expect([merged.js8HoldPtt, merged.js8TxOffsetWhileSending]).toEqual([true, true])
    const older = mergeProfile(settings({ js8HoldPtt: true, js8TxOffsetWhileSending: true }), settings({}))
    expect([older.js8HoldPtt, older.js8TxOffsetWhileSending], 'a profile from before them').toEqual([true, true])
  })

  it('loads the RTTY AFSK sideband from a profile, and keeps it over a profile from before it', () => {
    expect(mergeProfile(settings({ rttyAfskSideband: 'lsb' }), settings({ rttyAfskSideband: 'usb' })).rttyAfskSideband).toBe('usb')
    expect(mergeProfile(settings({ rttyAfskSideband: 'usb' }), settings({})).rttyAfskSideband, 'a profile from before it').toBe('usb')
  })

  it('loads the TX delay from a profile, and keeps it over a profile from before it', () => {
    expect(mergeProfile(settings({ js8TxDelayMs: 200 }), settings({ js8TxDelayMs: 400 })).js8TxDelayMs).toBe(400)
    expect(mergeProfile(settings({ js8TxDelayMs: 300 }), settings({})).js8TxDelayMs, 'a profile from before it').toBe(300)
  })
})

// A profile saved on a computer driving the station holds that read's notes about the station
// (which settings are set there, which keys are saved there). They are not settings: loaded at
// the station, they would mark its own form "Set at the station".
describe('a profile saved from afar loads at the station without the read’s notes', () => {
  it('keeps the current read’s notes, whatever the profile carries', () => {
    const saved = settings({
      stationOnlySettings: ['txWatchdogMin'],
      secretsSavedAtStation: { clublogApiKey: true },
      mygrid: 'EN61',
    })
    const merged = mergeProfile(settings({ mygrid: 'EN52' }), saved)
    expect(merged.stationOnlySettings).toBeUndefined()
    expect(merged.secretsSavedAtStation).toBeUndefined()
    expect(merged.mygrid, 'CONTROL: the rest of the profile loads').toBe('EN61')
  })
})

// A snapshot read without the station's stored keys (from afar, or while a stream watches the
// station) holds them blank. Saved so, loading the profile would blank the station's own key.
describe('a profile saved from a read without the stored keys keeps the station’s key on load', () => {
  beforeEach(() => memStore.clear())
  it('leaves the blank keys out, so the current key stays', () => {
    saveProfile('Field', settings({ clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: true }, mygrid: 'EN61' }))
    const [saved] = loadProfiles()
    expect('clublogApiKey' in saved.settings).toBe(false)
    const merged = mergeProfile(settings({ clublogApiKey: 'FAKE-KEY-0001' }), saved.settings)
    expect(merged.clublogApiKey).toBe('FAKE-KEY-0001')
    expect(merged.mygrid, 'CONTROL: the rest of the profile loads').toBe('EN61')
  })
  it('CONTROL: a profile read with its key keeps that key', () => {
    saveProfile('Home', settings({ clublogApiKey: 'FAKE-KEY-0002' }))
    expect(loadProfiles()[0].settings.clublogApiKey).toBe('FAKE-KEY-0002')
  })
})

describe('mergeProfile — the JS8 sounds’ WAV files travel with a profile', () => {
  it('a profile’s files replace the current ones, and a profile from before them keeps them', () => {
    const current = { js8Sounds: ['cq'], js8SoundFiles: { cq: '/home/op/sounds/cq.wav' } } as unknown as Settings
    const club = { js8Sounds: ['cq', 'inbox'], js8SoundFiles: { inbox: 'D:\\Club\\mail.wav' } } as unknown as Settings
    expect(mergeProfile(current, club).js8SoundFiles).toEqual({ inbox: 'D:\\Club\\mail.wav' })
    expect(mergeProfile(current, { band: '40m' } as unknown as Settings).js8SoundFiles).toEqual({ cq: '/home/op/sounds/cq.wav' })
  })
})
