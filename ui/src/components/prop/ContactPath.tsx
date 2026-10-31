// The contact card's Path section: which way round this contact's signal most likely went, said in
// one sentence (features/pathGrade), why and how sure one click away, and the path over the day
// it was made — the same strip as the Path over the day box, modelled at the solar flux the contact
// logged, its hour marked (src-tauri `path_grades::contact_day`). Display only, with one choice the
// operator makes: their antenna path (ADIF ANT_PATH), added to the card's fields and written with
// the card's own Save, like any field they add. Nexus writes it by itself only for a short path
// the beam heading confirmed; a model's guess never goes there.
import { useEffect, useState } from 'react'
import { contactPathDay } from '../../api'
import { t } from '../../i18n'
import type { ContactPathDay, LoggedQso } from '../../types'
import { gradeAsk, gradeGlyph, gradeKey, gradeLines, gradeSentence, POLAR_GLYPH, usePathGrade } from '../../features/pathGrade'
import { PathDayPane } from './PathDayStrip'

type Day = ContactPathDay | null | 'loading'

export function ContactPath({
  q,
  canPick,
  picked,
  onPick,
}: {
  q: LoggedQso
  /** The card can save (a Remote browser's cannot). */
  canPick: boolean
  /** An antenna path the contact holds or the card is about to save. */
  picked: string | null
  onPick: (path: 'S' | 'L') => void
}) {
  const key = gradeKey(q)
  const [day, setDay] = useState<Day>('loading')
  const shown = usePathGrade(q)
  useEffect(() => {
    let live = true
    setDay('loading')
    // Asked inside the chain, so any failure at all reads as "no path" rather than breaking the card.
    Promise.resolve()
      .then(() => contactPathDay(gradeAsk(q)))
      .then((d) => {
        if (live) setDay(d)
      })
      .catch(() => {
        if (live) setDay(null)
      })
    return () => {
      live = false
    }
    // The key is everything the day depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  const grade = (day !== 'loading' ? day?.grade : undefined) ?? shown ?? null
  const longInvolved = grade?.verdict === 'long' || grade?.verdict === 'either'
  return (
    <section className="qso-detail-section contact-card-path">
      <h3 className="qso-detail-heading">{t('contactCard.path')}</h3>
      {day === 'loading' && !grade ? (
        <p className="qso-detail-bare">{t('contactCard.path.loading')}</p>
      ) : !grade && !(day && day !== 'loading') ? (
        <p className="qso-detail-bare">{t('contactCard.path.none')}</p>
      ) : (
        <>
          {grade && (
            <p className={`contact-card-path-line${grade.tier === 'modelled' ? ' modelled' : ''}`}>
              <span className="mono">{[gradeGlyph(grade), grade.polar ? POLAR_GLYPH : ''].filter(Boolean).join(' ')}</span>{' '}
              {gradeSentence(grade)}
              {longInvolved && <span className="route-lp-badge">{t('pathRoute.lpBadge')}</span>}
            </p>
          )}
          {grade && (
            <details className="contact-card-path-why">
              <summary>{t('contactCard.path.details')}</summary>
              <ul>
                {gradeLines(grade)
                  .slice(1)
                  .map((l) => (
                    <li key={l}>{l}</li>
                  ))}
              </ul>
            </details>
          )}
          {day && day !== 'loading' && (
            <>
              <p className="qso-detail-bare contact-card-path-day">{t('contactCard.path.day')}</p>
              <PathDayPane pred={day.pred} scrub={null} />
            </>
          )}
        </>
      )}
      {canPick && !picked && (
        <div className="contact-card-path-pick" title={t('contactCard.path.pickTitle')}>
          <span>{t('contactCard.path.pick')}</span>
          <button type="button" className="export-btn" onClick={() => onPick('S')}>
            {t('contactCard.path.pickShort')}
          </button>
          <button type="button" className="export-btn" onClick={() => onPick('L')}>
            {t('contactCard.path.pickLong')}
          </button>
        </div>
      )}
    </section>
  )
}
