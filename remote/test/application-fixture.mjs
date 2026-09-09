// Synthetic station data for the compiled browser. This file is never imported
// by the application bundle. Settings use the actual native disclosure list.
import { readFile } from 'node:fs/promises'

export async function applicationFixture() {
  const source = await readFile(new URL('../../src-tauri/src/remote_service/application.rs', import.meta.url), 'utf8')
  const keys = [...source.match(/const SETTINGS_KEYS: &\[&str\] = &\[([\s\S]*?)\];/)[1].matchAll(/"([a-zA-Z0-9]+)"/g)].map(match => match[1])
  const defaults = JSON.parse(await readFile(new URL('../../ui/src/components/__fixtures__/defaultSettings.json', import.meta.url), 'utf8'))
  const settings = Object.fromEntries(Object.entries(defaults).filter(([key]) => keys.includes(key)))
  return {
    get_snapshot: {
      mycall: 'N0CALL', mygrid: 'AA00', mode: 'Normal',
      radio: { dialMhz: 3.573, band: '80m', catOk: true, sideband: 'USB', operatingMode: 'digital',
        transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500,
        txLevel: 0.5, slot: 0, nextSlotMs: 12000,
        amp: { family: 'spe', model: '15K', linked: true, reason: '', operate: false, transmitting: false,
          outputWatts: 0, bandLabel: '80m', alarm: 'none', alarmRaised: false, warning: 'none', warningRaised: false } },
      aiCw: { enabled: false, status: '', text: '' },
      link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
      stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
    },
    get_settings: { ...settings, mycall: 'N0CALL', mygrid: 'AA00', dialMhz: 3.573, band: '80m' },
    get_band_plan: [{ band: '80m', dialMhz: 3.573, mode: 'USB', label: '80m FT8', group: 'HF', tx: true, note: '' }],
    get_spectrum_row: { row: Array.from({ length: 512 }, (_, i) => i > 190 && i < 195 ? 0.8 : 0.01), loHz: 0, hiHz: 4000, source: 'audio' },
    get_meters: { rxLevel: 0.05, smeterDb: -12, cwToneHz: null },
    get_scope_snapshot: { row: Array.from({ length: 512 }, (_, i) => i > 74 && i < 79 ? 0.8 : 0.01), loHz: 0, hiHz: 4000, source: 'audio' },
    get_cw_state: { text: 'CQ TEST DE W1AW', wpm: 22, sent: ['DE N0CALL'], keyerError: null,
      candidates: [{ call: 'W1AW', best: true }], rst: null, name: null, state: 'cq', headline: '', prompt: '', recommended: null, workedCall: null },
  }
}
