// Memories — the first-class saved-channel manager: repeaters, HF nets, calling
// freqs, POTA/SOTA watering holes. Groups + ★ favorites in a sidebar; a hybrid
// main pane (clean LIST with an inline editor by default, a CHIRP-style GRID on
// demand); one-click Tune (App's recallMemory applies freq + mode + shift + tone
// and auto-switches to the right cockpit); full CHIRP CSV round-trip so channels
// flow Nexus ⇄ CHIRP ⇄ real radios. Store + model live in features/memories.ts
// (shared with the cockpit MemoryStrip and the Program section's save).
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT, because this screen is nearly all units: every
// dial and TX frequency, every offset, every CTCSS tone and DTCS code, the mode names and the
// mode/CTCSS datalists, the callsign, the group names the operator typed, the HF and VHF/UHF
// section labels, the POTA/SOTA programme names, the `value` of every <select> (the label is
// prose, the value is what is stored), and the export FILE NAME's slug. The weekday
// abbreviations are date formatting rather than catalog prose — the same ruling the DXpedition
// calendar carries — and stay here with the rest of the schedule handling.
import { Fragment, useEffect, useRef, useState, type InputHTMLAttributes } from 'react'
import {
  addGroup,
  addMemory,
  addMemoryDeduped,
  deleteGroup,
  deleteMemory,
  memoriesStore,
  moveFavorite,
  moveMemory,
  newMemoryId,
  parseChirpCsv,
  renameGroup,
  setMemoryGroups,
  siteOffset,
  STRIP_FAVORITE_LIMIT,
  toChirpCsv,
  toggleFavorite,
  updateMemory,
  useMemories,
  type Memory,
  type MemoryKind,
  type OffsetDir,
  type ToneMode,
} from '../features/memories'
import { octant } from '../features/radioprog'
import { fmtDistanceKm, useUnits, type Units } from '../units'
import { importPack, STARTER_PACKS, type Pack } from '../features/packs'
import { saveTextToDownloads } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { modChord } from '../platform'
import { parseOperatorNumber } from '../numInput'

export interface MemoriesViewProps {
  /** Current dial (MHz) + mode — what "Save current" captures. */
  dialMhz: number
  dialMode: string
  /** Recall = tune (App's recallMemory: settings + retune + cockpit switch). */
  onRecall: (m: Memory) => void
  /** Station grid from Settings — how far away a starred repeater is, recomputed
   * from here on every render (operate portable and the mileage follows you). */
  myGrid?: string
  /** Pop this section out into its own window (hidden when already detached). */
  onPopOut?: () => void
}

/** The sidebar's built-in views ahead of the custom groups. */
type Selection = 'all' | 'fav' | 'nets' | { group: string }

/** The programmes' own names — the same letters in every language (the rule is in i18n/index.ts). */
const POTA_SOTA_PROGRAMS = 'POTA/SOTA'

/**
 * The channel-kind words. Each KEY is the stored value (a token); only the word is prose.
 *
 * They resolve LAZILY, through getters, for the reason `features/needVisuals.ts` documents:
 * this is a module constant read during render, so looking the words up at import time would
 * freeze whichever locale happened to load first. The record's shape is unchanged.
 */
const KIND_LABEL: Record<MemoryKind, string> = {
  get repeater() {
    return t('memories.kind.repeater')
  },
  get simplex() {
    return t('memories.kind.simplex')
  },
  get hfnet() {
    return t('memories.kind.hfnet')
  },
  get calling() {
    return t('memories.kind.calling')
  },
  get pota() {
    return POTA_SOTA_PROGRAMS
  },
  get digital() {
    return t('memories.kind.digital')
  },
  get satellite() {
    return t('memories.kind.satellite')
  },
  get emcomm() {
    return t('memories.kind.emcomm')
  },
  get reference() {
    return t('memories.kind.reference')
  },
  get other() {
    return t('memories.kind.other')
  },
}

const MODE_SUGGESTIONS = ['USB', 'LSB', 'FM', 'NFM', 'AM', 'CW', 'FT8', 'FT4']
// The standard CTCSS ladder (EIA) — a datalist so typing is optional.
const CTCSS_SUGGESTIONS = [
  67, 71.9, 74.4, 77, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100, 103.5, 107.2, 110.9, 114.8,
  118.8, 123, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 162.2, 167.9, 173.8, 179.9, 186.2,
  192.8, 203.5, 210.7, 218.1, 225.7, 233.6, 241.8, 250.3,
]
const DAY_LABELS = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa']

/** An uncontrolled input that COMMITS on blur/Enter instead of per keystroke.
 * The validated fields (freq/mode/name…) reject invalid intermediate states, so a
 * controlled write-through input would snap back mid-typing (you couldn't even
 * type the "." in "146.52" — Number("146.") re-renders as "146"). `resetKey`
 * re-seeds the draft when the row (or its stored value) changes under it. */
function CommitInput({
  value,
  onCommit,
  resetKey,
  ...rest
}: {
  value: string
  onCommit: (v: string) => void
  resetKey: string
} & Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange'>) {
  return (
    <input
      key={resetKey}
      defaultValue={value}
      onBlur={(e) => {
        const el = e.target
        if (el.value !== value) {
          onCommit(el.value)
          // If the commit was rejected (store unchanged → no remount via resetKey),
          // snap the draft back so a garbage edit never LOOKS saved.
          requestAnimationFrame(() => {
            if (el.isConnected) el.value = value
          })
        }
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
        if (e.key === 'Escape') {
          ;(e.target as HTMLInputElement).value = value
          ;(e.target as HTMLInputElement).blur()
        }
      }}
      {...rest}
    />
  )
}

