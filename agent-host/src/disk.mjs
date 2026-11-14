// The agent's disk: one SQLite file. Paths are virtual and absolute, so the
// file holds no path of the machine and can be copied to another one. Content
// is stored in chunks named by their SHA-256: equal chunks are kept once.
// Every public call is one transaction, so a kill leaves the last whole call.
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";

export const FILE = 1;
export const DIR = 2;
export const LINK = 3;
const ROOT = 1;
const CHUNK = 256 * 1024;
/** Granted folders live here, outside the disk: see vfs.mjs. */
export const MOUNTS = "mnt";

const TEXT = {
	ENOENT: "no such file or directory", EEXIST: "file already exists", ENOTDIR: "not a directory",
	EISDIR: "illegal operation on a directory", EINVAL: "invalid argument", ENOTEMPTY: "directory not empty",
	ELOOP: "too many levels of symbolic links", EPERM: "operation not permitted", EACCES: "permission denied",
	EROFS: "read-only file system", EXDEV: "cross-device link not permitted", EIO: "input/output error",
};
const ERRNO = { EPERM: -1, ENOENT: -2, EIO: -5, EACCES: -13, EEXIST: -17, EXDEV: -18, ENOTDIR: -20, EISDIR: -21, EINVAL: -22, EROFS: -30, ENOTEMPTY: -39, ELOOP: -40 };

/** Shaped like Node's file errors, which isomorphic-git and the shell read by `code`. The path is always virtual. */
export class FsError extends Error {
	constructor(code, op, path, why) {
		super(`${code}: ${why ?? TEXT[code]}, ${op} '${path}'`);
		this.code = code;
		this.errno = ERRNO[code];
		this.syscall = op;
		this.path = path;
	}
}

/** Thrown when a path on the disk leads into /mnt, through a link for example. The caller continues there. */
export class Crossing extends Error {
	constructor(path) {
		super(`${path} is outside the disk`);
		this.path = path;
	}
}

/** "/a/./b/../c" -> ["a", "c"]. `..` is resolved on the text of the path. */
export function split(path) {
	if (typeof path !== "string" || !path.startsWith("/") || path.includes("\0")) throw new FsError("EINVAL", "path", String(path));
	const parts = [];
	for (const part of path.split("/")) {
		if (part === "" || part === ".") continue;
		if (part === "..") parts.pop();
		else parts.push(part);
	}
	return parts;
}

export const normalize = (path) => `/${split(path).join("/")}`;
const bytesOf = (content) => (typeof content === "string" ? new TextEncoder().encode(content) : content);

export class SqliteFs {
	#file;
	#db;
	#q;

	constructor(file) {
		this.#file = file;
		this.depth = 0;
		/** Called after each commit with the paths it changed. */
		this.listeners = new Set();
		this.pending = [];
		this.#open();
	}

