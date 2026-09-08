import '../styles.css'
import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { MonitorApp } from './MonitorApp'
import { fixtureSource, scenarios } from './fixtureSource'
import type { Scenario } from './fixtureSource'
import { t } from '../i18n'
import './monitor.css'

function Preview() {
  const [fixture] = useState(() => fixtureSource())
  const [scenario, setScenario] = useState<Scenario>('spe')
  const [paused, setPaused] = useState(false)
  const [scale, setScale] = useState(1)
  const controls = <details className="rm-preview">
    <summary>{t('monitor.previewTools')}</summary>
    <div className="rm-preview-fields">
      <label>{t('monitor.scenario')}<select value={scenario} onChange={(event) => {
        const next = event.target.value as Scenario
        fixture.controls.scenario = next
        setScenario(next)
      }}>{scenarios.map((name) => <option key={name} value={name}>{t(`monitor.scenario.${name}`)}</option>)}</select></label>
      <label>{t('monitor.textSize')}<select value={scale} onChange={(event) => {
        const next = Number(event.target.value)
        document.documentElement.style.setProperty('--ui-zoom', String(next))
        setScale(next)
      }}>{[1, 1.5, 1.75].map((value) => <option key={value} value={value}>{value * 100}%</option>)}</select></label>
      <button onClick={() => {
        fixture.controls.paused = !paused
        setPaused(!paused)
      }}>{t(paused ? 'monitor.resume' : 'monitor.pause')}</button>
    </div>
  </details>
  return <MonitorApp source={fixture.source} scale={scale} previewTools={controls} />
}

createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>)
