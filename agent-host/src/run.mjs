// The `run` tool: the one place where this host starts a real program for
// the agent. It exists only for an agent that the owner granted a folder with
// `exec`, and each command is put to bzz first (`_bzz/approve`), which may
// ask the owner.
//
// What it promises:
//   - The command is one string. The host does not read it, quote it or add
//     to it: the string bzz approved is the string `/bin/sh -c` receives.
//   - It starts in the granted folder, with an environment built from
//     nothing: PATH, HOME, LANG and TMPDIR, and nothing else of this process.
//   - It has a time limit, and its process group is ended on cancel, on
//     time-out, when the command returns, and when the host goes
//     (run-child.mjs).
//
// What it does not promise: the command is a real program with the owner's
// rights. The folder is where it starts, not a wall around it.
import { spawn } from "node:child_process";
import { statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineTool } from "@earendil-works/pi-durable";

const KEEPER = fileURLToPath(new URL("./run-child.mjs", import.meta.url));
/** Windows has no process groups, and without them the host cannot promise that a command's whole tree is ended. */
export const RUN_UNAVAILABLE = process.platform === "win32" ? "it is not offered on Windows, where the host cannot end a command's whole process tree" : undefined;
const DEFAULT_SECONDS = 120;
const MAX_SECONDS = 3_600;
/** One argument of a program may not be longer than this on Linux, and the command is one argument of the shell. */
const MAX_COMMAND_BYTES = 100_000;
/** How much of a long output is saved on the agent's disk. The model always gets the end of the output. */
const SAVE_BYTES = 8 * 1024 * 1024;
/** The keeper gives the group 2 s between TERM and KILL. A keeper that has not finished this long after is ended from here. */
const STOP_MS = 4_000;
const ENV = ["PATH", "HOME", "LANG", "TMPDIR"];

