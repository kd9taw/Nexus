-- A pairing code the operator RECEIVED is the whole primitive behind the phished-enrollment
-- attack: typing it attaches a station the attacker owns to the victim's account and, at approve,
-- permanently burns the victim's one trial. Approval happens at the attacker's own shack, so the
-- two-act protection does not cover it - they legitimately own both halves of their enrollment.
--
-- `confirmed` splits "the operator typed a code" from "the operator agreed to attach THIS station
-- and start their trial". Nothing is created and no clock starts until the second act.
ALTER TABLE enrollments ADD COLUMN confirmed INTEGER NOT NULL DEFAULT 0;

-- Grandfather claims made under the old rules. Without this, a pairing already in flight when the
-- migration lands is refused at approve with no way for the operator to understand why: the browser
-- of that era has no confirm button to offer them.
UPDATE enrollments SET confirmed=1 WHERE account_id IS NOT NULL;
