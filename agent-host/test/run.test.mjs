// Hands on this machine (docs/agent-host-protocol.md, `_bzz/approve`): the
// `run` tool starts a real command in a folder the owner granted with
// `exec`, after bzz said yes. The commands here are real ones, run in
// temporary folders; the client that stands in for bzz answers the question.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { SqliteFs } from "../src/disk.mjs";
import { bareEnv, script, sleep, startHost, tempDir, within } from "./support/client.mjs";

const skip = process.platform === "win32" && "the host does not offer `run` on Windows: it cannot end a command's whole process tree there";
const THREAD = "5b1e0c9e-7a34-4c53-9d0e-0f6c1a2b3c4d/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const alive = (pid) => {
	try {
		process.kill(pid, 0);
		return true;
	} catch {
		return false;
	}
};

/** An agent with `site` granted for commands and `docs` granted without, and a client that says yes unless told otherwise. */
async function world(t, { mounts } = {}) {
	// What a failing test leaves running is killed. Set up before the temporary folder is, because the hooks run in
	// that order and this one reads the process ids from a file in the folder.
	let site;
	t.after(() => {
		try {
			for (const pid of readFileSync(join(site, "pids"), "utf8").trim().split("\n")) try { process.kill(Number(pid), "SIGKILL"); } catch {}
		} catch {}
	});
	const root = tempDir(t, "run");
	const folder = join(root, "agent");
	site = join(root, "site");
	for (const dir of [site, join(root, "docs")]) mkdirSync(dir);
	const grants = mounts ?? [{ name: "site", path: site, write: false, exec: true }, { name: "docs", path: join(root, "docs"), write: true }];
	const state = { n: 0 };
	const start = async (options = {}) => {
		// The host's own environment holds things a command must not get.
		const host = startHost(t, folder, { home: root, env: bareEnv(root, { A_SECRET_OF_THE_HOST: "must-not-reach-a-command" }), ...options });
		host.on["_bzz/approve"] = () => ({ allow: true });
		host.answer = await host.initialize({ mounts: grants });
		host.sessionId = await host.session(THREAD);
		/** One `run` call by the model; resolves with the tool's result as the model got it. */
		host.run = async (args, request = `e${++state.n}`) => {
			await host.prompt(host.sessionId, request, script(["run", args], ["report"]));
			return host.tools.at(-1).arguments.text;
		};
		host.approvals = () => host.asked.filter((entry) => entry.method === "_bzz/approve").map((entry) => entry.params);
		return host;
	};
	/** The process ids a command wrote to `pids` in the folder, once it has. */
	const pids = async (count) => {
		const file = join(site, "pids");
		await within(10_000, (async () => { while (!existsSync(file) || readFileSync(file, "utf8").trim().split("\n").length < count) await sleep(20); })(), `${count} process ids in ${file}`);
		return readFileSync(file, "utf8").trim().split("\n").map(Number);
	};
	const gone = async (ids, ms = 6_000) => {
		const until = Date.now() + ms;
		while (ids.some(alive) && Date.now() < until) await sleep(25);
		assert.deepEqual(ids.filter(alive), [], `these processes are still there after ${ms} ms`);
	};
	return { root, folder, site, start, pids, gone };
}

/** A command that starts two children and waits, with every process id written to `pids`. */
const TREE = "echo $$ > pids; sleep 300 & echo $! >> pids; sleep 300 & echo $! >> pids; echo started; wait";

