// Space-weather strip: each index as value + severity bar + plain-language HF
// impact. The numbers stay visible (project rule: never hide the physics); the
// plain language is the Mission-Control glanceable layer.
import type { SpaceWxView } from '../../types'
import { sfiImpact, kpImpact, xrayImpact, type Impact } from '../../propViz'

const SEV_VAR: Record<Impact['sev'], string> = {
  quiet: 'var(--band-open)',
  active: 'var(--band-marginal)',
  warn: 'var(--alert-warning)',
}

function Gauge({ label, value, impact }: { label: string; value: string; impact: Impact }) {
  return (
    <div className="swx-gauge">
      <div className="swx-head">
        <span className="swx-k">{label}</span>
        <span className="swx-v">{value}</span>
      </div>
      <div className="swx-bar" aria-hidden="true">
        <span className="swx-bar-fill" style={{ background: SEV_VAR[impact.sev] }} />
      </div>
      <div className="swx-impact" style={{ color: SEV_VAR[impact.sev] }}>
        {impact.text}
      </div>
    </div>
  )
}

export function SpaceWxGauges({ wx }: { wx: SpaceWxView }) {
  return (
    <section className="swx-strip panel" aria-label="Space weather">
      <Gauge label="SFI" value={wx.sfi.toFixed(0)} impact={sfiImpact(wx.sfi)} />
      <Gauge label="Kp" value={wx.kp.toFixed(0)} impact={kpImpact(wx.kp)} />
      <Gauge label="A" value={wx.aIndex.toFixed(0)} impact={kpImpact(wx.kp)} />
      <Gauge label="X-ray" value={wx.xrayClass.replace('-class', '')} impact={xrayImpact(wx.xrayClass)} />
    </section>
  )
}
