// Credentials for a private git remote (docs/agent-host-protocol.md,
// `_bzz/credential`). They come from bzz when the remote asks, serve that one
// git command, and are kept nowhere. The remote is `git http-backend` behind
// a server that answers 401 without the right `Authorization` header; the
// secret is a marker, searched for afterwards in everything the host wrote.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { Bash } from "just-bash";
import { shellFs } from "../src/adapters.mjs";
import { SqliteFs } from "../src/disk.mjs";
import { gitCommand } from "../src/git.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { bareEnv, MODEL, script, startHost, tempDir } from "./support/client.mjs";
import { git, gitMissing, startGitServer } from "./support/git-server.mjs";

const skip = gitMissing() && `skipped: ${gitMissing()}. These tests need a real git remote that asks for credentials.`;
const THREAD = "5b1e0c9e-7a34-4c53-9d0e-0f6c1a2b3c4d/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/** Not a real token: a marker that must reach the remote and appear nowhere else. */
const AUTH = { username: "scout-bot", password: "ghp_NotARealTokenOnlyAMarkerForThisTest0000" };
const WRONG = { username: "scout-bot", password: "ghp_AWrongMarkerThatTheRemoteRefuses11111" };
/** Every form the secret travels or could be kept in. */
const forms = ({ username, password }) => [password, Buffer.from(`${username}:${password}`).toString("base64"), Buffer.from(`:${password}`).toString("base64"), encodeURIComponent(password), Buffer.from(password).toString("base64")];
const authorization = ({ username, password }) => `Basic ${Buffer.from(`${username}:${password}`).toString("base64")}`;

