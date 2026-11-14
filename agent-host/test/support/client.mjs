// A small ACP client that stands in for bzz: it starts the host as bzz does,
// with an environment built from nothing, and serves `_bzz/tool`.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

export const MAIN = fileURLToPath(new URL("../../main.mjs", import.meta.url));
export const SCRIPTED_MODEL = fileURLToPath(new URL("./scripted-model.mjs", import.meta.url));
export const MODEL = { provider: "scripted", modelId: "scripted" };

/** A temporary folder, removed when the test ends. Nothing a test touches lives anywhere else. */
export function tempDir(t, label = "t") {
	const dir = mkdtempSync(join(tmpdir(), `bzz-agent-host-${label}-`));
	t.after(() => {
		// Best effort: on Windows a file still open elsewhere cannot be removed, and the system's temporary folder is not ours to guard.
		try {
			rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
		} catch {}
	});
	return dir;
}

/** What bzz gives a host: a path to programs, a home, and nothing that names a key or a relay. */
export function bareEnv(home, extra = {}) {
	const keep = process.platform === "win32" ? ["PATH", "Path", "SystemRoot", "SYSTEMROOT", "TEMP", "TMP", "PATHEXT"] : ["PATH", "TMPDIR", "LANG"];
	const env = Object.fromEntries(keep.filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]]));
	return { ...env, HOME: home, USERPROFILE: home, BZZ_AGENT_HOST_SCRIPTED_MODEL: SCRIPTED_MODEL, ...extra };
}

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Resolves with `promise`, or fails with what was being waited for. A hung host fails a test instead of the run. */
export function within(ms, promise, what) {
	let timer;
	return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`timed out after ${ms} ms: ${what}`)), ms); })]).finally(() => clearTimeout(timer));
}

/**
 * Starts `node main.mjs <folder>`.
 * @param options.onTool answers `_bzz/tool`: `(params) => result`. Default: "sent as <n>", final as asked.
 * @param options.command another command line, for a host started through something else
 */
export function startHost(t, folder, { env, onTool, command = [process.execPath, MAIN, folder], home = folder } = {}) {
	const child = spawn(command[0], command.slice(1), { stdio: ["pipe", "pipe", "pipe"], env: env ?? bareEnv(home) });
	let stderr = "";
	child.stderr.on("data", (chunk) => { stderr += chunk; });
	const pending = new Map();
	let next = 0;
	const host = {
		child,
		/** Every `_bzz/tool` request received, in order. */
		tools: [],
		/** Every `session/update`, in order: `{ sessionId, update }`. */
		updates: [],
		/** Every line the host wrote, as text: for checks that something never appears on the pipe. */
		lines: [],
		stderr: () => stderr,
		exited: new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal }))),
		onTool: onTool ?? ((params) => ({ text: `sent as ${host.tools.length}`, final: params.arguments?.final === true })),
		write: (message) => child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`),
		rpc(method, params, ms = 30_000) {
			const id = ++next;
			return within(ms, new Promise((resolve, reject) => {
				pending.set(id, { resolve, reject });
				host.write({ id, method, params });
			}), `${method}\n${stderr.slice(-2000)}`);
		},
		notify: (method, params) => host.write({ method, params }),
		async initialize(bzz = {}) {
			return host.rpc("initialize", { protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: "bzz", version: "0.15.0" }, _meta: { bzz: { agent: { id: "00000000-0000-4000-8000-000000000001", name: "Scout" }, model: MODEL, instructions: "Be brief.", mounts: [], network: [], piDir: join(home, "pi"), ...bzz } } });
		},
		async session(place) {
			return (await host.rpc("session/new", { cwd: "/home/agent", mcpServers: [], _meta: { "bzz/place": place } })).sessionId;
		},
		prompt: (sessionId, request, text = "ann: hello") => host.rpc("session/prompt", { sessionId, prompt: [{ type: "text", text }], _meta: { "bzz/request": request } }),
		/** Closes the host's input, as bzz does to stop it, and waits for it to leave. */
		async stop(ms = 5_000) {
			child.stdin.end();
			return within(ms, host.exited, "the host to exit after its input ended");
		},
		async kill() {
			child.kill("SIGKILL");
			await host.exited;
		},
	};
	createInterface({ input: child.stdout }).on("line", async (line) => {
		host.lines.push(line);
		const message = JSON.parse(line);
		if (message.method === "session/update") host.updates.push(message.params);
		else if (message.method === "_bzz/tool") {
			host.tools.push(message.params);
			const result = await host.onTool(message.params);
			// `undefined` leaves the call unanswered, for a test that kills the host inside it.
			if (result !== undefined) host.write({ id: message.id, result });
		} else if (message.method !== undefined && message.id !== undefined) host.write({ id: message.id, error: { code: -32601, message: `Method not found: ${message.method}` } });
		else if (message.method === undefined) {
			const call = pending.get(message.id);
			pending.delete(message.id);
			if (call) message.error ? call.reject(Object.assign(new Error(message.error.message), { code: message.error.code, data: message.error.data })) : call.resolve(message.result);
		}
	});
	t.after(async () => {
		child.kill("SIGKILL");
		await host.exited;
	});
	return host;
}

/** A prompt that carries a script for the stand-in model. */
export const script = (...steps) => `ann: please\ndo: ${JSON.stringify(steps)}`;
