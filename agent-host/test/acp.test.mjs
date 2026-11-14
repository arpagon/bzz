// The host's ACP face, driven as bzz drives it: a real `node main.mjs`
// process, a scripted client on its pipe, and a scripted stand-in model.
// docs/agent-host-protocol.md, sections 2 to 5.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { getBuiltinModels } from "@earendil-works/pi-ai/providers/all";
import { SqliteFs } from "../src/disk.mjs";
import { MAIN, MODEL, script, sleep, startHost, tempDir, within } from "./support/client.mjs";

const LAUNCHER = fileURLToPath(new URL("./support/launcher.mjs", import.meta.url));
const THREAD = "5b1e0c9e-7a34-4c53-9d0e-0f6c1a2b3c4d/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DM = "9c2f1d0a-1111-4222-8333-444455556666";
const texts = (host) => host.tools.map((call) => call.arguments.text);

/** An agent folder beside a home of its own, and a host started on it. */
function agent(t) {
	const root = tempDir(t, "acp");
	const folder = join(root, "agent");
	const start = (options) => startHost(t, folder, { home: root, ...options });
	return { root, folder, start };
}

test("initialize answers as bzz's own host, and the pipe keeps to JSON-RPC", async (t) => {
	const { start } = agent(t);
	const host = start();
	await assert.rejects(host.rpc("session/new", { cwd: "/home/agent", mcpServers: [] }), /initialize comes first/);
	const answer = await host.initialize();
	assert.equal(answer.protocolVersion, 1);
	assert.deepEqual(answer._meta, { bzz: { host: JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version, tools: "pipe" } });
	assert.equal(answer.agentCapabilities.loadSession, false);
	await assert.rejects(host.initialize(), /already called/);
	await assert.rejects(host.rpc("session/load", {}), { code: -32601 });
	await assert.rejects(host.rpc("session/new", { cwd: "/home/agent", mcpServers: [] }), { code: -32602, message: /bzz\/place/ });
	const session = await host.session(DM);
	await assert.rejects(host.rpc("session/prompt", { sessionId: session, prompt: [{ type: "text", text: "hi" }] }), { code: -32602, message: /bzz\/request/ });
	await assert.rejects(host.prompt("no-such-session", "e1"), { code: -32602 });
	host.child.stdin.write("this is not JSON\n");
	await within(5_000, (async () => { while (!host.lines.some((line) => line.includes("-32700"))) await sleep(20); })(), "a parse error");
	// A notification it does not know is ignored, and the host goes on.
	host.notify("_x.ai/whatever", {});
	assert.deepEqual(await host.prompt(session, "e1"), { stopReason: "end_turn" });
});

test("two sessions at once in one process, each tool call on its own session", async (t) => {
	const { start } = agent(t);
	const host = start();
	await host.initialize();
	const [a, b] = [await host.session(THREAD), await host.session(DM)];
	assert.notEqual(a, b);
	// A's send is held until B's arrives: if the turns ran one after the other, this would never finish.
	let seenB;
	const whenB = new Promise((resolve) => { seenB = resolve; });
	host.onTool = async (call) => {
		if (call.sessionId === b) seenB();
		else await whenB;
		return { text: "sent", final: true };
	};
	const [ra, rb] = await Promise.all([
		host.prompt(a, "e1", script(["buzz_send", { text: "answer for the thread", final: true }])),
		host.prompt(b, "e2", script(["buzz_send", { text: "answer for the DM", final: true }])),
	]);
	assert.deepEqual([ra, rb], [{ stopReason: "end_turn" }, { stopReason: "end_turn" }]);
	assert.deepEqual(host.tools.map((call) => [call.sessionId, call.name, call.arguments.text]).sort(), [[a, "send", "answer for the thread"], [b, "send", "answer for the DM"]].sort());
	// The arguments never say which mention is answered: bzz knows it from the session.
	for (const call of host.tools) assert.deepEqual(Object.keys(call).sort(), ["arguments", "name", "sessionId"]);
	assert.equal(host.child.exitCode, null);
});

