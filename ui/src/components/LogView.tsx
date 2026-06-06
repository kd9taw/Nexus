import { useMemo, useState } from 'react'
import type { AppSnapshot } from '../types'
import { exportLog } from '../api'

interface Props {
  snap: AppSnapshot
}

interface LogRow {
  primary: string
  cols: string[]
}

type ExportFormat = 'cabrillo' | 'adif'

const FD_HEADERS = ['Call', 'Class', 'Section', 'Band']
const ACTIVITY_HEADERS = ['Slot', 'Station', 'Dir', 'Message']

const EXT: Record<ExportFormat, string> = { cabrillo: 'cbr', adif: 'adi' }
const MIME: Record<ExportFormat, string> = {
  cabrillo: 'text/plain',
  adif: 'text/plain',
}

/** Trigger a browser download of `text` as a file. */
function downloadText(filename: string, text: string, mime: string): void {
  const blob = new Blob([text], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(url)
}

export function LogView({ snap }: Props) {
  const fd = snap.fieldDay
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<ExportFormat | null>(null)

  const { headers, rows, kind } = useMemo<{
    headers: string[]
    rows: LogRow[]
    kind: 'fieldDay' | 'activity'
  }>(() => {
    if (fd && fd.log.length > 0) {
      return {
        kind: 'fieldDay',
        headers: FD_HEADERS,
        rows: fd.log.map((q) => ({
          primary: q.call,
          cols: [q.call, q.class, q.section, q.band],
        })),
      }
    }
    // Fall back to a flat activity log from all conversation messages.
    const items = snap.conversations
      .flatMap((c) =>
        c.messages.map((m) => ({
          slot: m.slot,
          peer: c.peer,
          dir: m.outbound ? 'TX' : 'RX',
          text: m.text,
        })),
      )
      .sort((a, b) => b.slot - a.slot)
      .slice(0, 100)
    return {
      kind: 'activity',
      headers: ACTIVITY_HEADERS,
      rows: items.map((it) => ({
        primary: it.peer,
        cols: [String(it.slot), it.peer, it.dir, it.text],
      })),
    }
  }, [fd, snap.conversations])

  const handleExport = async (format: ExportFormat) => {
    setError(null)
    setBusy(format)
    try {
      const text = await exportLog(format)
      const stamp = new Date().toISOString().slice(0, 10)
      downloadText(`tempo-log-${stamp}.${EXT[format]}`, text, MIME[format])
    } catch (err) {
      const detail = err instanceof Error ? err.message : 'Export failed'
      setError(detail)
    } finally {
      setBusy(null)
    }
  }

  return (
    <section className="panel log-view">
      <div className="panel-header log-header">
        <div className="log-title">
          <h2>Log</h2>
          <span className="count-badge">{rows.length}</span>
          <span className="log-sub">
            {kind === 'fieldDay' ? 'Field Day contacts' : 'recent activity'}
          </span>
        </div>
        {kind === 'fieldDay' && (
          <div className="log-export">
            {error && (
              <span className="log-export-error" role="alert">
                {error}
              </span>
            )}
            <button
              type="button"
              className="export-btn"
              disabled={busy !== null}
              onClick={() => handleExport('cabrillo')}
            >
              {busy === 'cabrillo' ? 'Exporting…' : 'Export Cabrillo'}
            </button>
            <button
              type="button"
              className="export-btn"
              disabled={busy !== null}
              onClick={() => handleExport('adif')}
            >
              {busy === 'adif' ? 'Exporting…' : 'Export ADIF'}
            </button>
          </div>
        )}
      </div>

      <div className="log-table" role="table">
        <div className="log-row head" role="row">
          {headers.map((h) => (
            <span key={h} className="log-cell" role="columnheader">
              {h}
            </span>
          ))}
        </div>
        <div className="log-scroll">
          {rows.length === 0 && <p className="empty">No logged contacts yet.</p>}
          {rows.map((r, i) => (
            <div className="log-row" role="row" key={`${r.primary}-${i}`}>
              {r.cols.map((c, j) => (
                <span
                  key={j}
                  className={`log-cell${j === 0 ? ' mono' : ''}`}
                  role="cell"
                >
                  {c}
                </span>
              ))}
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
