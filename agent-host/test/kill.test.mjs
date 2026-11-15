// A process killed at any moment leaves a disk that agrees with itself. The
// writer is killed without warning thirty times on the same file; after each
// kill the disk is opened and checked.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import test from "node:test";
import { fileURLToPath } from "node:url";
import git from "isomorphic-git";
import { nodeFs } from "../src/adapters.mjs";
import { DIR, SqliteFs } from "../src/disk.mjs";
import { Vfs } from "../src/vfs.mjs";
import { sleep, tempDir } from "./support/client.mjs";

const WRITER = fileURLToPath(new URL("./support/kill-writer.mjs", import.meta.url));
const KILLS = Number(process.env.BZZ_KILL_TRIALS ?? 30);
const hasGit = (() => {
	try {
		execFileSync("git", ["--version"], { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
})();

test(`${KILLS} kills in the middle of writes leave the disk consistent`, { timeout: 180_000 }, async (t) => {
	const work = tempDir(t, "kill");
	const file = join(work, "files.sqlite");
	const dir = "/home/agent/w";
	let commits = 0;
	for (let trial = 1; trial <= KILLS; trial++) {
		const child = spawn(process.execPath, [WRITER, file, String(trial * 7919)], { stdio: ["ignore", "pipe", "inherit"] });
		// The clock starts when the writer writes, so that every kill lands in the middle of work.
		await new Promise((resolve) => createInterface({ input: child.stdout }).once("line", resolve));
		await sleep(40 + Math.random() * 400);
		child.kill("SIGKILL");
		await new Promise((resolve) => child.once("exit", resolve));
		const disk = new SqliteFs(file);
		try {
			assert.deepEqual(disk.check(), [], `the disk after kill ${trial}`);
			// git's own files were cut at the same moments: the repository must still read.
			const fs = nodeFs(new Vfs(disk));
			await git.statusMatrix({ fs, dir });
			commits = (await git.log({ fs, dir }).catch(() => [])).length;
		} finally {
			disk.close();
		}
	}
	t.diagnostic(`${KILLS} kills, ${commits} commits on the disk`);

	// What real git thinks of the repository after all those kills.
	if (!hasGit) return t.diagnostic("git is not installed: the repository was not checked with real `git fsck`");
	const disk = new SqliteFs(file);
	const out = join(work, "exported");
	const exportTree = (from, to) => {
		mkdirSync(to, { recursive: true });
		for (const entry of disk.readdir(from)) {
			if (entry.kind === DIR) exportTree(`${from}/${entry.name}`, join(to, entry.name));
			else {
				writeFileSync(join(to, entry.name), disk.readFile(`${from}/${entry.name}`));
				chmodSync(join(to, entry.name), disk.stat(`${from}/${entry.name}`).mode);
			}
		}
	};
	exportTree(dir, out);
	disk.close();
	const real = (...args) => execFileSync("git", args, { cwd: out, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
	assert.equal(real("fsck", "--full", "--no-dangling"), "");
	if (commits > 0) assert.equal(real("rev-list", "--count", "HEAD"), String(commits));
});
