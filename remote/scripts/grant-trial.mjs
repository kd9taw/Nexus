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
// ⚠️ It writes `source='manual'` on purpose. A hand-made grant and a self-serve trial must stay
// distinguishable forever; `trials.source` is the only thing that keeps them apart, and the
// service reports it so a support question can be answered honestly.
//
// ⚠️ It UPDATEs, and never DELETEs. The trials row is the durable proof that an account consumed
// its one trial. Removing it re-opens reinstall, re-pair and second-station abuse all at once,
// which is the whole reason the one-trial-ever rule exists.
const STAGING_DB = 'nexus-remote-staging'
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

const args = process.argv.slice(2)
const read = name => {
  const i = args.indexOf(`--${name}`)
  return i === -1 ? null : args[i + 1] ?? null
}
const account = read('account')
const days = Number(read('days') ?? 14)

if (!account || !UUID.test(account)) {
  console.error('Usage: node remote/scripts/grant-trial.mjs --account <account-uuid> [--days 14]')
  console.error('The account UUID is shown in the Remote browser under Support details.')
  process.exitCode = 1
} else if (!Number.isInteger(days) || days < 1 || days > 365) {
  console.error('--days must be a whole number of days between 1 and 365.')
  process.exitCode = 1
} else {
  const seconds = days * 24 * 60 * 60
  // INSERT covers an account that has never had a trial; the conflict clause covers re-granting
  // one that has. Both land on source='manual' so neither can be mistaken for self-serve later.
  const sql = `INSERT INTO trials(account_id, enabled, expires_at, started_at, source) ` +
    `SELECT id, 1, (unixepoch() + ${seconds}) * 1000, unixepoch() * 1000, 'manual' ` +
    `FROM accounts WHERE id = '${account}' ` +
    `ON CONFLICT(account_id) DO UPDATE SET enabled = 1, ` +
    `expires_at = (unixepoch() + ${seconds}) * 1000, started_at = unixepoch() * 1000, source = 'manual';`
  console.log(`-- ${days} day trial for ${account}. Review it, then run:\n`)
  console.log(`npx wrangler d1 execute ${STAGING_DB} --remote --command "${sql.replace(/"/g, '\\"')}"\n`)
  console.log('-- Selecting FROM accounts means an id that does not exist grants nothing rather')
  console.log('-- than creating an orphan row. Confirm it reports one row written.')
}
