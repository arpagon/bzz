// The agent types git in its shell and no git program runs. Everything it
// does is judged by the real git: the remote is `git http-backend`, and what
// the agent wrote is read with `git fsck`, `git show` and `git log`.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { DIR, LINK, SqliteFs } from "../src/disk.mjs";
import { createShell } from "../src/env.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { tempDir } from "./support/client.mjs";
import { git, gitMissing, startGitServer } from "./support/git-server.mjs";

const skip = gitMissing() && `skipped: ${gitMissing()}. These tests compare the sandbox's git with the real one.`;
const author = { name: "Scout", email: "agent@bzz.invalid" };

async function sandbox(t, hosts) {
	const work = tempDir(t, "git");
	const disk = new SqliteFs(join(work, "files.sqlite"));
	t.after(() => disk.close());
	const vfs = new Vfs(disk);
	const bash = createShell(vfs, { net: network(hosts), author });
	const sh = async (command) => {
		const result = await bash.exec(command);
		return { out: result.stdout, err: result.stderr, code: result.exitCode };
	};
	const ok = async (command) => {
		const result = await sh(command);
		assert.equal(result.code, 0, `$ ${command}\n${result.out}${result.err}`);
		return result.out;
	};
	/** Copies a directory of the disk to a real one, so that real git can judge it. */
	const exportTree = (from, to) => {
		mkdirSync(to, { recursive: true });
		for (const entry of disk.readdir(from)) {
			const source = `${from}/${entry.name}`;
			if (entry.kind === DIR) exportTree(source, join(to, entry.name));
			else if (entry.kind === LINK) symlinkSync(disk.readlink(source), join(to, entry.name));
			else {
				writeFileSync(join(to, entry.name), disk.readFile(source));
				chmodSync(join(to, entry.name), disk.stat(source).mode);
			}
		}
	};
	return { work, disk, vfs, sh, ok, exportTree };
}

test("clone, edit, diff, commit, push and pull against real git", { skip }, async (t) => {
	const remote = await startGitServer(t, join(tempDir(t, "remote"), "srv"));
	const { work, disk, sh, ok, exportTree } = await sandbox(t, [remote.host]);

	assert.match(await ok(`git clone ${remote.origin}`), /Cloned into '\/home\/agent\/project'/);
	assert.match(await ok("cd project && git log --oneline && cat notes.txt"), /add notes\nalpha\nbeta\ngamma\n$/);
	assert.equal(await ok("cd project && sed -i 's/beta/BETA/' notes.txt && echo delta >> notes.txt && echo 'new file' > todo.md && git status --short"), " M notes.txt\n?? todo.md\n");
	assert.match(await ok("cd project && git diff"), /-beta\n\+BETA/);
	assert.match(await ok("cd project && git add -A && git commit -m 'agent: edit notes, add todo' && git status"), /nothing to commit, working tree clean/);
	await ok("cd project && git push origin main");

	// Real git reads the pushed commit in the remote, written by the agent under its own name.
	assert.equal(git(remote.bare, "show", "main:notes.txt"), "alpha\nBETA\ngamma\ndelta");
	assert.equal(git(remote.bare, "log", "-1", "--format=%an <%ae> %s"), "Scout <agent@bzz.invalid> agent: edit notes, add todo");
	assert.equal(git(remote.bare, "fsck", "--full", "--no-dangling"), "");

	// Someone else pushes; the agent pulls.
	remote.pullSeed();
	remote.ownerPush("owner.txt", "from the owner\n", "owner: add a file");
	assert.match(await ok("cd project && git pull origin main && cat owner.txt && git log --oneline -3"), /from the owner\n.*owner: add a file/s);

	// A branch of its own, merged back without conflict.
	await ok("cd project && git checkout -b agent/work && echo more >> todo.md && git commit -am 'agent: more' && git checkout main && git merge agent/work && git branch");
	assert.equal(await ok("cd project && cat todo.md"), "new file\nmore\n");

	// The repository the agent holds is one real git accepts whole.
	const out = join(work, "exported");
	exportTree("/home/agent/project", out);
	assert.equal(git(out, "fsck", "--full", "--no-dangling"), "");
	assert.equal(git(out, "status", "--porcelain"), "");
	assert.equal(git(out, "rev-list", "--count", "HEAD"), "4");
	assert.deepEqual(disk.check(), []);
});

test("what the sandbox's git lacks, it names", { skip }, async (t) => {
	const remote = await startGitServer(t, join(tempDir(t, "remote"), "srv"));
	const { sh, ok } = await sandbox(t, [remote.host]);
	await ok(`git clone ${remote.origin} && cd project && git config user.name Ada && git config user.name`);
	const rebase = await sh("cd project && git rebase main");
	assert.equal(rebase.code, 129);
	assert.match(rebase.err, /'rebase' is not supported in this sandbox\. Merge instead.*Supported: init, clone, status/);
	assert.match((await sh("cd project && git stash")).err, /'stash' is not supported/);
	const ssh = await sh("git clone git@github.com:arpagon/bzz.git");
	assert.notEqual(ssh.code, 0);
	assert.match(ssh.err, /SSH remotes are not supported/);
	assert.match((await sh("git clone ssh://git@github.com/arpagon/bzz.git")).err, /SSH remotes are not supported/);
	assert.match((await sh("cd /tmp && git status")).err, /not a git repository/);
	assert.match((await sh("cd project && git commit")).err, /commit needs -m/);
	// A refused clone leaves no half-made repository.
	assert.equal(await ok("ls -1"), "project\n");
});

test("a merge that conflicts changes nothing and says so", { skip }, async (t) => {
	const remote = await startGitServer(t, join(tempDir(t, "remote"), "srv"));
	const { sh, ok } = await sandbox(t, [remote.host]);
	await ok(`git clone ${remote.origin} && cd project && git checkout -b other && echo theirs > notes.txt && git commit -am theirs && git checkout main && echo ours > notes.txt && git commit -am ours`);
	const merge = await sh("cd project && git merge other");
	assert.notEqual(merge.code, 0);
	assert.match(merge.err, /Conflicts cannot be resolved in this sandbox/);
	assert.equal(await ok("cd project && cat notes.txt && git status --short"), "ours\n");
});
