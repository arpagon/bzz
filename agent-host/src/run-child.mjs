// The keeper of one real command, started by run.mjs for the `run` tool:
//   node run-child.mjs <command>
// It starts `/bin/sh -c -- <command>` in the working directory it was given, as
// the leader of a process group of its own, and sees to it that the whole
// group ends: when the command ends, when the host asks, and when the host is
// gone, however it went. That last case is why this is a process and not a
// function of the host: a host that is killed outright can do nothing more,
// but its end of the pipe closes, and this process is still here to see it.
//
//   standard input   a pipe from the host that carries nothing. Its end is the order to stop.
//   fd 1 and 2       handed to the command as they are: its output goes straight to the host.
//   fd 3             lines of JSON for the host: `{"group": <id>}` once started, then `{"code", "signal"}`
//                    when the command ended, or `{"error": "..."}` when it could not be started.
//
// POSIX only. Windows has no process groups; the host does not offer `run` there.
import { spawn } from "node:child_process";
import { writeSync } from "node:fs";

/** How long the group has between TERM and KILL when it is told to stop. */
const GRACE_MS = 2_000;
const command = process.argv[2];
const say = (message) => {
	try {
		writeSync(3, `${JSON.stringify(message)}\n`);
	} catch {}
};
// This process is not in the command's group, and whoever signals it by hand does not thereby free the command.
for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) process.on(signal, () => stop());

// The string goes to the shell as its command, exactly as the host received it. `--` ends the shell's own options:
// without it a command that begins with a dash would be read as one.
const child = spawn("/bin/sh", ["-c", "--", command], { stdio: ["ignore", "inherit", "inherit"], detached: true });
const group = child.pid;
const signalGroup = (signal) => {
	try {
		process.kill(-group, signal);
		return true;
	} catch {
		// No such group: every process of it has ended.
		return false;
	}
};
let status;
let stopping = false;

/** Ends the group: TERM, and KILL for what is still there after the grace. */
function stop() {
	if (stopping) return;
	stopping = true;
	if (group === undefined) return;
	signalGroup("SIGTERM");
	const until = Date.now() + GRACE_MS;
	const poll = setInterval(() => {
		const left = signalGroup(0);
		if (left && Date.now() < until) return;
		clearInterval(poll);
		if (left) signalGroup("SIGKILL");
		finish();
	}, 25);
}

function finish() {
	say(status ?? { code: null, signal: "SIGKILL" });
	process.exit(0);
}

child.on("error", (error) => {
	say({ error: error.message });
	process.exit(0);
});
child.on("exit", (code, signal) => {
	status = { code, signal };
	// Told to stop: the rest of the group still has its grace. Otherwise the command is over, and what it left
	// running in its group goes with it: nothing outlives the call.
	if (stopping) return;
	signalGroup("SIGKILL");
	finish();
});
if (group !== undefined) say({ group });
process.stdin.on("end", stop).on("close", stop).on("error", stop).resume();