/**
 * Apply an operator-typed number, or do nothing at all.
 *
 * Every numeric field here used to commit a bare `Number(v)` (Greek-Windows report, 2026-08),
 * which is two bugs in one expression: `Number('145,5')` is `NaN` on any comma-decimal locale,
 * and — far worse — `Number('')` is `0`, so a rejected entry moved the channel to 0 MHz.
 *
 * ⚠️ **THE DECIMAL FIELDS ARE `inputMode="decimal"`, NOT `type="number"`, AND THAT IS THE
 * ACTUAL FIX — DO NOT PUT `type="number"` BACK.** A number input never hands JavaScript what
 * the operator typed: the UA runs its own LOCALE-AWARE sanitisation first, and on a
 * comma-decimal locale `.` is the GROUP separator. So `14.074` typed into a number input on a
 * Greek/German Windows arrives at `.value` as `"14074"` — a valid number, a plausible number,
 * and a memory channel 14 GHz off frequency, with nothing for a guard to catch. Text with
 * `inputMode="decimal"` keeps the phone keypad and hands `parseOperatorNumber` the literal
 * characters, which is what the other three numeric sites in the app already do. `step` went
 * with the type; it is inert on a text input, and the spinner it drove was never the point.
 *
 * ⚠️ The `Number.isFinite` guard is NOT redundant with `coerceMemory`'s `posNum`. That rejects
 * a bad number, but for the OPTIONAL fields (`offsetMhz`, `txMhz`, `ctcssEncHz`, `dtcsCode`)
 * rejecting means the key is left off the rebuilt memory — so a fumbled edit did not fail, it
 * silently DELETED a repeater's offset or tone. Not committing at all leaves the stored value
 * where it was, and `CommitInput` snaps the draft back so the bad text never looks saved.
 */
function withNumber(v: string, apply: (n: number) => void): void {
  const n = parseOperatorNumber(v)
  if (Number.isFinite(n)) apply(n)
}

/** One-line offset/tone summary for a row ("−0.600 · 103.5" / "→52.030"). */
function rowSummary(m: Memory, myGrid: string, units: Units): string {
  const parts: string[] = []
  if (m.offsetDir === 'plus' || m.offsetDir === 'minus') {
    parts.push(`${m.offsetDir === 'plus' ? '+' : '−'}${(m.offsetMhz ?? 0).toFixed(3)}`)
  } else if (m.offsetDir === 'split' && m.txMhz !== undefined) {
    parts.push(`→${m.txMhz.toFixed(3)}`)
  }
  if ((m.toneMode === 'tone' || m.toneMode === 'tsql') && m.ctcssEncHz) {
    parts.push(m.ctcssEncHz.toFixed(1))
  }
  if (m.toneMode === 'dtcs' && m.dtcsCode) parts.push(`D${m.dtcsCode}`)
  if (m.net) parts.push(`${m.net.days.map((d) => DAY_LABELS[d]).join('')} ${m.net.utcTime}z`)
  // Repeaters starred from the Program picker know where they physically are.
  const off = siteOffset(m, myGrid)
  if (off) parts.push(`${fmtDistanceKm(off.km, units)} ${octant(off.bearing)}`)
  return parts.join(' · ')
}

