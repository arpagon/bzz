// Pi packages (docs/agent-host-protocol.md, `_meta.bzz.packages`): the Pi
// Durable entries of the packages in Pi's settings bring providers and
// extensions. The packages here are written into temporary folders; Pi's
// folder is a temporary one too, never the real `~/.pi`.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { openModels } from "../src/model.mjs";
import { loadPackages, NO_ENTRY, packagesNaming } from "../src/packages.mjs";
import { bareEnv, MAIN, MODEL, script, sleep, startHost, tempDir, WAIT_MARGIN_MS, within } from "./support/client.mjs";

const THREAD = "5b1e0c9e-7a34-4c53-9d0e-0f6c1a2b3c4d/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/** A provider package, in TypeScript: a model that answers through the chat tool, so that its answer is seen by bzz. */
const PROVIDER = `
import type { Provider } from "@earendil-works/pi-ai";
import { fauxAssistantMessage, fauxProvider, fauxToolCall } from "@earendil-works/pi-ai/providers/faux";

const faux = fauxProvider({ provider: "acme", models: [{ id: "acme-1" }] });
const answer = () => fauxAssistantMessage(fauxToolCall("buzz_send", { text: "answered by acme-1", final: true }), { stopReason: "toolUse" });
faux.setResponses(Array(64).fill(answer));
const provider: Provider = faux.provider;
export default async () => ({ providers: [provider] });
`;
/** A dependency the guard package installed in its own folder, in TypeScript: Node itself strips no types under `node_modules`. */
const WORDS = `export const count = (text: string): number => text.trim().split(/\\s+/).filter(Boolean).length;\n`;
/**
 * An extension package: a tool, a prompt section, and a hook that blocks a command. Asked to, it prints, as packages
 * do. Pi's own packages it has not installed: it gets the host's.
 */
const GUARD = `
import { defineExtension, defineTool, hook, section, ToolTask } from "@earendil-works/pi-durable";
import { obj, str } from "@earendil-works/pi-schema";
import { count } from "words";

if (process.env.GUARD_PRINTS) {
	console.log("the guard package printed this with console.log");
	process.stdout.write("the guard package wrote this to standard output\\n");
}
const wordCount = defineTool({
	name: "word_count",
	description: "Count the words in a text",
	parameters: obj({ text: str() }),
	execute: async (args) => ({ output: [{ type: "text", text: \`\${count(args.text)} words\` }] }),
});
export default () => ({
	extensions: [defineExtension({
		name: "guard",
		sections: [section("house-rules", () => "House rule: never delete files recursively.")],
		tools: [wordCount],
		hooks: [hook(ToolTask, { beforeTool: (call) => (call.name === "bash" && /\\brm\\s+-\\w*r/.test(String(call.arguments?.command ?? "")) ? { block: "The guard package blocks recursive deletes" } : undefined) })],
	})],
});
`;
const extension = (name, tools = "") => `import { defineExtension, defineTool } from "@earendil-works/pi-durable";
const tool = (name) => defineTool({ name, description: name, parameters: { type: "object", properties: {} }, execute: async () => ({ output: [{ type: "text", text: "a package's tool ran" }] }) });
export default () => ({ extensions: [defineExtension({ name: ${JSON.stringify(name)}, tools: [${tools}] })] });`;

/** Writes one package folder: `files` by name, and a package.json with `pi` as given. */
function writePackage(root, name, pi, files = {}) {
	const dir = join(root, "packages", name);
	mkdirSync(dir, { recursive: true });
	writeFileSync(join(dir, "package.json"), JSON.stringify({ name, private: true, type: "module", ...(pi && { pi }) }));
	for (const [file, text] of Object.entries(files)) {
		mkdirSync(dirname(join(dir, file)), { recursive: true });
		writeFileSync(join(dir, file), text);
	}
	return dir;
}

/** A Pi folder whose settings name `packages`. */
function writeSettings(root, packages, more = {}) {
	const pi = join(root, "pi");
	mkdirSync(pi, { recursive: true });
	writeFileSync(join(pi, "settings.json"), `\uFEFF${JSON.stringify({ packages, ...more })}`);
	return pi;
}