test("one conversation per place, found again whatever the session and after a restart", async (t) => {
	const { start } = agent(t);
	let host = start();
	await host.initialize();
	await host.prompt(await host.session(THREAD), "e1");
	// Another session for the same place is the same conversation.
	await host.prompt(await host.session(THREAD), "e2");
	await host.prompt(await host.session(DM), "e3");
	assert.deepEqual(texts(host).map((text) => /holds (\d+)/.exec(text)[1]), ["1", "2", "1"]);
	assert.deepEqual(await host.stop(), { code: 0, signal: null });

	host = start();
	await host.initialize();
	await host.prompt(await host.session(THREAD), "e4");
	await host.prompt(await host.session(DM), "e5");
	await host.prompt(await host.session(`${DM}/another-thread`), "e6");
	assert.deepEqual(texts(host).map((text) => /holds (\d+)/.exec(text)[1]), ["3", "2", "1"]);
});

test("the same request handed over again is the same submission and starts no work", async (t) => {
	const { start } = agent(t);
	let host = start();
	await host.initialize();
	const session = await host.session(THREAD);
	assert.deepEqual(await host.prompt(session, "e1"), { stopReason: "end_turn" });
	assert.deepEqual(await host.prompt(session, "e1"), { stopReason: "end_turn" });
	assert.deepEqual(await host.prompt(await host.session(THREAD), "e1", "ann: the same mention, worded again"), { stopReason: "end_turn" });
	assert.equal(host.tools.length, 1);
	// Twice at the same moment, before the first has settled.
	const both = await Promise.all([host.prompt(session, "e2"), host.prompt(session, "e2")]);
	assert.deepEqual(both, [{ stopReason: "end_turn" }, { stopReason: "end_turn" }]);
	assert.equal(host.tools.length, 2);
	await host.stop();

	// And in the next process, as after a crash of bzz.
	host = start();
	await host.initialize();
	const again = await host.session(THREAD);
	assert.deepEqual(await host.prompt(again, "e1"), { stopReason: "end_turn" });
	assert.deepEqual(await host.prompt(again, "e2"), { stopReason: "end_turn" });
	assert.equal(host.tools.length, 0);
	await host.prompt(again, "e3");
	assert.deepEqual(texts(host), ["pong: this conversation holds 3 message(s) from people"]);
});

test("a turn cut off by a kill continues in the next process, on the session bzz opens there", async (t) => {
	const { start } = agent(t);
	const mention = script(["buzz_send", { text: "working on it" }], ["buzz_send", { text: "done after the restart", final: true }]);
	let host = start();
	await host.initialize();
	// Killed inside the first tool call: bzz got the call, the host never got an answer.
	let inside;
	const called = new Promise((resolve) => { inside = resolve; });
	host.onTool = () => void inside();
	host.prompt(await host.session(THREAD), "e1", mention).catch(() => undefined);
	await within(10_000, called, "the first tool call");
	await host.kill();

	host = start();
	await host.initialize();
	// The turn runs again at once, but it has nobody to send through until bzz hands the request over again.
	await sleep(500);
	assert.equal(host.tools.length, 0);
	const session = await host.session(THREAD);
	assert.deepEqual(await host.prompt(session, "e1", mention), { stopReason: "end_turn" });
	// The interrupted call was not made again; the model went on to its final message.
	assert.deepEqual(host.tools, [{ sessionId: session, name: "send", arguments: { text: "done after the restart", final: true } }]);
});

test("a checkpoint does not wait for a cut-off turn that nobody has asked for again", async (t) => {
	const { folder, start } = agent(t);
	let host = start();
	await host.initialize();
	let inside;
	const called = new Promise((resolve) => { inside = resolve; });
	host.onTool = () => void inside();
	host.prompt(await host.session(THREAD), "e1").catch(() => undefined);
	await within(10_000, called, "the tool call");
	await host.kill();

	host = start();
	await host.initialize();
	await sleep(300);
	// The turn runs again and waits for a session. Storage closes around it, and it is still there afterwards.
	assert.deepEqual(await host.rpc("_bzz/checkpoint", undefined, 10_000), {});
	assert.deepEqual(readdirSync(folder).sort(), ["files.sqlite", "memory.sqlite"]);
	const session = await host.session(THREAD);
	assert.deepEqual(await host.prompt(session, "e1"), { stopReason: "end_turn" });
	assert.equal(host.tools.length, 1);
	assert.equal(host.tools[0].sessionId, session);
});

