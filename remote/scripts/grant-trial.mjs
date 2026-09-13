// Print the SQL that grants or extends a trial for ONE account, for the operator to run.
//
// It deliberately does not execute anything and holds no credentials: it composes a statement and
// tells you the wrangler command to run it with, so the privileged step stays an explicit act by
// somebody who can see what they are about to do. During a closed beta this IS the intended
// mechanism - every tester is invited by hand, so granting by hand costs nothing and needs no
// admin surface to exist.
//
//   node remote/scripts/grant-trial.mjs --account <uuid> [--days 14]
//
// An operator with no wrangler login runs the SAME statement through the Remote staging workflow's
// `grant-trial` operation (cloudflare-staging.mjs), which binds the account as a parameter instead
// of printing it. Both come from `trialGrant` below, so the printed and executed forms cannot drift.
//
// ⚠️ It never erases how a trial began. A hand-made grant and a self-serve trial must stay
// distinguishable forever; `trials.source` and `started_at` are the only things that keep them apart,
// and the service reports them so a support question can be answered honestly. A fresh grant writes
// 'manual'; extending a trial the account earned by pairing records 'trial+manual' and keeps its
// original start. This is the same rule `admin/grant-trial` follows - the two must never disagree.
//
// ⚠️ It UPDATEs, and never DELETEs. The trials row is the durable proof that an account consumed
// its one trial. Removing it re-opens reinstall, re-pair and second-station abuse all at once,
// which is the whole reason the one-trial-ever rule exists.
import { pathToFileURL } from 'node:url'

export const STAGING_DB = 'nexus-remote-staging'
export const GRANT_ACCOUNT = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
// The same bound admin/grant-trial enforces. The two must never disagree.
export const GRANT_MAX_DAYS = 365

// INSERT covers an account that has never had a trial; the conflict clause covers re-granting one
// that has. The conflict clause used to overwrite started_at and source outright, so extending a
// trial an operator had EARNED by pairing made it indistinguishable from one that was never earned.
// admin/grant-trial was fixed for exactly this; this script was missed, and handing it out
// unfixed would have reintroduced the erasure on whichever account it was run against.
export function trialGrant(account, days) {
  if (typeof account !== 'string' || !GRANT_ACCOUNT.test(account)) {
    throw new Error('The account must be the account UUID shown in the Remote browser under Support details.')
  }
  if (!Number.isInteger(days) || days < 1 || days > GRANT_MAX_DAYS) {
    throw new Error(`Days must be a whole number between 1 and ${GRANT_MAX_DAYS}.`)
  }
  const seconds = days * 24 * 60 * 60
  const statement = target => `INSERT INTO trials(account_id, enabled, expires_at, started_at, source) ` +
    `SELECT id, 1, (unixepoch() + ${seconds}) * 1000, unixepoch() * 1000, 'manual' ` +
    `FROM accounts WHERE id = ${target} ` +
    `ON CONFLICT(account_id) DO UPDATE SET enabled = 1, ` +
    `expires_at = (unixepoch() + ${seconds}) * 1000, ` +
    `started_at = COALESCE(trials.started_at, unixepoch() * 1000), ` +
    `source = CASE WHEN trials.source = 'trial' THEN 'trial+manual' ELSE 'manual' END;`
  // The account has passed a strict UUID pattern, so quoting it into the printable form is safe;
  // the executed form never splices it in at all.
  return { printable: statement(`'${account}'`), statement: statement('?'), params: [account] }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2)
  const read = name => {
    const i = args.indexOf(`--${name}`)
    return i === -1 ? null : args[i + 1] ?? null
  }
  const account = read('account')
  const days = Number(read('days') ?? 14)

  if (!account || !GRANT_ACCOUNT.test(account)) {
    console.error('Usage: node remote/scripts/grant-trial.mjs --account <account-uuid> [--days 14]')
    console.error('The account UUID is shown in the Remote browser under Support details.')
    process.exitCode = 1
  } else if (!Number.isInteger(days) || days < 1 || days > GRANT_MAX_DAYS) {
    console.error(`--days must be a whole number of days between 1 and ${GRANT_MAX_DAYS}.`)
    process.exitCode = 1
  } else {
    const sql = trialGrant(account, days).printable
    console.log(`-- ${days} day trial for ${account}. Review it, then run:\n`)
    console.log(`npx wrangler d1 execute ${STAGING_DB} --remote --command "${sql.replace(/"/g, '\\"')}"\n`)
    console.log('-- Selecting FROM accounts means an id that does not exist grants nothing rather')
    console.log('-- than creating an orphan row. Confirm it reports one row written.')
  }
}
