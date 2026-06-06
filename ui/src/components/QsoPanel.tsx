import type { ModeRequest, QsoStatus } from '../types'

interface Props {
  qso: QsoStatus | null
  onSetMode: (mode: ModeRequest) => void
}

function reportLabel(rxReport: number | null): string {
  if (rxReport === null || rxReport === undefined) return '—'
  return `${rxReport > 0 ? '+' : ''}${rxReport} dB`
}

export function QsoPanel({ qso, onSetMode }: Props) {
  // Default to Search-&-Pounce (listen first), never an implied "Calling CQ".
  const running = qso?.running ?? false
  const dxcall = qso?.dxcall ?? null
  const state = qso?.state ?? 'Idle'

  // Human-readable status banner.
  const banner = running
    ? dxcall
      ? `In QSO with ${dxcall}`
      : 'Calling CQ…'
    : dxcall
      ? `Working ${dxcall} (S&P)`
      : 'Monitoring (S&P)…'

  return (
    <section className="conversation panel qso-panel">
      <div className="panel-header conv-header">
        <h2 className="conv-peer">QSO</h2>
        <span className="conv-sub">sequenced 1:1 contact</span>
      </div>

      <div className="qso-body">
        <div className={`qso-status-banner ${running ? 'running' : 'sp'}`}>
          <span className="qso-status-dot" aria-hidden />
          <span className="qso-status-text">{banner}</span>
        </div>

        <dl className="qso-readouts">
          <div>
            <dt>Sequencer</dt>
            <dd>{state}</dd>
          </div>
          <div>
            <dt>DX Call</dt>
            <dd className="mono">{dxcall ?? '—'}</dd>
          </div>
          <div>
            <dt>RX Report</dt>
            <dd className="mono">{reportLabel(qso?.rxReport ?? null)}</dd>
          </div>
          <div>
            <dt>Role</dt>
            <dd>{running ? 'Running (CQ)' : 'Search & Pounce'}</dd>
          </div>
        </dl>

        <div className="qso-actions">
          <button
            type="button"
            className={`qso-action-btn primary${running ? ' active' : ''}`}
            aria-pressed={running}
            onClick={() => onSetMode('qso-run')}
          >
            Call CQ
            <small>start running</small>
          </button>
          <button
            type="button"
            className={`qso-action-btn${!running ? ' active' : ''}`}
            aria-pressed={!running}
            onClick={() => onSetMode('qso-monitor')}
          >
            Monitor (S&amp;P)
            <small>search &amp; pounce</small>
          </button>
        </div>

        <p className="qso-hint">
          The auto-sequencer advances each slot: <strong>Running</strong> calls
          CQ and works answers as they arrive; <strong>Monitor</strong> answers
          the next CQ it decodes. It picks up the contact automatically — no
          manual targeting needed.
        </p>
      </div>
    </section>
  )
}
