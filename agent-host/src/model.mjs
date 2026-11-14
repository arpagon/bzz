// Which model an agent runs on, and with whose sign-in: Pi's own. bzz stores
// no provider key. The login file and the defaults are read from Pi's folder
// as the `pi` command reads them.
//
// `AuthFile` and `piDefaults` are adapted from Buzz's Pi host
// (block/buzz-app, src/bundled/pi/host.mjs, Apache-2.0); see
// THIRD_PARTY_LICENSES.md. Changed here: a read never creates the file, a
// write leaves it readable by its owner only, and the refusal of `!command`
// and `$NAME` keys says what to do in bzz.
import { chmod, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { clampThinkingLevel } from "@earendil-works/pi-ai";
import { builtinModels } from "@earendil-works/pi-ai/providers/all";
import lockfile from "proper-lockfile";

/** A lock left by a `pi` that was killed is taken over after this long. */
const STALE_MS = 30_000;
/** Pi's own default thinking level, for a model that thinks. */
const THINKING = "medium";
/** Set to a module path, it adds a scripted stand-in model for tests. bzz builds the host's environment from nothing, so it is never set there. */
export const SCRIPTED = "BZZ_AGENT_HOST_SCRIPTED_MODEL";

/** Pi's `auth.json`, read and written under the lock the `pi` command takes, so a token one refreshes works in both. */
export class AuthFile {
	constructor(path) {
		this.path = path;
	}

	async load() {
		let text;
		try {
			text = await readFile(this.path, "utf8");
		} catch (error) {
			if (error.code === "ENOENT") return undefined;
			throw error;
		}
		const parsed = JSON.parse(text.replace(/^\uFEFF/, ""));
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("Pi's auth.json is not a JSON object");
		return parsed;
	}

	/** Every entry, read under the lock: `pi` rewrites the file in place. */
	all() {
		return this.locked((all) => all, false);
	}

	async read(id) {
		const credential = (await this.all())[id];
		return credential?.type === "api_key" && credential.key ? { ...credential, key: this.resolve(credential.key) } : credential;
	}

	async list() {
		return Object.entries(await this.all()).map(([providerId, value]) => ({ providerId, type: value.type }));
	}

	/** As pi-ai's stores do: a change that returns nothing, such as a refresh another process already made, leaves the entry. */
	modify(id, change) {
		return this.locked(async (all) => {
			const next = await change(all[id]);
			if (next !== undefined) all[id] = next;
			return all[id];
		});
	}

	async delete(id) {
		await this.locked((all) => {
			delete all[id];
		});
	}

	/** `pi` can get a key by running a command or from its environment. This host starts no program and has neither. */
	resolve(key) {
		if (/^(?:!|\$(?:\{\w+\}|\w+)$)/.test(key))
			throw new Error("bzz does not use `!command` or `$NAME` keys from Pi's auth.json. Sign in with `pi` (`/login`), or store the key itself there.");
		return key;
	}

	async locked(change, save = true) {
		// Nothing to lock, and a host that only reads leaves Pi's folder as it found it.
		if ((await this.load()) === undefined) {
			if (!save) return change({});
			await writeFile(this.path, "{}", { flag: "wx", mode: 0o600 }).catch((error) => {
				if (error.code !== "EEXIST") throw error;
			});
		}
		const release = await lockfile.lock(this.path, { realpath: false, stale: STALE_MS, retries: { retries: 30, minTimeout: 20, maxTimeout: 2_000 } });
		try {
			const all = (await this.load()) ?? {};
			const result = await change(all);
			if (save) {
				await writeFile(this.path, `${JSON.stringify(all, null, 2)}\n`, { mode: 0o600 });
				// The mode above applies to a new file only. Sign-ins are the owner's alone, whoever made the file.
				await chmod(this.path, 0o600).catch(() => undefined);
			}
			return result;
		} finally {
			await release();
		}
	}
}

/** The default provider, model and thinking level in Pi's settings. */
export async function piDefaults(dir) {
	try {
		const settings = JSON.parse((await readFile(join(dir, "settings.json"), "utf8")).replace(/^\uFEFF/, ""));
		const text = (value) => (typeof value === "string" && value ? value : undefined);
		return { provider: text(settings.defaultProvider), modelId: text(settings.defaultModel), thinkingLevel: text(settings.defaultThinkingLevel) };
	} catch {
		return {};
	}
}

/** Pi's folder when bzz did not name it: where the `pi` command keeps it. */
export const defaultPiDir = (env) => (env.PI_CODING_AGENT_DIR ? env.PI_CODING_AGENT_DIR.replace(/^~(?=$|[\\/])/, homedir()) : join(homedir(), ".pi", "agent"));

/** A turn that cannot start because of the model. `kind` lets bzz name the failure. */
export class ModelUnavailable extends Error {
	constructor(kind, message) {
		super(message);
		this.kind = kind;
	}
}

/**
 * Pi's built-in providers with Pi's own login.
 * @returns `models` for the Harness, and `choose(wanted)` for the model of one turn
 */
export async function openModels({ piDir, env = process.env }) {
	const models = builtinModels({ credentials: new AuthFile(join(piDir, "auth.json")) });
	if (env[SCRIPTED]) await addScripted(models, env[SCRIPTED]);

	/** The model for a turn: the one bzz names, or else Pi's default. Throws `ModelUnavailable` with the reason. */
	async function choose(wanted = {}) {
		const defaults = await piDefaults(piDir);
		const provider = wanted.provider || defaults.provider;
		const modelId = wanted.modelId || (provider === defaults.provider ? defaults.modelId : undefined);
		if (!provider || !modelId)
			throw new ModelUnavailable("no_model", "This agent has no model: none is set for it in bzz, and Pi's settings name no default provider and model.");
		if (!models.getProvider(provider))
			throw new ModelUnavailable("provider_unavailable", `The provider "${provider}" is not available: it is not one of Pi's built-in providers. A provider that comes from a Pi extension works here only if that package has a Pi Durable entry. Choose another model for this agent.`);
		const found = models.getModel(provider, modelId);
		if (!found) throw new ModelUnavailable("model_unavailable", `The provider "${provider}" has no model "${modelId}". Choose another model for this agent.`);
		let auth;
		try {
			auth = await models.checkAuth(provider);
		} catch (error) {
			throw new ModelUnavailable("sign_in_needed", `Pi's sign-in for "${provider}" could not be read: ${error.message}`);
		}
		if (!auth) throw new ModelUnavailable("sign_in_needed", `The provider "${provider}" is not signed in. Run \`pi\` and \`/login\` for it on this machine, then ask again.`);
		return { model: { provider, modelId }, thinkingLevel: clampThinkingLevel(found, wanted.thinkingLevel || defaults.thinkingLevel || THINKING) };
	}

	return { models, choose };
}

/**
 * The stand-in model of the tests: a provider named `scripted` whose answers come from a module, called with what the
 * model would see. Whoever can set this process's environment can already run any code in it (`NODE_OPTIONS`), so
 * loading a module named there gives nothing new.
 */
async function addScripted(models, path) {
	const { fauxAssistantMessage, fauxProvider, fauxText, fauxToolCall } = await import("@earendil-works/pi-ai/providers/faux");
	const { default: respond } = await import(pathToFileURL(path).href);
	const faux = fauxProvider({ provider: "scripted", models: [{ id: "scripted" }] });
	const helpers = { message: fauxAssistantMessage, text: fauxText, call: (name, args) => fauxAssistantMessage(fauxToolCall(name, args), { stopReason: "toolUse" }) };
	const step = (context, options) => {
		if (faux.getPendingResponseCount() < 8) faux.appendResponses(Array(64).fill(step));
		return respond(context, helpers, options);
	};
	faux.setResponses(Array(64).fill(step));
	models.setProvider(faux.provider);
}