const refuse = (text) => ({ output: [{ type: "text", text }], isError: true });
const size = (bytes) => (bytes < 1024 * 1024 ? `${Math.ceil(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

/**
 * @param folders the granted folders with `exec`, by name: `name -> { root }`, the real path, or none when the folder is not there
 * @param approve `({ conversationId, mount, command }, signal) => { allow, reason? }`: asks bzz. A throw is a refusal
 * @param save `(bytes) => path`: keeps a long output on the agent's disk
 * @param env this process's environment: the four names above are copied from it
 * @returns `tool`, for the registry, and `stopAll()`, which ends every command that runs
 */
export function createRun({ folders, approve, save, env, log = () => undefined }) {
	const names = [...folders.keys()];
	const where = names.map((name) => `/mnt/${name}`).join(", ");
	const running = new Set();
	const clean = Object.fromEntries(ENV.filter((name) => typeof env[name] === "string").map((name) => [name, env[name]]));
	clean.PATH ??= "/usr/local/bin:/usr/bin:/bin";

	/** Starts the approved command and resolves when its group has ended: `{ code, signal, error, stopped, total, kept }`. */
	function start(command, cwd, seconds, signal, onOutput) {
		return new Promise((resolve) => {
			const keeper = spawn(process.execPath, [KEEPER, command], { cwd, env: clean, detached: true, stdio: ["pipe", "pipe", "pipe", "pipe"] });
			const kept = [];
			let keptBytes = 0;
			let total = 0;
			let status = {};
			let group;
			let stopped;
			let done = false;
			let report = "";
			const timers = [];
			const end = (extra = {}) => {
				if (done) return;
				done = true;
				for (const timer of timers) clearTimeout(timer);
				signal?.removeEventListener("abort", onAbort);
				running.delete(stop);
				// Should the keeper have died without ending the group, the group is ended from here.
				if (group !== undefined && status.code === undefined && status.error === undefined) try { process.kill(-group, "SIGKILL"); } catch {}
				for (const stream of keeper.stdio) stream?.destroy();
				resolve({ ...status, ...extra, stopped, total, kept: Buffer.concat(kept) });
			};
			/** The order to stop is the end of the keeper's input: the same thing the keeper sees when this process dies. */
			const stop = (why) => {
				if (done || stopped) return;
				stopped = why;
				keeper.stdin.end();
				timers.push(setTimeout(() => {
					try { keeper.kill("SIGKILL"); } catch {}
					end();
				}, STOP_MS));
			};
			const onAbort = () => stop("cancel");
			running.add(stop);
			keeper.stdin.on("error", () => undefined);
			keeper.on("error", (error) => end({ error: error.message }));
			for (const stream of [keeper.stdout, keeper.stderr]) {
				// One decoder per stream: a character cut in two by a chunk boundary is put together again.
				const decoder = new TextDecoder();
				stream.on("data", (chunk) => {
					if (done) return;
					total += chunk.length;
					if (keptBytes < SAVE_BYTES) {
						const part = chunk.subarray(0, SAVE_BYTES - keptBytes);
						kept.push(part);
						keptBytes += part.length;
					}
					onOutput(decoder.decode(chunk, { stream: true }));
				}).on("error", () => undefined);
			}
			keeper.stdio[3].on("data", (chunk) => {
				report += chunk;
				for (let newline = report.indexOf("\n"); newline >= 0; newline = report.indexOf("\n")) {
					let message;
					try { message = JSON.parse(report.slice(0, newline)); } catch {}
					report = report.slice(newline + 1);
					if (message?.group !== undefined) group = message.group;
					else if (message) status = message;
				}
			}).on("error", () => undefined);
			// `close` comes when the keeper is gone and its output has been read to the end. Something that left the
			// group and still holds the output open must not hold the call: half a second after the keeper, it is over.
			keeper.on("exit", () => timers.push(setTimeout(end, 500)));
			keeper.on("close", () => end());
			timers.push(setTimeout(() => stop("timeout"), seconds * 1000));
			if (signal?.aborted) stop("cancel");
			else signal?.addEventListener("abort", onAbort, { once: true });
		});
	}

	const tool = defineTool({
		name: "run",
		description: `Start a real command of the owner's machine in a folder granted to you for that (${where}). The command is one line for /bin/sh and starts in that folder. The owner may be asked first and can refuse. It is stopped after its time limit, and nothing it starts keeps running after it returns. Returns combined stdout and stderr, truncated to the last 2000 lines or 50KB; a longer output is saved on your own disk and the result says where.`,
		parameters: {
			type: "object",
			properties: {
				folder: { type: "string", description: `The folder's name: ${names.join(", ")}.` },
				command: { type: "string", description: "The command line, for /bin/sh." },
				timeout: { type: "number", description: `Time limit in seconds. Default ${DEFAULT_SECONDS}, at most ${MAX_SECONDS}.` },
			},
			required: ["folder", "command"],
		},
		// One at a time within a round: the owner is asked about one command, then the next.
		executionMode: "sequential",
		outputLimits: { retain: "tail" },
		// A model may give the folder as it sees it in its file tools.
		prepareArguments(args) {
			if (!args || typeof args !== "object" || typeof args.folder !== "string") return args;
			return { ...args, folder: args.folder.replace(/^\/mnt\//, "").replace(/\/+$/, "") };
		},
		async execute(args, api, context) {
			const signal = context.abortSignal;
			const { folder, command } = args;
			const granted = folders.get(folder);
			if (!granted) return refuse(`\`run\` is not allowed in ${JSON.stringify(folder)}. It is allowed in: ${names.join(", ")}.`);
			if (typeof command !== "string" || !command.trim()) return refuse("run needs `command`: the command line, as a string.");
			if (command.includes("\0")) return refuse("The command holds a NUL character, which no command line can.");
			if (Buffer.byteLength(command) > MAX_COMMAND_BYTES) return refuse(`The command is longer than ${MAX_COMMAND_BYTES} bytes. Write a script to a file in the folder and run that.`);
			const seconds = args.timeout ?? DEFAULT_SECONDS;
			if (typeof seconds !== "number" || !Number.isFinite(seconds) || seconds <= 0 || seconds > MAX_SECONDS) return refuse(`\`timeout\` must be a number of seconds, above 0 and at most ${MAX_SECONDS}.`);
			// Looked at now, not when the host started: the owner may have moved the folder since.
			if (!granted.root || !statSync(granted.root, { throwIfNoEntry: false })?.isDirectory()) return refuse(`The folder /mnt/${folder} is not there on the owner's machine.`);

			// Asked before anything starts. Only `allow: true` is a yes: an error, another value or no answer is a no.
			let verdict;
			try {
				verdict = await approve({ conversationId: api.conversationId, mount: folder, command }, signal);
			} catch (error) {
				if (signal?.aborted) throw new Error("The turn was stopped before the command was approved. It was not started.");
				log(`run in /mnt/${folder}: not started, bzz could not be asked`);
				return refuse(`The command was not started: bzz did not approve it (${error?.message ?? error}).`);
			}
			if (verdict?.allow !== true) {
				log(`run in /mnt/${folder}: refused`);
				const reason = typeof verdict?.reason === "string" && verdict.reason.trim() ? `: ${verdict.reason.trim()}` : ".";
				return refuse(`The command was not allowed${reason}\nIt was not started.`);
			}
			if (signal?.aborted) throw new Error("The turn was stopped before the command started. It was not started.");

			const ended = await start(command, granted.root, seconds, signal, (text) => {
				try { api.output(text); } catch {}
			});
			log(`run in /mnt/${folder}: ${ended.stopped ?? (ended.error ? "could not start" : `exit ${ended.code ?? ended.signal}`)}, ${ended.total} bytes of output`);
			if (api.retainedOutput().truncated) {
				try {
					const path = save(ended.kept);
					api.diagnostic({ severity: "info", code: "full_output", message: ended.total > ended.kept.length ? `The output was ${size(ended.total)}. Its first ${size(ended.kept.length)}: ${path}` : `Full output: ${path}` });
				} catch (error) {
					api.diagnostic({ severity: "warn", code: "full_output", message: `The full output could not be saved on your disk: ${error?.message ?? error}` });
				}
			}
			if (ended.stopped === "cancel") throw new Error("The command was stopped with the turn.");
			if (ended.stopped === "timeout") throw new Error(`The command was stopped after ${seconds} seconds, its time limit.`);
			if (ended.stopped) throw new Error("The command was stopped: the host is closing.");
			if (ended.error !== undefined) throw new Error(`The command could not be started: ${ended.error}`);
			if (ended.code === 0) return {};
			const how = typeof ended.code === "number" ? `Command exited with code ${ended.code}` : `Command was ended by ${ended.signal ?? "a signal"}`;
			return { isError: true, diagnostics: [{ severity: "error", code: "exit_code", message: how }] };
		},
	});

	return {
		tool,
		names,
		/** Ends every command that runs: the host is closing. A host that is killed needs no call: the keepers see it go. */
		stopAll() {
			for (const stop of [...running]) stop("closing");
		},
	};
}
