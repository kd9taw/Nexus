import { createContext, useContext, useEffect, useState } from 'react'
import { BookOpen, Crosshair, PanelsTopLeft, Radio, SlidersHorizontal } from 'lucide-react'
import type { View } from '../components/ModeNav'
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
export function QuickNavigation({ view, onSelect, available }: {
  view: View; onSelect: (view: View) => void; available: (view: View) => boolean
}) {
  const display = useRemotePresentation()
  const operating = OPERATING_VIEWS.includes(view) && available(view)
  const [last, setLast] = useState<View>(operating ? view : 'operate')
  useEffect(() => { if (operating) setLast(view) }, [view, operating])
  if (display?.presentation !== 'quick') return null
  return <nav className="remote-quick-nav" aria-label={t('remote.quick.navigation')}>
    <button type="button" aria-current={operating ? 'page' : undefined}
      onClick={() => onSelect(available(last) ? last : 'operate')}>
      <Radio size={20} aria-hidden="true" /><span>{t('remote.quick.operate')}</span>
    </button>
    <button type="button" disabled={!available('needed')}
      aria-current={view === 'needed' ? 'page' : undefined} onClick={() => onSelect('needed')}>
      <Crosshair size={20} aria-hidden="true" /><span>{t('remote.quick.hunt')}</span>
    </button>
    <button type="button" disabled={!available('logbook')}
      aria-current={view === 'logbook' ? 'page' : undefined} onClick={() => onSelect('logbook')}>
      <BookOpen size={20} aria-hidden="true" /><span>{t('remote.quick.log')}</span>
    </button>
    <button type="button" onClick={() => display.change('full')}>
      <PanelsTopLeft size={20} aria-hidden="true" /><span>{t('remote.quick.full')}</span>
    </button>
  </nav>
}
