// Pi's own login and settings, read as the `pi` command reads them: under
// its lock, and without leaving anything behind in Pi's folder.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import lockfile from "proper-lockfile";
import { AuthFile, defaultPiDir, openModels, piDefaults } from "../src/model.mjs";
import { SCRIPTED_MODEL, tempDir } from "./support/client.mjs";

test("a read of Pi's login leaves its folder as it was", async (t) => {
	const dir = join(tempDir(t, "pi"), "agent");
	const auth = new AuthFile(join(dir, "auth.json"));
	// No file, no folder: nothing is signed in, and nothing is created.
	assert.equal(await auth.read("openai"), undefined);
	assert.deepEqual(await auth.list(), []);
	assert.equal(existsSync(dir), false);
	mkdirSync(dir);
	// As an editor may save it, with a byte order mark.
	writeFileSync(join(dir, "auth.json"), `\uFEFF${JSON.stringify({ openai: { type: "api_key", key: "sk-test-not-real" }, "openai-codex": { type: "oauth", access: "a", refresh: "r", expires: 1 } })}`);
	assert.deepEqual(await auth.read("openai"), { type: "api_key", key: "sk-test-not-real" });
	assert.deepEqual(await auth.list(), [{ providerId: "openai", type: "api_key" }, { providerId: "openai-codex", type: "oauth" }]);
	assert.deepEqual(readdirSync(dir), ["auth.json"], "the lock is released and removed");
});

test("a refreshed token is written under Pi's lock, for the owner only", async (t) => {
	const dir = tempDir(t, "pi");
	const path = join(dir, "auth.json");
	writeFileSync(path, JSON.stringify({ "openai-codex": { type: "oauth", access: "old", refresh: "r", expires: 1 } }));
	const auth = new AuthFile(path);
	// While `pi` holds the lock, the host waits for it instead of reading a half-written file.
	const release = await lockfile.lock(path, { realpath: false });
	let done = false;
	const refresh = auth.modify("openai-codex", async (current) => ({ ...current, access: "new" })).then((value) => (done = true, value));
	await new Promise((resolve) => setTimeout(resolve, 150));
	assert.equal(done, false);
	await release();
	assert.equal((await refresh).access, "new");
	assert.equal(JSON.parse(readFileSync(path, "utf8"))["openai-codex"].access, "new");
	// A change that returns nothing, such as a refresh another process already made, leaves the entry.
	assert.equal((await auth.modify("openai-codex", async () => undefined)).access, "new");
	if (process.platform !== "win32") assert.equal(statSync(path).mode & 0o077, 0);
	await auth.delete("openai-codex");
	assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), {});
});

test("a key that Pi would get by running a command or from its environment is refused, with what to do", async (t) => {
	const path = join(tempDir(t, "pi"), "auth.json");
	writeFileSync(path, JSON.stringify({ a: { type: "api_key", key: "!op read secret" }, b: { type: "api_key", key: "$OPENAI_API_KEY" }, c: { type: "api_key", key: "${OPENAI_API_KEY}" } }));
	const auth = new AuthFile(path);
	for (const id of ["a", "b", "c"]) await assert.rejects(auth.read(id), /does not use `!command` or `\$NAME` keys.*Sign in with `pi`/);
});

test("Pi's defaults, and the model of a turn", async (t) => {
	const dir = tempDir(t, "pi");
	assert.deepEqual(await piDefaults(dir), {});
	writeFileSync(join(dir, "settings.json"), `\uFEFF${JSON.stringify({ defaultProvider: "scripted", defaultModel: "scripted", defaultThinkingLevel: "high", packages: ["./x"] })}`);
	assert.deepEqual(await piDefaults(dir), { provider: "scripted", modelId: "scripted", thinkingLevel: "high" });
	// Without the switch for tests there is no stand-in model: a host bzz starts cannot be handed one.
	const plain = await openModels({ piDir: dir, env: {} });
	assert.equal(plain.models.getProvider("scripted"), undefined);
	await assert.rejects(plain.choose({}), { kind: "provider_unavailable" });
	const { choose } = await openModels({ piDir: dir, env: { BZZ_AGENT_HOST_SCRIPTED_MODEL: SCRIPTED_MODEL } });
	assert.deepEqual((await choose({})).model, { provider: "scripted", modelId: "scripted" });
	assert.deepEqual((await choose({ provider: "scripted", modelId: "scripted", thinkingLevel: "low" })).model, { provider: "scripted", modelId: "scripted" });
	// Pi's default model belongs to Pi's default provider: it is not lent to another one.
	await assert.rejects(choose({ provider: "openai" }), { kind: "no_model" });
	assert.equal(defaultPiDir({ PI_CODING_AGENT_DIR: dir }), dir);
});
