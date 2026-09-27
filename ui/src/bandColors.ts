// Shared per-band colors — low bands cool → high bands warm, so a band reads at a
// glance everywhere it appears (the Connect map spot dots AND the band-selection
// picker in the cockpits). One source of truth so the map and the controls agree.
export const BAND_COLOR: Record<string, string> = {
  '2200m': '#6a4cff',
  '630m': '#6e54ff',
  '160m': '#7c5cff',
  '80m': '#5c7cff',
  '60m': '#4a8eff',
  '40m': '#3aa0ff',
  '30m': '#2bd4c0',
  '20m': '#3ddc6a',
  '17m': '#9bdc3d',
  '15m': '#ffcc44',
  '12m': '#ff9d3d',
  '10m': '#ff6d3d',
  '6m': '#ff4d6d',
  '4m': '#ff4da6',
  '2m': '#d24dff',
  '1.25m': '#c04dff',
  '70cm': '#b04dff',
  '33cm': '#a24dff',
  '23cm': '#944dff',
}

/** The color for a band label ('20m', '2m', …); a neutral fallback for anything unknown. */
export function bandColor(band: string): string {
  return BAND_COLOR[band] ?? '#8aa0b0'
}

/** Six violets read under 4.5:1 as lettering on the dark theme's band chip (3.4–4.5:1 on the top
 *  bar's `--bg-elev`), so the chip letters them in the same violet taken just light enough to
 *  read 4.6:1 there (operator, 2026-09-27: "Tune the six violets"). The CHIP's only: the palette
 *  above, which the globes and the Field Day board also paint, keeps its own values. */
const CHIP_INK: Readonly<Record<string, string>> = {
  '2200m': '#7b6eff',
  '630m': '#7b6efe',
  '160m': '#836bfe',
  '70cm': '#b151ff',
  '33cm': '#a659ff',
  '23cm': '#9a60ff',
}

/** The band name's ink on the band chip (FrequencyControl, BandPicker): the band's colour, or its
 *  lifted violet. The light theme letters the name in its own text colour instead (styles.css,
 *  THE BAND CHIP). The chip's border, glow and dot keep the palette colour in both themes. */
export function bandChipInk(band: string): string {
  return CHIP_INK[band] ?? bandColor(band)
}

// Propagation-mode colors for opening visuals (map sectors, mode chips): one hue
// per physical mode so "what KIND of opening" reads at a glance on every surface.
// Keys are the backend PropMode labels carried in OpeningView.mode.
export const OPENING_MODE_COLOR: Record<string, string> = {
  Tropo: '#ffb347', // warm amber — weather-driven lift
  'Sporadic-E': '#4dff88', // green — the classic Es flash
  Aurora: '#c77dff', // violet — geomagnetic
  F2: '#4dd2ff', // cyan — ionospheric DX
}

/** The color for a propagation-mode label; a neutral fallback for Unknown. */
export function openingModeColor(mode: string): string {
  return OPENING_MODE_COLOR[mode] ?? '#8aa0b0'
}
