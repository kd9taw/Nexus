/**
 * Settings ▸ Logging & Connectors ▸ Integrations & Feeds ▸ Spot Sources: which human DX-cluster
 * nodes Nexus connects to for SSB/phone spots, and how each one is doing.
 *
 * Two choices, and the second is never switched for the operator:
 *  - **Pick working nodes automatically** — Nexus keeps two of the nodes built into the release
 *    connected and moves off one that stops working (`src-tauri/src/cluster_nodes.rs`). The rows
 *    are read-only: this is what Nexus is doing, not a list to edit.
 *  - **Use my list** — the operator's own nodes, all connected, as written. Each row shows how its
 *    node is doing, and nothing else changes by itself.
 *
 * The standings come from `get_cluster_nodes` and describe what is SAVED. While the form holds the
 * other choice, unsaved, the rows show no standing at all: "In use" or "Standby" would describe
 * the mode the operator is leaving.
 */
import type { ClusterNode, ClusterNodeFailure, ClusterNodes } from '../types'
import { t } from '../i18n'

/** Node software names — product names, the same in every language, so not catalog entries. */
const SOFTWARE_NAMES: Record<NonNullable<ClusterNode['software']>, string> = {
  dxSpider: 'DXSpider',
  ccCluster: 'CC Cluster',
}

function reasonText(failure: ClusterNodeFailure): string {
  switch (failure) {
    case 'unreachable':
      return t('settings.integrations.clusterNodes.reason.unreachable')
    case 'noGreeting':
      return t('settings.integrations.clusterNodes.reason.noGreeting')
    case 'noPrompt':
      return t('settings.integrations.clusterNodes.reason.noPrompt')
    case 'droppedAfterLogin':
      return t('settings.integrations.clusterNodes.reason.droppedAfterLogin')
  }
}

/** A clock time for "skipped until", in the operator's own time — as the connection log shows. */
function clockTime(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
}

/**
 * How a node is doing, in words, or null when there is nothing honest to say. A node being tried
 * right now says so before any skip it carries: with every node skipped, Nexus still keeps two
 * going, and "skipped" would misreport those.
 */
function nodeStanding(
  node: ClusterNode,
  auto: boolean,
): { tone: 'good' | 'bad' | 'pending' | 'idle'; mark: string; text: string } | null {
  if (node.connected) {
    return { tone: 'good', mark: '●', text: t('settings.integrations.clusterNodes.status.inUse') }
  }
  if (node.running && node.failure) {
    return {
      tone: 'bad',
      mark: '▲',
      text: t('settings.integrations.clusterNodes.status.failing', { reason: reasonText(node.failure) }),
    }
  }
  if (node.running) {
    return { tone: 'pending', mark: '◌', text: t('settings.integrations.clusterNodes.status.connecting') }
  }
  if (!auto) return null
  if (node.skippedUntilUnix != null && node.failure) {
    return {
      tone: 'bad',
      mark: '▲',
      text: t('settings.integrations.clusterNodes.status.skipped', {
        reason: reasonText(node.failure),
        time: clockTime(node.skippedUntilUnix),
      }),
    }
  }
  return { tone: 'idle', mark: '○', text: t('settings.integrations.clusterNodes.status.standby') }
}

/** A node's mark and words, or nothing when there is no honest standing to show. */
function Standing({ node, auto }: { node: ClusterNode | undefined; auto: boolean }) {
  const standing = node && nodeStanding(node, auto)
  if (!standing) return null
  return (
    <>
      <span className={`cluster-node-mark ${standing.tone}`} aria-hidden="true">
        {standing.mark}
      </span>
      <span className={`cluster-node-standing ${standing.tone}`}>{standing.text}</span>
    </>
  )
}

export interface SettingsClusterNodesProps {
  /** The form's choice. */
  auto: boolean
  onAutoChange: (auto: boolean) => void
  /** The form's own list. */
  hosts: string[]
  /** Edits the form's list — a function of the latest list, so rapid edits never race. */
  onHostsChange: (edit: (hosts: string[]) => string[]) => void
  /** Each node's standing; null until `get_cluster_nodes` has answered. */
  standings: ClusterNodes | null
  disabled: boolean
  /** The example node address shown in an empty row — an invariant token, not prose. */
  placeholder: string
}

