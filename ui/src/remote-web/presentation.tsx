import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { BookOpen, ChevronDown, Crosshair, PanelsTopLeft, Radio, SlidersHorizontal } from 'lucide-react'
import type { View } from '../components/ModeNav'
import { Menu } from '../components/ui/Menu'
import { featureById } from '../features/registry'
import { t } from '../i18n'

export type RemotePresentation = 'full' | 'quick'
export type PresentationState = {
  presentation: RemotePresentation
  change: (value: RemotePresentation) => void
  radioDetails: boolean
  setRadioDetails: (expanded: boolean) => void
}

// Browser-local presentation only. The provider remains above the one App and
// never owns a station command, connection, lease, QSO or pane preference.
export const RemotePresentationContext = createContext<PresentationState | null>(null)
export function useRemotePresentation() { return useContext(RemotePresentationContext) }

export function QuickRadioDetails() {
  const display = useRemotePresentation()
  if (display?.presentation !== 'quick') return null
  return <button type="button" className="remote-button remote-quick-details"
    aria-label={t('remote.quick.radioDetails')} title={t('remote.quick.radioDetails')}
    aria-expanded={display.radioDetails}
    onClick={() => display.setRadioDetails(!display.radioDetails)}>
    <SlidersHorizontal size={20} aria-hidden="true" /><span>{t('remote.quick.radioDetails')}</span>
  </button>
}

const OPERATING_VIEWS: View[] = ['operate', 'phone', 'cw', 'rtty', 'psk', 'js8', 'chat', 'sstv', 'aprs']
// Match the native mode rail's invariant FT/Tempo names; other labels come from
// the shared feature catalog at render time so locale changes remain live.
const operatingLabel = (view: View) => view === 'operate' ? 'FT' : view === 'chat' ? 'Tempo' : featureById(view)?.label ?? view
export function QuickNavigation({ view, onSelect, available }: {
  view: View; onSelect: (view: View) => void; available: (view: View) => boolean
}) {
  const display = useRemotePresentation()
  const navigation = useRef<HTMLElement>(null)
  useLayoutEffect(() => {
    const nav = navigation.current, app = nav?.closest<HTMLElement>('.app')
    if (!nav || !app || display?.presentation !== 'quick') return
    // The translated, zoomed navigation can wrap. Reserve its measured CSS
    // height for the existing toast viewport instead of covering its buttons.
    const measure = () => app.style.setProperty('--remote-quick-nav-height', `${nav.offsetHeight}px`)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(nav)
    return () => { observer.disconnect(); app.style.removeProperty('--remote-quick-nav-height') }
  }, [display?.presentation])
  const operating = OPERATING_VIEWS.includes(view) && available(view)
  const [last, setLast] = useState<View>(operating ? view : 'operate')
  useEffect(() => { if (operating) setLast(view) }, [view, operating])
  if (display?.presentation !== 'quick') return null
  return <nav ref={navigation} className="remote-quick-nav" aria-label={t('remote.quick.navigation')}>
    {operating ? <Menu requiresStationData={false} className="remote-quick-mode-menu"
      trigger={<button type="button" aria-current="page" aria-label={t('remote.quick.chooseMode')} title={t('remote.quick.chooseMode')}>
        <ChevronDown size={20} aria-hidden="true" /><span>{operatingLabel(view)}</span>
      </button>}
      items={OPERATING_VIEWS.filter(available).map(next => ({ label: operatingLabel(next), onSelect: () => onSelect(next) }))}
    /> : <button type="button"
      onClick={() => onSelect(available(last) ? last : 'operate')}>
      <Radio size={20} aria-hidden="true" /><span>{t('remote.quick.operate')}</span>
    </button>}
    <button type="button" disabled={!available('needed')}
      aria-current={view === 'needed' ? 'page' : undefined} onClick={() => onSelect('needed')}>
      <Crosshair size={20} aria-hidden="true" /><span>{t('remote.quick.hunt')}</span>
    </button>
    <button type="button" disabled={!available('logbook')}
      aria-current={view === 'logbook' ? 'page' : undefined} onClick={() => onSelect('logbook')}>
      <BookOpen size={20} aria-hidden="true" /><span>{t('remote.quick.log')}</span>
    </button>
    <button type="button" aria-label={t('remote.quick.full')} title={t('remote.quick.full')} onClick={() => display.change('full')}>
      <PanelsTopLeft size={20} aria-hidden="true" /><span>{t('remote.quick.full')}</span>
    </button>
  </nav>
}