/** The packages most tests use: a provider, a tool, one that throws, one without a Pi Durable entry, one from npm. */
function world(t) {
	const root = tempDir(t, "packages");
	const acme = writePackage(root, "acme", { durable: ["./durable.ts"] }, { "durable.ts": PROVIDER });
	writePackage(root, "guard", { durable: ["./src/durable.mjs"] }, {
		"src/durable.mjs": GUARD,
		"node_modules/words/package.json": JSON.stringify({ name: "words", type: "module", exports: "./index.ts" }),
		"node_modules/words/index.ts": WORDS,
	});
	const throws = writePackage(root, "throws", { durable: ["./durable.mjs"] }, { "durable.mjs": `export default () => { throw new Error("boom at setup\\nwith a second line that is not passed on"); };` });
	// Written for the \`pi\` command only: it registers a provider there, and has nothing for this host.
	const piOnly = writePackage(root, "pi-acme-cloud", { extensions: ["./extensions/index.ts"] }, { "extensions/index.ts": `export default (pi) => { pi.registerProvider("acme-cloud", { baseUrl: "https://llm.example.invalid/v1", models: [] }); };` });
	// The first by its full path, the second relative to Pi's folder, as \`pi install\` may write either.
	const sources = [acme, "../packages/guard", throws, { source: piOnly }, "npm:pi-something"];
	const pi = writeSettings(root, sources);
	const report = [
		{ source: acme, loaded: true, extensions: [], providers: ["acme"] },
		{ source: "../packages/guard", loaded: true, extensions: ["guard"], providers: [] },
		{ source: throws, loaded: false, error: "boom at setup" },
		{ source: piOnly, loaded: false, error: NO_ENTRY },
		{ source: "npm:pi-something", loaded: false, error: "Only Pi packages in a local folder are loaded" },
	];
	return { root, pi, acme, throws, piOnly, report };
}

test("providers and extensions come from the Pi Durable entries of the packages in Pi's settings", async (t) => {
	const { root, pi, report } = world(t);
	const packages = await loadPackages(pi);
	assert.deepEqual(packages.report, report);
	assert.deepEqual(packages.providers.map((provider) => provider.id), ["acme"]);
	assert.deepEqual(packages.extensions.map((extension) => [extension.name, extension.tools.map((tool) => tool.name), extension.sections.map((entry) => entry.key)]), [["guard", ["word_count"], ["house-rules"]]]);
	// Their providers join Pi's own.
	const { models, choose } = await openModels({ piDir: pi, env: {}, packages });
	assert.deepEqual((await choose({ provider: "acme", modelId: "acme-1" })).model, { provider: "acme", modelId: "acme-1" });
	assert.ok(models.getProvider("openai"), "Pi's own providers are still there");
	// No settings, no list, an empty list: nothing, and no complaint.
	assert.deepEqual((await loadPackages(join(root, "no-such-folder"))).report, []);
	assert.deepEqual((await loadPackages(writeSettings(tempDir(t, "pi"), undefined))).report, []);
	assert.deepEqual((await loadPackages(writeSettings(tempDir(t, "pi"), []))).report, []);
});

