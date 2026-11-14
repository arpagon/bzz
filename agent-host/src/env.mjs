// The agent's file tree and shell as a Pi Durable execution environment: what
// Pi's own `read`, `write`, `edit` and `bash` tools run on. Commands run in
// just-bash, an interpreter inside this process: no real process starts.
import { randomUUID } from "node:crypto";
import { posix } from "node:path";
import { err, ExecutionError, FileError, LineScanner, ok } from "@earendil-works/pi-durable/env";
import { Bash, DefenseInDepthBox } from "just-bash";
import { shellFs } from "./adapters.mjs";
import { DIR, FILE, FsError, LINK, normalize } from "./disk.mjs";
import { gitCommand } from "./git.mjs";

export const HOME = "/home/agent";
const KIND = { [FILE]: "file", [DIR]: "directory", [LINK]: "symlink" };
const CODE = { ENOENT: "not_found", ENOTDIR: "not_directory", EISDIR: "is_directory", EINVAL: "invalid", EPERM: "permission_denied", EACCES: "permission_denied", EROFS: "permission_denied" };
const fileError = (error, path) =>
	error instanceof FileError ? error
	: error instanceof FsError ? new FileError(CODE[error.code] ?? "unknown", error.message, path, error)
	: new FileError("unknown", String(error?.message ?? error), path, error instanceof Error ? error : undefined);