test("a private remote: bzz is asked when the remote asks, once per git command, and the answer is used for it", { skip }, async (t) => {
	const root = tempDir(t, "cred");
	const folder = join(root, "agent");
	const remote = await startGitServer(t, join(root, "srv"), { auth: AUTH });
	// A second private remote that the owner did not list.
	const unlisted = await startGitServer(t, join(root, "srv2"), { auth: AUTH });
	const host = startHost(t, folder, { home: root });
	let answer = () => AUTH;
	host.on["_bzz/credential"] = (params) => answer(params);
	await host.initialize({ network: [remote.host] });
	const session = await host.session(THREAD);
	let n = 0;
	const run = async (command) => {
		await host.prompt(session, `e${++n}`, script(["bash", { command: `{ ${command}; } 2>&1; echo "exit $?"` }], ["report"]));
		return host.tools.at(-1).arguments.text;
	};

	// The right ones: the remote asks, bzz is asked for that host on the turn's session, the clone succeeds.
	assert.match(await run(`git clone ${remote.origin} && cat project/notes.txt`), /Cloned into '\/home\/agent\/project'\nalpha\nbeta\ngamma\nexit 0\n/);
	assert.deepEqual(host.asked, [{ method: "_bzz/credential", params: { sessionId: session, host: remote.host } }]);
	// The remote was first asked without credentials, then with them, and with them for the pack.
	assert.deepEqual(remote.requests.map((request) => request.authorization), [undefined, authorization(AUTH), authorization(AUTH)]);

	// A push asks again: nothing was kept from the clone.
	assert.match(await run("cd project && echo delta >> notes.txt && git commit -am 'agent: delta' && git push origin main"), /main -> main\nexit 0\n/);
	assert.equal(host.asked.length, 2);
	assert.equal(git(remote.bare, "show", "main:notes.txt"), "alpha\nbeta\ngamma\ndelta");
	// So does a fetch. A command that talks to no remote asks nothing.
	assert.match(await run("cd project && git fetch origin && git log --oneline -1 && git status --short"), /Fetched origin\n[0-9a-f]{7} agent: delta\nexit 0\n/);
	assert.equal(host.asked.length, 3);

	// Wrong ones: the remote refuses, git says so, and bzz is not asked a second time for the same command.
	answer = () => WRONG;
	const before = remote.requests.length;
	const refused = await run("cd project && git pull origin main");
	assert.match(refused, new RegExp(`git: ${remote.host.replace(".", "\\.")} refused the credentials bzz has for it\\.\nexit 128\n`));
	assert.equal(host.asked.length, 4);
	assert.deepEqual(remote.requests.slice(before).map((request) => request.authorization), [undefined, authorization(WRONG)]);
	// Half an answer is no answer.
	answer = () => ({ username: "scout-bot" });
	assert.match(await run("cd project && git fetch origin"), /asks for credentials, and bzz's answer held none\.\nexit 128\n/);

	// None: bzz answers with an error, and git passes its reason on. A clone that failed leaves nothing behind.
	answer = () => { throw Object.assign(new Error("No credential is stored for this host. The owner can add one with `bzz agent credential`."), { code: -32001 }); };
	const none = await run(`git clone ${remote.origin} second; ls -1`);
	assert.match(none, /asks for credentials, and bzz gave none: No credential is stored for this host\. The owner can add one with `bzz agent credential`\.\n/);
	// `ls` shows the first clone only.
	assert.match(none, /^project\ngit: /);
	// A bzz that does not know the request at all.
	delete host.on["_bzz/credential"];
	assert.match(await run("cd project && git fetch origin"), /bzz gave none: Method not found: _bzz\/credential\nexit 128\n/);
	host.on["_bzz/credential"] = (params) => answer(params);

	// A host that is not on the allow-list never causes the request: nothing is sent to it, so it never asks.
	answer = () => AUTH;
	const asked = host.asked.length;
	assert.match(await run(`git clone ${unlisted.origin} third`), /is not on this agent's allow-list/);
	assert.equal(host.asked.length, asked);
	assert.deepEqual(unlisted.requests, []);

	// The model can make a file that is found in the search below: the search sees what is on the disk.
	await run("echo CONTROL-MARKER-ON-THE-DISK > project/control.txt && cat project/.git/config");
	assert.match(host.tools.at(-1).arguments.text, new RegExp(`url = ${remote.origin}\n`));
	assert.deepEqual(await host.rpc("_bzz/checkpoint"), {});
	await host.stop();

	// Afterwards the secret is in neither file, nor in the log, nor in anything the host wrote on the pipe.
	assert.deepEqual(readdirSync(folder).sort(), ["files.sqlite", "memory.sqlite"]);
	const written = { "files.sqlite": readFileSync(join(folder, "files.sqlite")), "memory.sqlite": readFileSync(join(folder, "memory.sqlite")), "the log": Buffer.from(host.stderr()), "the pipe": Buffer.from(host.lines.join("\n")) };
	for (const [name, bytes] of Object.entries(written))
		for (const secret of [...forms(AUTH), ...forms(WRONG)]) assert.equal(bytes.includes(secret), false, `${name} holds the secret, as ${secret}`);
	assert.equal(written["files.sqlite"].includes("CONTROL-MARKER-ON-THE-DISK"), true);
	assert.equal(written["memory.sqlite"].includes("CONTROL-MARKER-ON-THE-DISK"), true);
	assert.equal(written["memory.sqlite"].includes("refused the credentials"), true);
	assert.equal(written["the pipe"].includes(`"_bzz/credential"`), true);
});

test("a cancelled turn does not wait for bzz's answer about credentials", { skip }, async (t) => {
	const root = tempDir(t, "cred-cancel");
	const remote = await startGitServer(t, join(root, "srv"), { auth: AUTH });
	const host = startHost(t, join(root, "agent"), { home: root });
	let asked;
	const waiting = new Promise((resolve) => { asked = resolve; });
	// bzz never answers: the owner has a question on his screen and is away.
	host.on["_bzz/credential"] = () => void asked();
	await host.initialize({ network: [remote.host] });
	const session = await host.session(THREAD);
	const turn = host.prompt(session, "e1", script(["bash", { command: `git clone ${remote.origin}` }], ["report"]));
	await waiting;
	host.notify("session/cancel", { sessionId: session });
	assert.deepEqual(await turn, { stopReason: "cancelled" });
	assert.deepEqual(await host.prompt(session, "e2", script(["bash", { command: "ls -1 | wc -l" }], ["report"])), { stopReason: "end_turn" });
	assert.equal(host.tools.at(-1).arguments.text.trim(), "0");
});

test("under buzz-acp there is nobody to ask: git says that this copy has no credentials", { skip }, async (t) => {
	const root = tempDir(t, "cred-remote");
	const folder = join(root, "agent");
	const remote = await startGitServer(t, join(root, "srv"), { auth: AUTH });
	mkdirSync(folder);
	writeFileSync(join(folder, "agent.json"), JSON.stringify({ name: "Scout", model: MODEL, network: [remote.host] }));
	const ledger = join(root, "ledger.jsonl");
	const fakeBuzz = new URL("./support/fake-buzz.mjs", import.meta.url).pathname;
	const host = startHost(t, folder, { home: root, env: bareEnv(root, { PI_CODING_AGENT_DIR: join(root, "pi"), BZZ_AGENT_HOST_BUZZ: JSON.stringify([process.execPath, fakeBuzz]), FAKE_BUZZ_LEDGER: ledger }) });
	await host.rpc("initialize", { protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: "buzz-acp", version: "1.0.0" } });
	const session = (await host.rpc("session/new", { cwd: root, mcpServers: [] })).sessionId;
	const prompt = readFileSync(new URL("../../tests/fixtures/agents/buzz-acp-prompts/01-top-level-mention.txt", import.meta.url), "utf8");
	await host.rpc("session/prompt", { sessionId: session, prompt: [{ type: "text", text: `${prompt}\ndo: ${JSON.stringify([["bash", { command: `git clone ${remote.origin} 2>&1; echo "exit $?"` }], ["report"]])}` }] });
	const posted = JSON.parse(readFileSync(ledger, "utf8").trim().split("\n").at(-1)).stdin;
	assert.match(posted, /asks for credentials, and this copy of the agent has none: it runs without bzz, which is where they are kept\.\nexit 128\n/);
	// buzz-acp was asked nothing, and the remote was tried once, without credentials.
	assert.deepEqual(host.asked, []);
	assert.deepEqual(remote.requests.map((request) => request.authorization), [undefined]);
});

test("a remote that is not on the list is never asked about, even if its answer came through", async (t) => {
	// The allow-list stops the request itself (network.test.mjs). Here the request gets through by a fault planted in
	// the transport, and the remote asks for credentials: the question to bzz has its own check.
	const disk = new SqliteFs(join(tempDir(t, "cred-list"), "files.sqlite"));
	t.after(() => disk.close());
	const vfs = new Vfs(disk);
	vfs.mkdir("/home/agent", { recursive: true });
	const asked = [];
	const sent = [];
	const http = { async request({ url, headers }) {
		sent.push([url, headers.Authorization]);
		return { url, statusCode: 401, statusMessage: "Unauthorized", headers: { "www-authenticate": "Basic" }, body: [new TextEncoder().encode("who are you\n")] };
	} };
	const credential = async (host) => (asked.push(host), AUTH);
	const bash = new Bash({ fs: shellFs(vfs), cwd: "/home/agent", customCommands: [gitCommand(vfs, { http, check: network(["listed.example"]).check, author: { name: "a", email: "a@b.invalid" }, credential })] });
	const other = await bash.exec("git clone https://other.example/r.git");
	assert.match(other.stderr, /other\.example is not on this agent's allow-list/);
	assert.deepEqual(asked, []);
	// The listed one is asked about, by the name the list knows it by, and a remote that echoes what it was sent does not get the secret printed.
	http.request = async ({ url, headers }) => (sent.push([url, headers.Authorization]), { url, statusCode: headers.Authorization ? 403 : 401, statusMessage: `Forbidden for ${headers.Authorization}`, headers: {}, body: [] });
	const listed = await bash.exec("git clone https://listed.example/r.git");
	assert.deepEqual(asked, ["listed.example"]);
	assert.equal(sent.at(-1)[1], authorization(AUTH));
	assert.equal(listed.stderr, "git: HTTP Error: 403 Forbidden for Basic ***\n");
});
