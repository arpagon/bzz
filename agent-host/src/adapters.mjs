// The agent's file tree shown through two interfaces: Node's `fs.promises`
// (what isomorphic-git asks for) and just-bash's IFileSystem (the shell).
import { DIR, FILE, LINK, normalize } from "./disk.mjs";

const TYPE_BITS = { [FILE]: 0o100000, [DIR]: 0o040000, [LINK]: 0o120000 };

/** What isomorphic-git needs from `fs`. */
export function nodeFs(vfs) {
	const stats = (node) => ({
		mode: TYPE_BITS[node.kind] | node.mode,
		size: node.size,
		ino: node.ino,
		mtimeMs: node.mtimeMs,
		ctimeMs: node.mtimeMs,
		uid: 1000,
		gid: 1000,
		dev: 1,
		isFile: () => node.kind === FILE,
		isDirectory: () => node.kind === DIR,
		isSymbolicLink: () => node.kind === LINK,
	});
	return {
		promises: {
			async readFile(path, options) {
				const encoding = typeof options === "string" ? options : options?.encoding;
				const bytes = Buffer.from(vfs.readFile(path));
				return encoding ? bytes.toString(encoding) : bytes;
			},
			async writeFile(path, data, options) {
				const mode = typeof options === "object" && options?.mode ? options.mode & 0o777 : 0o644;
				vfs.writeFile(path, typeof data === "string" ? data : new Uint8Array(data.buffer, data.byteOffset, data.byteLength), { mode });
			},
			async unlink(path) { vfs.unlink(path); },
			async readdir(path) { return vfs.readdir(path).map((entry) => entry.name); },
			async mkdir(path) { vfs.mkdir(path); },
			async rmdir(path) { vfs.rmdir(path); },
			async stat(path) { return stats(vfs.stat(path, true)); },
			async lstat(path) { return stats(vfs.stat(path, false)); },
			async readlink(path) { return vfs.readlink(path); },
			async symlink(target, path) { vfs.symlink(target, path); },
			async chmod(path, mode) { vfs.chmod(path, mode & 0o777); },
		},
	};
}

/** just-bash's IFileSystem. */
export function shellFs(vfs) {
	const stat = (node) => ({
		isFile: node.kind === FILE,
		isDirectory: node.kind === DIR,
		isSymbolicLink: node.kind === LINK,
		mode: node.mode,
		size: node.size,
		mtime: new Date(node.mtimeMs),
		ino: node.ino,
		dev: 1,
	});
	const encodingOf = (options) => (typeof options === "string" ? options : options?.encoding) ?? "utf8";
	const bytes = (content, options) => (typeof content === "string" ? Buffer.from(content, encodingOf(options)) : content);
	const copy = (from, to, recursive) => {
		const node = vfs.stat(from, true);
		if (node.kind !== DIR) return void vfs.writeFile(to, vfs.readFile(from), { mode: node.mode });
		if (!recursive) throw Object.assign(new Error(`EISDIR: is a directory, cp '${from}'`), { code: "EISDIR" });
		vfs.mkdir(to, { recursive: true });
		for (const entry of vfs.readdir(from)) copy(`${from}/${entry.name}`, `${to}/${entry.name}`, true);
	};
	// Scripts send unwanted output to /dev/null all the time. It is the one device the shell has: nothing is kept.
	const sink = (path) => path === "/dev/null";
	const device = { isFile: true, isDirectory: false, isSymbolicLink: false, mode: 0o666, size: 0, mtime: new Date(0), ino: 0, dev: 1 };
	return {
		async readFile(path, options) { return sink(path) ? "" : Buffer.from(vfs.readFile(path)).toString(encodingOf(options)); },
		async readFileBuffer(path) { return sink(path) ? new Uint8Array(0) : vfs.readFile(path); },
		async writeFile(path, content, options) { if (!sink(path)) vfs.writeFile(path, bytes(content, options)); },
		async appendFile(path, content, options) { if (!sink(path)) vfs.appendFile(path, bytes(content, options)); },
		async exists(path) { return sink(path) || vfs.exists(path); },
		async stat(path) { return sink(path) ? device : stat(vfs.stat(path, true)); },
		async lstat(path) { return sink(path) ? device : stat(vfs.stat(path, false)); },
		async mkdir(path, options) { vfs.mkdir(path, { recursive: options?.recursive }); },
		async readdir(path) { return vfs.readdir(path).map((entry) => entry.name); },
		async readdirWithFileTypes(path) {
			return vfs.readdir(path).map((entry) => ({
				name: entry.name, isFile: entry.kind === FILE, isDirectory: entry.kind === DIR, isSymbolicLink: entry.kind === LINK,
			}));
		},
		async rm(path, options) { vfs.rm(path, options); },
		async cp(source, destination, options) { vfs.tx(() => copy(normalize(source), normalize(destination), options?.recursive)); },
		async mv(source, destination) {
			try {
				vfs.rename(source, destination);
			} catch (error) {
				// Between the disk and a granted folder a move is a copy and a removal, as `mv` does across devices.
				if (error.code !== "EXDEV") throw error;
				// Refused before anything is copied, when the source could not be removed afterwards.
				vfs.removable(source);
				copy(normalize(source), normalize(destination), true);
				vfs.rm(source, { recursive: true });
			}
		},
		resolvePath(base, path) { return normalize(path.startsWith("/") ? path : `${base}/${path}`); },
		getAllPaths() { return vfs.allPaths(); },
		async chmod(path, mode) { vfs.chmod(path, mode); },
		async symlink(target, path) { vfs.symlink(target, path); },
		async link(existing, path) { vfs.link(existing, path); },
		async readlink(path) { return vfs.readlink(path); },
		async realpath(path) { return vfs.realpath(path); },
		async utimes(path, _atime, mtime) { vfs.utimes(path, mtime.getTime()); },
	};
}
