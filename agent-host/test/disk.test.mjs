// The agent's disk (files.sqlite): it behaves like a filesystem, keeps equal
// content once, survives a reopen, and can tell when it disagrees with itself.
import assert from "node:assert/strict";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { DIR, LINK, SqliteFs } from "../src/disk.mjs";
import { createShell } from "../src/env.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { tempDir } from "./support/client.mjs";

const text = (bytes) => Buffer.from(bytes).toString();
const open = (t) => {
	const file = join(tempDir(t, "disk"), "files.sqlite");
	const disk = new SqliteFs(file);
	t.after(() => disk.close());
	return { file, disk };
};

test("files, directories, links and renames", (t) => {
	const { disk } = open(t);
	disk.mkdir("/home/agent", { recursive: true });
	disk.writeFile("/home/agent/a.txt", "hello\n");
	disk.appendFile("/home/agent/a.txt", "world\n");
	assert.equal(text(disk.readFile("/home/agent/a.txt")), "hello\nworld\n");
	disk.symlink("a.txt", "/home/agent/link");
	assert.equal(text(disk.readFile("/home/agent/link")), "hello\nworld\n");
	assert.equal(disk.stat("/home/agent/link", false).kind, LINK);
	assert.equal(disk.realpath("/home/agent/link"), "/home/agent/a.txt");
	disk.link("/home/agent/a.txt", "/home/agent/hard");
	assert.equal(disk.stat("/home/agent/hard").nlink, 2);
	disk.rename("/home/agent/a.txt", "/home/agent/b.txt");
	assert.throws(() => disk.readFile("/home/agent/link"), { code: "ENOENT" });
	assert.equal(text(disk.readFile("/home/agent/hard")), "hello\nworld\n");
	assert.throws(() => disk.rmdir("/home/agent"), { code: "ENOTEMPTY" });
	assert.throws(() => disk.writeFile("/home/agent", "x"), { code: "EISDIR" });
	assert.throws(() => disk.readFile("/home/agent/b.txt/x"), { code: "ENOTDIR" });
	assert.throws(() => disk.mkdir("/home/agent"), { code: "EEXIST" });
	assert.throws(() => disk.stat("relative"), { code: "EINVAL" });
	assert.equal(disk.stat("/home/agent/../agent/./b.txt").size, 12);
	assert.deepEqual(disk.readdir("/home/agent").map((entry) => entry.name), ["b.txt", "hard", "link"]);
	disk.rm("/home", { recursive: true });
	assert.deepEqual(disk.readdir("/"), []);
	assert.deepEqual(disk.check(), []);
	assert.deepEqual(disk.usage(), { files: 0, dirs: 1, logicalBytes: 0, storedBytes: 0, chunks: 0, blobs: 0 });
});

test("a link that loops is refused, not followed for ever", (t) => {
	const { disk } = open(t);
	disk.symlink("/b", "/a");
	disk.symlink("/a", "/b");
	assert.throws(() => disk.readFile("/a"), { code: "ELOOP" });
});

test("content is stored in chunks named by their hash: equal content once", (t) => {
	const { file, disk } = open(t);
	const big = new Uint8Array(700_000).map((_, i) => (i * 31) % 251);
	disk.writeFile("/big.bin", big);
	assert.deepEqual(disk.readFile("/big.bin"), big);
	const handle = disk.open("/big.bin");
	assert.deepEqual(handle.read(262_000, 1_000), big.subarray(262_000, 263_000));
	assert.equal(disk.usage().chunks, 3);
	disk.writeFile("/copy.bin", big);
	assert.equal(disk.usage().chunks, 6);
	assert.equal(disk.usage().blobs, 3);
	assert.equal(disk.usage().storedBytes, big.length);
	// The name of a chunk is the SHA-256 of its bytes.
	const db = new DatabaseSync(file, { readOnly: true });
	const wrong = db.prepare("SELECT count(*) AS n FROM blob WHERE hash != ?").get(Buffer.alloc(32)).n;
	const named = db.prepare("SELECT hash, data FROM blob").all().every(({ hash, data }) => Buffer.from(hash).equals(createHash("sha256").update(data).digest()));
	db.close();
	assert.equal(wrong, 3);
	assert.equal(named, true);
	disk.appendFile("/big.bin", new Uint8Array(10).fill(7));
	assert.equal(disk.stat("/big.bin").size, 700_010);
	assert.deepEqual(disk.readFile("/copy.bin"), big);
	disk.truncate("/big.bin", 10);
	assert.deepEqual(disk.readFile("/big.bin"), big.subarray(0, 10));
	disk.rm("/copy.bin");
	assert.equal(disk.usage().blobs, 1);
	assert.deepEqual(disk.check(), []);
});

