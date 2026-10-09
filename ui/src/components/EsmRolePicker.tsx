// ENTER SENDS MESSAGE — the role picker for one macro set: which of the set's keys Enter sends
// for each step, and what that step would send. Settings ▸ Contesting shows one per cockpit.
//
// It edits the set's OWN mapping only. A set with a built-in table shows it beside, read-only,
// and a step the operator leaves unmapped keeps the built-in key (`esmRoles`). A set that is not
// the operator's to map (a CW profile still on the built-in sets) shows its built-in steps and
// no control at all. What each step sends is decided by the model a press uses (`esmMessage`),
// and a step that would send nothing says why, by name, in the strip's own words.
//
// Nothing here sends: the picker writes a mapping, and every cockpit's send path is untouched.
import { ESM_ROLES, esmHasSteps, esmMessage, type EsmCockpit, type EsmRole } from '../features/esm'
import { esmRoles, type EsmRoleMap, type EsmSlot } from '../features/esmRoles'
import { esmInertText, esmRefusalText, esmStepName } from '../features/esmWords'
import type { MacroKey } from '../features/macroSets'
import { t } from '../i18n'

interface Props {
  /** The cockpit the set is in. Phone's keyer plays one recording per press, so a step there
   *  takes one slot. */
  cockpit: EsmCockpit
  /** The set's built-in table, or null for a set that has none. */
  builtIn: EsmRoleMap | null
  /** The operator's own mapping for the set; absent = nothing mapped. */
  own?: EsmRoleMap
  /** The set's keys as the dock shows them. Absent = the texts depend on the contest running
   *  (a CW profile on the built-in sets), so only the steps' keys are shown. */
  slots?: readonly EsmSlot[]
  /** The keys a step may be mapped to. */
  keys: readonly MacroKey[]
  /** Called with the set's new own mapping (undefined = nothing mapped). Absent = read-only. */
  onChange?: (own: EsmRoleMap | undefined) => void
  disabled?: boolean
}

export function EsmRolePicker({ cockpit, builtIn, own, slots, keys, onChange, disabled }: Props) {
  const roles = esmRoles(builtIn, own)
  if (!onChange && !esmHasSteps(roles)) {
    return <span className="settings-hint esm-roles-none">{esmInertText('noRoles')}</span>
  }
  const most = cockpit === 'phone' ? 1 : 2
  const map = (role: EsmRole, next: MacroKey[]) => {
    if (!onChange) return
    const out: EsmRoleMap = { ...own }
    if (next.length) out[role] = next
    else delete out[role]
    onChange(Object.keys(out).length ? out : undefined)
  }
  return (
    <div className="esm-roles" data-esm-cockpit={cockpit}>
      {ESM_ROLES.map((role) => {
        const step = esmStepName(role)
        const fixed = builtIn?.[role] ?? []
        const mine = own?.[role] ?? []
        const message = slots ? esmMessage(roles, role, slots, cockpit) : null
        return (
          <div key={role} className="cw-macro-row esm-role-row" data-esm-role={role}>
            <span className="cw-macro-role">{step}</span>
            {builtIn && (
              <span className="cw-macro-key esm-role-builtin" title={t('contest.esm.picker.builtIn.title')}>
                {fixed.length ? fixed.join(' ') : t('contest.esm.picker.builtIn.none')}
              </span>
            )}
            {onChange && (
              <select
                className="settings-input esm-role-key"
                aria-label={t('contest.esm.picker.key.aria', { step })}
                value={mine[0] ?? ''}
                disabled={disabled}
                onChange={(e) =>
                  map(role, e.target.value ? [e.target.value as MacroKey, ...mine.slice(1, most)] : [])
                }
              >
                <option value="">
                  {fixed.length
                    ? t('contest.esm.picker.useBuiltIn', { keys: fixed.join(' ') })
                    : t('contest.esm.picker.none')}
                </option>
                {keys.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
            )}
            {onChange && most > 1 && mine.length > 0 && (
              <select
                className="settings-input esm-role-key"
                aria-label={t('contest.esm.picker.then.aria', { step })}
                value={mine[1] ?? ''}
                disabled={disabled}
                onChange={(e) =>
                  map(role, e.target.value ? [mine[0], e.target.value as MacroKey] : [mine[0]])
                }
              >
                <option value="">{t('contest.esm.picker.thenNone')}</option>
                {keys.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
            )}
            {message && (
              <span
                className={`esm-role-sends${'why' in message ? ' settings-hint' : ' mono'}`}
                data-esm-refused={'why' in message ? message.why : undefined}
              >
                {'why' in message ? esmRefusalText(message) : message.text}
              </span>
            )}
          </div>
        )
      })}
      {!esmHasSteps(roles) && <span className="settings-hint esm-roles-none">{esmInertText('noRoles')}</span>}
    </div>
  )
}
