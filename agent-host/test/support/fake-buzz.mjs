// A stand-in for Buzz's `buzz` command. It posts nothing: it writes what it
// was asked to a ledger, with whether the agent's key was in its environment,
// and answers as the real command does.
//   FAKE_BUZZ_LEDGER  the ledger, one JSON object per line
import { appendFileSync, existsSync, readFileSync } from "node:fs";

const args = process.argv.slice(2);
let stdin = "";
for await (const chunk of process.stdin) stdin += chunk;
const ledger = process.env.FAKE_BUZZ_LEDGER;
const before = existsSync(ledger) ? readFileSync(ledger, "utf8").trim().split("\n").filter(Boolean).length : 0;
const flag = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
const event = (before + 1).toString(16).padStart(64, "e");
appendFileSync(ledger, `${JSON.stringify({ command: args.slice(0, 2).join(" "), args, stdin, channel: flag("--channel"), replyTo: flag("--reply-to"), key: process.env.BUZZ_PRIVATE_KEY ?? null, event })}\n`);
if (process.env.FAKE_BUZZ_FAIL) {
	console.error("Error: relay refused the event");
	process.exit(1);
}
if (args[0] === "messages" && args[1] === "send") console.log(JSON.stringify({ event_id: event, accepted: true }));
else if (args[0] === "messages") console.log(JSON.stringify([{ id: "a".repeat(64), content: "earlier message" }]));
else console.log(JSON.stringify({ ok: true }));