export function MemoriesView({
  dialMhz,
  dialMode,
  onRecall,
  myGrid = '',
  onPopOut,
}: MemoriesViewProps) {
  const bank = useMemories()
  const units = useUnits()
  const [sel, setSel] = useState<Selection>('all')
  const [q, setQ] = useState('')
  const [grid, setGrid] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [newGroupName, setNewGroupName] = useState('')
  const [renamingGroup, setRenamingGroup] = useState<string | null>(null)
  const [sort, setSort] = useState<{ col: 'name' | 'rxMhz' | 'mode' | 'kind'; dir: 1 | -1 } | null>(null)
  const [showPacks, setShowPacks] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  const groupSel = typeof sel === 'object' ? sel.group : null
  const groupName = bank.groups.find((g) => g.id === groupSel)?.name
  // The export FILE NAME is built from an invariant slug, never from the view's displayed name:
  // the ASCII squeeze in `exportCsv` would strip a translated name to nothing on any locale that
  // does not write in ASCII, and every export would come out called `nexus-memories-.csv`.
  const selSlug =
    sel === 'all' ? 'all' : sel === 'fav' ? 'favorites' : sel === 'nets' ? 'nets' : (groupName ?? 'group')
  // One whole placeholder per view rather than a name spliced into "Search …": lower-casing a
  // translated noun is wrong in every language that capitalises them. The GROUP name is the
  // operator's own text, so it keeps the lower-casing it has always had.
  const searchPlaceholder =
    sel === 'all'
      ? t('memories.search.placeholder.all')
      : sel === 'fav'
        ? t('memories.search.placeholder.fav')
        : sel === 'nets'
          ? t('memories.search.placeholder.nets')
          : groupName
            ? t('memories.search.placeholder.group', { group: groupName.toLowerCase() })
            : t('memories.search.placeholder.groupless')

  const query = q.trim().toLowerCase()
  const filtered = bank.memories.filter((m) => {
    if (sel === 'fav' && !m.favorite) return false
    if (sel === 'nets' && m.kind !== 'hfnet') return false
    if (groupSel && !m.groups.includes(groupSel)) return false
    if (!query) return true
    return (
      m.name.toLowerCase().includes(query) ||
      m.mode.toLowerCase().includes(query) ||
      (m.callsign ?? '').toLowerCase().includes(query) ||
      (m.notes ?? '').toLowerCase().includes(query) ||
      m.rxMhz.toFixed(4).includes(query)
    )
  })
  // ★ Favorites (in the LIST) is the cockpit MEM strip written down: row n is chip n. Its
  // order therefore has to be the MASTER order the strip and Ctrl+1..9 read — no column
  // sort, no band sections, both of which would show a truthful set in a misleading order.
  // Ranking is what makes the strip's cap legible and its ▲▼ meaningful. The Grid stays
  // the CHIRP spreadsheet under every selection, sortable and sectioned as before.
  const rankView = sel === 'fav' && !grid
  const shown =
    sort && !rankView
      ? [...filtered].sort((a, b) => {
          const av = a[sort.col]
          const bv = b[sort.col]
          const c = typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv))
          return c * sort.dir
        })
      : filtered

  // Rank = position among ALL favorites in master order — not among the rows on screen, so
  // it still names the right chip while a search narrows the list.
  const favRank = new Map(bank.memories.filter((m) => m.favorite).map((m, i) => [m.id, i + 1]))

  // Organize the list cleanly by band range: HF (< 30 MHz — 10 m at 28-29.7 stays HF) then
  // VHF/UHF (>= 30 MHz — 6 m at 50 up). Applies everywhere this list shows: All memories AND a
  // pack/group's channels. The active sort is preserved within each section; a section header
  // only shows when the view actually spans both ranges (an all-HF group needs no label).
  const HF_MAX_MHZ = 30
  const bandSections = rankView
    ? [{ key: 'rank', label: '', rows: shown }]
    : [
        { key: 'hf', label: 'HF', rows: shown.filter((m) => m.rxMhz < HF_MAX_MHZ) },
        { key: 'vu', label: 'VHF / UHF', rows: shown.filter((m) => m.rxMhz >= HF_MAX_MHZ) },
      ].filter((s) => s.rows.length > 0)
  const showSectionHeaders = !rankView && bandSections.length > 1

  const commit = (fn: (b: typeof bank) => typeof bank) => memoriesStore.update(fn)

  // An operator edit to a channel's CONTENT makes the row theirs: a pack re-install
  // reconciles only rows the pack still owns (source 'curated'), so this stamp is what
  // protects the edit from being overwritten by a later corrected pack. Favorite, group
  // and recall changes go through their own verbs and deliberately do NOT stamp — they
  // aren't content, and a starred pack channel should still receive pack corrections.
  const editRow = (id: string, patch: Partial<Memory>) =>
    commit((b) => updateMemory(b, id, { ...patch, source: 'user' }))

  // Initial focus goes to the DIALOG CONTAINER, not the ✕: autoFocus on the close
  // button meant the Enter that opened Starter packs immediately dismissed it.
  const packsRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (showPacks) packsRef.current?.focus({ preventScroll: true })
  }, [showPacks])

  // Escape closes the starter-packs dialog (it's also dismissable by backdrop click / ✕).
  useEffect(() => {
    if (!showPacks) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setShowPacks(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [showPacks])

  const installPack = (pack: Pack) => {
    let added = 0
    let updated = 0
    commit((b) => {
      const r = importPack(b, pack)
      added = r.added
      updated = r.updated
      return r.bank
    })
    // Report added and refreshed separately — "already up to date" is only honest when
    // the re-install genuinely changed nothing. Each clause is its OWN message with its own
    // plural: one entry cannot select a form for two different counts.
    const parts: string[] = []
    if (added > 0) parts.push(t('memories.packs.toast.added', { count: added }))
    if (updated > 0) parts.push(t('memories.packs.toast.refreshed', { count: updated }))
    pushToast(
      parts.length > 0
        ? t('memories.packs.toast', { pack: pack.name, parts: parts.join(', ') })
        : t('memories.packs.toast.upToDate', { pack: pack.name }),
      'success',
      3000,
    )
  }

  const saveCurrent = () => {
    let added = false
    commit((b) => {
      const res = addMemoryDeduped(b, {
        rxMhz: dialMhz,
        mode: dialMode,
        favorite: sel === 'fav', // saving while looking at Favorites stars it
        groups: groupSel ? [groupSel] : [],
      })
      added = res.added
      return res.bank
    })
    pushToast(
      added
        ? t('memories.toast.saved', { freq: dialMhz.toFixed(3), mode: dialMode })
        : t('memories.toast.alreadySaved', { freq: dialMhz.toFixed(3), mode: dialMode }),
      added ? 'success' : 'info',
      2500,
    )
  }

  const addNew = () => {
    // ⚠️ CLEAR THE SEARCH FIRST, or the button looks broken. `addNew` already matches the
    // view's OTHER filters — it stars the row under Favorites, makes it a net under Nets,
    // joins the selected group — but a search box was never one of them. A new memory has an
    // empty name, no callsign and no notes, so any active query filters it straight back out:
    // the row is created, the editor opens on a row nobody can see, and the operator presses
    // the button again. Reported as "I cannot hit the add memories button".
    setQ('')
    const id = newMemoryId()
    commit((b) =>
      addMemory(b, {
        id,
        rxMhz: dialMhz > 0 ? dialMhz : 14.074,
        mode: dialMode || 'USB',
        // Match the active view's filter so the new row is actually visible (and its editor
        // opens) instead of being created invisibly: star it under Favorites, make it a net
        // under Nets, join the selected group under a group.
        favorite: sel === 'fav',
        kind: sel === 'nets' ? 'hfnet' : undefined,
        groups: groupSel ? [groupSel] : [],
      }),
    )
    setEditingId(id)
  }

  const exportCsv = () => {
    if (shown.length === 0) {
      pushToast(t('memories.export.empty'), 'info', 2500)
      return
    }
    const name = `nexus-memories-${selSlug.toLowerCase().replace(/[^a-z0-9]+/g, '-')}.csv`
    void saveTextToDownloads(name, toChirpCsv(shown))
      .then((path) =>
        pushToast(t('memories.export.done', { count: shown.length, path }), 'success', 5000),
      )
      .catch((e) => pushToast(String(e), 'error'))
  }

  const importCsv = (file: File) => {
    void file
      .text()
      .then((text) => {
        const rows = parseChirpCsv(text)
        if (rows.length === 0) {
          pushToast(t('memories.import.notChirp'), 'info', 4000)
          return
        }
        let added = 0
        commit((b) => {
          let next = b
          for (const r of rows) {
            const res = addMemoryDeduped(next, { ...r, groups: groupSel ? [groupSel] : [] })
            next = res.bank
            if (res.added) added += 1
          }
          return next
        })
        const skipped = rows.length - added
        // Two counts, two messages, each with its own plural — and the skipped clause carries
        // its own leading space so a locale that drops it is not left with a double space.
        pushToast(
          t('memories.import.done', { count: added }) +
            (skipped ? t('memories.import.dupes', { count: skipped }) : ''),
          'success',
          5000,
        )
      })
      .catch((e) => pushToast(String(e), 'error'))
  }

  // ---- the inline editor (list view) --------------------------------------
  const editor = (m: Memory) => {
    const up = (patch: Partial<Memory>) => editRow(m.id, patch)
    const showOffset = m.kind === 'repeater' || m.kind === 'simplex' || m.kind === 'calling'
    return (
      <div className="mv-editor">
        <label className="mv-field">
          <span>{t('memories.editor.name.label')}</span>
          <CommitInput resetKey={`${m.id}:name:${m.name}`} value={m.name} onCommit={(v) => up({ name: v })} />
        </label>
        <label className="mv-field">
          <span>{t('memories.editor.kind.label')}</span>
          <select value={m.kind} onChange={(e) => up({ kind: e.target.value as MemoryKind })}>
            {(Object.keys(KIND_LABEL) as MemoryKind[]).map((k) => (
              <option key={k} value={k}>
                {KIND_LABEL[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="mv-field">
          <span>{t('memories.editor.rx.label')}</span>
          <CommitInput
            resetKey={`${m.id}:rx:${m.rxMhz}`}
            inputMode="decimal"
            value={String(m.rxMhz)}
            onCommit={(v) => withNumber(v, (n) => up({ rxMhz: n }))}
          />
        </label>
        <label className="mv-field">
          <span>{t('memories.editor.mode.label')}</span>
          <CommitInput
            resetKey={`${m.id}:mode:${m.mode}`}
            list="mv-modes"
            value={m.mode}
            onCommit={(v) => up({ mode: v })}
          />
        </label>
        {showOffset && (
          <>
            <label className="mv-field">
              <span>{t('memories.editor.offset.label')}</span>
              {/* The LABEL is prose; the `value` is what is stored and sent to the radio. */}
              <select
                value={m.offsetDir ?? 'simplex'}
                onChange={(e) => up({ offsetDir: e.target.value as OffsetDir })}
              >
                <option value="simplex">{t('memories.editor.offset.simplex')}</option>
                <option value="plus">{t('memories.editor.offset.plus')}</option>
                <option value="minus">{t('memories.editor.offset.minus')}</option>
                <option value="split">{t('memories.editor.offset.split')}</option>
              </select>
            </label>
            {(m.offsetDir === 'plus' || m.offsetDir === 'minus') && (
              <label className="mv-field">
                <span>{t('memories.editor.offsetMhz.label')}</span>
                <CommitInput
                  resetKey={`${m.id}:off:${m.offsetMhz ?? 0}`}
                  inputMode="decimal"
                  value={String(m.offsetMhz ?? 0)}
                  onCommit={(v) => withNumber(v, (n) => up({ offsetMhz: n }))}
                />
              </label>
            )}
            {m.offsetDir === 'split' && (
              <label className="mv-field">
                <span>{t('memories.editor.txMhz.label')}</span>
                <CommitInput
                  resetKey={`${m.id}:tx:${m.txMhz ?? m.rxMhz}`}
                  inputMode="decimal"
                  value={String(m.txMhz ?? m.rxMhz)}
                  onCommit={(v) => withNumber(v, (n) => up({ txMhz: n }))}
                />
              </label>
            )}
            <label className="mv-field">
              <span>{t('memories.editor.tone.label')}</span>
              <select
                value={m.toneMode ?? 'none'}
                onChange={(e) => up({ toneMode: e.target.value as ToneMode })}
              >
                <option value="none">{t('memories.editor.tone.none')}</option>
                <option value="tone">{t('memories.editor.tone.tone')}</option>
                <option value="tsql">{t('memories.editor.tone.tsql')}</option>
                <option value="dtcs">{t('memories.editor.tone.dtcs')}</option>
              </select>
            </label>
            {(m.toneMode === 'tone' || m.toneMode === 'tsql') && (
              <label className="mv-field">
                <span>{t('memories.editor.ctcss.label')}</span>
                <CommitInput
                  resetKey={`${m.id}:ctcss:${m.ctcssEncHz ?? ''}`}
                  list="mv-ctcss"
                  inputMode="decimal"
                  value={m.ctcssEncHz != null ? String(m.ctcssEncHz) : ''}
                  onCommit={(v) => withNumber(v, (n) => up({ ctcssEncHz: n }))}
                />
              </label>
            )}
            {m.toneMode === 'dtcs' && (
              <label className="mv-field">
                <span>{t('memories.editor.dtcs.label')}</span>
                <CommitInput
                  resetKey={`${m.id}:dtcs:${m.dtcsCode ?? ''}`}
                  type="number"
                  value={m.dtcsCode != null ? String(m.dtcsCode) : ''}
                  onCommit={(v) => withNumber(v, (n) => up({ dtcsCode: n }))}
                />
              </label>
            )}
            <label className="mv-field">
              <span>{t('memories.editor.callsign.label')}</span>
              <CommitInput
                resetKey={`${m.id}:call:${m.callsign ?? ''}`}
                value={m.callsign ?? ''}
                onCommit={(v) => up({ callsign: v })}
              />
            </label>
          </>
        )}
        {m.kind === 'hfnet' && (
          <>
            <div className="mv-field mv-days">
              <span>{t('memories.editor.days.label')}</span>
              <div className="mv-daychips" role="group" aria-label={t('memories.editor.days.aria')}>
                {DAY_LABELS.map((d, i) => {
                  const days = m.net?.days ?? []
                  const on = days.includes(i)
                  return (
                    <button
                      key={d}
                      type="button"
                      className={`mv-daychip${on ? ' on' : ''}`}
                      aria-pressed={on}
                      onClick={() => {
                        const next = on ? days.filter((x) => x !== i) : [...days, i].sort()
                        up({
                          net: {
                            ...m.net,
                            days: next,
                            utcTime: m.net?.utcTime ?? '00:00',
                            alertEnabled: m.net?.alertEnabled ?? false,
                            alertLeadMin: m.net?.alertLeadMin ?? 10,
                          },
                        })
                      }}
                    >
                      {d}
                    </button>
                  )
                })}
              </div>
            </div>
            <label className="mv-field">
              <span>{t('memories.editor.start.label')}</span>
              <input
                type="time"
                value={m.net?.utcTime ?? ''}
                onChange={(e) =>
                  e.target.value &&
                  up({
                    net: {
                      ...m.net,
                      days: m.net?.days ?? [],
                      utcTime: e.target.value,
                      alertEnabled: m.net?.alertEnabled ?? false,
                      alertLeadMin: m.net?.alertLeadMin ?? 10,
                    },
                  })
                }
              />
            </label>
            <div className="mv-field mv-net-alert">
              <span>{t('memories.editor.remind.label')}</span>
              <span className="mv-net-alert-row">
                <input
                  type="checkbox"
                  aria-label={t('memories.editor.remind.aria')}
                  checked={m.net?.alertEnabled ?? false}
                  onChange={(e) =>
                    up({
                      net: {
                        ...m.net,
                        days: m.net?.days ?? [],
                        utcTime: m.net?.utcTime ?? '00:00',
                        alertLeadMin: m.net?.alertLeadMin ?? 10,
                        alertEnabled: e.target.checked,
                      },
                    })
                  }
                />
                <CommitInput
                  type="number"
                  min={1}
                  max={120}
                  aria-label={t('memories.editor.lead.aria')}
                  resetKey={`${m.id}:lead:${m.net?.alertLeadMin ?? 10}`}
                  value={String(m.net?.alertLeadMin ?? 10)}
                  onCommit={(v) => {
                    const lead = Math.max(1, Math.min(120, Math.round(Number(v) || 10)))
                    up({
                      net: {
                        ...m.net,
                        days: m.net?.days ?? [],
                        utcTime: m.net?.utcTime ?? '00:00',
                        alertEnabled: m.net?.alertEnabled ?? false,
                        alertLeadMin: lead,
                      },
                    })
                  }}
                />
                <span className="mv-net-alert-unit">{t('memories.editor.lead.unit')}</span>
              </span>
            </div>
          </>
        )}
        <label className="mv-field mv-notes">
          <span>{t('memories.editor.notes.label')}</span>
          <CommitInput
            resetKey={`${m.id}:notes:${m.notes ?? ''}`}
            value={m.notes ?? ''}
            onCommit={(v) => up({ notes: v })}
          />
        </label>
        {bank.groups.length > 0 && (
          <div className="mv-field mv-groups">
            <span>{t('memories.editor.groups.label')}</span>
            <div className="mv-groupchips" role="group" aria-label={t('memories.editor.groups.aria')}>
              {bank.groups.map((g) => {
                const on = m.groups.includes(g.id)
                return (
                  <button
                    key={g.id}
                    type="button"
                    className={`mv-daychip${on ? ' on' : ''}`}
                    aria-pressed={on}
                    onClick={() =>
                      commit((b) =>
                        setMemoryGroups(b, m.id, on ? m.groups.filter((x) => x !== g.id) : [...m.groups, g.id]),
                      )
                    }
                  >
                    {g.name}
                  </button>
                )
              })}
            </div>
          </div>
        )}
        <button type="button" className="mv-editor-done" onClick={() => setEditingId(null)}>
          {t('memories.editor.done')}
        </button>
      </div>
    )
  }

  // ---- render --------------------------------------------------------------
  const sideItem = (key: Selection, label: string, count: number) => {
    const active =
      key === sel || (typeof key === 'object' && typeof sel === 'object' && key.group === sel.group)
    return (
      <button
        type="button"
        className={`mv-side-item${active ? ' active' : ''}`}
        onClick={() => setSel(key)}
      >
        <span className="mv-side-name">{label}</span>
        <span className="mv-side-count">{count}</span>
      </button>
    )
  }

  const invalidRow = (m: Memory) => !(m.rxMhz > 0) || !m.mode
  const th = (col: NonNullable<typeof sort>['col'], label: string) => (
    <th
      aria-sort={sort?.col === col ? (sort.dir === 1 ? 'ascending' : 'descending') : 'none'}
      className={sort?.col === col ? `sorted${sort.dir === 1 ? ' asc' : ' desc'}` : undefined}
    >
      <button
        type="button"
        className="mv-sort"
        onClick={() =>
          setSort((s) => (s?.col === col ? (s.dir === 1 ? { col, dir: -1 } : null) : { col, dir: 1 }))
        }
      >
        {label}
        {sort?.col === col ? (sort.dir === 1 ? ' ▲' : ' ▼') : ''}
      </button>
    </th>
  )

  return (
    <section className="memories-view" aria-label={t('memories.aria')}>
      <datalist id="mv-modes">
        {MODE_SUGGESTIONS.map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <datalist id="mv-ctcss">
        {CTCSS_SUGGESTIONS.map((t) => (
          <option key={t} value={t} />
        ))}
      </datalist>

      {showPacks && (
        <div className="mv-packs-overlay" onClick={() => setShowPacks(false)}>
          <div
            className="mv-packs"
            role="dialog"
            aria-modal="true"
            aria-labelledby="mv-packs-title"
            tabIndex={-1}
            ref={packsRef}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mv-packs-head">
              <h3 id="mv-packs-title">{t('memories.packs.title')}</h3>
              <button
                type="button"
                className="mv-packs-close"
                onClick={() => setShowPacks(false)}
                aria-label={t('memories.packs.close.aria')}
              >
                ✕
              </button>
            </div>
            <p className="mv-packs-sub">{t('memories.packs.sub')}</p>
            <ul className="mv-packs-list">
              {STARTER_PACKS.map((pack) => {
                const installed = bank.groups.some((g) => g.name === pack.name)
                return (
                  <li key={pack.id} className="mv-pack">
                    <div className="mv-pack-info">
                      <span className="mv-pack-name">{pack.name}</span>
                      <span className="mv-pack-desc">{pack.description}</span>
                      <span className="mv-pack-meta">
                        {t('memories.packs.meta', {
                          count: pack.memories.length,
                          region: pack.region,
                        })}
                      </span>
                    </div>
                    <button type="button" className="mv-pack-add" onClick={() => installPack(pack)}>
                      {installed ? t('memories.packs.update') : t('memories.packs.install')}
                    </button>
                  </li>
                )
              })}
            </ul>
          </div>
        </div>
      )}

      <aside className="mv-side">
        {sideItem('all', t('memories.side.all'), bank.memories.length)}
        {sideItem('fav', t('memories.side.fav'), bank.memories.filter((m) => m.favorite).length)}
        {sideItem('nets', t('memories.side.nets'), bank.memories.filter((m) => m.kind === 'hfnet').length)}
        {bank.groups.length > 0 && <div className="mv-side-sep" />}
        {bank.groups.map((g) => (
          <div key={g.id} className="mv-side-group">
            {renamingGroup === g.id ? (
              <input
                autoFocus
                defaultValue={g.name}
                onBlur={(e) => {
                  commit((b) => renameGroup(b, g.id, e.target.value))
                  setRenamingGroup(null)
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                  if (e.key === 'Escape') setRenamingGroup(null)
                }}
              />
            ) : (
              sideItem({ group: g.id }, g.name, bank.memories.filter((m) => m.groups.includes(g.id)).length)
            )}
            {groupSel === g.id && renamingGroup !== g.id && (
              <span className="mv-side-tools">
                <button
                  type="button"
                  onClick={() => setRenamingGroup(g.id)}
                  title={t('memories.side.group.rename.title')}
                >
                  ✎
                </button>
                <button
                  type="button"
                  onClick={() => {
                    commit((b) => deleteGroup(b, g.id))
                    setSel('all')
                  }}
                  title={t('memories.side.group.delete.title')}
                >
                  ✕
                </button>
              </span>
            )}
          </div>
        ))}
        <form
          className="mv-side-add"
          onSubmit={(e) => {
            e.preventDefault()
            commit((b) => addGroup(b, newGroupName))
            setNewGroupName('')
          }}
        >
          <input
            value={newGroupName}
            placeholder={t('memories.side.newGroup.placeholder')}
            onChange={(e) => setNewGroupName(e.target.value)}
          />
          <button type="submit" disabled={!newGroupName.trim()}>
            ＋
          </button>
        </form>
      </aside>

      <div className="mv-main">
        <div className="mv-toolbar">
          <input
            className="mv-search"
            value={q}
            placeholder={searchPlaceholder}
            onChange={(e) => setQ(e.target.value)}
          />
          <button
            type="button"
            className={`mv-tool${grid ? '' : ' active'}`}
            onClick={() => setGrid(false)}
            title={t('memories.toolbar.list.title')}
          >
            {t('memories.toolbar.list.label')}
          </button>
          <button
            type="button"
            className={`mv-tool${grid ? ' active' : ''}`}
            onClick={() => setGrid(true)}
            title={t('memories.toolbar.grid.title')}
          >
            {t('memories.toolbar.grid.label')}
          </button>
          <span className="mv-toolbar-gap" />
          <button
            type="button"
            className="mv-tool"
            onClick={saveCurrent}
            title={t('memories.toolbar.save.title')}
          >
            {t('memories.toolbar.save.label', {
              freq: dialMhz > 0 ? dialMhz.toFixed(3) : '—',
              mode: dialMode,
            })}
          </button>
          <button
            type="button"
            className="mv-tool"
            onClick={addNew}
            title={t('memories.toolbar.new.title')}
          >
            {t('memories.toolbar.new.label')}
          </button>
          <button
            type="button"
            className="mv-tool"
            onClick={() => fileRef.current?.click()}
            title={t('memories.toolbar.import.title')}
          >
            {t('memories.toolbar.import.label')}
          </button>
          <button
            type="button"
            className="mv-tool"
            onClick={exportCsv}
            title={t('memories.toolbar.export.title', { count: shown.length })}
          >
            {t('memories.toolbar.export.label', { count: shown.length })}
          </button>
          {onPopOut && (
            <button
              type="button"
              className="mv-tool"
              onClick={onPopOut}
              title={t('memories.toolbar.popOut.title')}
            >
              {t('memories.toolbar.popOut.label')}
            </button>
          )}
          <button
            type="button"
            className="mv-tool"
            onClick={() => setShowPacks(true)}
            title={t('memories.toolbar.packs.title')}
          >
            {t('memories.toolbar.packs.label')}
          </button>
          <input
            ref={fileRef}
            type="file"
            accept=".csv,text/csv"
            style={{ display: 'none' }}
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) importCsv(f)
              e.target.value = '' // re-importing the same file re-fires onChange
            }}
          />
        </div>

        {shown.length === 0 ? (
          <div className="mv-empty">
            {bank.memories.length === 0 ? (
              <>
                <p>{t('memories.empty.none')}</p>
                <p className="mv-empty-hint">
                  <T k="memories.empty.hint" tags={{ b: <strong /> }} />
                </p>
                <button type="button" className="mv-empty-packs" onClick={() => setShowPacks(true)}>
                  {t('memories.empty.browsePacks')}
                </button>
              </>
            ) : (
              <p>{t('memories.empty.noMatch')}</p>
            )}
          </div>
        ) : grid ? (
          <div className="mv-scroll">
            <table className="mv-grid">
              <thead>
                <tr>
                  <th aria-label={t('memories.grid.column.favorite')}>★</th>
                  {th('name', t('memories.grid.column.name'))}
                  {th('rxMhz', t('memories.grid.column.rx'))}
                  {th('mode', t('memories.grid.column.mode'))}
                  <th>{t('memories.grid.column.offset')}</th>
                  <th>{t('memories.grid.column.tone')}</th>
                  {th('kind', t('memories.grid.column.kind'))}
                  <th aria-label={t('memories.grid.column.actions')} />
                </tr>
              </thead>
              <tbody>
                {bandSections.map((sec) => (
                  <Fragment key={sec.key}>
                    {showSectionHeaders && (
                      <tr className="mv-section-row">
                        <td colSpan={8}>
                          {sec.label} <span className="mv-section-count">{sec.rows.length}</span>
                        </td>
                      </tr>
                    )}
                    {sec.rows.map((m) => (
                  <tr key={m.id} className={invalidRow(m) ? 'invalid' : undefined}>
                    <td>
                      <button
                        type="button"
                        className={`mv-star${m.favorite ? ' on' : ''}`}
                        onClick={() => commit((b) => toggleFavorite(b, m.id))}
                        title={m.favorite ? t('memories.row.unstar.title') : t('memories.row.star.title')}
                      >
                        {m.favorite ? '★' : '☆'}
                      </button>
                    </td>
                    <td>
                      <CommitInput
                        className="mv-cell"
                        resetKey={`${m.id}:gname:${m.name}`}
                        value={m.name}
                        onCommit={(v) => editRow(m.id, { name: v })}
                      />
                    </td>
                    <td>
                      <CommitInput
                        className="mv-cell mv-cell-num"
                        resetKey={`${m.id}:grx:${m.rxMhz}`}
                        inputMode="decimal"
                        value={String(m.rxMhz)}
                        onCommit={(v) => withNumber(v, (n) => editRow(m.id, { rxMhz: n }))}
                      />
                    </td>
                    <td>
                      <CommitInput
                        className="mv-cell mv-cell-mode"
                        resetKey={`${m.id}:gmode:${m.mode}`}
                        list="mv-modes"
                        value={m.mode}
                        onCommit={(v) => editRow(m.id, { mode: v })}
                      />
                    </td>
                    <td className="mv-ro">{rowSummary(m, myGrid, units) || '—'}</td>
                    <td className="mv-ro">
                      {m.toneMode && m.toneMode !== 'none' ? m.toneMode.toUpperCase() : '—'}
                    </td>
                    <td className="mv-ro">{KIND_LABEL[m.kind]}</td>
                    <td className="mv-row-actions">
                      <button
                        type="button"
                        onClick={() => onRecall(m)}
                        title={t('memories.grid.tune.title')}
                      >
                        {t('memories.grid.tune.label')}
                      </button>
                      <button
                        type="button"
                        onClick={() => commit((b) => deleteMemory(b, m.id))}
                        title={t('memories.row.delete.title')}
                      >
                        ✕
                      </button>
                    </td>
                  </tr>
                    ))}
                  </Fragment>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <ul className="mv-list mv-scroll">
            {bandSections.map((sec) => (
              <Fragment key={sec.key}>
                {showSectionHeaders && (
                  <li className="mv-section" aria-hidden="true">
                    {sec.label} <span className="mv-section-count">{sec.rows.length}</span>
                  </li>
                )}
                {sec.rows.map((m) => {
              const rank = favRank.get(m.id) ?? 0
              const offStrip = rank > STRIP_FAVORITE_LIMIT
              return (
              <li key={m.id} className={`mv-row${invalidRow(m) ? ' invalid' : ''}`}>
                <div className="mv-row-line">
                  {rankView && (
                    <span
                      className={`mv-rank${offStrip ? ' off' : ''}`}
                      title={
                        offStrip
                          ? t('memories.rank.off.title', {
                              rank,
                              limit: STRIP_FAVORITE_LIMIT,
                            })
                          : t('memories.rank.on.title', {
                              rank,
                              // A keyboard chord, not prose — `modChord` names the key.
                              hotkey: rank <= 9 ? ` · ${modChord(rank)}` : '',
                            })
                      }
                    >
                      {rank}
                    </span>
                  )}
                  <button
                    type="button"
                    className={`mv-star${m.favorite ? ' on' : ''}`}
                    onClick={() => commit((b) => toggleFavorite(b, m.id))}
                    title={m.favorite ? t('memories.row.unstar.title') : t('memories.row.star.title')}
                  >
                    {m.favorite ? '★' : '☆'}
                  </button>
                  <button
                    type="button"
                    className="mv-row-main"
                    onClick={() => onRecall(m)}
                    title={t('memories.row.main.title', {
                      freq: m.rxMhz.toFixed(4),
                      mode: m.mode,
                    })}
                  >
                    <span className="mv-row-name">{m.name}</span>
                    <span className="mv-row-freq">
                      {m.rxMhz.toFixed(m.rxMhz >= 100 ? 3 : 4)} {m.mode}
                    </span>
                    {rowSummary(m, myGrid, units) && <span className="mv-row-sum">{rowSummary(m, myGrid, units)}</span>}
                    {m.groups.map((gid) => {
                      const g = bank.groups.find((x) => x.id === gid)
                      return g ? (
                        <span key={gid} className="mv-row-group">
                          {g.name}
                        </span>
                      ) : null
                    })}
                  </button>
                  {(rankView || (sel === 'all' && !sort && !query)) && (
                    <span className="mv-row-move">
                      <button
                        type="button"
                        aria-label={t('memories.row.moveUp.aria', { name: m.name })}
                        onClick={() =>
                          commit((b) => (rankView ? moveFavorite(b, m.id, -1) : moveMemory(b, m.id, -1)))
                        }
                        title={
                          rankView
                            ? t('memories.row.moveUp.rank.title', { limit: STRIP_FAVORITE_LIMIT })
                            : t('memories.row.moveUp.title')
                        }
                      >
                        ▲
                      </button>
                      <button
                        type="button"
                        aria-label={t('memories.row.moveDown.aria', { name: m.name })}
                        onClick={() =>
                          commit((b) => (rankView ? moveFavorite(b, m.id, 1) : moveMemory(b, m.id, 1)))
                        }
                        title={
                          rankView
                            ? t('memories.row.moveDown.rank.title', { limit: STRIP_FAVORITE_LIMIT })
                            : t('memories.row.moveDown.title')
                        }
                      >
                        ▼
                      </button>
                    </span>
                  )}
                  <button
                    type="button"
                    className="mv-row-tune"
                    onClick={() => onRecall(m)}
                    title={t('memories.row.tune.title')}
                  >
                    {t('memories.row.tune.label')}
                  </button>
                  <button
                    type="button"
                    className={`mv-row-edit${editingId === m.id ? ' active' : ''}`}
                    onClick={() => setEditingId(editingId === m.id ? null : m.id)}
                    title={t('memories.row.edit.title')}
                  >
                    ✎
                  </button>
                  <button
                    type="button"
                    className="mv-row-del"
                    onClick={() => commit((b) => deleteMemory(b, m.id))}
                    title={t('memories.row.delete.title')}
                  >
                    ✕
                  </button>
                </div>
                {editingId === m.id && editor(m)}
              </li>
                  )
                })}
              </Fragment>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}