	#open() {
		const db = new DatabaseSync(this.#file);
		db.exec(`
			PRAGMA journal_mode = WAL;
			PRAGMA synchronous = NORMAL;
			CREATE TABLE IF NOT EXISTS node (
				ino INTEGER PRIMARY KEY, kind INTEGER NOT NULL, mode INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
				size INTEGER NOT NULL DEFAULT 0, nlink INTEGER NOT NULL DEFAULT 1, target TEXT);
			CREATE TABLE IF NOT EXISTS dirent (
				parent INTEGER NOT NULL, name TEXT NOT NULL, ino INTEGER NOT NULL, PRIMARY KEY (parent, name)) WITHOUT ROWID;
			CREATE TABLE IF NOT EXISTS chunk (
				ino INTEGER NOT NULL, idx INTEGER NOT NULL, hash BLOB NOT NULL, PRIMARY KEY (ino, idx)) WITHOUT ROWID;
			CREATE TABLE IF NOT EXISTS blob (hash BLOB PRIMARY KEY, data BLOB NOT NULL, refs INTEGER NOT NULL) WITHOUT ROWID;
			CREATE INDEX IF NOT EXISTS chunk_hash ON chunk (hash);
			INSERT OR IGNORE INTO node (ino, kind, mode, mtime_ms) VALUES (${ROOT}, ${DIR}, 493, 0);
		`);
		const p = (sql) => db.prepare(sql);
		this.#q = {
			node: p("SELECT ino, kind, mode, mtime_ms AS mtimeMs, size, nlink, target FROM node WHERE ino = ?"),
			newNode: p("INSERT INTO node (kind, mode, mtime_ms, target) VALUES (?, ?, ?, ?)"),
			delNode: p("DELETE FROM node WHERE ino = ?"),
			setSize: p("UPDATE node SET size = ?, mtime_ms = ? WHERE ino = ?"),
			setMode: p("UPDATE node SET mode = ? WHERE ino = ?"),
			setMtime: p("UPDATE node SET mtime_ms = ? WHERE ino = ?"),
			addLink: p("UPDATE node SET nlink = nlink + ? WHERE ino = ?"),
			dirent: p("SELECT ino FROM dirent WHERE parent = ? AND name = ?"),
			addDirent: p("INSERT INTO dirent (parent, name, ino) VALUES (?, ?, ?)"),
			delDirent: p("DELETE FROM dirent WHERE parent = ? AND name = ?"),
			list: p("SELECT d.name, n.kind, n.ino FROM dirent d JOIN node n ON n.ino = d.ino WHERE d.parent = ? ORDER BY d.name"),
			anyChild: p("SELECT 1 FROM dirent WHERE parent = ? LIMIT 1"),
			chunks: p("SELECT b.data FROM chunk c JOIN blob b ON b.hash = c.hash WHERE c.ino = ? ORDER BY c.idx"),
			chunkRange: p("SELECT c.idx, b.data FROM chunk c JOIN blob b ON b.hash = c.hash WHERE c.ino = ? AND c.idx BETWEEN ? AND ? ORDER BY c.idx"),
			chunkHashes: p("SELECT idx, hash FROM chunk WHERE ino = ? AND idx >= ? ORDER BY idx"),
			addChunk: p("INSERT INTO chunk (ino, idx, hash) VALUES (?, ?, ?)"),
			delChunksFrom: p("DELETE FROM chunk WHERE ino = ? AND idx >= ?"),
			addBlob: p("INSERT INTO blob (hash, data, refs) VALUES (?, ?, 1) ON CONFLICT (hash) DO UPDATE SET refs = refs + 1"),
			dropBlob: p("UPDATE blob SET refs = refs - 1 WHERE hash = ?"),
			delBlob: p("DELETE FROM blob WHERE hash = ? AND refs <= 0"),
			allDirents: p("SELECT parent, name, ino FROM dirent ORDER BY parent, name"),
		};
		this.#db = db;
		return db;
	}

	/** The connection, opened again after a checkpoint closed it. */
	get db() {
		return this.#db ?? this.#open();
	}

	get q() {
		this.db;
		return this.#q;
	}

	note(...paths) {
		if (this.listeners.size) this.pending.push(...paths);
	}

	/**
	 * Moves everything into the main file and closes it. SQLite removes the write-ahead file only when the last
	 * connection closes, so that is how none is left; the next call opens the file again.
	 */
	checkpoint() {
		if (!this.#db) return;
		this.#db.exec("PRAGMA wal_checkpoint(TRUNCATE)");
		this.#db.close();
		this.#db = undefined;
	}

	close() {
		this.checkpoint();
	}

	tx(fn) {
		if (this.depth > 0) return fn();
		const db = this.db;
		db.exec("BEGIN IMMEDIATE");
		this.depth = 1;
		try {
			const value = fn();
			db.exec("COMMIT");
			this.depth = 0;
			const changed = this.pending;
			this.pending = [];
			if (changed.length) for (const listener of this.listeners) listener(changed);
			return value;
		} catch (error) {
			db.exec("ROLLBACK");
			this.pending = [];
			throw error;
		} finally {
			this.depth = 0;
		}
	}

	/** Where a path leads: its directory, its name there, and its node if it exists. */
	lookup(path, follow, op, hops = 0) {
		const parts = split(path);
		if (parts[0] === MOUNTS) throw new Crossing(`/${parts.join("/")}`);
		let ino = ROOT;
		let parent = ROOT;
		let name = "";
		let node = this.q.node.get(ROOT);
		for (let i = 0; i < parts.length; i++) {
			if (node.kind !== DIR) throw new FsError("ENOTDIR", op, path);
			parent = ino;
			name = parts[i];
			const last = i === parts.length - 1;
			const entry = this.q.dirent.get(parent, name);
			if (!entry) {
				if (last) return { parent, name, node: undefined, path: `/${parts.join("/")}` };
				throw new FsError("ENOENT", op, path);
			}
			ino = entry.ino;
			node = this.q.node.get(ino);
			if (node.kind === LINK && (!last || follow)) {
				if (hops >= 40) throw new FsError("ELOOP", op, path);
				const base = `/${parts.slice(0, i).join("/")}`;
				const target = node.target.startsWith("/") ? node.target : `${base}/${node.target}`;
				const rest = parts.slice(i + 1).join("/");
				return this.lookup(rest ? `${target}/${rest}` : target, follow, op, hops + 1);
			}
		}
		return { parent, name, node, path: `/${parts.join("/")}` };
	}

	need(path, follow, op) {
		const found = this.lookup(path, follow, op);
		if (!found.node) throw new FsError("ENOENT", op, path);
		return found;
	}

	#release(ino, fromIdx = 0) {
		for (const { hash } of this.q.chunkHashes.all(ino, fromIdx)) {
			this.q.dropBlob.run(hash);
			this.q.delBlob.run(hash);
		}
		this.q.delChunksFrom.run(ino, fromIdx);
	}

	#store(ino, fromIdx, bytes) {
		for (let offset = 0, idx = fromIdx; offset < bytes.length; offset += CHUNK, idx++) {
			const data = bytes.subarray(offset, Math.min(offset + CHUNK, bytes.length));
			const hash = createHash("sha256").update(data).digest();
			this.q.addBlob.run(hash, data);
			this.q.addChunk.run(ino, idx, hash);
		}
	}

	#content(ino, size) {
		const out = new Uint8Array(size);
		let offset = 0;
		for (const { data } of this.q.chunks.all(ino)) {
			out.set(data, offset);
			offset += data.length;
		}
		return out;
	}

	#drop(ino) {
		const node = this.q.node.get(ino);
		if (node.nlink > 1) return void this.q.addLink.run(-1, ino);
		this.#release(ino);
		this.q.delNode.run(ino);
	}

	#create(parent, name, kind, mode, target = null) {
		const { lastInsertRowid } = this.q.newNode.run(kind, mode, Date.now(), target);
		const ino = Number(lastInsertRowid);
		this.q.addDirent.run(parent, name, ino);
		this.q.setMtime.run(Date.now(), parent);
		return ino;
	}

	stat(path, follow = true) {
		return this.tx(() => this.need(path, follow, follow ? "stat" : "lstat").node);
	}

	realpath(path) {
		return this.tx(() => this.need(path, true, "realpath").path);
	}

	readdir(path) {
		return this.tx(() => {
			const { node } = this.need(path, true, "scandir");
			if (node.kind !== DIR) throw new FsError("ENOTDIR", "scandir", path);
			return this.q.list.all(node.ino);
		});
	}

	mkdir(path, { recursive = false, mode = 0o755 } = {}) {
		return this.tx(() => {
			if (!recursive) {
				const found = this.lookup(path, true, "mkdir");
				if (found.node || found.name === "") throw new FsError("EEXIST", "mkdir", path);
				this.#create(found.parent, found.name, DIR, mode);
				this.note(found.path);
				return;
			}
			const parts = split(path);
			for (let i = 1; i <= parts.length; i++) {
				const prefix = `/${parts.slice(0, i).join("/")}`;
				const found = this.lookup(prefix, true, "mkdir");
				if (!found.node) {
					this.#create(found.parent, found.name, DIR, mode);
					this.note(found.path);
				} else if (found.node.kind !== DIR) throw new FsError(i === parts.length ? "EEXIST" : "ENOTDIR", "mkdir", path);
			}
		});
	}

	readFile(path) {
		return this.tx(() => {
			const { node } = this.need(path, true, "open");
			if (node.kind === DIR) throw new FsError("EISDIR", "read", path);
			return this.#content(node.ino, node.size);
		});
	}

	/** Up to `length` bytes at `offset` of the file `ino`. */
	readAt(ino, offset, length) {
		return this.tx(() => {
			const node = this.q.node.get(ino);
			if (!node) throw new FsError("ENOENT", "read", `#${ino}`);
			const end = Math.min(node.size, offset + length);
			if (offset >= end) return new Uint8Array(0);
			const out = new Uint8Array(end - offset);
			const first = Math.floor(offset / CHUNK);
			for (const { idx, data } of this.q.chunkRange.all(ino, first, Math.floor((end - 1) / CHUNK))) {
				const base = idx * CHUNK;
				const from = Math.max(offset, base);
				const to = Math.min(end, base + data.length);
				out.set(data.subarray(from - base, to - base), from - offset);
			}
			return out;
		});
	}

	/**
	 * A file held by its node, so that a rename of the path does not change what is read.
	 * `stat()` is undefined once the file is gone.
	 */
	open(path) {
		const { ino, kind } = this.stat(path, true);
		if (kind === DIR) throw new FsError("EISDIR", "open", path);
		return {
			stat: () => this.tx(() => this.q.node.get(ino)),
			read: (offset, length) => this.readAt(ino, offset, length),
			close() {},
		};
	}

	writeFile(path, content, { mode = 0o644, exclusive = false } = {}) {
		const bytes = bytesOf(content);
		return this.tx(() => {
			const found = this.lookup(path, true, "open");
			if (found.name === "") throw new FsError("EISDIR", "open", path);
			let ino;
			if (found.node) {
				if (exclusive) throw new FsError("EEXIST", "open", path);
				if (found.node.kind === DIR) throw new FsError("EISDIR", "open", path);
				ino = found.node.ino;
				this.#release(ino);
			} else ino = this.#create(found.parent, found.name, FILE, mode);
			this.#store(ino, 0, bytes);
			this.q.setSize.run(bytes.length, Date.now(), ino);
			this.note(found.path);
			return ino;
		});
	}

	appendFile(path, content) {
		const bytes = bytesOf(content);
		return this.tx(() => {
			const found = this.lookup(path, true, "open");
			if (!found.node) return void this.writeFile(path, bytes);
			if (found.node.kind === DIR) throw new FsError("EISDIR", "open", path);
			const { ino, size } = found.node;
			const lastIdx = Math.floor(size / CHUNK);
			const tail = size % CHUNK ? this.readAt(ino, lastIdx * CHUNK, CHUNK) : new Uint8Array(0);
			const joined = new Uint8Array(tail.length + bytes.length);
			joined.set(tail);
			joined.set(bytes, tail.length);
			this.#release(ino, lastIdx);
			this.#store(ino, lastIdx, joined);
			this.q.setSize.run(size + bytes.length, Date.now(), ino);
			this.note(found.path);
		});
	}

	truncate(path, size) {
		return this.tx(() => {
			const { node, path: at } = this.need(path, true, "truncate");
			this.note(at);
			if (node.kind === DIR) throw new FsError("EISDIR", "truncate", path);
			const old = this.#content(node.ino, node.size);
			const next = new Uint8Array(size);
			next.set(old.subarray(0, Math.min(size, old.length)));
			this.#release(node.ino);
			this.#store(node.ino, 0, next);
			this.q.setSize.run(size, Date.now(), node.ino);
		});
	}

	unlink(path) {
		return this.tx(() => {
			const found = this.need(path, false, "unlink");
			if (found.node.kind === DIR) throw new FsError("EISDIR", "unlink", path);
			this.q.delDirent.run(found.parent, found.name);
			this.#drop(found.node.ino);
			this.note(found.path);
		});
	}

	rmdir(path) {
		return this.tx(() => {
			const found = this.need(path, false, "rmdir");
			if (found.node.kind !== DIR) throw new FsError("ENOTDIR", "rmdir", path);
			if (found.name === "") throw new FsError("EPERM", "rmdir", path);
			if (this.q.anyChild.get(found.node.ino)) throw new FsError("ENOTEMPTY", "rmdir", path);
			this.q.delDirent.run(found.parent, found.name);
			this.q.delNode.run(found.node.ino);
			this.note(found.path);
		});
	}

	#removeTree(parent, name, node) {
		if (node.kind === DIR) for (const child of this.q.list.all(node.ino)) this.#removeTree(node.ino, child.name, this.q.node.get(child.ino));
		this.q.delDirent.run(parent, name);
		if (node.kind === DIR) this.q.delNode.run(node.ino);
		else this.#drop(node.ino);
	}

	rm(path, { recursive = false, force = false } = {}) {
		return this.tx(() => {
			let found;
			try {
				found = this.lookup(path, false, "rm");
			} catch (error) {
				if (force && error instanceof FsError && error.code === "ENOENT") return;
				throw error;
			}
			if (!found.node) {
				if (force) return;
				throw new FsError("ENOENT", "rm", path);
			}
			if (found.name === "") throw new FsError("EPERM", "rm", path);
			if (found.node.kind === DIR && !recursive) {
				if (this.q.anyChild.get(found.node.ino)) throw new FsError("ENOTEMPTY", "rm", path);
			}
			this.#removeTree(found.parent, found.name, found.node);
			this.note(found.path);
		});
	}

	rename(source, destination) {
		return this.tx(() => {
			const from = this.need(source, false, "rename");
			if (from.name === "") throw new FsError("EPERM", "rename", source);
			const to = this.lookup(destination, false, "rename");
			if (to.name === "") throw new FsError("EEXIST", "rename", destination);
			if (from.node.kind === DIR && `${to.path}/`.startsWith(`${from.path}/`) && to.path !== from.path)
				throw new FsError("EINVAL", "rename", destination);
			if (to.node) {
				if (to.node.ino === from.node.ino) return;
				if (to.node.kind === DIR) {
					if (from.node.kind !== DIR) throw new FsError("EISDIR", "rename", destination);
					if (this.q.anyChild.get(to.node.ino)) throw new FsError("ENOTEMPTY", "rename", destination);
					this.q.delDirent.run(to.parent, to.name);
					this.q.delNode.run(to.node.ino);
				} else {
					if (from.node.kind === DIR) throw new FsError("ENOTDIR", "rename", destination);
					this.q.delDirent.run(to.parent, to.name);
					this.#drop(to.node.ino);
				}
			}
			this.q.delDirent.run(from.parent, from.name);
			this.q.addDirent.run(to.parent, to.name, from.node.ino);
			this.note(from.path, to.path);
		});
	}

	symlink(target, path) {
		return this.tx(() => {
			const found = this.lookup(path, false, "symlink");
			if (found.node || found.name === "") throw new FsError("EEXIST", "symlink", path);
			this.#create(found.parent, found.name, LINK, 0o777, target);
			this.note(found.path);
		});
	}

	readlink(path) {
		return this.tx(() => {
			const { node } = this.need(path, false, "readlink");
			if (node.kind !== LINK) throw new FsError("EINVAL", "readlink", path);
			return node.target;
		});
	}

	link(existing, path) {
		return this.tx(() => {
			const { node } = this.need(existing, true, "link");
			if (node.kind === DIR) throw new FsError("EPERM", "link", existing);
			const found = this.lookup(path, false, "link");
			if (found.node || found.name === "") throw new FsError("EEXIST", "link", path);
			this.q.addDirent.run(found.parent, found.name, node.ino);
			this.q.addLink.run(1, node.ino);
			this.note(found.path);
		});
	}

	chmod(path, mode) {
		return this.tx(() => {
			const found = this.need(path, true, "chmod");
			this.q.setMode.run(mode, found.node.ino);
			this.note(found.path);
		});
	}

	utimes(path, mtimeMs) {
		return this.tx(() => {
			const found = this.need(path, true, "utimes");
			this.q.setMtime.run(Math.floor(mtimeMs), found.node.ino);
			this.note(found.path);
		});
	}

	/** Every path, for callers that index the whole disk. */
	allPaths() {
		return this.tx(() => {
			const children = new Map();
			for (const row of this.q.allDirents.all()) {
				if (!children.has(row.parent)) children.set(row.parent, []);
				children.get(row.parent).push(row);
			}
			const out = ["/"];
			const walk = (ino, prefix) => {
				for (const row of children.get(ino) ?? []) {
					out.push(`${prefix}/${row.name}`);
					walk(row.ino, `${prefix}/${row.name}`);
				}
			};
			walk(ROOT, "");
			return out;
		});
	}

	/** Whether what is stored agrees with itself. An empty list means it does. */
	check() {
		const one = (sql) => this.db.prepare(sql).get();
		const problems = [];
		const integrity = one("PRAGMA integrity_check").integrity_check;
		if (integrity !== "ok") problems.push(`sqlite: ${integrity}`);
		const count = (label, sql) => {
			const n = one(`SELECT count(*) AS n FROM (${sql})`).n;
			if (n) problems.push(`${label}: ${n}`);
		};
		count("names that point to no node", "SELECT 1 FROM dirent d LEFT JOIN node n ON n.ino = d.ino WHERE n.ino IS NULL");
		count("names in something that is not a directory", `SELECT 1 FROM dirent d JOIN node n ON n.ino = d.parent WHERE n.kind != ${DIR}`);
		count("nodes with no name", `SELECT 1 FROM node n LEFT JOIN dirent d ON d.ino = n.ino WHERE d.ino IS NULL AND n.ino != ${ROOT}`);
		count("chunks with no content", "SELECT 1 FROM chunk c LEFT JOIN blob b ON b.hash = c.hash WHERE b.hash IS NULL");
		count("content nothing uses", "SELECT 1 FROM blob b LEFT JOIN chunk c ON c.hash = b.hash WHERE c.hash IS NULL");
		count("content with a wrong use count", "SELECT 1 FROM blob b WHERE b.refs != (SELECT count(*) FROM chunk c WHERE c.hash = b.hash)");
		count("files whose size disagrees with their chunks", `SELECT 1 FROM node n WHERE n.kind = ${FILE} AND n.size != (SELECT coalesce(sum(length(b.data)), 0) FROM chunk c JOIN blob b ON b.hash = c.hash WHERE c.ino = n.ino)`);
		count("link counts that disagree with names", `SELECT 1 FROM node n WHERE n.kind != ${DIR} AND n.nlink != (SELECT count(*) FROM dirent d WHERE d.ino = n.ino)`);
		return problems;
	}

	usage() {
		const one = (sql) => this.db.prepare(sql).get();
		return {
			files: one(`SELECT count(*) AS n FROM node WHERE kind = ${FILE}`).n,
			dirs: one(`SELECT count(*) AS n FROM node WHERE kind = ${DIR}`).n,
			logicalBytes: one(`SELECT coalesce(sum(size), 0) AS n FROM node WHERE kind = ${FILE}`).n,
			storedBytes: one("SELECT coalesce(sum(length(data)), 0) AS n FROM blob").n,
			chunks: one("SELECT count(*) AS n FROM chunk").n,
			blobs: one("SELECT count(*) AS n FROM blob").n,
		};
	}
}
