import { LOCALE_BETA_NOTE, useLocale } from '../i18n/useLocale'

/**
 * The beta-translation note, in the operator's language, while the language on screen is a beta one
 * (`LOCALE_BETA_NOTE`); nothing otherwise. A line of hint text in its host's own class (the Language
 * picker's `settings-hint`, the wizard's `wizard-sub`), so it takes the place and look of what is
 * already there. The address is a real link; the app's external-link interceptor opens it in the
 * browser.
 */
export function LocaleBetaNote({ className, as: Tag = 'span' }: { className: string; as?: 'span' | 'p' }) {
  const note = LOCALE_BETA_NOTE[useLocale()]
  if (!note) return null
  return (
    <Tag className={className} role="note">
      {note.text}{' '}
      <a href={note.href} target="_blank" rel="noopener noreferrer">
        {note.link}
      </a>
    </Tag>
  )
}