const quote = (word) => `'${word.replaceAll("'", `'\\''`)}'`;
const info = (path, node) => ({ name: posix.basename(path), path, kind: KIND[node.kind], size: node.size, mtimeMs: node.mtimeMs });

/**
 * The agent's shell. Its environment is its own: nothing of this process's environment is visible in it, which
 * matters most in a remote copy, where Buzz puts the agent's key there.
 */
export function createShell(vfs, { net, author }) {
	for (const dir of [HOME, "/tmp"]) vfs.mkdir(dir, { recursive: true });
	return new Bash({
		fs: shellFs(vfs),
		cwd: HOME,
		env: { HOME, USER: "agent", LOGNAME: "agent" },
		// just-bash blocks parts of the runtime while a script runs, Node's own `fetch` among what breaks. The request
		// is the host's code, not the script's: the allow-list is inside it.
		fetch: (url, options) => DefenseInDepthBox.runTrustedAsync(() => net.shell(url, options)),
		customCommands: [gitCommand(vfs, { http: net.git, author })],
	});
}

export class BzzVfsEnv {
	/**
	 * @param vfs the agent's file tree
	 * @param bash the shell of `createShell`
	 * @param options.agent names the file namespace: equal ids see the same files
	 */
	constructor(vfs, bash, { agent, cwd = HOME }) {
		this.vfs = vfs;
		this.bash = bash;
		this.id = `bzz-vfs:${agent}`;
		this.cwd = cwd;
	}

	#abs(path) {
		return normalize(path.startsWith("/") ? path : `${this.cwd}/${path}`);
	}

	/** Runs `fn` on the absolute path and maps what it throws to a FileError. */
	#file(path, context, fn) {
		let resolved = path;
		try {
			resolved = this.#abs(path);
			if (context.abortSignal?.aborted) return err(new FileError("aborted", "aborted", resolved));
			return ok(fn(resolved));
		} catch (error) {
			return err(fileError(error, resolved));
		}
	}

	async absolutePath(path) { return ok(this.#abs(path)); }
	async joinPath(parts) { return ok(posix.join(...parts)); }
	async readTextFile(path, context) { return this.#file(path, context, (at) => new TextDecoder().decode(this.vfs.readFile(at))); }
	async readBinaryFile(path, context) { return this.#file(path, context, (at) => this.vfs.readFile(at)); }

	async openTextLineReader(path, context) {
		return this.#file(path, context, (at) => {
			let rest = new TextDecoder().decode(this.vfs.readFile(at));
			let closed = false;
			return {
				async readLine(inner) {
					if (inner.abortSignal?.aborted) return err(new FileError("aborted", "aborted", at));
					if (closed) return err(new FileError("invalid", "Text line reader is closed", at));
					const newline = rest.indexOf("\n");
					if (newline !== -1) {
						const text = rest.slice(0, newline);
						rest = rest.slice(newline + 1);
						return ok({ text, terminated: true });
					}
					if (rest.length === 0) return ok(undefined);
					const text = rest;
					rest = "";
					return ok({ text, terminated: false });
				},
				async close() { closed = true; },
			};
		});
	}

	async readTextLines(path, options, context) {
		if (options?.maxLines !== undefined && options.maxLines <= 0) return ok([]);
		const opened = await this.openTextLineReader(path, context);
		if (!opened.ok) return opened;
		const lines = [];
		while (options?.maxLines === undefined || lines.length < options.maxLines) {
			const line = await opened.value.readLine(context);
			if (!line.ok) return line;
			if (line.value === undefined) break;
			lines.push(line.value.text);
		}
		return ok(lines);
	}

	async openBinaryReader(path, options, context) {
		const vfs = this.vfs;
		return this.#file(path, context, (at) => {
			if (options?.noFollow && vfs.stat(at, false).kind === LINK)
				throw new FileError("invalid", "Refusing to follow a symbolic link", at);
			// Held by the file itself, so a rename of the path does not change what is read.
			const file = vfs.open(at);
			let closed = false;
			const guard = (inner) =>
				inner.abortSignal?.aborted ? err(new FileError("aborted", "aborted", at))
				: closed ? err(new FileError("invalid", "Binary reader is closed", at))
				: undefined;
			const attempt = (fn) => {
				try {
					return ok(fn());
				} catch (error) {
					return err(fileError(error, at));
				}
			};
			const stat = () => {
				const node = file.stat();
				if (!node) throw new FileError("not_found", "The file is gone", at);
				return node;
			};
			return {
				async info(inner) { return guard(inner) ?? attempt(() => info(at, stat())); },
				async read(offset, length, inner) {
					const refused = guard(inner);
					if (refused) return refused;
					if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(length) || length < 0)
						return err(new FileError("invalid", "Offset and length must be non-negative safe integers", at));
					return attempt(() => file.read(offset, length));
				},
				async scanLines(range, inner) {
					const refused = guard(inner);
					if (refused) return refused;
					let scanner;
					try {
						scanner = new LineScanner(range.startLine, range.endLine);
					} catch {
						return err(new FileError("invalid", "Invalid line range", at));
					}
					return attempt(() => {
						const size = file.stat()?.size ?? 0;
						for (let offset = 0; offset < size; offset += 65536) scanner.push(file.read(offset, 65536));
						return scanner.finish();
					});
				},
				async close() {
					closed = true;
					file.close();
				},
			};
		});
	}

	async writeFile(path, content, context) {
		return this.#file(path, context, (at) => {
			this.vfs.tx(() => {
				this.vfs.mkdir(posix.dirname(at), { recursive: true });
				this.vfs.writeFile(at, content);
			});
		});
	}

	async appendFile(path, content, context) {
		return this.#file(path, context, (at) => {
			this.vfs.tx(() => {
				this.vfs.mkdir(posix.dirname(at), { recursive: true });
				this.vfs.appendFile(at, content);
			});
		});
	}

	async truncateFile(path, size, context) { return this.#file(path, context, (at) => void this.vfs.truncate(at, size)); }
	// Every write is already committed when the call returns.
	async flushFile(path, context) { return this.#file(path, context, (at) => void this.vfs.stat(at, true)); }
	async renameFile(source, destination, context) { return this.#file(source, context, (at) => void this.vfs.rename(at, this.#abs(destination))); }
	async fileInfo(path, context) { return this.#file(path, context, (at) => info(at, this.vfs.stat(at, false))); }
	async canonicalPath(path, context) { return this.#file(path, context, (at) => this.vfs.realpath(at)); }
	async createDir(path, options, context) { return this.#file(path, context, (at) => void this.vfs.mkdir(at, { recursive: options?.recursive ?? true })); }
	async remove(path, options, context) { return this.#file(path, context, (at) => void this.vfs.rm(at, { recursive: options?.recursive ?? false, force: options?.force ?? false })); }

	async exists(path, context) {
		const found = await this.fileInfo(path, context);
		return found.ok ? ok(true) : found.error.code === "not_found" ? ok(false) : err(found.error);
	}

	async listDir(path, context) {
		return this.#file(path, context, (at) => this.vfs.readdir(at).map((entry) => info(`${at === "/" ? "" : at}/${entry.name}`, this.vfs.stat(`${at}/${entry.name}`, false))));
	}

	async openDirReader(path, context) {
		const vfs = this.vfs;
		return this.#file(path, context, (at) => {
			const names = vfs.readdir(at).map((entry) => entry.name);
			let next = 0;
			let closed = false;
			return {
				async next(maxEntries, inner) {
					if (inner.abortSignal?.aborted) return err(new FileError("aborted", "aborted", at));
					if (closed) return err(new FileError("invalid", "Directory reader is closed", at));
					if (!Number.isSafeInteger(maxEntries) || maxEntries <= 0)
						return err(new FileError("invalid", "maxEntries must be a positive safe integer", at));
					const entries = [];
					while (next < names.length && entries.length < maxEntries) {
						const entry = `${at === "/" ? "" : at}/${names[next++]}`;
						try {
							entries.push(info(entry, vfs.stat(entry, false)));
						} catch (error) {
							// Removed since the listing was taken: no longer part of it.
							if (!(error instanceof FsError && error.code === "ENOENT")) return err(fileError(error, entry));
						}
					}
					return ok({ entries, done: next >= names.length });
				},
				async close() { closed = true; },
			};
		});
	}

	/**
	 * Every change the agent makes goes through this process, so watching is exact: each change names its paths. A
	 * path is reported for a target when it is at or below it, above it, or at the file a link points to. What another
	 * program changes in a granted folder is not seen.
	 */
	async watch(targets, onChange, context) {
		if (context.abortSignal?.aborted) return err(new FileError("aborted", "aborted"));
		const wanted = targets.map((target) => ({ ...target, path: this.#abs(target.path) }));
		const below = (path, base) => path === base || path.startsWith(base === "/" ? "/" : `${base}/`);
		const excluded = (target, rest) =>
			rest.some((name) => (target.exclude?.hidden && name.startsWith(".")) || target.exclude?.names?.includes(name));
		let stopped = false;
		const listener = (changed) => {
			const report = new Set();
			for (const target of wanted) {
				let real;
				try {
					real = this.vfs.realpath(target.path);
				} catch {}
				for (const path of changed) {
					if (below(path, target.path)) {
						const rest = path === target.path ? [] : path.slice(target.path.length + 1).split("/");
						if (excluded(target, rest)) continue;
						report.add(target.recursive || rest.length <= 1 ? path : `${target.path}/${rest[0]}`);
					} else if (below(target.path, path)) report.add(target.path);
					else if (real !== undefined && real !== target.path && (below(path, real) || below(real, path))) report.add(target.path);
				}
			}
			if (report.size) queueMicrotask(() => stopped || onChange({ kind: "paths", paths: [...report] }));
		};
		this.vfs.listeners.add(listener);
		return ok({
			mode: "native",
			stop: async () => {
				stopped = true;
				this.vfs.listeners.delete(listener);
			},
		});
	}

	async createTempDir(prefix, context) { return this.#file(`/tmp/${prefix ?? "tmp-"}${randomUUID().slice(0, 8)}`, context, (at) => (this.vfs.mkdir(at, { recursive: true }), at)); }

	async createTempFile(options, context) {
		const dir = await this.createTempDir("tmp-", context);
		if (!dir.ok) return dir;
		return this.#file(`${dir.value}/${options?.prefix ?? ""}${randomUUID()}${options?.suffix ?? ""}`, context, (at) => (this.vfs.writeFile(at, ""), at));
	}

	async exec(command, options, context) {
		const outer = context.abortSignal;
		if (outer?.aborted) return err(new ExecutionError("aborted", "aborted"));
		const timeout = options?.timeout;
		if (timeout !== undefined && !(Number.isFinite(timeout) && timeout > 0))
			return err(new ExecutionError("unknown", `Invalid timeout: ${timeout}`));
		const cwd = options?.cwd ? this.#abs(options.cwd) : this.cwd;
		if (!this.vfs.exists(cwd)) return err(new ExecutionError("spawn_error", `Working directory does not exist: ${cwd}`));
		let script = command;
		if (typeof command !== "string") {
			if (command.length === 0) return err(new ExecutionError("spawn_error", "Empty argv: no program to run"));
			const known = await this.bash.exec(`command -v ${quote(command[0])}`, { cwd });
			if (known.exitCode !== 0) return err(new ExecutionError("spawn_error", `${command[0]}: no such command in this sandbox`));
			script = command.map(quote).join(" ");
		}
		const controller = new AbortController();
		const onAbort = () => controller.abort();
		outer?.addEventListener("abort", onAbort, { once: true });
		let timedOut = false;
		const timer = timeout === undefined ? undefined : setTimeout(() => { timedOut = true; controller.abort(); }, timeout * 1000);
		let result;
		try {
			result = await this.bash.exec(script, { cwd, signal: controller.signal, ...(options?.env && { env: options.env }) });
		} catch (error) {
			// just-bash lets a refused write of a redirection escape the whole script. The command failed; nothing more.
			if (!(error instanceof FsError)) return err(new ExecutionError("spawn_error", String(error?.message ?? error), error instanceof Error ? error : undefined));
			result = { stdout: "", stderr: `bash: ${error.message}\n`, exitCode: 1 };
		} finally {
			clearTimeout(timer);
			outer?.removeEventListener("abort", onAbort);
		}
		if (timedOut) return err(new ExecutionError("timeout", `timeout:${timeout}`));
		if (outer?.aborted) return err(new ExecutionError("aborted", "aborted"));
		// just-bash returns the output when the script ends, so each stream arrives as one chunk.
		try {
			if (result.stdout) options?.onOutput?.(result.stdout, context, { stream: "stdout" });
			if (result.stderr) options?.onOutput?.(result.stderr, context, { stream: "stderr" });
		} catch (error) {
			return err(new ExecutionError("callback_error", String(error?.message ?? error), error instanceof Error ? error : undefined));
		}
		const output = result.stdout + result.stderr;
		const spill = options?.spill;
		if (spill && (Buffer.byteLength(output) > spill.afterBytes || output.split("\n").length - 1 > spill.afterLines)) {
			const file = await this.createTempFile({ prefix: "exec-", suffix: ".log" }, context);
			if (file.ok) {
				this.vfs.writeFile(file.value, output);
				return ok({ exitCode: result.exitCode, spillPath: file.value });
			}
		}
		return ok({ exitCode: result.exitCode });
	}

	async cleanup() {}
}
