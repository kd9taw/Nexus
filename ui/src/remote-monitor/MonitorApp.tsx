import { useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { ChevronDown, Moon, Radio, Sun } from 'lucide-react'
import { AmpPane } from '../components/prop/AmpPane'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { initialState, startMonitor } from './session'
import type { MonitorSource, MonitorState } from './session'
import type { MonitorAmplifier } from './protocol'
import './monitor.css'

const DASH = '—'
const BRAND = 'Nexus'
const MHZ = 'MHz'
const SWR = 'SWR'

function Amplifier({ amp, current }: { amp: MonitorAmplifier; current: boolean }) {
  const [expanded, setExpanded] = useState(false)
  const available = current && amp.linked && !amp.reason
  const state = !available || amp.operate === null ? t('monitor.unavailable')
    : amp.operate ? t('amp.operate') : t('amp.standby')
  const fault = available && amp.alarmRaised
  const warning = available && amp.warningRaised
  // AmpPane is the established read-only renderer. Gate its display copy on transport
  // AND measurement availability, including the first miss while linked is still true.
  const display = { ...amp, linked: available }
  return (
    <section className="rm-card rm-amplifier" aria-label={t('monitor.amplifier')}>
      <button className="rm-amp-strip" aria-expanded={expanded} aria-controls="monitor-amp-details"
        onClick={() => setExpanded((value) => !value)}>
        <span className="rm-amp-name">{amp.family.toUpperCase() || t('monitor.amplifier')} {amp.model}</span>
        <span className={fault ? 'rm-fault' : warning ? 'rm-warning' : 'rm-muted'}>
          {fault ? t('monitor.ampAlarm') : warning ? t('monitor.ampWarning') : state}
        </span>
        <span className="rm-amp-reading">
          {available && amp.outputWatts !== null ? `${amp.outputWatts} W` : DASH}
          <span className="rm-muted">{SWR} {available && amp.swr !== null ? `${amp.swr.toFixed(1)}:1` : DASH}</span>
        </span>
        <ChevronDown size={20} aria-hidden="true" className={expanded ? 'rm-chevron-open' : undefined} />
      </button>
      {expanded && <div id="monitor-amp-details" className="rm-amp-details">
        {!current && <p className="rm-warning">{t('monitor.ampLastIdentity')}</p>}
        {current && !available && <p className="rm-warning">{t('monitor.ampNoReading')}</p>}
        <AmpPane amp={display} />
        <dl className="rm-facts">
          <Fact label={t('monitor.ampBand')} value={available ? amp.bandLabel : null} />
          <Fact label={t('monitor.ampTx')} value={!available || amp.transmitting === null ? null
            : amp.transmitting ? t('monitor.keyed') : t('monitor.unkeyed')} />
          <Fact label={t('monitor.followBand')} value={current
            ? amp.followBand ? t('monitor.on') : t('monitor.off') : null} />
        </dl>
      </div>}
    </section>
  )
}

function Fact({ label, value }: { label: string; value: string | null }) {
  return <div className="rm-fact"><dt>{label}</dt><dd>{value ?? DASH}</dd></div>
}

type Props = { source: MonitorSource; previewTools?: ReactNode; scale?: number }

function MonitorSession({ source, previewTools, scale }: Props) {
  const [state, setState] = useState<MonitorState>(initialState)
  const [theme, setTheme] = useState(() => document.documentElement.dataset.theme === 'light' ? 'light' : 'dark')
  useViewport(scale, true)
  useEffect(() => startMonitor(source, setState), [source])
  useEffect(() => { document.documentElement.dataset.theme = theme }, [theme])
  const current = state.status === 'current'
  const station = state.frame?.station
  const radio = station?.radio
  const dial = current ? radio?.dialMhz : null
  const statusText = {
    connecting: t('monitor.connecting'), current: t('monitor.current'),
    unavailable: t('monitor.connectionLost'), invalid: t('monitor.invalid'),
  }
  return (
    <div className="app remote-monitor-app">
      <header className="rm-header">
        <div className="rm-brand"><Radio size={23} aria-hidden="true" /><strong>{BRAND}</strong><span>{t('monitor.title')}</span></div>
        <button className="rm-theme" aria-label={theme === 'dark' ? t('monitor.light') : t('monitor.dark')}
          onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
          {theme === 'dark' ? <Sun aria-hidden="true" size={22} /> : <Moon aria-hidden="true" size={22} />}
        </button>
      </header>
      <main className="rm-scroll" aria-label={t('monitor.title')}>
        <div className="rm-content">
          <div className="rm-source"><span>{source.kind === 'fixture' ? t('monitor.fixture') : t('monitor.native')}</span>
            <span className="rm-observer">{t('monitor.observer')}</span></div>
          <p className={`rm-notice ${current ? '' : 'rm-warning'}`} role="status">{statusText[state.status]}</p>
          <div className="rm-station-grid">
            <section className="rm-card rm-radio" aria-label={t('monitor.stationDial')}>
              <div className="rm-identity"><strong>{station?.call || DASH}</strong><span>{station?.grid || DASH}</span></div>
              <h1 className="rm-frequency" aria-label={t('monitor.stationDial')}>{dial == null ? DASH : dial.toFixed(6)}<small>{MHZ}</small></h1>
              <div className="rm-mode"><strong>{current ? radio?.mode : DASH}</strong><span>{current ? radio?.band : DASH}</span></div>
              <p className="rm-muted">{t('monitor.stationDialHint')}</p>
            </section>
            <section className="rm-card rm-radio-status" aria-label={t('monitor.radioStatus')}>
              <h2>{radio?.name || t('monitor.radioStatus')}</h2>
              <dl className="rm-facts">
                <Fact label={t('monitor.cat')} value={!current || radio?.catConnected == null ? null
                  : radio.catConnected ? t('monitor.connected') : t('monitor.disconnected')} />
                <Fact label={t('monitor.rigMode')} value={current ? radio?.rigMode ?? null : null} />
                <Fact label={t('monitor.rigKeyed')} value={!current || radio?.rigKeyed == null ? null
                  : radio.rigKeyed ? t('monitor.keyed') : t('monitor.unkeyed')} />
                <Fact label={t('monitor.nexusTx')} value={!current || !radio ? null
                  : radio.nexusBusy ? t('monitor.busy') : t('monitor.idle')} />
              </dl>
              <p className="rm-muted">{t('monitor.txHint')}</p>
            </section>
          </div>
          {station?.amplifier && <Amplifier key={`${state.frame?.epoch}:${radio?.id}`}
            amp={station.amplifier} current={current} />}
          <p className="rm-footnote">{t('monitor.readingsHint')}</p>
          {previewTools}
        </div>
      </main>
    </div>
  )
}

// A different source must never render even one paint of the preceding station.
export function MonitorApp(props: Props) {
  return <MonitorSession key={props.source.id} {...props} />
}