test("session/cancel stops the turn and answers the prompt with cancelled", async (t) => {
	const { start } = agent(t);
	const host = start();
	await host.initialize();
	const session = await host.session(THREAD);
	let started;
	const running = new Promise((resolve) => { started = resolve; });
	host.onTool = (call) => {
		started();
		return { text: "sent", final: call.arguments.final === true };
	};
	const turn = host.prompt(session, "e1", script(["buzz_send", { text: "this will take a while" }], ["sleep", 120_000], ["buzz_send", { text: "never said", final: true }]));
	await within(10_000, running, "the turn to start");
	const at = Date.now();
	host.notify("session/cancel", { sessionId: session });
	assert.deepEqual(await within(10_000, turn, "the cancelled prompt"), { stopReason: "cancelled" });
	assert.ok(Date.now() - at < 5_000);
	assert.deepEqual(texts(host), ["this will take a while"]);
	// Also while a tool call waits for bzz's answer: the turn does not stay open for it.
	host.onTool = () => undefined;
	const waitingTurn = host.prompt(session, "e1b", script(["buzz_send", { text: "bzz never answers this", final: true }]));
	await within(10_000, (async () => { while (texts(host).at(-1) !== "bzz never answers this") await sleep(20); })(), "the tool call");
	host.notify("session/cancel", { sessionId: session });
	assert.deepEqual(await within(10_000, waitingTurn, "the prompt cancelled inside a tool call"), { stopReason: "cancelled" });
	host.onTool = (call) => ({ text: "sent", final: call.arguments.final === true });
	// A cancel with nothing running is harmless, and the conversation works afterwards.
	host.notify("session/cancel", { sessionId: session });
	assert.deepEqual(await host.prompt(session, "e2", script(["buzz_send", { text: "still here", final: true }])), { stopReason: "end_turn" });
	assert.equal(texts(host).at(-1), "still here");
});

test("_bzz/checkpoint leaves both files whole with no write-ahead file, and the host goes on", async (t) => {
	const { folder, start } = agent(t);
	const host = start();
	await host.initialize();
	const session = await host.session(THREAD);
	await host.prompt(session, "e1", script(["write", { path: "kept.txt", content: "kept\n" }], ["buzz_send", { text: "written", final: true }]));
	assert.ok(readdirSync(folder).some((name) => name.endsWith("-wal")), "a write-ahead file exists while the host works");
	assert.deepEqual(await host.rpc("_bzz/checkpoint"), {});
	assert.deepEqual(readdirSync(folder).sort(), ["files.sqlite", "memory.sqlite"]);
	// Both files are complete on their own: another process reads them while the host still runs.
	const disk = new SqliteFs(join(folder, "files.sqlite"));
	assert.equal(Buffer.from(disk.readFile("/home/agent/kept.txt")).toString(), "kept\n");
	assert.deepEqual(disk.check(), []);
	disk.close();

	// A checkpoint asked for during a turn waits for the turn: writes finish first.
	const order = [];
	const turn = host.prompt(session, "e2", script(["sleep", 400], ["bash", { command: "cat kept.txt" }], ["report"])).then((result) => (order.push("turn"), result));
	await sleep(100);
	const checkpoint = host.rpc("_bzz/checkpoint").then(() => order.push("checkpoint"));
	await Promise.all([turn, checkpoint]);
	assert.deepEqual(order, ["turn", "checkpoint"]);
	assert.equal(texts(host).at(-1), "kept\n");
	assert.deepEqual(readdirSync(folder).sort(), ["files.sqlite", "memory.sqlite"]);
	await host.prompt(session, "e3");
	assert.equal(texts(host).at(-1), "pong: this conversation holds 3 message(s) from people");
});