test("a reader holds its file through a rename and sees it gone", (t) => {
	const { disk } = open(t);
	disk.writeFile("/a", "one");
	const handle = disk.open("/a");
	disk.rename("/a", "/b");
	assert.equal(text(handle.read(0, 10)), "one");
	disk.rm("/b");
	assert.equal(handle.stat(), undefined);
});

test("what a new process sees, and no write-ahead file after a checkpoint", (t) => {
	const { file, disk } = open(t);
	disk.mkdir("/home/agent", { recursive: true });
	disk.writeFile("/home/agent/kept.txt", "kept\n");
	assert.equal(existsSync(`${file}-wal`), true);
	disk.checkpoint();
	assert.deepEqual(readdirSync(join(file, "..")), ["files.sqlite"]);
	// The disk opens itself again at the next call.
	assert.equal(text(disk.readFile("/home/agent/kept.txt")), "kept\n");
	disk.close();
	const again = new SqliteFs(file);
	assert.equal(text(again.readFile("/home/agent/kept.txt")), "kept\n");
	assert.deepEqual(again.check(), []);
	again.close();
	assert.deepEqual(readdirSync(join(file, "..")), ["files.sqlite"]);
});

test("the self-check names each kind of damage", (t) => {
	const { file, disk } = open(t);
	disk.mkdir("/d");
	disk.writeFile("/d/f", "content");
	disk.writeFile("/g", "other");
	assert.deepEqual(disk.check(), []);
	disk.close();
	const damage = (sql) => {
		const db = new DatabaseSync(file);
		db.exec(sql);
		db.close();
		const reopened = new SqliteFs(file);
		const problems = reopened.check();
		reopened.close();
		return problems.join("; ");
	};
	assert.match(damage("UPDATE blob SET refs = refs + 1"), /content with a wrong use count: 2/);
	assert.match(damage("UPDATE blob SET refs = refs - 1; UPDATE node SET size = size + 1 WHERE kind = 1"), /files whose size disagrees with their chunks: 2/);
	assert.match(damage("UPDATE node SET size = size - 1 WHERE kind = 1; DELETE FROM dirent WHERE name = 'g'"), /nodes with no name: 1/);
	assert.match(damage("DELETE FROM blob"), /chunks with no content: 2/);
});

test("the shell works on the disk", async (t) => {
	const { disk } = open(t);
	const vfs = new Vfs(disk);
	const bash = createShell(vfs, { net: network([]), author: { name: "agent", email: "agent@bzz.invalid" } });
	const run = async (command) => {
		const result = await bash.exec(command);
		return { out: result.stdout, err: result.stderr, code: result.exitCode };
	};
	assert.deepEqual(await run("pwd; echo $HOME"), { out: "/home/agent\n/home/agent\n", err: "", code: 0 });
	// Output sent to /dev/null goes nowhere, and nothing named /dev appears on the disk.
	assert.deepEqual(await run("echo gone > /dev/null; cat missing 2>/dev/null; echo more >> /dev/null; cat /dev/null; echo kept"), { out: "kept\n", err: "", code: 0 });
	assert.deepEqual(await run("printf 'hello\\nworld\\n' > b.txt && cat b.txt | tr a-z A-Z"), { out: "HELLO\nWORLD\n", err: "", code: 0 });
	assert.equal((await run("mkdir -p src/lib && printf 'one\\ntwo\\nthree\\n' > src/lib/n.txt && wc -l < src/lib/n.txt")).out.trim(), "3");
	assert.equal((await run("sed -i 's/two/2/' src/lib/n.txt && grep -rn 2 src")).out, "src/lib/n.txt:2:2\n");
	assert.equal((await run("cp -r src dst && mv dst/lib/n.txt dst/m.txt && find dst -type f")).out, "dst/m.txt\n");
	assert.equal((await run("ln -s b.txt link && cat link | head -1 && ls -1")).out, "hello\nb.txt\ndst\nlink\nsrc\n");
	assert.equal((await run("cat missing.txt")).code, 1);
	assert.equal((await run("echo '{\"a\":[1,2,3]}' | jq '.a | length'")).out, "3\n");
	// The root holds what the agent made, the home the host made, and /mnt for granted folders: nothing of the machine.
	assert.equal((await run("ls -1 /")).out, "home\nmnt\ntmp\n");
	// No way to run other code: the interpreters just-bash can offer are left off.
	for (const program of ["python3", "python", "js-exec", "node", "sh -c 'exec /bin/ls'"]) assert.notEqual((await run(program)).code, 0, program);
	assert.equal(vfs.stat("/home/agent/dst", true).kind, DIR);
	assert.deepEqual(disk.check(), []);
});
