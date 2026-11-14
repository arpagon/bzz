// Granted folders: real folders of the machine under /mnt/<name>. They are
// read-only unless the owner allowed writes, the shell, git and Pi's tools
// all see the same thing, and no path leads outside the granted folder.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { SqliteFs } from "../src/disk.mjs";
import { BzzVfsEnv, createShell } from "../src/env.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { tempDir } from "./support/client.mjs";

const text = (bytes) => Buffer.from(bytes).toString();

/** A machine with a secret beside two granted folders: `site` read-only and `drop` writable. */
function machine(t) {
	const root = tempDir(t, "mounts");
	mkdirSync(join(root, "site", "docs"), { recursive: true });
	mkdirSync(join(root, "drop"));
	mkdirSync(join(root, "private"));
	writeFileSync(join(root, "site", "index.html"), "<h1>hi</h1>\n");
	writeFileSync(join(root, "site", "docs", "guide.md"), "# Guide\n");
	writeFileSync(join(root, "private", "secret.txt"), "SECRET\n");
	const disk = new SqliteFs(join(root, "files.sqlite"));
	t.after(() => disk.close());
	const vfs = new Vfs(disk, [{ name: "site", path: join(root, "site"), write: false }, { name: "drop", path: join(root, "drop"), write: true }]);
	const bash = createShell(vfs, { net: network([]), author: { name: "agent", email: "agent@bzz.invalid" } });
	const env = new BzzVfsEnv(vfs, bash, { agent: "mounts" });
	/** A command run the way Pi's `bash` tool runs it. */
	const sh = async (command) => {
		const seen = { stdout: "", stderr: "" };
		const result = await env.exec(command, { onOutput: (chunk, _context, { stream }) => void (seen[stream] += chunk) }, ctx);
		assert.equal(result.ok, true, `${command}: ${result.error?.message}`);
		return { out: seen.stdout, err: seen.stderr, code: result.value.exitCode };
	};
	return { root, disk, vfs, sh, env };
}

/** Links to make on the real machine. Windows lets only some accounts make them: without that the cases are skipped. */
function link(t, target, path) {
	try {
		symlinkSync(target, path);
		return true;
	} catch (error) {
		if (error.code !== "EPERM") throw error;
		t.skip("this account cannot create symbolic links (Windows without developer mode)");
		return false;
	}
}

test("a granted folder is visible to the shell, to git and to Pi's tools alike", async (t) => {
	const { vfs, sh, env } = machine(t);
	assert.equal((await sh("ls -1 /mnt && cat /mnt/site/index.html && find /mnt/site -type f | sort")).out, "drop\nsite\n<h1>hi</h1>\n/mnt/site/docs/guide.md\n/mnt/site/index.html\n");
	assert.equal(text(vfs.readFile("/mnt/site/docs/guide.md")), "# Guide\n");
	const read = await env.readTextFile("/mnt/site/index.html", ctx);
	assert.deepEqual(read, { ok: true, value: "<h1>hi</h1>\n" });
	const listed = await env.listDir("/mnt", ctx);
	assert.deepEqual(listed.value.map((entry) => [entry.path, entry.kind]), [["/mnt/drop", "directory"], ["/mnt/site", "directory"]]);
	// git, with no git program, on a real folder the owner granted for writing.
	const made = await sh("cd /mnt/drop && git init && echo one > a.txt && git add -A && git commit -m first && git log --oneline");
	assert.equal(made.code, 0, made.err);
	assert.match(made.out, /first\n$/);
	// A copy from a granted folder lands on the agent's own disk; a move into one leaves the disk.
	assert.equal((await sh("cp -r /mnt/site ~/copy && cat ~/copy/docs/guide.md && echo note > ~/n.txt && mv ~/n.txt /mnt/drop/n.txt && ls ~ && cat /mnt/drop/n.txt")).out, "# Guide\ncopy\nnote\n");
});

test("a folder granted read-only cannot be changed, by any of the three", async (t) => {
	const { root, vfs, sh, env } = machine(t);
	for (const command of ["echo x > /mnt/site/new.txt", "echo x >> /mnt/site/index.html", "rm /mnt/site/index.html", "mkdir /mnt/site/dir", "mv /mnt/site/index.html /mnt/site/moved.html",
		"sed -i s/hi/HO/ /mnt/site/index.html", "touch /mnt/site/index.html", "chmod 777 /mnt/site/index.html", "ln -s index.html /mnt/site/l", "cp ~/.. /mnt/site/x -r", "cd /mnt/site && git init"]) {
		assert.notEqual((await sh(command)).code, 0, command);
	}
	assert.match((await sh("echo x > /mnt/site/new.txt")).err, /EROFS: this folder was granted read-only/);
	assert.throws(() => vfs.writeFile("/mnt/site/index.html", "changed"), { code: "EROFS", message: /granted read-only/ });
	const written = await env.writeFile("/mnt/site/new.txt", "x", ctx);
	assert.equal(written.ok, false);
	assert.equal(written.error.code, "permission_denied");
	// On the machine, nothing moved.
	assert.deepEqual(readdirSync(join(root, "site")).sort(), ["docs", "index.html"]);
	assert.equal(readFileSync(join(root, "site", "index.html"), "utf8"), "<h1>hi</h1>\n");
});