test("the host leaves within 2 s when its input ends, with nothing beside its two files", async (t) => {
	const { folder, start } = agent(t);
	const host = start();
	await host.initialize();
	await host.prompt(await host.session(THREAD), "e1");
	const at = Date.now();
	assert.deepEqual(await host.stop(2_000), { code: 0, signal: null });
	t.diagnostic(`left ${Date.now() - at} ms after its input ended`);
	assert.deepEqual(readdirSync(folder).sort(), ["files.sqlite", "memory.sqlite"]);

	// Also in the middle of a turn: the turn is in storage and waits for the next process.
	const busy = start();
	await busy.initialize();
	let inside;
	const called = new Promise((resolve) => { inside = resolve; });
	busy.onTool = () => void inside();
	busy.prompt(await busy.session(DM), "e2").catch(() => undefined);
	await within(10_000, called, "the tool call");
	assert.deepEqual(await busy.stop(2_000), { code: 0, signal: null });
});

test("the host leaves when its parent is gone, even with its input still open", { skip: process.platform === "win32" && "a pipe cannot be handed to a second process this way on Windows: the parent probe there is unproven" }, async (t) => {
	const { root, folder } = agent(t);
	const pidFile = join(root, "pids.json");
	const launcher = startHost(t, folder, { home: root, command: [process.execPath, LAUNCHER, pidFile, MAIN, folder] });
	await within(10_000, (async () => { while (!existsSync(pidFile)) await sleep(20); })(), "the launcher to start the host");
	const pids = JSON.parse(readFileSync(pidFile, "utf8"));
	const alive = (pid) => {
		try {
			process.kill(pid, 0);
			return true;
		} catch {
			return false;
		}
	};
	t.after(() => {
		for (const pid of Object.values(pids)) try { process.kill(pid, "SIGKILL"); } catch {}
	});
	await within(10_000, (async () => { while (!/host \S+ started/.test(launcher.stderr())) await sleep(20); })(), "the host to start");
	assert.equal(alive(pids.host), true);
	// Only the launcher dies. The holder keeps the host's input open.
	await launcher.kill();
	await within(6_000, (async () => { while (alive(pids.host)) await sleep(50); })(), "the orphaned host to leave");
	assert.equal(alive(pids.holder), true, "the host's input was still held open when it left");
	assert.match(launcher.stderr(), /the parent process is gone/);
});

test("a model that is not available fails the turn and says why", async (t) => {
	const { root, start } = agent(t);
	const attempt = async (bzz, files = {}) => {
		const pi = join(root, "pi");
		mkdirSync(pi, { recursive: true });
		for (const [name, content] of Object.entries(files)) writeFileSync(join(pi, name), JSON.stringify(content));
		const host = start();
		await host.initialize(bzz);
		const failure = await host.prompt(await host.session(THREAD), "e1").then((result) => result, (error) => error);
		await host.stop();
		return { failure, host };
	};
	const openai = getBuiltinModels("openai")[0].id;

	let { failure, host } = await attempt({ model: { provider: "my-extension", modelId: "x" } });
	assert.match(failure.message, /The provider "my-extension" is not available.*Pi Durable entry/);
	assert.equal(failure.data.kind, "provider_unavailable");
	assert.equal(host.tools.length, 0);

	({ failure } = await attempt({ model: { provider: "openai", modelId: "no-such-model" } }));
	assert.match(failure.message, /has no model "no-such-model"/);
	({ failure } = await attempt({ model: { provider: "openai", modelId: openai } }));
	assert.match(failure.message, /The provider "openai" is not signed in\. Run `pi` and `\/login`/);
	assert.equal(failure.data.kind, "sign_in_needed");
	({ failure } = await attempt({ model: { provider: "openai", modelId: openai } }, { "auth.json": { openai: { type: "api_key", key: "!security find-generic-password" } } }));
	assert.match(failure.message, /does not use `!command`/);
	({ failure } = await attempt({ model: undefined }, { "auth.json": {} }));
	assert.match(failure.message, /This agent has no model/);

	// Nothing was kept of the failed turns: with a model, the same request is answered. With none named, Pi's default is used.
	({ failure, host } = await attempt({ model: undefined }, { "settings.json": { defaultProvider: MODEL.provider, defaultModel: MODEL.modelId } }));
	assert.deepEqual(failure, { stopReason: "end_turn" });
	assert.deepEqual(texts(host), ["pong: this conversation holds 1 message(s) from people"]);
	// Pi's folder was only read.
	assert.deepEqual(readdirSync(join(root, "pi")).sort(), ["auth.json", "settings.json"]);
});

