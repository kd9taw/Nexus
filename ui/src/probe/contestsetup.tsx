// THE CONTEST SETUP IN A REAL BROWSER, for `scripts/browser-probe --desktop contestsetup`
// (ui/probe/contestsetup-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// setup's step buttons are on screen when its pick list (every shipped contest and the operator's own)
// or the builder's steps are taller than the dialog. This is the real `ContestSetupWizard` on the app's
// stylesheets at zoom 100 %; the station's answers are given at the IPC bridge, and a command not
// answered here is refused.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { ContestSetupWizard } from '../components/ContestSetupWizard'
import settings from '../components/__fixtures__/defaultSettings.json'

/** Three of the operator's own contests, one with a long name, as `list_custom_contests` lists them. */
const own = (id: string, name: string, token: string) => ({
  def: {
    format: 'nexus-custom-contest',
    version: 1,
    id,
    name,
    short: '',
    cabrillo_contest: token,
    modes: ['CW', 'SSB'],
    window: { kind: 'weekend', month: 3, weekend: 'nth_full', n: 1, start_hour_utc: 14, duration_hours: 4 },
    exchange: [{ field: 'rst' }, { field: 'serial' }],
    dupe: 'band',
    points: { CW: 1, PH: 1, DIG: 1 },
  },
  enteredSlots: [],
  sentSettings: [],
  exchangeLocked: false,
})
const mine = [
  own('custom_club_sprint', 'Club Sprint', 'CLUB-SPRINT'),
  own('custom_county_hunters_weekend_marathon', 'County Hunters Weekend Marathon on Every Band', 'CH-MARATHON'),
  own('custom_tuesday_night_net', 'Tuesday Night Net', 'TNN'),
]

type Def = { exchange: { field: string }[]; modes: string[] }
const answers: Record<string, (args: Record<string, unknown>) => unknown> = {
  get_settings: () => ({ ...settings, mycall: 'KD9TAW', fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI' }),
  get_snapshot: () => ({ fieldDay: null, mode: 'Normal', radio: { catOk: true, operatingMode: 'CW' }, link: { tier: 'Ft8' } }),
  contest_windows: () => [],
  list_custom_contests: () => mine,
  list_unusable_custom_contests: () => [],
  preview_fd_ruleset: (a) => {
    const patch = (a.patch ?? {}) as { fdEvent?: string }
    const event = patch.fdEvent || 'arrlfd'
    return {
      event,
      rulesYear: 2026,
      bannedModes: [],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: '',
      exchange: ['1D', 'WI'],
      problem: '',
      sentSettings: ['fd_class', 'fd_section'],
    }
  },
  preview_custom_contest: (a) => {
    const def = a.def as Def
    const ft = def.modes.includes('FT8') || def.modes.includes('FT4')
    const roundup = ft && def.exchange.map((f) => f.field).join() === 'rst,serial'
    return {
      refusal: '',
      youSend: '599 1',
      startProblem: '',
      // A Cabrillo QSO line as wide as one gets: it must wrap inside the dialog, never widen it.
      sampleQsoLine: 'QSO:  7025 CW 2026-03-07 1400 KD9TAW/VE3        599 1      WI  1D  W1AW/KH6         599 1      CT  2A',
      ftShape: roundup ? 'rtty_roundup' : null,
      ftActivity: null,
      ftMessages: 'standard',
      ftCq: '',
      ftSends: '',
      ftRefusal: '',
      enteredSlots: [],
    }
  },
}
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd](args ?? {}) as T)
    return Promise.reject(new Error(`${cmd} is not answered in the contest setup probe`))
  },
}

document.documentElement.setAttribute('data-theme', 'dark')
createRoot(document.getElementById('root')!).render(<ContestSetupWizard onClose={() => {}} />)
