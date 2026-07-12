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