test("a package that fails is left out with the reason, and the others load", async (t) => {
	const root = tempDir(t, "bad-packages");
	const pkg = (name, source, pi = { durable: ["./durable.mjs"] }) => writePackage(root, name, pi, { "durable.mjs": source });
	const cases = [
		[pkg("hangs", "export default () => new Promise(() => {});"), /^It took over 1\.5 seconds to load$/],
		[pkg("hangs-at-import", "await new Promise(() => {});\nexport default () => ({});"), /^It took over 1\.5 seconds to load$/],
		[pkg("throws-at-import", `throw new Error("boom at import");`), /^boom at import$/],
		[pkg("rejects", `export default async () => { throw new Error("boom, later"); };`), /^boom, later$/],
		[pkg("no-function", "export default 5;"), /durable\.mjs has no default export to call/],
		[pkg("not-a-list", "export default () => ({});", { durable: "./durable.mjs" }), /"pi\.durable" in its package\.json must list files/],
		[pkg("missing-file", "export default () => ({});", { durable: ["./absent.mjs"] }), /Cannot find module/],
		[join(root, "packages", "absent"), /There is no package\.json in that folder/],
		["git:github.com/someone/pi-thing", /Only Pi packages in a local folder are loaded/],
		["https://example.invalid/pkg.tgz", /Only Pi packages in a local folder are loaded/],
		[42, /Only Pi packages in a local folder are loaded/],
		// Names that are bzz's or Pi's own.
		[pkg("named-bzz", extension("bzz")), /Another extension is named bzz/],
		[pkg("named-coding-tools", extension("coding-tools")), /Another extension is named coding-tools/],
		[pkg("tool-run", extension("hands", `tool("run")`)), /Its tool run has the name of one of bzz's own tools/],
		[pkg("tool-buzz-send", extension("chatty", `tool("ok"), tool("buzz_send")`)), /Its tool buzz_send has the name of one of bzz's own tools/],
		[pkg("bad-section", `import { defineExtension, section } from "@earendil-works/pi-durable";\nexport default () => ({ extensions: [defineExtension({ name: "shouty", sections: [section("NOT LOWERCASE", () => "x")] })] });`), /Section key/],
		[pkg("no-name", "export default () => ({ extensions: [{ tools: [] }] });"), /An extension it returns has no name/],
		// Providers the host could not list or use.
		[pkg("half-provider", `export default () => ({ providers: [{ id: "half", name: "Half" }] });`), /A provider it returns needs an id, name, auth and getModels/],
		[pkg("no-models", `export default () => ({ providers: [{ id: "empty", name: "Empty", auth: { apiKey: {} }, getModels: () => [] }] });`), /Its provider empty lists no models/],
	];
	// Two that are fine, first and last: a failure between them stops nothing. The last takes a name the first has.
	const first = pkg("first", extension("first", `tool("first_tool")`));
	const twin = pkg("twin", extension("first"));
	const last = pkg("last", extension("last", `tool("last_tool")`));
	const pi = writeSettings(root, [first, ...cases.map(([source]) => source), twin, last]);
	const reserved = (name) => name === "run" || name.startsWith("buzz_");
	const at = Date.now();
	const packages = await loadPackages(pi, { limit: 1_500, reservedTools: reserved });
	// The packages load at the same time: two that hang cost one time limit, not two.
	const took = Date.now() - at;
	assert.ok(took >= 1_450 && took < 2_800, `it took ${took} ms`);
	assert.deepEqual(packages.report[0], { source: first, loaded: true, extensions: ["first"], providers: [] });
	assert.deepEqual(packages.report.at(-1), { source: last, loaded: true, extensions: ["last"], providers: [] });
	assert.deepEqual(packages.report.at(-2), { source: twin, loaded: false, error: "Another extension is named first" });
	for (const [index, [source, pattern]] of cases.entries()) {
		const entry = packages.report[index + 1];
		assert.equal(entry.source, typeof source === "string" ? source : JSON.stringify(source));
		assert.equal(entry.loaded, false, String(source));
		assert.match(entry.error, pattern, String(source));
	}
	// Nothing of a package that was left out is installed: not the extension that was fine before the one that was not.
	assert.deepEqual(packages.extensions.map((entry) => entry.name), ["first", "last"]);
	assert.deepEqual(packages.providers, []);
});

test("a provider that is missing is traced to a package without a Pi Durable entry, when one names it", async (t) => {
	const { pi, piOnly, throws } = world(t);
	const packages = await loadPackages(pi);
	assert.deepEqual(packages.withoutEntry.map((entry) => entry.source), [piOnly]);
	assert.deepEqual(await packagesNaming("acme-cloud", packages.withoutEntry), [piOnly]);
	// A name is a whole quoted string: part of one does not count.
	assert.deepEqual(await packagesNaming("acme", packages.withoutEntry), []);
	assert.deepEqual(await packagesNaming("registerProvider", packages.withoutEntry), []);
	const { choose } = await openModels({ piDir: pi, env: {}, packages });
	await assert.rejects(choose({ provider: "acme-cloud", modelId: "x" }), (error) => {
		assert.deepEqual(error.data, { kind: "provider_unavailable", package: piOnly });
		assert.equal(error.message, `The provider "acme-cloud" is not available: it is not one of Pi's built-in providers, and no Pi package loaded for this agent adds it. The package ${piOnly} in Pi's settings names "acme-cloud" but has no Pi Durable entry ("pi": {"durable": [...]} in its package.json), so this host can load nothing from it. The package ${throws} did not load: boom at setup. The package npm:pi-something did not load: Only Pi packages in a local folder are loaded. Choose another model for this agent.`);
		return true;
	});
	await assert.rejects(choose({ provider: "nobody", modelId: "x" }), (error) => {
		assert.deepEqual(error.data, { kind: "provider_unavailable" });
		assert.match(error.message, /No package in Pi's settings without a Pi Durable entry names "nobody"\. Choose another model for this agent\.$/);
		return true;
	});
});

test("in the host: a package's model answers, its tool, section and hook work, and bzz is told what loaded", async (t) => {
	const { root, pi, report, piOnly } = world(t);
	const start = () => startHost(t, join(root, "agent"), { home: root, env: bareEnv(root, { GUARD_PRINTS: "1" }) });
	let host = start();
	const answer = await host.initialize({ model: { provider: "acme", modelId: "acme-1" }, piDir: pi });
	assert.deepEqual(answer._meta.bzz.packages, report);
	// The turn runs on the package's provider.
	assert.deepEqual(await host.prompt(await host.session(THREAD), "e1"), { stopReason: "end_turn" });
	assert.deepEqual(host.tools.map((call) => call.arguments), [{ text: "answered by acme-1", final: true }]);
	// What the package printed went to the log. On the pipe every line is a message: the client parsed them all.
	assert.match(host.stderr(), /the guard package printed this with console\.log\n/);
	assert.match(host.stderr(), /the guard package wrote this to standard output\n/);
	assert.match(host.stderr(), /package \.\.\/packages\/guard loaded: extensions \[guard\], providers \[\]\n/);
	assert.match(host.stderr(), /package \S+throws was left out: boom at setup\n/);
	assert.equal(host.lines.some((line) => line.includes("the guard package")), false);
	await host.stop();

	// On the stand-in model, so that the test chooses the tool calls.
	host = start();
	await host.initialize({ piDir: pi });
	const session = await host.session(THREAD);
	const turn = async (request, ...steps) => {
		await host.prompt(session, request, script(...steps));
		return host.tools.at(-1).arguments.text;
	};
	assert.equal(await turn("e2", ["word_count", { text: "one two  three" }], ["report"]), "3 words");
	// The hook stops the call before the shell sees it.
	assert.match(await turn("e3", ["bash", { command: "mkdir x" }], ["bash", { command: "rm -rf x; echo it ran" }], ["report"]), /The guard package blocks recursive deletes/);
	assert.equal(await turn("e4", ["bash", { command: "ls -d x" }], ["report"]), "x\n");
	const told = JSON.parse(await turn("e5", ["describe"]));
	assert.match(told.system, /House rule: never delete files recursively\./);
	assert.deepEqual(told.tools, ["read", "write", "edit", "bash", "word_count", "buzz_send", "buzz_read", "buzz_react"]);

	// A model that is not available: the error says which package names its provider without an entry for this host.
	const failure = async (model) => {
		const other = startHost(t, join(tempDir(t, "other"), "agent"), { home: root });
		await other.initialize({ model, piDir: pi });
		return other.prompt(await other.session(THREAD), "e1").catch((error) => error);
	};
	const missing = await failure({ provider: "acme-cloud", modelId: "big" });
	assert.equal(missing.code, -32010);
	assert.deepEqual(missing.data, { kind: "provider_unavailable", package: piOnly });
	assert.match(missing.message, /^The provider "acme-cloud" is not available: .* The package \S+pi-acme-cloud in Pi's settings names "acme-cloud" but has no Pi Durable entry /);
	assert.deepEqual((await failure({ provider: "acme", modelId: "acme-9" })).data, { kind: "model_unavailable" });
});

test("what a package leaves uncaught does not end the host, and the other packages work", async (t) => {
	const root = tempDir(t, "stray");
	const pkg = (name, source) => writePackage(root, name, { durable: ["./durable.mjs"] }, { "durable.mjs": source });
	// A package that loads, and leaves three things behind: a promise that fails at once, one that fails later with
	// no file of the package in its stack (as a fetch at start does on a machine that is offline), and a timer that throws.
	const stray = pkg("stray", `${extension("stray", `tool("stray_tool")`)}
Promise.reject(new Error("rejected at import, with nobody waiting"));
import("node:fs/promises").then((fs) => fs.readFile(${JSON.stringify(join(root, "no-such-file"))}));
setTimeout(() => { throw new Error("thrown from a timer of the package"); }, 40);
`);
	const fine = pkg("fine", extension("fine", `tool("fine_tool")`));
	const pi = writeSettings(root, [stray, fine]);
	const host = startHost(t, join(root, "agent"), { home: root });
	const answer = await host.initialize({ piDir: pi });
	assert.deepEqual(answer._meta.bzz.packages, [{ source: stray, loaded: true, extensions: ["stray"], providers: [] }, { source: fine, loaded: true, extensions: ["fine"], providers: [] }]);
	await within(5_000, (async () => { while (!host.stderr().includes("thrown from a timer")) await sleep(20); })(), () => `the timer's exception in the log\n${host.stderr()}`);
	// Each is in the log, with the package where the stack names it, and the host is still there.
	assert.match(host.stderr(), / a promise failed with nobody waiting for it in package \S+stray, and the host goes on: rejected at import, with nobody waiting\n/);
	assert.match(host.stderr(), / a promise failed with nobody waiting for it, and the host goes on: ENOENT: no such file or directory, open \S+no-such-file'\n/);
	assert.match(host.stderr(), / an uncaught exception in package \S+stray, and the host goes on: thrown from a timer of the package\n/);
	assert.equal(host.child.exitCode, null);
	const session = await host.session(THREAD);
	for (const [request, tool] of [["e1", "fine_tool"], ["e2", "stray_tool"]]) {
		await host.prompt(session, request, script([tool, {}], ["report"]));
		assert.equal(host.tools.at(-1).arguments.text, "a package's tool ran");
	}
	await host.stop();

	// The limit, shown as it is: an exception that no stack traces to a package is taken for the host's own, and ends it.
	const silent = pkg("throws-a-string", `setTimeout(() => { throw "a string, which names no file"; }, 40);\nexport default () => ({});`);
	const ended = startHost(t, join(tempDir(t, "ended"), "agent"), { home: root });
	// The package's timer runs from its import, inside `initialize`: on a slow machine it fires before the host has
	// answered, and the host ends with the request unanswered (it did in CI, on macOS on Intel). Both orders are
	// the same end, so the answer is not waited for; the time allowed covers the host's start, hence the margin.
	ended.initialize({ piDir: writeSettings(tempDir(t, "pi"), [silent]) }).catch(() => {});
	assert.deepEqual(await within(5_000 + WAIT_MARGIN_MS, ended.exited, () => `the host to end\n${ended.stderr()}`), { code: 1, signal: null });
	assert.match(ended.stderr(), /a string, which names no file\n$/);
});

test("bzz can switch package loading off for an agent, and a moved agent keeps the switch", async (t) => {
	const { root, pi, report } = world(t);
	const host = startHost(t, join(root, "agent"), { home: root });
	const answer = await host.initialize({ packages: false, piDir: pi });
	assert.deepEqual(answer._meta.bzz.packages, []);
	assert.equal(host.stderr().includes("the guard package"), false, "no code of a package ran");
	assert.match(host.stderr(), /, packages off, /);
	const session = await host.session(THREAD);
	const told = JSON.parse((await host.prompt(session, "e1", script(["describe"])), host.tools.at(-1).arguments.text));
	assert.deepEqual(told.tools, ["read", "write", "edit", "bash", "buzz_send", "buzz_read", "buzz_react"]);
	assert.equal(told.system.includes("House rule"), false);
	await host.prompt(session, "e2", script(["word_count", { text: "one two" }], ["report"]));
	assert.match(host.tools.at(-1).arguments.text, /word_count/);
	assert.equal(host.tools.at(-1).arguments.text.includes("2 words"), false);
	await host.stop();
	// The provider of a package is not there either, and the error says why.
	const off = startHost(t, join(tempDir(t, "off"), "agent"), { home: root });
	await off.initialize({ packages: false, model: { provider: "acme", modelId: "acme-1" }, piDir: pi });
	await assert.rejects(off.prompt(await off.session(THREAD), "e1"), { code: -32010, message: /no Pi package loaded for this agent adds it\. Pi packages are switched off for this agent\. Choose another model/ });

	// A remote copy reads Pi's settings on the machine it runs on, and `"packages": false` in agent.json keeps them off.
	const remote = (agentFile) => {
		const folder = join(tempDir(t, "remote"), "agent");
		mkdirSync(folder);
		writeFileSync(join(folder, "agent.json"), JSON.stringify({ name: "Scout", model: MODEL, ...agentFile }));
		const copy = startHost(t, folder, { home: root, env: bareEnv(root, { PI_CODING_AGENT_DIR: pi }) });
		return copy.rpc("initialize", { protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: "buzz-acp", version: "1.0.0" } });
	};
	assert.deepEqual((await remote({}))._meta.bzz, { host: answer._meta.bzz.host, tools: "buzz", packages: report, run: [] });
	assert.deepEqual((await remote({ packages: false }))._meta.bzz.packages, []);
});

/**
 * `node main.mjs --check-model <Pi's folder> <model as JSON> [--no-packages]`, as bzz runs it before an agent
 * exists. Its arguments and the one line it prints are the contract with bzz, which imports nothing of `src/`.
 */
function checkModel(env, ...args) {
	return within(30_000 + WAIT_MARGIN_MS, new Promise((resolve, reject) => {
		execFile(process.execPath, [MAIN, "--check-model", ...args], { env }, (error, stdout, stderr) => {
			if (error) return reject(new Error(`--check-model ended with ${error.code ?? error.signal}: ${stderr}`));
			const lines = stdout.split("\n").filter(Boolean);
			resolve({ lines, answer: JSON.parse(lines.at(-1)), stderr });
		});
	}), "--check-model");
}

test("`main.mjs --check-model` answers, without an agent, whether a model can be used and which package names a missing provider", async (t) => {
	const { root, pi, piOnly, report } = world(t);
	const env = bareEnv(root, { GUARD_PRINTS: "1" });
	const ask = async (wanted, ...flags) => (await checkModel(env, pi, JSON.stringify(wanted), ...flags)).answer;

	// The stand-in model, and a model of a provider that only a package brings.
	assert.deepEqual(await ask(MODEL), { ok: true, provider: "scripted", modelId: "scripted", thinkingLevel: "off", packages: report });
	const first = await checkModel(env, pi, JSON.stringify({ provider: "acme", modelId: "acme-1" }));
	assert.deepEqual(first.answer, { ok: true, provider: "acme", modelId: "acme-1", thinkingLevel: "off", packages: report });
	// One line on standard output, whatever a package prints while it loads.
	assert.equal(first.lines.length, 1);
	assert.match(first.stderr, /the guard package printed this with console\.log/);

	// A provider nothing has: with the package that names it and has no entry for this host.
	const missing = await ask({ provider: "acme-cloud", modelId: "big" });
	assert.deepEqual([missing.ok, missing.kind, missing.package], [false, "provider_unavailable", piOnly]);
	assert.match(missing.message, /in Pi's settings names "acme-cloud" but has no Pi Durable entry/);
	const nobody = await ask({ provider: "nobody-has-this", modelId: "big" });
	assert.deepEqual([nobody.ok, nobody.kind, nobody.package], [false, "provider_unavailable", undefined]);
	assert.match(nobody.message, /No package in Pi's settings without a Pi Durable entry names "nobody-has-this"\./);
	// The other reasons, by their kind.
	assert.equal((await ask({ provider: "acme", modelId: "acme-9" })).kind, "model_unavailable");
	assert.equal((await ask({ provider: "openai-codex", modelId: "gpt-6-luna" })).kind, "sign_in_needed");
	assert.equal((await ask({})).kind, "no_model");

	// Switched off: no code of a package runs, and its provider is not there.
	const off = await checkModel(env, pi, JSON.stringify({ provider: "acme", modelId: "acme-1" }), "--no-packages");
	assert.deepEqual([off.answer.ok, off.answer.kind, off.answer.packages], [false, "provider_unavailable", []]);
	assert.match(off.answer.message, /Pi packages are switched off for this agent\./);
	assert.equal(off.stderr.includes("the guard package"), false);

	// What cannot be asked is said in the same form. Asking leaves a Pi folder that is not there as it was.
	assert.deepEqual(await checkModel(env, pi, "not json").then(({ answer }) => [answer.ok, answer.kind]), [false, "error"]);
	assert.deepEqual(await checkModel(env, pi).then(({ answer }) => [answer.ok, answer.kind]), [false, "error"]);
	const nowhere = join(root, "no-pi-here");
	assert.equal((await ask(MODEL)).ok, true);
	assert.equal((await checkModel(env, nowhere, JSON.stringify({ provider: "openai", modelId: "gpt-5" }))).answer.kind, "sign_in_needed");
	assert.equal(existsSync(nowhere), false);
});
