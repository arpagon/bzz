// The network allow-list. `git` and the shell's `curl` reach only the hosts
// the owner listed; everything else is refused before a byte leaves, and a
// redirect is not a way around it. The "network" here is two servers on this
// machine: one listed, one not, which counts every request that reaches it.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { join } from "node:path";
import test from "node:test";
import { SqliteFs } from "../src/disk.mjs";
import { createShell } from "../src/env.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { tempDir } from "./support/client.mjs";

async function serve(t, handler) {
	const hits = [];
	const server = createServer((request, response) => {
		hits.push(`${request.method} ${request.url}`);
		handler(request, response);
	});
	await new Promise((done) => server.listen(0, "127.0.0.1", done));
	t.after(() => new Promise((done) => {
		server.closeAllConnections();
		server.close(done);
	}));
	return { host: `127.0.0.1:${server.address().port}`, hits };
}

async function world(t) {
	const other = await serve(t, (_request, response) => response.end("the other host was reached\n"));
	const listed = await serve(t, (request, response) => {
		if (request.url === "/hello") return response.end("hello from the listed host\n");
		if (request.url === "/back") return response.writeHead(302, { location: "/hello" }).end();
		if (request.url.startsWith("/out")) return response.writeHead(302, { location: `http://${other.host}/secret` }).end();
		if (request.url === "/echo") return request.pipe(response);
		response.writeHead(404).end("nothing here\n");
	});
	const disk = new SqliteFs(join(tempDir(t, "net"), "files.sqlite"));
	t.after(() => disk.close());
	const bash = createShell(new Vfs(disk), { net: network([listed.host]), author: { name: "agent", email: "agent@bzz.invalid" } });
	const sh = async (command) => {
		const result = await bash.exec(command);
		return { out: result.stdout, err: result.stderr, code: result.exitCode };
	};
	return { listed, other, sh };
}

test("curl reaches a listed host and nothing else", async (t) => {
	const { listed, other, sh } = await world(t);
	assert.deepEqual(await sh(`curl -s http://${listed.host}/hello`), { out: "hello from the listed host\n", err: "", code: 0 });
	assert.equal((await sh(`curl -s -X POST -d 'sent up' http://${listed.host}/echo`)).out, "sent up");
	const refused = await sh(`curl -sS http://${other.host}/secret`);
	assert.notEqual(refused.code, 0);
	assert.equal(refused.out, "");
	assert.match(refused.err, new RegExp(`${other.host.replace(".", "\\.")} is not on this agent's allow-list`));
	for (const command of [`curl -sS http://${other.host}/a | cat`, `curl -sS -o got.txt http://${other.host}/b; cat got.txt`, `curl -sS "http://${listed.host}@${other.host}/c"`, `curl -sS http://localhost:${other.host.split(":")[1]}/d`])
		assert.equal((await sh(command)).out.includes("reached"), false, command);
	assert.deepEqual(other.hits, []);
});

test("a redirect to a host that is not listed is refused before it is followed", async (t) => {
	const { listed, other, sh } = await world(t);
	assert.equal((await sh(`curl -sL http://${listed.host}/back`)).out, "hello from the listed host\n");
	const out = await sh(`curl -sSL http://${listed.host}/out`);
	assert.notEqual(out.code, 0);
	assert.equal(out.out.includes("reached"), false);
	assert.match(out.err, /is not on this agent's allow-list/);
	assert.deepEqual(other.hits, []);
	assert.deepEqual(listed.hits, ["GET /back", "GET /hello", "GET /out"]);
});

test("git is held to the same list, redirects included", async (t) => {
	const { listed, other, sh } = await world(t);
	const direct = await sh(`git clone http://${other.host}/project.git`);
	assert.notEqual(direct.code, 0);
	assert.match(direct.err, /is not on this agent's allow-list/);
	const bounced = await sh(`git clone http://${listed.host}/out/project.git`);
	assert.notEqual(bounced.code, 0);
	assert.match(bounced.err, /is not on this agent's allow-list/);
	assert.deepEqual(other.hits, []);
	assert.equal(listed.hits.length, 1);
	// Nothing is left of either clone.
	assert.equal((await sh("ls -A")).out, "");
});

test("the list is exact: https, that host, that port", async (t) => {
	const sent = [];
	const realFetch = globalThis.fetch;
	globalThis.fetch = async (url) => {
		sent.push(String(url));
		return new Response("ok");
	};
	t.after(() => {
		globalThis.fetch = realFetch;
	});
	const net = network(["github.com", "Example.org:8443"]);
	for (const url of ["https://github.com/arpagon/bzz", "https://example.org:8443/x"]) assert.equal((await net.request(url)).status, 200, url);
	for (const url of ["http://github.com/arpagon/bzz", "https://api.github.com/", "https://github.com.evil.example/", "https://github.com:8443/", "https://example.org/", "https://github.com@evil.example/",
		"https://evil.example/?github.com", "ssh://git@github.com/arpagon/bzz.git", "git@github.com:arpagon/bzz.git", "file:///etc/passwd", "http://127.0.0.1/", "http://169.254.169.254/latest/meta-data/"])
		await assert.rejects(net.request(url), /^Error: network: /, url);
	assert.deepEqual(sent, ["https://github.com/arpagon/bzz", "https://example.org:8443/x"]);
	await assert.rejects(network([]).request("https://github.com/"), /allow-list \(it is empty\)/);
	assert.match(String(await net.request("ssh://git@github.com/x").catch((error) => error)), /SSH remotes are not supported/);
	// An entry is a host, not a URL or a pattern.
	for (const entry of ["https://github.com", "*.github.com", "github.com/arpagon", "", 7]) assert.throws(() => network([entry]), /takes host names/, String(entry));
});

test("credentials given for one host do not follow a redirect to another", async (t) => {
	const seen = [];
	const realFetch = globalThis.fetch;
	globalThis.fetch = async (url, init) => {
		seen.push([String(url), init.headers.authorization]);
		return String(url).includes("first") ? new Response(null, { status: 302, headers: { location: "https://second.example/x" } }) : new Response("ok");
	};
	t.after(() => {
		globalThis.fetch = realFetch;
	});
	await network(["first.example", "second.example"]).request("https://first.example/x", { headers: { authorization: "Basic abc" } });
	assert.deepEqual(seen, [["https://first.example/x", "Basic abc"], ["https://second.example/x", undefined]]);
});
