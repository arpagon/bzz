// One folder of the machine that the owner granted to the agent, shown at
// /mnt/<name>. This is the only code of the host that touches real files for
// the agent, so the rule lives here: every path is resolved to where it
// really leads, links included, and refused unless that is inside the folder.
//
// The check and the use are two steps. Another program of the machine that
// swaps a link between them can win that race; the agent itself cannot, since
// all of its file access goes through this process, one call at a time.
import * as fs from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { DIR, FILE, FsError, LINK, MOUNTS, split } from "./disk.mjs";

const KNOWN = new Set(["ENOENT", "EEXIST", "ENOTDIR", "EISDIR", "EINVAL", "ENOTEMPTY", "ELOOP", "EPERM", "EACCES", "EROFS", "EXDEV"]);
const OUTSIDE = "it leads outside the granted folder";
const READ_ONLY = "this folder was granted read-only";
const WINDOWS = process.platform === "win32";

export class Granted {
	constructor({ name, path, write }) {
		this.name = name;
		this.write = write === true;
		this.prefix = `/${MOUNTS}/${name}`;
		try {
			// The owner may name a link: what it points to is the folder he granted.
			const root = fs.realpathSync.native(path);
			if (fs.statSync(root).isDirectory()) this.root = root;
		} catch {}
	}

