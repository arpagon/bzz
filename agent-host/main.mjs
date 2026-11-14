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

// Standard output carries the protocol and nothing else. A Pi package runs in this process and may print: only the
// protocol keeps the real stream, and whatever else writes to standard output lands in the log.
const protocol = { write: process.stdout.write.bind(process.stdout) };
process.stdout.write = (...args) => process.stderr.write(...args);
console.log = console.info = console.debug = (...args) => console.error(...args);
// Prompts and messages are never logged: the log names what happened, not what was said.
const log = (line) => process.stderr.write(`${new Date().toISOString()} ${line}\n`);
const { serve, VERSION } = await import("./src/host.mjs");
const { packageOf, packagesRan } = await import("./src/packages.mjs");

/**
 * A Pi package is code of the owner's inside this process, and one package must not be able to end the agent. What a
 * package leaves uncaught is logged, and the host goes on:
 *   - an exception thrown from a timer or an event, when its stack names a package's file;
 *   - a promise that failed with nobody waiting for it, whenever a package's code is in this process: such a failure
 *     often names no file at all, as when a package fetches something at start and the machine is offline.
 * Anything else is the host's own fault, and Node's rule stands: the process ends, and the next one continues from
 * storage.
 */
const goesOn = (what, error, source) => log(`${what}${source ? ` in package ${source}` : ""}, and the host goes on: ${String(error?.message ?? error).split("\n")[0].slice(0, 300)}`);
const ends = (error) => {
	process.stderr.write(`${error?.stack ?? error}\n`);
	process.exit(1);
};
process.on("uncaughtException", (error) => {
	const source = packageOf(error);
	if (source === undefined) return ends(error);
	goesOn("an uncaught exception", error, source);
});
process.on("unhandledRejection", (reason) => {
	const source = packageOf(reason);
	if (source === undefined && !packagesRan()) return ends(reason);
	goesOn("a promise failed with nobody waiting for it", reason, source);
});
const host = serve({ folder: resolve(folder), input: process.stdin, output: protocol, log });

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
