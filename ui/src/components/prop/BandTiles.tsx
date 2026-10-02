// BANDS FOR YOU — the band advice as coloured tiles (features/bandTiles.ts has the rules). A Connect
// box: it sits in any slot, and in the Band Advisor's default slot out of the box, with the ranked
// rows one pick away.
//
// THE LOOK, and why each part is what it is:
//   · the band in big letters, the word under it — the word is always on the tile, so colour is never
//     the only channel (colour-blind operators; the Open and Marginal tints are close to a protan eye);
//   · Open: a green tint and a solid green edge; Marginal: an amber tint and a DASHED amber edge (a
//     second channel besides hue); Closed: no tint and dim letters — it recedes, never red (red is the
//     transmit colour and the ON AIR sign); no data: a hollow neutral tile, never green;
//   · the letters are the theme's inks on the tint, never the band colour (a word lettered in the band
//     colour fails the 4.5:1 lettering floor — the map's list learned that as #382);
//   · a dot for what is heard, ★ for the advisor's best band, a ring for the radio's band, and the mode
//     of a VHF opening (Es, Tropo, Aurora, F2, MS — tokens, the same in every language).
// Click a tile to show that band on the map (Connect's band focus); click it again to clear.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The words on the tiles are the
// list's own (the backend's BandModeled vocabulary, like the list and the band menu), band and mode names
// are tokens, and every sentence resolves through t().
import type { CSSProperties } from 'react'
import type { BandOutlook, PropagationSnapshot } from '../../types'
import { bandTiles, type BandTile } from '../../features/bandTiles'
import { bandTiming } from '../../propViz'
import { t } from '../../i18n'

/** The heard dots — glyphs, not words; each tile's tooltip says what they mean. */
const DOT: Record<BandTile['heard'], string> = { active: '●', some: '◐', none: '○' }
const STAR = '★'

function tileTitle(tile: BandTile, outlook: readonly BandOutlook[], nowMs: number): string {
  const lines: string[] = []
  const r = tile.report
  // No data: the band menu's own words for it, so the two say the same.
  if (tile.state === 'unknown') lines.push(t('bandMenu.condition.unknown.title'))
  else lines.push(t('bandTiles.title.state', { band: tile.band, word: tile.word }))
  if (tile.mode) lines.push(t('bandTiles.title.opening', { mode: tile.mode }))
  if (r) {
    if (r.reason) lines.push(r.reason)
    lines.push(t('bandTiles.title.heard', { hearYou: r.nHearMe, youHear: r.nIHear }))
    if (r.bestRegion)
      lines.push(t('bandTiles.title.region', { region: r.bestRegion.region, octant: r.bestRegion.octant, bearing: Math.round(r.bestRegion.bearingDeg) }))
  }
  const hourly = outlook.find((b) => b.band === tile.band)?.hourly
  const timing = hourly ? bandTiming(hourly, nowMs) : ''
  if (timing) lines.push(t('bandTiles.title.modelled', { timing }))
  if (tile.best) lines.push(t('bandTiles.title.best'))
  if (tile.onRig) lines.push(t('bandTiles.title.rig'))
  lines.push(t('bandTiles.title.focus'))
  return lines.join('\n')
}

export function BandTiles({
  prop,
  outlook,
  rigBand,
  focusBand,
  onBandClick,
  nowMs = Date.now(),
}: {
  prop: PropagationSnapshot | null
  /** The band outlook (hourly, modelled) for the "when it next changes" line of each tooltip. */
  outlook?: readonly BandOutlook[]
  /** The band the active radio is on: its tile is ringed. */
  rigBand?: string | null
  /** The band shown on the map (Connect's band focus): its tile is pressed. */
  focusBand?: string | null
  onBandClick?: (band: string) => void
  nowMs?: number
}) {
  const tiles = bandTiles(prop, rigBand ?? null, Math.floor(nowMs / 1000))
  return (
    <div className="band-tiles" role="group" aria-label={t('bandTiles.aria')}>
      {tiles.map((tile) => (
        <button
          type="button"
          key={tile.band}
          className={`bt-tile is-${tile.state}${tile.onRig ? ' is-rig' : ''}`}
          style={tile.color ? ({ '--bt-color': tile.color } as CSSProperties) : undefined}
          aria-pressed={focusBand === tile.band}
          title={tileTitle(tile, outlook ?? [], nowMs)}
          onClick={onBandClick ? () => onBandClick(tile.band) : undefined}
        >
          <span className="bt-band">{tile.band}</span>
          <span className="bt-word">{tile.state === 'unknown' ? t('bandMenu.condition.unknown') : tile.word}</span>
          <span className="bt-marks" aria-hidden="true">
            {tile.best && <span className="bt-star">{STAR}</span>}
            <span className={`bt-dot is-${tile.heard}`}>{DOT[tile.heard]}</span>
          </span>
          {tile.mode && <span className="bt-mode">{tile.mode}</span>}
        </button>
      ))}
    </div>
  )
}