	#inside(real) {
		const rest = relative(this.root, real);
		return rest === "" || !(rest === ".." || rest.startsWith(`..${sep}`) || isAbsolute(rest));
	}

	/** Runs `fn` and turns what Node throws into an error that names the virtual path, never the real one. */
	#try(op, path, fn) {
		try {
			return fn();
		} catch (error) {
			if (error instanceof FsError) throw error;
			throw new FsError(KNOWN.has(error?.code) ? error.code : "EIO", op, path);
		}
	}

	/**
	 * The real path for a virtual one. The directory part is always followed to where it really is. The last name
	 * is followed too unless `follow` is false. A path that does not exist yet resolves to where it would be made.
	 */
	#real(path, op, follow = true) {
		if (!this.root) throw new FsError("ENOENT", op, path, "the granted folder is not there");
		const parts = split(path).slice(2);
		// On Windows `\` and `:` inside a name would be read as path syntax and could climb out.
		for (const part of parts) if (WINDOWS && /[\\:]/.test(part)) throw new FsError("EINVAL", op, path);
		if (parts.length === 0) return this.root;
		return this.#try(op, path, () => {
			const parent = fs.realpathSync.native(join(this.root, ...parts.slice(0, -1)));
			if (!this.#inside(parent)) throw new FsError("EACCES", op, path, OUTSIDE);
			const leaf = join(parent, parts.at(-1));
			if (!follow) return leaf;
			let real;
			try {
				real = fs.realpathSync.native(leaf);
			} catch (error) {
				if (error.code !== "ENOENT") throw error;
				// A link to nothing: writing through it would create its target, wherever that is.
				if (fs.lstatSync(leaf, { throwIfNoEntry: false })) throw new FsError("ENOENT", op, path);
				return leaf;
			}
			if (!this.#inside(real)) throw new FsError("EACCES", op, path, OUTSIDE);
			return real;
		});
	}

	#writable(op, path) {
		if (!this.write) throw new FsError("EROFS", op, path, READ_ONLY);
	}

	#node(stats) {
		const kind = stats.isDirectory() ? DIR : stats.isSymbolicLink() ? LINK : FILE;
		return { ino: Number(stats.ino), kind, mode: stats.mode & 0o7777, mtimeMs: Math.floor(stats.mtimeMs), size: kind === DIR ? 0 : stats.size, nlink: stats.nlink };
	}

	/** Content is read and written only for regular files: opening a pipe or a device would block the whole host. */
	#regular(real, op, path) {
		const stats = fs.statSync(real, { throwIfNoEntry: false });
		if (stats?.isDirectory()) throw new FsError("EISDIR", op, path);
		if (stats && !stats.isFile()) throw new FsError("EPERM", op, path, "not a regular file");
		return stats;
	}

	stat(path, follow = true) {
		const op = follow ? "stat" : "lstat";
		const real = this.#real(path, op, follow);
		return this.#try(op, path, () => this.#node(follow ? fs.statSync(real) : fs.lstatSync(real)));
	}

	realpath(path) {
		const real = this.#real(path, "realpath");
		this.#try("realpath", path, () => fs.statSync(real));
		const rest = relative(this.root, real);
		return rest ? `${this.prefix}/${rest.split(sep).join("/")}` : this.prefix;
	}

	readdir(path) {
		const real = this.#real(path, "scandir");
		return this.#try("scandir", path, () =>
			fs.readdirSync(real, { withFileTypes: true })
				.map((entry) => ({ name: entry.name, kind: entry.isDirectory() ? DIR : entry.isSymbolicLink() ? LINK : FILE, ino: 0 }))
				.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)));
	}

	readFile(path) {
		const real = this.#real(path, "open");
		return this.#try("open", path, () => {
			if (!this.#regular(real, "read", path)) throw new FsError("ENOENT", "open", path);
			return new Uint8Array(fs.readFileSync(real));
		});
	}

	open(path) {
		const real = this.#real(path, "open");
		return this.#try("open", path, () => {
			if (!this.#regular(real, "open", path)) throw new FsError("ENOENT", "open", path);
			const fd = fs.openSync(real, "r");
			let open = true;
			return {
				stat: () => (open ? this.#node(fs.fstatSync(fd)) : undefined),
				read: (offset, length) => {
					const buffer = new Uint8Array(length);
					return buffer.subarray(0, fs.readSync(fd, buffer, 0, length, offset));
				},
				close: () => {
					if (open) fs.closeSync(fd);
					open = false;
				},
			};
		});
	}

	mkdir(path, { recursive = false, mode = 0o755 } = {}) {
		this.#writable("mkdir", path);
		if (!recursive) {
			const real = this.#real(path, "mkdir");
			return this.#try("mkdir", path, () => void fs.mkdirSync(real, { mode }));
		}
		const parts = split(path);
		// One level at a time, so that each new directory is checked from a parent that was.
		for (let i = 3; i <= parts.length; i++) {
			const prefix = `/${parts.slice(0, i).join("/")}`;
			const real = this.#real(prefix, "mkdir");
			this.#try("mkdir", path, () => {
				const stats = fs.statSync(real, { throwIfNoEntry: false });
				if (!stats) fs.mkdirSync(real, { mode });
				else if (!stats.isDirectory()) throw new FsError(i === parts.length ? "EEXIST" : "ENOTDIR", "mkdir", path);
			});
		}
	}

	writeFile(path, content, { mode = 0o644, exclusive = false } = {}) {
		this.#writable("open", path);
		const real = this.#real(path, "open");
		this.#try("open", path, () => {
			this.#regular(real, "open", path);
			fs.writeFileSync(real, content, { mode, flag: exclusive ? "wx" : "w" });
		});
	}

	appendFile(path, content) {
		this.#writable("open", path);
		const real = this.#real(path, "open");
		this.#try("open", path, () => {
			this.#regular(real, "open", path);
			fs.appendFileSync(real, content);
		});
	}

	truncate(path, size) {
		this.#writable("truncate", path);
		const real = this.#real(path, "truncate");
		this.#try("truncate", path, () => {
			if (!this.#regular(real, "truncate", path)) throw new FsError("ENOENT", "truncate", path);
			fs.truncateSync(real, size);
		});
	}

	unlink(path) {
		this.#writable("unlink", path);
		const real = this.#real(path, "unlink", false);
		this.#try("unlink", path, () => {
			if (fs.lstatSync(real).isDirectory()) throw new FsError("EISDIR", "unlink", path);
			fs.unlinkSync(real);
		});
	}

	rmdir(path) {
		this.#writable("rmdir", path);
		const real = this.#real(path, "rmdir", false);
		if (real === this.root) throw new FsError("EPERM", "rmdir", path);
		this.#try("rmdir", path, () => fs.rmdirSync(real));
	}

	rm(path, { recursive = false, force = false } = {}) {
		this.#writable("rm", path);
		let real;
		try {
			real = this.#real(path, "rm", false);
		} catch (error) {
			if (force && error.code === "ENOENT") return;
			throw error;
		}
		if (real === this.root) throw new FsError("EPERM", "rm", path);
		this.#try("rm", path, () => {
			const stats = fs.lstatSync(real, { throwIfNoEntry: false });
			if (!stats) {
				if (force) return;
				throw new FsError("ENOENT", "rm", path);
			}
			// `rmSync` removes a link, never what it points to, so a tree cannot be emptied through one.
			if (stats.isDirectory() && !recursive) fs.rmdirSync(real);
			else fs.rmSync(real, { recursive: true, force });
		});
	}

	removable(path) {
		this.#writable("rm", path);
		if (this.#real(path, "rm", false) === this.root) throw new FsError("EPERM", "rm", path);
	}

	/** Both paths are in this folder. */
	rename(source, destination) {
		this.#writable("rename", source);
		const from = this.#real(source, "rename", false);
		const to = this.#real(destination, "rename", false);
		if (from === this.root || to === this.root) throw new FsError("EPERM", "rename", source);
		this.#try("rename", source, () => fs.renameSync(from, to));
	}

	/** Only a relative link that stays in the folder: an absolute one would name a path of this machine. */
	symlink(target, path) {
		this.#writable("symlink", path);
		const real = this.#real(path, "symlink", false);
		if (target.startsWith("/") || isAbsolute(target) || !this.#inside(resolve(real, "..", target)))
			throw new FsError("EACCES", "symlink", path, "a link in a granted folder must be relative and stay inside it");
		this.#try("symlink", path, () => fs.symlinkSync(target, real));
	}

	readlink(path) {
		const real = this.#real(path, "readlink", false);
		return this.#try("readlink", path, () => fs.readlinkSync(real).split(sep).join("/"));
	}

	link(existing, path) {
		this.#writable("link", path);
		const from = this.#real(existing, "link");
		const to = this.#real(path, "link", false);
		this.#try("link", path, () => fs.linkSync(from, to));
	}

	chmod(path, mode) {
		this.#writable("chmod", path);
		const real = this.#real(path, "chmod");
		// The granted folder itself stays as its owner set it: a mode of 000 would lock him out of it.
		if (real === this.root) throw new FsError("EPERM", "chmod", path);
		this.#try("chmod", path, () => fs.chmodSync(real, mode));
	}

	utimes(path, mtimeMs) {
		this.#writable("utimes", path);
		const real = this.#real(path, "utimes");
		this.#try("utimes", path, () => fs.utimesSync(real, new Date(), new Date(mtimeMs)));
	}
}

/** /mnt itself: a fixed directory that lists the granted folders. Nothing else can be put there. */
export class MountPoint {
	constructor(names) {
		this.names = names;
		this.path = `/${MOUNTS}`;
	}

	#refuse(op, path) {
		const parts = split(path);
		// Below a name that is not granted there is nothing; /mnt itself and its entries cannot change.
		throw parts.length > 2 ? new FsError("ENOENT", op, path) : new FsError("EPERM", op, path, `only granted folders appear in ${this.path}`);
	}

	stat(path, follow = true) {
		if (split(path).length > 1) throw new FsError("ENOENT", follow ? "stat" : "lstat", path);
		return { ino: 0, kind: DIR, mode: 0o555, mtimeMs: 0, size: 0, nlink: 1 };
	}

	realpath(path) {
		this.stat(path);
		return this.path;
	}

	readdir(path) {
		this.stat(path);
		return this.names.map((name) => ({ name, kind: DIR, ino: 0 }));
	}

	readFile(path) {
		this.stat(path);
		throw new FsError("EISDIR", "read", path);
	}

	open(path) { return this.readFile(path); }
	readlink(path) { this.stat(path); throw new FsError("EINVAL", "readlink", path); }
	mkdir(path, options) {
		if (options?.recursive && split(path).length === 1) return;
		this.#refuse("mkdir", path);
	}
	rm(path, options) {
		if (options?.force && split(path).length > 1) return;
		this.#refuse("rm", path);
	}
	removable(path) { this.#refuse("rm", path); }
	writeFile(path) { this.#refuse("open", path); }
	appendFile(path) { this.#refuse("open", path); }
	truncate(path) { this.#refuse("truncate", path); }
	unlink(path) { this.#refuse("unlink", path); }
	rmdir(path) { this.#refuse("rmdir", path); }
	rename(path) { this.#refuse("rename", path); }
	symlink(_target, path) { this.#refuse("symlink", path); }
	link(_existing, path) { this.#refuse("link", path); }
	chmod(path) { this.#refuse("chmod", path); }
	utimes(path) { this.#refuse("utimes", path); }
}
