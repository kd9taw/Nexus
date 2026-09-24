// THE QSO DETAIL VIEW — one contact, everything the record actually holds.
//
// "It's a 'Log List' not a Log Book… There's no View capability." (#313, KR4FQG, 2026-09-15.)
// The table shows 17 columns and the record carries far more, so a contact's grid, its QSL
// state, its award credits, its upload state per connector, its park references and any ADIF
// field Nexus preserved but does not model were all logged and then invisible. This is the
// window that shows them.
//
// ⭐ READ-ONLY, DELIBERATELY. Editing already exists — the row's pencil opens the inline form,
// which is the one writer. A second editing surface would be a second source of truth for the
// same record, and the two come apart exactly where it matters.
//
// ⚠️ ABSENT IS NOT EMPTY. A field the contact does not carry is OMITTED, never rendered as a
// blank or a dash: "this QSO has no grid" and "this QSO's grid is unknown to me" are different
// facts, and a row of empty labels reads as the second while meaning the first. Sections
// disappear entirely when they hold nothing, so what remains on screen is what is actually
// known about the contact.
import type { LoggedQso } from '../types'
import { Dialog } from './ui/Dialog'
import { useFocusReturn } from '../focusReturn'
import { t } from '../i18n'

/** A rendered field: a label and a value that is definitely worth showing. */
type Field = { label: string; value: string; mono?: boolean }

const has = (v: unknown): boolean =>
  v !== null && v !== undefined && v !== '' && !(Array.isArray(v) && v.length === 0)

