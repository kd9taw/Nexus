// THE MAP STYLE ROW of the 2-D map's Layers panel (Connect's map and the APRS map): Atlas or Dark,
// and, under a dark built-in theme, that theme's own map. What each choice stores and the precedence
// are features/mapStyle's; this is only the control. The 3-D globe's panel has no such row: it keeps its
// photographs of the Earth.
import { useSkinActive } from '../useSkin'
import { mapStyleOptions, mapStylePick, mapStyleShown, type MapStyle, type MapStyleOption } from '../features/mapStyle'
import { t } from '../i18n'

const LABEL: Record<MapStyleOption, () => string> = {
  theme: () => t('map.style.theme'),
  atlas: () => t('map.style.atlas'),
  dark: () => t('map.style.dark'),
}

export function MapStylePick({ pick, onPick }: { pick: MapStyle | null; onPick: (pick: MapStyle | null) => void }) {
  const skin = useSkinActive()
  return (
    <div className="map-layer map-style">
      <label>
        {/* One line, whatever the select leaves it: the row's flex would otherwise wrap the words. */}
        <span className="map-style-label">{t('map.style.label')}</span>
        <select
          className="map-style-pick"
          value={mapStyleShown(pick, skin)}
          onChange={(e) => onPick(mapStylePick(e.target.value as MapStyleOption, skin))}
          title={t('map.style.title')}
        >
          {/* The <option> VALUES are the stored choices; only these labels are read. */}
          {mapStyleOptions(skin).map((o) => (
            <option key={o} value={o}>
              {LABEL[o]()}
            </option>
          ))}
        </select>
      </label>
    </div>
  )
}
