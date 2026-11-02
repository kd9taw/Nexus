// BAND-PLAN ADVICE — DISPLAY ONLY. It says when phone is outside the usual band plan; it NEVER
// blocks, never keys or unkeys, and nothing in the transmit path reads it. The licence gate lives
// in the engine (`privileges.rs`) and is the only thing that decides whether a station transmits.
//
// ONE WORLDWIDE TABLE, NO REGION. Nexus has no region setting, so each line is the LOWEST phone
// line any of the three IARU regional HF band plans draws (Region 1 HF band plan, 2023 revision;
// Region 2 band plan; Region 3 band plan): below it, every region keeps the band for CW, narrow
// data and beacons. A warning therefore means "outside the usual plan everywhere", never "outside
// your region's plan", and an operator inside any region's phone segment sees nothing. The 20,
// 17, 15, 12 and 10 m lines are the International Beacon Project frequencies (14.100, 18.110,
// 21.150, 24.930, 28.200), which every plan keeps clear of phone below them. 30 m has no phone in
// any plan. 60 m is left out: its allocations differ country by country.
//
// The judged frequency is the LOWEST edge of the transmitted phone signal: the carrier for USB,
// 2.8 kHz below it for LSB (the passband the engine's gate measures), and 3 kHz below it for any
// other phone mode (AM, FM), whose voice sits on both sides of the carrier.

/** [band, band bottom, band top, phone line] in MHz. `null` = no phone anywhere on the band. */
const PHONE_LINES: ReadonlyArray<readonly [string, number, number, number | null]> = [
  ['160m', 1.8, 2.0, 1.84],
  ['80m', 3.5, 4.0, 3.535],
  ['40m', 7.0, 7.3, 7.03],
  ['30m', 10.1, 10.15, null],
  ['20m', 14.0, 14.35, 14.1],
  ['17m', 18.068, 18.168, 18.11],
  ['15m', 21.0, 21.45, 21.15],
  ['12m', 24.89, 24.99, 24.93],
  ['10m', 28.0, 29.7, 28.2],
  ['6m', 50.0, 54.0, 50.1],
  ['2m', 144.0, 148.0, 144.1],
]

/** Half a hertz, so a carrier typed exactly on a line is not judged by float rounding. */
const EPS_MHZ = 0.0000005

/**
 * The band whose usual band plan a PHONE signal with its carrier at `carrierMhz`, in `sideband`
 * ('USB' | 'LSB' | 'AM' | 'FM' | …), falls outside — or `null` when it is inside every region's
 * plan, or off the bands this table covers.
 */
export function phoneOutsideBandPlan(carrierMhz: number, sideband: string): string | null {
  if (!Number.isFinite(carrierMhz)) return null
  const sb = sideband.toUpperCase()
  const lowest = sb === 'USB' ? carrierMhz : carrierMhz - (sb === 'LSB' ? 0.0028 : 0.003)
  for (const [band, lo, hi, line] of PHONE_LINES) {
    if (carrierMhz < lo || carrierMhz >= hi) continue
    if (line === null) return band
    return lowest < line - EPS_MHZ ? band : null
  }
  return null
}