test("a folder granted for writing is changed on the machine", async (t) => {
	const { root, sh, env } = machine(t);
	assert.equal((await sh("mkdir -p /mnt/drop/a/b && echo made > /mnt/drop/a/b/f.txt && ln -s a/b/f.txt /mnt/drop/short && cat /mnt/drop/short")).out, "made\n");
	assert.equal(readFileSync(join(root, "drop", "a", "b", "f.txt"), "utf8"), "made\n");
	assert.equal((await env.writeFile("/mnt/drop/deep/er/t.txt", "tool\n", ctx)).ok, true);
	assert.equal(readFileSync(join(root, "drop", "deep", "er", "t.txt"), "utf8"), "tool\n");
	assert.equal((await sh("rm -r /mnt/drop/a && ls -1 /mnt/drop")).out, "deep\nshort\n");
	// The folder itself is the owner's: it can be emptied, not removed or renamed.
	assert.notEqual((await sh("rmdir /mnt/drop")).code, 0);
	assert.notEqual((await sh("mv /mnt/drop /mnt/other")).code, 0);
	assert.equal(existsSync(join(root, "drop")), true);
});

test("/mnt holds granted folders and nothing else", async (t) => {
	const { disk, vfs, sh } = machine(t);
	for (const command of ["mkdir /mnt/mine", "echo x > /mnt/file", "rm -r /mnt", "mv /mnt /mnt2", "ln -s /tmp /mnt/t", "cat /mnt/ghost/file"]) {
		assert.notEqual((await sh(command)).code, 0, command);
	}
	assert.notEqual((await sh("mv /mnt/site ~/taken")).code, 0);
	assert.equal(vfs.exists("/home/agent/taken"), false);
	assert.deepEqual(vfs.readdir("/mnt").map((entry) => entry.name), ["drop", "site"]);
	// Nothing about the grants is written to the disk: it lists no /mnt at all.
	assert.deepEqual(disk.readdir("/").map((entry) => entry.name), ["home", "tmp"]);
	assert.throws(() => new Vfs(disk, [{ name: "../up", path: "/tmp" }]), /simple name/);
	assert.throws(() => new Vfs(disk, [{ name: "a", path: "/tmp" }, { name: "a", path: "/var" }]), /two granted folders/);
});

test("no path leads out of a granted folder", async (t) => {
	const { root, vfs, sh, env } = machine(t);
	// `..` is resolved on the virtual path: above /mnt/site is /mnt, then the agent's own root, never the machine's.
	assert.throws(() => vfs.readFile("/mnt/site/../../private/secret.txt"), { code: "ENOENT" });
	assert.throws(() => vfs.readFile("/mnt/site/../private/secret.txt"), { code: "ENOENT" });
	assert.notEqual((await sh("cat /mnt/site/../../../../../../etc/passwd /mnt/site/../../private/secret.txt")).out.includes("SECRET"), true);
	// A name that is path syntax on Windows stays a name, or is refused there.
	assert.throws(() => vfs.readFile("/mnt/site/..\\private\\secret.txt"));
	assert.throws(() => vfs.readFile("/mnt/site/docs\0/../../private/secret.txt"), { code: "EINVAL" });
	// A link the agent tries to plant in a folder it may write.
	for (const command of [`ln -s ${JSON.stringify(join(root, "private", "secret.txt"))} /mnt/drop/a`, "ln -s ../private/secret.txt /mnt/drop/b", "ln -s /etc/passwd /mnt/drop/c", "ln -s ../../../../../../etc /mnt/drop/d"])
		assert.notEqual((await sh(command)).code, 0, command);
	assert.deepEqual(readdirSync(join(root, "drop")), []);
	assert.throws(() => vfs.symlink("../private", "/mnt/drop/e"), { code: "EACCES", message: /must be relative and stay inside/ });
	// A hard link or a move between the disk and a granted folder is not a way around either.
	vfs.writeFile("/home/agent/mine.txt", "mine");
	assert.throws(() => vfs.link("/mnt/site/index.html", "/home/agent/hard"), { code: "EXDEV" });
	assert.throws(() => vfs.rename("/mnt/site/index.html", "/home/agent/stolen"), { code: "EXDEV" });
	assert.equal((await env.readTextFile("/mnt/site/../../private/secret.txt", ctx)).ok, false);
});