test("a provider that fails during the turn fails the prompt with its reason", async (t) => {
	const { start } = agent(t);
	const host = start();
	await host.initialize();
	const session = await host.session(THREAD);
	const failure = await host.prompt(session, "e1", script(["error", "401 the key was refused"])).catch((error) => error);
	assert.equal(failure.code, -32000);
	assert.match(failure.message, /The turn failed: .*401 the key was refused/);
	// The next mention in the same place is answered.
	assert.deepEqual(await host.prompt(session, "e2", script(["buzz_send", { text: "back", final: true }])), { stopReason: "end_turn" });
});

test("Pi's read, write, edit and bash work on the agent's disk, and bzz is told what happens", async (t) => {
	const { folder, start } = agent(t);
	const host = start();
	await host.initialize();
	const session = await host.session(THREAD);
	const command = "cd project && git init -q && git add -A && git commit -m 'agent: notes' && git log --oneline && cat notes.txt && pwd";
	await host.prompt(session, "e1", script(
		["write", { path: "project/notes.txt", content: "alpha\nbeta\ngamma\n" }],
		["edit", { path: "project/notes.txt", edits: [{ oldText: "beta", newText: "BETA" }] }],
		["read", { path: "project/notes.txt" }],
		["bash", { command }],
		["report"],
	));
	assert.match(texts(host)[0], /^\[main [0-9a-f]{7}\] agent: notes\n[0-9a-f]{7} agent: notes\nalpha\nBETA\ngamma\n\/home\/agent\/project\n$/);
	const seen = host.updates.filter((entry) => entry.sessionId === session).map(({ update }) => [update.sessionUpdate, update.title ?? update.status, update.kind ?? update.status]);
	assert.deepEqual(seen.filter(([kind]) => kind === "tool_call"), [
		["tool_call", "write: project/notes.txt", "edit"], ["tool_call", "edit: project/notes.txt", "edit"], ["tool_call", "read: project/notes.txt", "read"],
		["tool_call", `bash: ${command.slice(0, 80)}`, "execute"], ["tool_call", "buzz_send", "other"],
	]);
	assert.equal(seen.filter(([kind, status]) => kind === "tool_call_update" && status === "completed").length, 5);
	// A turn that ends in plain text: bzz gets the text, which it publishes when no send happened.
	assert.deepEqual(await host.prompt(session, "e2", script("say: I have nothing to add.")), { stopReason: "end_turn" });
	assert.deepEqual(host.updates.at(-1), { sessionId: session, update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "I have nothing to add." } } });
	// A refusal by bzz reaches the model as an error it can act on.
	host.onTool = (call) => (call.arguments.text === "first try" ? { text: "refused: too long. Send a shorter text.", isError: true } : { text: "sent", final: true });
	await host.prompt(session, "e3", script(["buzz_send", { message: "first try" }], ["report"]));
	assert.deepEqual(host.tools.slice(-2).map((call) => call.arguments), [{ text: "first try" }, { text: "refused: too long. Send a shorter text.", final: true }]);
	await host.stop();

	const disk = new SqliteFs(join(folder, "files.sqlite"));
	assert.equal(Buffer.from(disk.readFile("/home/agent/project/notes.txt")).toString(), "alpha\nBETA\ngamma\n");
	assert.deepEqual(disk.check(), []);
	disk.close();
});

