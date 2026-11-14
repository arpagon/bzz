// The agent's whole file tree: its disk, and the folders the owner granted
// under /mnt. The shell, git and Pi's tools all go through this one object,
// so a rule enforced here holds for the three of them.
import { Crossing, DIR, FsError, MOUNTS, normalize, split } from "./disk.mjs";
import { Granted, MountPoint } from "./granted.mjs";

const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

export class Vfs {
	/**
	 * @param disk the agent's SqliteFs
	 * @param grants `[{ name, path, write }]`, as bzz sends them at each start. They are never stored.
	 */
	constructor(disk, grants = []) {
		this.disk = disk;
		this.grants = new Map();
		for (const grant of grants) {
			if (typeof grant?.name !== "string" || !NAME.test(grant.name)) throw new Error(`a granted folder needs a simple name, got ${JSON.stringify(grant?.name)}`);
			if (typeof grant.path !== "string" || !grant.path) throw new Error(`the granted folder ${grant.name} has no path`);
			if (this.grants.has(grant.name)) throw new Error(`two granted folders are named ${grant.name}`);
			this.grants.set(grant.name, new Granted(grant));
		}
		this.mnt = new MountPoint([...this.grants.keys()].sort());
		/** Called after each change with the paths it touched. Shared with the disk, which calls it on commit. */
		this.listeners = disk.listeners;
	}

	#backend(path) {
		const parts = split(path);
		if (parts[0] !== MOUNTS) return this.disk;
		return (parts.length > 1 && this.grants.get(parts[1])) || this.mnt;
	}

	/** Runs `call` where `path` really is. A link on the disk may lead into /mnt: the disk says so and the call moves. */
	#on(path, call, changes = false) {
		const run = (at) => {
			const backend = this.#backend(at);
			const value = call(backend, at);
			if (changes && backend !== this.disk) for (const listener of this.listeners) listener([at]);
			return value;
		};
		const at = normalize(path);
		try {
			return run(at);
		} catch (error) {
			if (!(error instanceof Crossing)) throw error;
			return run(error.path);
		}
	}

	/** The path with its directory part resolved, for calls that act on the name itself and not on what it links to. */
	#entry(path, op) {
		const parts = split(path);
		if (parts.length === 0) throw new FsError("EPERM", op, path);
		const parent = this.realpath(`/${parts.slice(0, -1).join("/")}`);
		return `${parent === "/" ? "" : parent}/${parts.at(-1)}`;
	}

	tx(fn) { return this.disk.tx(fn); }
	stat(path, follow = true) { return this.#on(path, (fs, at) => fs.stat(at, follow)); }
	realpath(path) { return this.#on(path, (fs, at) => fs.realpath(at)); }
	readFile(path) { return this.#on(path, (fs, at) => fs.readFile(at)); }
	open(path) { return this.#on(path, (fs, at) => fs.open(at)); }
	readlink(path) { return this.#on(path, (fs, at) => fs.readlink(at)); }
	mkdir(path, options) { return this.#on(path, (fs, at) => fs.mkdir(at, options), true); }
	writeFile(path, content, options) { return this.#on(path, (fs, at) => fs.writeFile(at, content, options), true); }
	appendFile(path, content) { return this.#on(path, (fs, at) => fs.appendFile(at, content), true); }
	truncate(path, size) { return this.#on(path, (fs, at) => fs.truncate(at, size), true); }
	unlink(path) { return this.#on(path, (fs, at) => fs.unlink(at), true); }
	rmdir(path) { return this.#on(path, (fs, at) => fs.rmdir(at), true); }
	rm(path, options) { return this.#on(path, (fs, at) => fs.rm(at, options), true); }
	symlink(target, path) { return this.#on(path, (fs, at) => fs.symlink(target, at), true); }
	chmod(path, mode) { return this.#on(path, (fs, at) => fs.chmod(at, mode), true); }
	utimes(path, mtimeMs) { return this.#on(path, (fs, at) => fs.utimes(at, mtimeMs), true); }

	exists(path) {
		try {
			this.stat(path, true);
			return true;
		} catch (error) {
			if (error instanceof FsError) return false;
			throw error;
		}
	}

	readdir(path) {
		return this.#on(path, (fs, at) => {
			const entries = fs.readdir(at);
			return at === "/" ? [...entries, { name: MOUNTS, kind: DIR, ino: 0 }].sort((a, b) => (a.name < b.name ? -1 : 1)) : entries;
		});
	}

	/** A move or a hard link works inside the disk or inside one granted folder, not between them. */
	#pair(op, source, destination, call) {
		const from = this.#entry(source, op);
		const to = this.#entry(destination, op);
		const backend = this.#backend(from);
		if (backend !== this.#backend(to)) throw new FsError("EXDEV", op, destination);
		let value;
		try {
			value = call(backend, from, to);
		} catch (error) {
			throw error instanceof Crossing ? new FsError("EXDEV", op, destination) : error;
		}
		if (backend !== this.disk) for (const listener of this.listeners) listener([from, to]);
		return value;
	}

	rename(source, destination) { return this.#pair("rename", source, destination, (fs, from, to) => fs.rename(from, to)); }
	link(existing, path) { return this.#pair("link", existing, path, (fs, from, to) => fs.link(from, to)); }

	/** Throws what removing `path` would be refused with, when that can be known without trying. */
	removable(path) {
		const at = this.#entry(path, "rm");
		this.#backend(at).removable?.(at);
	}

	/** Every path of the disk, and the granted folders by name. What is inside a granted folder is not walked. */
	allPaths() {
		return [...this.disk.allPaths(), this.mnt.path, ...this.mnt.names.map((name) => `${this.mnt.path}/${name}`)];
	}

	/** One line per granted folder, for the model. Names only: the real paths stay in bzz. */
	describeGrants() {
		return [...this.grants.values()].map((grant) => `${grant.prefix} (${grant.write ? "read and write" : "read-only"})`);
	}
}