export function SettingsClusterNodes({
  auto,
  onAutoChange,
  hosts,
  onHostsChange,
  standings,
  disabled,
  placeholder,
}: SettingsClusterNodesProps) {
  const nodes = standings?.nodes ?? []
  const builtIn = nodes.filter((n) => n.callsign)
  // Standings describe the SAVED choice; see the module comment.
  const live = standings != null && standings.auto === auto
  const standingOf = (host: string) =>
    live ? nodes.find((n) => n.host.toLowerCase() === host.trim().toLowerCase()) : undefined

  return (
    <div className="settings-field">
      <span className="settings-label" id="cluster-nodes-label">
        {t('settings.integrations.clusterNodes.label')}
      </span>
      <div className="cluster-node-mode" role="radiogroup" aria-labelledby="cluster-nodes-label">
        <label>
          <input
            type="radio"
            name="cluster-node-mode"
            checked={auto}
            disabled={disabled}
            onChange={() => onAutoChange(true)}
          />
          {t('settings.integrations.clusterNodes.mode.auto')}
        </label>
        <label>
          <input
            type="radio"
            name="cluster-node-mode"
            checked={!auto}
            disabled={disabled}
            onChange={() => onAutoChange(false)}
          />
          {t('settings.integrations.clusterNodes.mode.manual')}
        </label>
      </div>

      {auto ? (
        <>
          <ul className="cluster-node-list">
            {builtIn.map((node) => {
              const standing = live ? nodeStanding(node, true) : null
              return (
                <li key={node.host} className="cluster-node-row">
                  {standing && (
                    <span className={`cluster-node-mark ${standing.tone}`} aria-hidden="true">
                      {standing.mark}
                    </span>
                  )}
                  <code className="cluster-node-host">{node.host}</code>
                  {standing && (
                    <span className={`cluster-node-standing ${standing.tone}`}>{standing.text}</span>
                  )}
                </li>
              )
            })}
          </ul>
          <span className="settings-hint">{t('settings.integrations.clusterNodes.autoHint')}</span>
        </>
      ) : (
        <>
          {hosts.length === 0 ? (
            <span className="settings-hint cluster-node-empty">
              {t('settings.integrations.clusterNodes.empty')}
            </span>
          ) : (
            hosts.map((host, i) => (
              <div key={i} className="cluster-node-row">
                <input
                  disabled={disabled}
                  className="settings-input"
                  value={host}
                  onChange={(e) => onHostsChange((hs) => hs.map((h, j) => (j === i ? e.target.value : h)))}
                  placeholder={placeholder}
                  spellCheck={false}
                />
                <Standing node={standingOf(host)} auto={false} />
                <button
                  disabled={disabled}
                  type="button"
                  className="cluster-node-remove"
                  title={t('settings.integrations.clusterNodes.remove.title')}
                  aria-label={
                    host
                      ? t('settings.integrations.clusterNodes.remove.aria', { host })
                      : t('settings.integrations.clusterNodes.remove.ariaBlank')
                  }
                  onClick={() => onHostsChange((hs) => hs.filter((_, j) => j !== i))}
                >
                  ✕
                </button>
              </div>
            ))
          )}
          <div className="cluster-node-add">
            <select
              disabled={disabled}
              className="settings-input"
              value=""
              onChange={(e) => {
                const host = e.target.value
                if (!host) return
                onHostsChange((hs) =>
                  hs.some((h) => h.trim().toLowerCase() === host.toLowerCase()) ? hs : [...hs, host],
                )
              }}
            >
              <option value="">{t('settings.integrations.clusterNodes.add.option')}</option>
              {/* The known nodes are the ones built into this release — the same list the
                  automatic choice picks from, never a second copy that can drift. */}
              {builtIn.map((node) => (
                <option key={node.host} value={node.host}>
                  {t('settings.integrations.clusterNodes.preset', {
                    callsign: node.callsign ?? '',
                    software: node.software ? SOFTWARE_NAMES[node.software] : '',
                    port: node.host.slice(node.host.lastIndexOf(':') + 1),
                  })}
                </option>
              ))}
            </select>
            <button
              disabled={disabled}
              type="button"
              className="cluster-node-add-blank"
              title={t('settings.integrations.clusterNodes.addCustom.title')}
              onClick={() => onHostsChange((hs) => [...hs, ''])}
            >
              {t('settings.integrations.clusterNodes.addCustom.action')}
            </button>
          </div>
          <span className="settings-hint">{t('settings.integrations.clusterNodes.hint')}</span>
        </>
      )}
    </div>
  )
}
