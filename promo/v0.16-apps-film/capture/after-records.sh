#!/usr/bin/env bash
# after-records.sh: after the take records (chapter 9, the real model's one
# turn), the evidence of the validation: what bzz recorded of the agent's
# turn (bzz agent show: its activity), each person's public key, and the
# relay's own record of every revision of the card (kind 45010), with who
# signed each. Writes validation/.
set -uo pipefail
. "$(dirname "$0")/env.sh"
V=$C/validation; mkdir -p "$V"
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" you agent show Scout > "$V/agent-show.txt" 2>&1
for who in $PEOPLE_NAMES; do echo "== $who"; "$C/bzz.sh" "$who" identity list 2>&1; done > "$V/identities.txt"
"$C/bzz.sh" you app show tasks > "$V/app-show-you.txt" 2>&1
(cd "$PIN" && docker compose -p buzz-harness -f docker-compose.harness.yml exec -T postgres psql -U buzz -d buzz -A -F ' | ' -c \
  "select to_char(created_at at time zone 'UTC','HH24:MI:SS') as at, encode(pubkey,'hex') as signer, encode(id,'hex') as event, content from events where kind = 45010 order by created_at, encode(id,'hex')") > "$V/relay-records.txt" 2>&1
(cd "$PIN" && docker compose -p buzz-harness -f docker-compose.harness.yml exec -T postgres psql -U buzz -d buzz -A -F ' | ' -c \
  "select to_char(created_at at time zone 'UTC','HH24:MI:SS') as at, encode(pubkey,'hex') as signer, kind, left(content, 160) from events where kind in (9, 1111, 40002, 1) and created_at > now() - interval '15 minutes' order by created_at") > "$V/relay-messages.txt" 2>&1
wc -l "$V"/*