test("an approved command runs in the granted folder, exactly as approved, in an environment built from nothing", { skip }, async (t) => {
	const { root, site, start } = await world(t);
	const host = await start();
	assert.deepEqual(host.answer._meta.bzz.run, ["site"]);
	const command = "pwd; echo 'made by run' > made.txt; env | sort";
	const out = await host.run({ folder: "site", command });
	// bzz was asked first, about that string and that folder, on the turn's session.
	assert.deepEqual(host.asked, [{ method: "_bzz/approve", params: { sessionId: host.sessionId, kind: "exec", mount: "site", command } }]);
	const lines = out.trim().split("\n");
	assert.equal(lines[0], realpathSync(site));
	assert.equal(readFileSync(join(site, "made.txt"), "utf8"), "made by run\n");
	// The environment: the four names, and what the shell itself adds. Nothing of the host's own.
	const env = Object.fromEntries(lines.slice(1).map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
	// (`__CF_USER_TEXT_ENCODING` is what macOS gives every process.)
	const own = ["PWD", "OLDPWD", "SHLVL", "_", "__CF_USER_TEXT_ENCODING"];
	assert.deepEqual(Object.keys(env).filter((name) => !own.includes(name)).sort(), ["HOME", "LANG", "PATH", "TMPDIR"].filter((name) => process.env[name] !== undefined || name === "HOME" || name === "PATH").sort());
	assert.equal(env.HOME, root);
	assert.equal(env.PATH, process.env.PATH);
	assert.equal(out.includes("must-not-reach-a-command"), false);
	assert.equal(out.includes("SCRIPTED_MODEL"), false);

	// The string is the shell's, untouched: quotes, variables, pipes and arithmetic mean what they mean in sh.
	const quoted = `printf '%s\\n' "$HOME" 'single $quoted' "a  b" $((6 * 7)); echo "x; y" | tr x z; echo \`echo back\`ticks`;
	assert.equal(await host.run({ folder: "/mnt/site/", command: quoted }), `${root}\nsingle $quoted\na  b\n42\nz; y\nbackticks\n`);
	assert.equal(host.approvals().at(-1).command, quoted);
	assert.equal(host.approvals().at(-1).mount, "site");
	// What the shell was started with is the approved string, whole and alone, as its one command.
	const self = "ps -o args= -p $$";
	assert.equal((await host.run({ folder: "site", command: self })).trim(), `/bin/sh -c -- ${self}`);
	// Also a line that begins like an option of the shell: it is the command, not an option.
	assert.match(await host.run({ folder: "site", command: "-x; echo after the dash" }), /-x: (command )?not found\nafter the dash\n/);

	// No terminal and nothing to read: a command that waits for input gets the end of it at once.
	assert.equal(await host.run({ folder: "site", command: "cat; echo end of input; test -t 0 || echo no terminal" }), "end of input\nno terminal\n");
	// A process group of its own: the shell leads it, and it is neither the host's group nor this test's.
	const [pid, pgid] = (await host.run({ folder: "site", command: "ps -o pid= -o pgid= -p $$" })).trim().split(/\s+/).map(Number);
	const groupOf = (id) => Number(execFileSync("ps", ["-o", "pgid=", "-p", String(id)], { encoding: "utf8" }).trim());
	assert.equal(pgid, pid);
	assert.notEqual(pgid, groupOf(host.child.pid));
	assert.notEqual(pgid, groupOf(process.pid));

	// A command that fails: its output, and how it ended.
	assert.equal(await host.run({ folder: "site", command: "echo out; echo err >&2; exit 3" }), "out\nerr\n<harness>\n[error] Command exited with code 3\n</harness>");
	assert.match(await host.run({ folder: "site", command: "kill -SEGV $$" }), /Command was ended by SIGSEGV/);
	assert.match(await host.run({ folder: "site", command: "no-such-program-on-this-machine" }), /not found\n<harness>\n\[error\] Command exited with code 127/);
	// bzz is told what the agent is doing, with the folder.
	const calls = host.updates.map(({ update }) => update).filter((update) => update.title?.startsWith("run"));
	assert.deepEqual(calls.map(({ title, kind }) => [title, kind])[0], [`run in /mnt/site: ${command}`, "execute"]);
	// And is told before it is asked: the call is on bzz's screen when the question about it arrives.
	const line = (text) => host.lines.findIndex((entry) => entry.includes(text));
	assert.ok(line('"sessionUpdate":"tool_call"') < line('"_bzz/approve"'), "the tool call is announced before the question");
	assert.deepEqual(host.updates.map(({ update }) => update.status).filter((status) => status === "failed").length, 3);


	// The model is told what it has: its disk, the folders and which it may write, `run`, and that the owner may be asked.
	await host.prompt(await host.session("another-place"), "d1", script(["describe"]));
	const told = JSON.parse(host.tools.at(-1).arguments.text);
	assert.deepEqual(told.tools, ["read", "write", "edit", "bash", "buzz_send", "buzz_read", "buzz_react", "run"]);
	assert.match(told.system, /You have a disk of your own: `read`, `write`, `edit` and `bash` work on it\./);
	assert.match(told.system, /Folders of the owner's machine granted to you, which the same tools reach: \/mnt\/site \(read-only\), \/mnt\/docs \(read and write\)\./);
	assert.match(told.system, /`run` starts a real command of the owner's machine in \/mnt\/site: .*The owner may be asked before a command starts and can refuse it/);
	assert.match(host.stderr(), /run in \/mnt\/site: exit 0, \d+ bytes of output\n/);
	// The log names what happened, not what was run.
	assert.equal(host.stderr().includes("made by run"), false);
});

test("a command gets its three streams and no other open file of the host", { skip }, async (t) => {
	const { root, start } = await world(t);
	// A file of whoever started bzz, left open to its children: the host is born with it, as number 200.
	const theirs = join(root, "a-file-of-the-parent.txt");
	const descriptor = openSync(theirs, "a");
	t.after(() => closeSync(descriptor));
	const host = await start({ descriptors: { 200: descriptor } });
	// The command tries to write through that number, and lists what it has open.
	const probe = `${JSON.stringify(process.execPath)} -e 'try { require("fs").writeSync(200, "written by a command\\n"); console.log("200 is open") } catch (error) { console.log("200:", error.code) }'`;
	const out = await host.run({ folder: "site", command: probe });
	if (process.platform !== "linux") {
		// Only Linux tells the host which descriptors it inherited. Elsewhere the command has them, and bzz must hand the host none.
		t.diagnostic(`not Linux: the command inherits the host's descriptors here (${out.trim()})`);
		return;
	}
	assert.equal(out, "200: EBADF\n");
	assert.equal(readFileSync(theirs, "utf8"), "");
	// The shell itself: the three streams, and the descriptor `ls` reads the list with.
	assert.equal(await host.run({ folder: "site", command: "ls /proc/self/fd | tr '\\n' ' '" }), "0 1 2 3 ");
});

test("a refused command does not start, and the model is told why", { skip }, async (t) => {
	const { site, start } = await world(t);
	const host = await start();
	const command = "echo it ran > ran.txt";
	host.on["_bzz/approve"] = () => ({ allow: false, reason: "Not while the deploy runs." });
	assert.equal(await host.run({ folder: "site", command }), "The command was not allowed: Not while the deploy runs.\nIt was not started.");
	assert.equal(host.updates.at(-3).update.status, "failed");
	// Only `allow: true` is a yes.
	for (const answer of [{ allow: false }, {}, { allow: "true" }, { allow: 1 }, { allowed: true }, null, "yes"]) {
		host.on["_bzz/approve"] = () => answer;
		assert.match(await host.run({ folder: "site", command }), /^The command was not allowed\.\nIt was not started\.$/, JSON.stringify(answer));
	}
	// A bzz that fails, or does not know the question, has not said yes.
	host.on["_bzz/approve"] = () => { throw Object.assign(new Error("The owner did not answer in time."), { code: -32002 }); };
	assert.equal(await host.run({ folder: "site", command }), "The command was not started: bzz did not approve it (The owner did not answer in time.).");
	delete host.on["_bzz/approve"];
	assert.match(await host.run({ folder: "site", command }), /bzz did not approve it \(Method not found: _bzz\/approve\)/);
	assert.equal(host.approvals().length, 10);
	assert.equal(existsSync(join(site, "ran.txt")), false);

	// Two commands asked for in one answer of the model: the owner is asked about the second when the first is over.
	const order = [];
	host.on["_bzz/approve"] = ({ command: asked }) => {
		order.push(`asked: ${asked}`, `first has run: ${existsSync(join(site, "first.txt"))}`);
		return { allow: !asked.includes("second") };
	};
	await host.prompt(host.sessionId, "two", script(["calls", [["run", { folder: "site", command: "sleep 0.4; echo 1 > first.txt" }], ["run", { folder: "site", command: "echo second > second.txt" }]]], ["report"]));
	assert.deepEqual(order, ["asked: sleep 0.4; echo 1 > first.txt", "first has run: false", "asked: echo second > second.txt", "first has run: true"]);
	assert.equal(existsSync(join(site, "second.txt")), false);
	assert.equal(host.approvals().length, 12);

	// What can never run is refused before the owner is troubled with it.
	host.on["_bzz/approve"] = () => ({ allow: true });
	for (const [args, pattern] of [
		[{ folder: "site", command: "  " }, /run needs `command`/], [{ folder: "site", command: "echo a\0b" }, /NUL character/], [{ folder: "site", command, timeout: 0 }, /`timeout` must be a number of seconds, above 0 and at most 3600/],
		[{ folder: "site", command, timeout: 99_999 }, /at most 3600/], [{ folder: "site", command: `echo ${"x".repeat(100_001)}` }, /longer than 100000 bytes/],
	]) assert.match(await host.run(args), pattern);
	assert.equal(host.approvals().length, 12);
	assert.equal(existsSync(join(site, "ran.txt")), false);
	// And then a yes: it runs.
	assert.equal(await host.run({ folder: "site", command }), "");
	assert.equal(readFileSync(join(site, "ran.txt"), "utf8"), "it ran\n");
});

test("a command that outlives its time limit is stopped, with the children it started", { skip }, async (t) => {
	const { start, pids, gone } = await world(t);
	const host = await start();
	const at = Date.now();
	const out = await host.run({ folder: "site", command: TREE, timeout: 1 });
	assert.match(out, /^started\n<harness>\n\[error\] The command was stopped after 1 seconds, its time limit\.\n<\/harness>$/);
	assert.ok(Date.now() - at < 4_000, `it took ${Date.now() - at} ms`);
	const ids = await pids(3);
	// The call returns when the group has ended: nothing is left to wait for.
	assert.deepEqual(ids.filter(alive), []);

	// One that ignores TERM, children included: after the grace it is killed.
	const stubborn = await host.run({ folder: "site", command: `trap '' TERM; ${TREE}`, timeout: 1 });
	assert.match(stubborn, /stopped after 1 seconds/);
	await gone(await pids(3), 1_000);
	// The agent goes on, and so does the tool.
	assert.equal(await host.run({ folder: "site", command: "echo still here" }), "still here\n");
});

test("what a command leaves running ends with it, and a turn that is cancelled ends its command", { skip }, async (t) => {
	const { site, start, pids, gone } = await world(t);
	const host = await start();
	// Left in the background, also by a child of a child: the command returns, and they are gone.
	const at = Date.now();
	assert.equal(await host.run({ folder: "site", command: "sleep 300 & echo $! > pids; (sleep 300 & echo $! >> pids); echo done" }), "done\n");
	assert.ok(Date.now() - at < 3_000, `it took ${Date.now() - at} ms`);
	await gone(await pids(2), 1_000);

	// Cancelled in the middle: the prompt answers `cancelled`, and the tree is gone.
	const turn = host.prompt(host.sessionId, "c1", script(["run", { folder: "site", command: TREE }], ["report"]));
	const ids = await pids(3);
	assert.deepEqual(ids.filter(alive).length, 3);
	host.notify("session/cancel", { sessionId: host.sessionId });
	assert.deepEqual(await within(8_000, turn, "the cancelled prompt"), { stopReason: "cancelled" });
	await gone(ids, 1_000);

	// Cancelled while the owner is being asked: nothing was started.
	let asked;
	const waiting = new Promise((resolve) => { asked = resolve; });
	host.on["_bzz/approve"] = () => void asked();
	const pending = host.prompt(host.sessionId, "c2", script(["run", { folder: "site", command: "echo it ran > ran.txt" }], ["report"]));
	await waiting;
	host.notify("session/cancel", { sessionId: host.sessionId });
	assert.deepEqual(await within(8_000, pending, "the prompt cancelled during the question"), { stopReason: "cancelled" });
	await sleep(200);
	assert.equal(existsSync(join(site, "ran.txt")), false);

	// The limit of a process group, shown as it is: a process that leaves the group is not ended, and the call does not wait for it.
	host.on["_bzz/approve"] = () => ({ allow: true });
	const leave = `${JSON.stringify(process.execPath)} -e 'const child = require("child_process").spawn("sleep", ["300"], { detached: true, stdio: "inherit" }); require("fs").writeFileSync("pids", String(child.pid)); child.unref()'; echo left`;
	const before = Date.now();
	assert.equal(await host.run({ folder: "site", command: leave }), "left\n");
	assert.ok(Date.now() - before < 3_000, `it took ${Date.now() - before} ms`);
	const [escaped] = await pids(1);
	assert.equal(alive(escaped), true, "a process that put itself in a new session is beyond the group");
	process.kill(escaped, "SIGKILL");
});

test("a long output comes back bounded, and the whole of it is on the agent's disk", { skip }, async (t) => {
	const { folder, start } = await world(t);
	const host = await start();
	const out = await host.run({ folder: "site", command: "seq 1 5000" });
	const lines = out.split("\n");
	// The end of the output, within Pi's bound of 2000 lines, and where the rest is.
	assert.equal(lines.includes("5000"), true);
	assert.equal(lines.includes("2999"), false);
	assert.ok(lines.length < 2_100, `${lines.length} lines`);
	const path = /Full output: (\/tmp\/run-[0-9a-f]{8}\.log)/.exec(out)?.[1];
	assert.ok(path, out.slice(-300));
	// The agent reads it with its own tools.
	await host.prompt(host.sessionId, "o1", script(["bash", { command: `wc -l < ${path}; head -n 2 ${path}; tail -n 1 ${path}` }], ["report"]));
	assert.equal(host.tools.at(-1).arguments.text.replace(/ +/g, ""), "5000\n1\n2\n5000\n");
	// A short output is not saved: it is all in the result.
	assert.equal(await host.run({ folder: "site", command: "seq 1 5" }), "1\n2\n3\n4\n5\n");

	// More than is kept: the result says how much there was and how much of it is in the file.
	const huge = await host.run({ folder: "site", command: "yes 0123456789abcdef | head -c 9000000" });
	assert.ok(huge.length < 60_000, `${huge.length} characters`);
	const saved = /The output was 8\.6 MB\. Its first 8\.0 MB: (\/tmp\/run-[0-9a-f]{8}\.log)/.exec(huge)?.[1];
	assert.ok(saved, huge.slice(-300));
	// What a command made in the folder comes to the agent's disk by a copy with the agent's own tools.
	assert.equal(await host.run({ folder: "site", command: "echo built by a real command > artifact.txt" }), "");
	await host.prompt(host.sessionId, "o2", script(["bash", { command: "cp /mnt/site/artifact.txt /home/agent/artifact.txt" }], ["report"]));
	await host.stop();
	const disk = new SqliteFs(join(folder, "files.sqlite"));
	t.after(() => disk.close());
	assert.equal(disk.stat(saved).size, 8 * 1024 * 1024);
	assert.equal(Buffer.from(disk.readFile("/home/agent/artifact.txt")).toString(), "built by a real command\n");
	assert.equal(Buffer.from(disk.readFile(path)).toString(), `${Array.from({ length: 5000 }, (_, n) => n + 1).join("\n")}\n`);
	assert.deepEqual(disk.readdir("/tmp").map((entry) => entry.name).filter((name) => name.startsWith("run-")).length, 2);
});

test("without a folder granted for it there is no `run`, and a grant without `exec` gives none", { skip }, async (t) => {
	// `docs` is granted, for writing even, but not for commands.
	const both = await world(t);
	const host = await both.start();
	assert.equal(await host.run({ folder: "docs", command: "echo it ran > ran.txt" }), '`run` is not allowed in "docs". It is allowed in: site.');
	assert.match(await host.run({ folder: "../site", command: "echo x" }), /`run` is not allowed in "\.\.\/site"/);
	assert.match(await host.run({ folder: "nowhere", command: "echo x" }), /`run` is not allowed in "nowhere"/);
	assert.deepEqual(host.asked, [], "bzz is not asked about a folder that has no leave");
	assert.equal(existsSync(join(both.root, "docs", "ran.txt")), false);
	await host.stop();

	// No grant with `exec`: the tool does not exist, the model is not told of it, and a call to it finds nothing.
	for (const mounts of [[], [{ name: "docs", path: join(both.root, "docs"), write: true }], [{ name: "docs", path: join(both.root, "docs"), write: true, exec: "true" }]]) {
		const none = await world(t, { mounts });
		const plain = await none.start();
		assert.deepEqual(plain.answer._meta.bzz.run, []);
		await plain.prompt(plain.sessionId, "d1", script(["describe"]));
		const told = JSON.parse(plain.tools.at(-1).arguments.text);
		assert.deepEqual(told.tools, ["read", "write", "edit", "bash", "buzz_send", "buzz_read", "buzz_react"]);
		assert.equal(/`run`|real command|may be asked/.test(told.system), false, told.system);
		assert.match(await plain.run({ folder: "docs", command: "echo it ran > ran.txt" }), /Tool run is not available/);
		assert.deepEqual(plain.asked, []);
		assert.equal(existsSync(join(none.root, "docs", "ran.txt")), false);
		await plain.stop();
	}
	// A folder granted for commands that is not there: said, and bzz is not asked.
	const absent = await world(t, { mounts: [{ name: "gone", path: join(both.root, "no-such-folder"), exec: true }] });
	const lost = await absent.start();
	assert.equal(await lost.run({ folder: "gone", command: "echo x" }), "The folder /mnt/gone is not there on the owner's machine.");
	assert.deepEqual(lost.asked, []);
});

test("a command does not outlive the host, however the host goes, and is not started again after a restart", { skip }, async (t) => {
	const { site, start, pids, gone } = await world(t);
	const command = `echo started >> starts.txt; ${TREE}`;
	// The host is killed outright: it can do nothing, and the keeper of the command sees it go.
	let host = await start();
	host.prompt(host.sessionId, "k1", script(["run", { folder: "site", command }], ["report"])).catch(() => undefined);
	let ids = await pids(3);
	assert.equal(ids.filter(alive).length, 3);
	await host.kill();
	await gone(ids, 5_000);

	// The next process continues the turn. The command that was cut off is not started a second time: the model is told it was interrupted.
	host = await start();
	// Should it be put to bzz again, bzz says no here, so that such a fault shows at once and starts nothing.
	host.on["_bzz/approve"] = () => ({ allow: false, reason: "not a second time" });
	assert.deepEqual(await host.prompt(host.sessionId, "k1", script(["run", { folder: "site", command }], ["report"])), { stopReason: "end_turn" });
	assert.match(host.tools.at(-1).arguments.text, /Tool run was interrupted/);
	assert.deepEqual(host.asked, []);
	assert.equal(readFileSync(join(site, "starts.txt"), "utf8"), "started\n");

	// The host is stopped as bzz stops it, by the end of its input: it leaves in time, and the command with it.
	writeFileSync(join(site, "pids"), "");
	host.on["_bzz/approve"] = () => ({ allow: true });
	host.prompt(host.sessionId, "k2", script(["run", { folder: "site", command: TREE }], ["report"])).catch(() => undefined);
	ids = await pids(3);
	assert.deepEqual(await host.stop(2_000), { code: 0, signal: null });
	await gone(ids, 5_000);

	// A command that ignores TERM, children included, under a host that is killed: the keeper outlives the host, gives
	// the group its grace, and kills what is left. Nothing but the keeper could.
	const other = await world(t);
	host = await other.start();
	host.prompt(host.sessionId, "k3", script(["run", { folder: "site", command: `trap '' TERM; ${TREE}` }], ["report"])).catch(() => undefined);
	ids = await other.pids(3);
	await host.kill();
	await sleep(300);
	assert.equal(ids.filter(alive).length, 3, "TERM was ignored: only KILL ends this group");
	await other.gone(ids, 5_000);
});