/** UTC, the way every other surface in this app states a time. */
function utc(unix: number | null | undefined): string | null {
  if (!unix) return null
  const d = new Date(unix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ` +
    `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}Z`
}

export interface QsoDetailProps {
  qso: LoggedQso | null
  onClose: () => void
}

export function QsoDetail({ qso, onClose }: QsoDetailProps) {
  // Closed, the keyboard goes back to the row it was opened from (focusReturn.ts).
  const returnFocus = useFocusReturn(!!qso)
  if (!qso) return null
  const q = qso

  const section = (title: string, fields: (Field | null)[]): { title: string; fields: Field[] } | null => {
    const kept = fields.filter((f): f is Field => f !== null && has(f.value))
    return kept.length ? { title, fields: kept } : null
  }
  const f = (label: string, value: unknown, mono = false): Field | null =>
    has(value) ? { label, value: String(value), mono } : null

  // The QSL picture, which the table cannot show at all: four inbound channels and the
  // outbound card with its route and date.
  const rcvd = q.qslRcvd
  const qslIn = [
    rcvd?.card && t('qso.detail.qsl.card'),
    rcvd?.lotw && 'LoTW',
    rcvd?.eqsl && 'eQSL',
    rcvd?.qrz && 'QRZ',
  ].filter(Boolean).join(' · ')

  const sections = [
    // The callsign is the dialog's TITLE; a first field repeating it says the same thing
    // twice in the same box.
    section(t('qso.detail.section.contact'), [
      f(t('logbook.column.name'), q.name),
      // ⭐ THE PEER'S GRID. The table shows MY grid and not theirs, which is the one most
      // worth having on a contact you are looking up.
      f(t('qso.detail.theirGrid'), q.grid, true),
      f(t('logbook.column.qth'), q.qth),
      f(t('logbook.column.state'), q.state),
      f(t('logbook.column.country'), q.country ?? q.entity),
      f('DXCC', q.dxcc, true),
    ]),
    section(t('qso.detail.section.contact.radio'), [
      f(t('logbook.column.band'), q.band, true),
      f(t('logbook.column.freq'), q.freqMhz ? `${q.freqMhz.toFixed(6)} MHz` : null, true),
      f(t('logbook.column.mode'), q.mode, true),
      f(t('logbook.column.sent'), q.rstSent, true),
      f(t('logbook.column.rcvd'), q.rstRcvd, true),
      f(t('logbook.column.power'), q.txPower != null ? `${q.txPower} W` : null, true),
      f(t('qso.detail.propMode'), q.propMode, true),
      f(t('qso.detail.satName'), q.satName, true),
      // ⚠️ `timeKnown: false` means the record carries a DATE and no time of day — an
      // imported contact whose source had none. Saying so is the point: a bare 00:00 read
      // as fact is what made LoTW/eQSL hold those contacts unmatched forever.
      f(t('qso.detail.time'), q.timeKnown === false
        ? t('qso.detail.time.dateOnly', { when: utc(q.whenUnix) ?? '' })
        : utc(q.whenUnix), true),
    ]),
    section(t('qso.detail.section.myStation'), [
      f(t('logbook.column.operator'), q.operator, true),
      f(t('qso.detail.stationCallsign'), q.stationCallsign, true),
      f(t('logbook.column.myGrid'), q.myGrid, true),
      f(t('logbook.column.myRig'), q.myRig),
    ]),
    section(t('qso.detail.section.qsl'), [
      f(t('qso.detail.qsl.received'), qslIn),
      f(t('qso.detail.qsl.sent'), q.qslSent?.sent
        ? [q.qslSent.via, utc(q.qslSent.dateUnix)].filter(Boolean).join(' · ') ||
          t('qso.detail.qsl.yes')
        : null),
      f(t('qso.detail.qsl.confirmed'), q.confirmed ? t('qso.detail.qsl.yes') : null),
      f(t('qso.detail.qsl.awardConfirmed'), q.awardConfirmed ? t('qso.detail.qsl.yes') : null),
      f(t('qso.detail.credit.granted'), q.creditGranted?.join(' · ')),
      f(t('qso.detail.credit.submitted'), q.creditSubmitted?.join(' · ')),
    ]),
    section(t('qso.detail.section.programs'), [
      f(t('qso.detail.ota.mine'), q.ota?.myProgram && q.ota?.myRef
        ? `${q.ota.myProgram} ${q.ota.myRef}` : null, true),
      f(t('qso.detail.ota.theirs'), q.ota?.theirProgram && q.ota?.theirRef
        ? `${q.ota.theirProgram} ${q.ota.theirRef}` : null, true),
    ]),
    section(t('qso.detail.section.notes'), [
      f(t('qso.detail.comment'), q.comment),
      f(t('logbook.column.notes'), q.notes),
    ]),
  ].filter((s): s is { title: string; fields: Field[] } => s !== null)

  // ⭐ ADIF FIELDS NEXUS PRESERVED BUT DOES NOT MODEL. An import carries tags this app has no
  // column for; they survive a round trip and were never visible anywhere. Shown raw, last,
  // and only when there are any — this is the operator's own data, not ours to hide.
  const extra = (q.extra ?? []).filter(([k, v]) => has(k) && has(v))

  return (
    <Dialog
      open={!!qso}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('qso.detail.title', { call: q.call })}
      className="qso-detail-dialog"
      onCloseAutoFocus={returnFocus}
    >
      <div className="qso-detail">
        {sections.map((s) => (
          <section key={s.title} className="qso-detail-section">
            <h3 className="qso-detail-heading">{s.title}</h3>
            <dl className="qso-detail-fields">
              {s.fields.map((fl) => (
                <div key={fl.label} className="qso-detail-field">
                  <dt>{fl.label}</dt>
                  <dd className={fl.mono ? 'mono' : undefined}>{fl.value}</dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
        {extra.length > 0 && (
          <section className="qso-detail-section">
            <h3 className="qso-detail-heading">{t('qso.detail.section.adif')}</h3>
            <dl className="qso-detail-fields">
              {extra.map(([k, v]) => (
                <div key={k} className="qso-detail-field">
                  <dt className="mono">{k}</dt>
                  <dd className="mono">{v}</dd>
                </div>
              ))}
            </dl>
          </section>
        )}
        {/* ⚠️ THERE IS DELIBERATELY NO "nothing recorded" BRANCH. It cannot happen: every
            record carries `whenUnix`, so the contact section always holds at least the time,
            and the bare row this app can produce still shows a band, a mode and a date. A
            branch that cannot run is a claim no test can check — written down instead. */}
      </div>
    </Dialog>
  )
}
