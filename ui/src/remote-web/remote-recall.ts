import { planRecall, type Memory } from '../features/memories'
import { bandLabelForMhz } from '../band'

export type RemoteRecallArgs = {
  section: 'cw' | 'phone' | 'digital'
  dialMhz: number
  band: string
  sideband: 'USB' | 'LSB' | null
  fm: { shift: 'simplex' | 'plus' | 'minus'; offsetHz: number; toneHz: number } | null
}

/** What a browser asks the station to recall: the desktop's own plan (`planRecall`) as the closed
 * `radio.memoryRecall` intent. The station applies the phone mode and FM plumbing itself, so the
 * browser never writes Settings. `null` when the dial is on no band (nothing the station admits). */
export function remoteRecallArgs(m: Memory): { view: 'cw' | 'phone' | 'operate'; args: RemoteRecallArgs } | null {
  const plan = planRecall(m)
  const band = bandLabelForMhz(plan.freqMhz)
  if (!band) return null
  const section = plan.view === 'operate' ? 'digital' : plan.view
  const patch = plan.settingsPatch
  // FM voice starts at 29 MHz; below it the band's own sideband applies and no machine travels.
  const fm = patch?.rptrShift !== undefined && plan.freqMhz >= 29
    ? { shift: patch.rptrShift as 'simplex' | 'plus' | 'minus', offsetHz: patch.rptrOffsetOverrideHz ?? 0, toneHz: patch.ctcssToneHz ?? 0 }
    : null
  const mode = m.mode.toUpperCase()
  const sideband = section === 'phone' && !fm && (mode === 'USB' || mode === 'LSB') ? mode : null
  return { view: plan.view, args: { section, dialMhz: plan.freqMhz, band, sideband, fm } }
}
