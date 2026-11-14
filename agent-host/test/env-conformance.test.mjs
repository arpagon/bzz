// Pi Durable's own suite for execution environments, run on the agent's disk
// and shell. It is the proof that Pi's `read`, `write`, `edit` and `bash`
// tools get what they expect from this environment.
import assert from "node:assert/strict";
import { join } from "node:path";
import { after, describe, it } from "node:test";
import { createEnvConformance } from "@earendil-works/pi-durable/testing";
import { SqliteFs } from "../src/disk.mjs";
import { BzzVfsEnv, createShell } from "../src/env.mjs";
import { network } from "../src/network.mjs";
import { Vfs } from "../src/vfs.mjs";
import { tempDir } from "./support/client.mjs";

/** The suite's assertions on Node's own. `deepEqual` there is Jest's `toEqual`: a field that is undefined equals one that is absent. */
const plain = (value) => (value !== null && typeof value === "object" && !(value instanceof Uint8Array) ? JSON.parse(JSON.stringify(value)) : value);
const assertions = {
	ok: (value, message) => assert.ok(value, message),
	strictEqual: (actual, expected) => assert.strictEqual(actual, expected),
	deepEqual: (actual, expected) => assert.deepStrictEqual(plain(actual), plain(expected)),
	partialDeepEqual: (actual, expected) => assert.partialDeepStrictEqual(actual, expected),
	greaterThan: (actual, expected) => assert.ok(actual > expected, `${actual} is not greater than ${expected}`),
	rejects: (operation, messageIncludes) => assert.rejects(operation, (error) => String(error?.message).includes(messageIncludes)),
};

describe("Pi Durable's environment suite on the agent's disk", () => {
	let n = 0;
	const cases = createEnvConformance({
		assertions,
		async withEnv(use) {
			const disk = new SqliteFs(join(work, `${++n}.sqlite`));
			const vfs = new Vfs(disk);
			const cwd = "/home/agent/case";
			vfs.mkdir(cwd, { recursive: true });
			try {
				await use(new BzzVfsEnv(vfs, createShell(vfs, { net: network([]), author: { name: "agent", email: "agent@bzz.invalid" } }), { agent: "conformance", cwd }));
				assert.deepEqual(disk.check(), [], "the disk after the case");
			} finally {
				disk.close();
			}
		},
	});
	// `tempDir` removes its folder when what it was given ends: here that is the whole suite.
	const work = tempDir({ after }, "conformance");
	it("is the whole suite", () => assert.ok(cases.length >= 24, `${cases.length} cases`));
	for (const testCase of cases) it(testCase.name, { timeout: testCase.timeoutMs ?? 30_000 }, testCase.run);
});