test("grants and the allow-list arrive in initialize, apply to the tools, and leave no path of the machine in either file", async (t) => {
	const { root, folder, start } = agent(t);
	mkdirSync(join(root, "site"));
	mkdirSync(join(root, "drop"));
	writeFileSync(join(root, "site", "index.html"), "<h1>granted</h1>\n");
	writeFileSync(join(root, "secret.txt"), "SECRET\n");
	const reached = [];
	const servers = await Promise.all([0, 1].map(async (n) => {
		const server = createServer((request, response) => {
			reached.push(n);
			response.end(`server ${n} answered ${request.url}\n`);
		});
		await new Promise((done) => server.listen(0, "127.0.0.1", done));
		t.after(() => new Promise((done) => (server.closeAllConnections(), server.close(done))));
		return `127.0.0.1:${server.address().port}`;
	}));
	const host = start();
	await assert.rejects(host.initialize({ mounts: [{ name: "../site", path: join(root, "site") }] }), { code: -32602, message: /simple name/ });
	await host.initialize({ mounts: [{ name: "site", path: join(root, "site"), write: false }, { name: "drop", path: join(root, "drop"), write: true }], network: [servers[0]] });
	const session = await host.session(THREAD);
	const run = async (request, command) => {
		await host.prompt(session, request, script(["bash", { command }], ["report"]));
		return texts(host).at(-1);
	};
	assert.equal(await run("e1", "ls -1 /mnt; cat /mnt/site/index.html; echo made > /mnt/drop/made.txt; cat /mnt/drop/made.txt"), "drop\nsite\n<h1>granted</h1>\nmade\n");
	assert.equal(readFileSync(join(root, "drop", "made.txt"), "utf8"), "made\n");
	assert.match(await run("e2", "echo no > /mnt/site/no.txt"), /EROFS: this folder was granted read-only/);
	// Errors of the machine's own filesystem come back with the virtual path.
	assert.match(await run("e3", "cat /mnt/site/../../secret.txt /mnt/drop/../secret.txt; ls /mnt/site/index.html/x; cat /mnt/site/index.html/x; echo end"), /^(?!.*SECRET).*end\n/s);
	assert.equal(existsSync(join(root, "site", "no.txt")), false);
	assert.equal(await run("e4", `curl -s http://${servers[0]}/a`), "server 0 answered /a\n");
	assert.match(await run("e5", `curl -sS http://${servers[1]}/b; git clone http://${servers[1]}/r.git`), /not on this agent's allow-list.*not on this agent's allow-list/s);
	assert.deepEqual(reached, [0]);
	// The model is told what it has, by name.
	await host.prompt(session, "e6", script(["write", { path: "/mnt/drop/from-tool.txt", content: "tool\n" }], ["read", { path: "/mnt/site/index.html" }], ["report"]));
	assert.match(texts(host).at(-1), /<h1>granted<\/h1>/);
	assert.equal(readFileSync(join(root, "drop", "from-tool.txt"), "utf8"), "tool\n");
	// What the machine's own filesystem refuses is told with the virtual path, in Pi's tools and in the shell.
	await host.prompt(session, "e7", script(["read", { path: "/mnt/site/index.html/x" }], ["bash", { command: "touch /mnt/drop/absent/x; mkdir /mnt/site/index.html/d" }], ["report"]));
	assert.match(texts(host).at(-1), /'\/mnt\/drop\/absent\/x'/);
	await host.rpc("_bzz/checkpoint");
	await host.stop();

	// What a move carries holds the virtual names, and nothing of this machine: not the granted paths, not Pi's folder, not its own.
	for (const name of ["memory.sqlite", "files.sqlite"]) {
		const bytes = readFileSync(join(folder, name));
		for (const real of [root, root.replaceAll("\\", "\\\\"), join(root, "site"), "bzz-agent-host-acp-"]) assert.equal(bytes.includes(real), false, `${name} holds ${real}`);
	}
	assert.equal(readFileSync(join(folder, "memory.sqlite")).includes("/mnt/site"), true);
});