test("links already inside a granted folder are followed only while they stay inside", async (t) => {
	const { root, vfs, sh, env } = machine(t);
	if (!link(t, "index.html", join(root, "site", "inside"))) return;
	link(t, join(root, "private", "secret.txt"), join(root, "site", "leak-absolute"));
	link(t, join("..", "private", "secret.txt"), join(root, "site", "leak-relative"));
	link(t, join("..", "private"), join(root, "site", "leak-dir"));
	link(t, "nowhere", join(root, "drop", "dangling"));
	link(t, join("..", "private"), join(root, "drop", "out"));

	assert.equal(text(vfs.readFile("/mnt/site/inside")), "<h1>hi</h1>\n");
	assert.equal(vfs.realpath("/mnt/site/inside"), "/mnt/site/index.html");
	for (const path of ["/mnt/site/leak-absolute", "/mnt/site/leak-relative", "/mnt/site/leak-dir/secret.txt", "/mnt/drop/out/secret.txt"]) {
		assert.throws(() => vfs.readFile(path), { code: "EACCES", message: /leads outside the granted folder/ }, path);
		assert.throws(() => vfs.stat(path), { code: "EACCES" }, path);
		assert.throws(() => vfs.open(path), { code: "EACCES" }, path);
		const read = await env.readBinaryFile(path, ctx);
		assert.equal(read.ok, false, path);
		const shown = await sh(`cat ${path}; head -c 3 ${path}; cp ${path} ~/got; grep -r SECRET /mnt`);
		assert.equal((shown.out + shown.err).includes("SECRET"), false, path);
	}
	assert.deepEqual(vfs.readdir("/mnt/site").map((entry) => entry.name), ["docs", "index.html", "inside", "leak-absolute", "leak-dir", "leak-relative"]);
	assert.throws(() => vfs.readdir("/mnt/site/leak-dir"), { code: "EACCES" });
	// Writing through a link must not create or change anything outside, in a folder granted for writing.
	assert.throws(() => vfs.writeFile("/mnt/drop/out/planted.txt", "x"), { code: "EACCES" });
	assert.throws(() => vfs.writeFile("/mnt/drop/dangling", "x"), { code: "ENOENT" });
	assert.throws(() => vfs.mkdir("/mnt/drop/out/dir/deeper", { recursive: true }), { code: "EACCES" });
	assert.throws(() => vfs.rename("/mnt/drop/dangling", "/mnt/drop/out/moved"), { code: "EACCES" });
	assert.deepEqual(readdirSync(join(root, "private")), ["secret.txt"]);
	assert.equal(existsSync(join(root, "drop", "nowhere")), false);
	// Removing a link removes the link, not what it points to.
	vfs.rm("/mnt/drop/out", { recursive: true });
	assert.equal(readFileSync(join(root, "private", "secret.txt"), "utf8"), "SECRET\n");
	// A link on the agent's own disk may point into a granted folder: the same rules apply at the end of it.
	vfs.symlink("/mnt/site", "/home/agent/site");
	assert.equal((await sh("cat ~/site/index.html")).out, "<h1>hi</h1>\n");
	assert.throws(() => vfs.readFile("/home/agent/site/leak-relative"), { code: "EACCES" });
	assert.throws(() => vfs.writeFile("/home/agent/site/new.txt", "x"), { code: "EROFS" });
});

test("an error names the virtual path, never the real one", async (t) => {
	const { root, vfs, sh, env } = machine(t);
	const seen = [];
	for (const attempt of [() => vfs.readFile("/mnt/site/absent"), () => vfs.writeFile("/mnt/site/x", "x"), () => vfs.readdir("/mnt/site/index.html"), () => vfs.rmdir("/mnt/drop/absent"), () => vfs.readFile("/mnt/site/docs")]) {
		try {
			attempt();
			assert.fail("should have thrown");
		} catch (error) {
			seen.push(error.message);
		}
	}
	const shell = await sh("cat /mnt/site/absent; ls /mnt/site/none; echo x > /mnt/drop/a/b/c 2>&1; stat /mnt/site/zz");
	seen.push(shell.out, shell.err, JSON.stringify(await env.readTextFile("/mnt/site/absent", ctx)));
	for (const message of seen) assert.equal(message.includes(root), false, message);
	assert.match(seen[0], /ENOENT.*'\/mnt\/site\/absent'/);
});

test("a granted folder that is not there is empty of everything, and a pipe is not read", { skip: process.platform === "win32" && "mkfifo is not on Windows" }, async (t) => {
	const { root, disk, sh } = machine(t);
	const vfs = new Vfs(disk, [{ name: "gone", path: join(root, "no-such-folder"), write: true }, { name: "file", path: join(root, "site", "index.html"), write: true }]);
	assert.deepEqual(vfs.readdir("/mnt").map((entry) => entry.name), ["file", "gone"]);
	assert.throws(() => vfs.readdir("/mnt/gone"), { code: "ENOENT", message: /the granted folder is not there/ });
	assert.throws(() => vfs.writeFile("/mnt/gone/x", "x"), { code: "ENOENT" });
	assert.throws(() => vfs.readdir("/mnt/file"), { code: "ENOENT" });
	// Opening a pipe nobody writes to would block the whole host.
	execFileSync("mkfifo", [join(root, "drop", "pipe")]);
	const started = Date.now();
	assert.notEqual((await sh("cat /mnt/drop/pipe")).code, 0);
	assert.ok(Date.now() - started < 2_000);
});
