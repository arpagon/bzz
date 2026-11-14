#!/usr/bin/env node
// bzz's Pi Durable agent host. bzz starts one per agent:
//   node main.mjs <agent folder>
// Standard input and output carry the protocol (docs/agent-host-protocol.md);
// standard error is a log. It ends when its input ends or its parent is gone.
import { resolve } from "node:path";

// Read before anything else loads: a parent that dies while the host starts must not be mistaken for its successor.
const parent = process.ppid;
const [major, minor] = process.versions.node.split(".").map(Number);
if (major < 22 || (major === 22 && minor < 19)) {
	console.error(`bzz agent host needs Node 22.19 or newer; this is ${process.versions.node}`);
	process.exit(2);
}
const folder = process.argv[2];
if (!folder) {
	console.error("usage: node main.mjs <agent folder>");
	process.exit(2);
}

// Prompts and messages are never logged: the log names what happened, not what was said.
const log = (line) => process.stderr.write(`${new Date().toISOString()} ${line}\n`);
const { serve, VERSION } = await import("./src/host.mjs");
const host = serve({ folder: resolve(folder), input: process.stdin, output: process.stdout, log });

/**
 * The host must not outlive bzz. The end of input covers it when the pipe closes. It does not when something else
 * still holds the pipe, so the parent is watched too: POSIX hands an orphan to another parent; on Windows the parent
 * id stays, so the process itself is probed.
 */
const alive = () => {
	if (process.ppid !== parent) return false;
	try {
		process.kill(parent, 0);
		return true;
	} catch (error) {
		return error.code === "EPERM";
	}
};
const watch = setInterval(() => {
	if (alive()) return;
	log("the parent process is gone");
	host.stop();
}, 1_000);
watch.unref();
// TERM is what bzz sends 2 s after closing the input. Windows has no such signal: there the input's end is the notice.
process.on("SIGTERM", () => host.stop());

log(`host ${VERSION} started on Node ${process.versions.node}`);

await host.closed;
process.exit(0);
