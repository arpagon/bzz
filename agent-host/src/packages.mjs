// Pi packages: providers and extensions written for Pi Durable, taken from
// the packages named in Pi's settings. A package lists its Pi Durable entries
// as `"pi": { "durable": ["./durable.ts"] }` in its package.json, which the
// `pi` command ignores. Each entry's default export returns
// `{ extensions?, providers? }`.
//
// What a package brings runs inside this process, with everything this
// process may do: it is the owner's code, as much as Pi's own. Nothing here
// confines it.
//
// `loadPackages`, `loadPackage`, `isProvider` and the import hook are adapted
// from Buzz's Pi host (block/buzz-app, src/bundled/pi/host.mjs, Apache-2.0);
// see THIRD_PARTY_LICENSES.md. Changed here: the packages load at the same
// time, each under its own time limit; a package may not take the names of
// bzz's own tools; a package without a Pi Durable entry is remembered, so
// that a missing provider can be traced to it; and what loaded is reported
// per package.
import { readFileSync, realpathSync } from "node:fs";
import { readdir, readFile, stat } from "node:fs/promises";
import { registerHooks, stripTypeScriptTypes } from "node:module";
import { homedir } from "node:os";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRegistry } from "@earendil-works/pi-durable";
import { CodingTools } from "@earendil-works/pi-durable/tools";

/**
 * How long a package has to load before it is left out. bzz waits for `initialize` meanwhile, so a package that
 * hangs delays the agent's start by this much, once, and no more: the packages load at the same time.
 */
export const PACKAGE_MS = 10_000;
/** What a package says of itself when it has nothing for this host. */
export const NO_ENTRY = 'It has no Pi Durable entry ("pi": {"durable": [...]} in its package.json)';
/** No package: none is listed, or, with `off`, the owner switched them off for this agent. */
export const noPackages = (off = false) => ({ extensions: [], providers: [], report: [], withoutEntry: [], off });

/** Whether a provider has what listing Pi's providers reads, which one made without pi-ai's `createProvider` may lack. */
const isProvider = (provider) =>
	typeof provider?.id === "string" && typeof provider.name === "string" && Boolean(provider.auth?.apiKey || provider.auth?.oauth) && typeof provider.getModels === "function";

/**
 * Packages get Pi's own packages from this host, so that what they define is what the host runs, and they may be
 * TypeScript, also under `node_modules`, where Node does not strip types itself.
 */
let hooked = false;
function hookPackages() {
	if (hooked) return;
	hooked = true;
	const here = import.meta.url;
	registerHooks({
		resolve(specifier, context, next) {
			if (specifier.startsWith("@earendil-works/"))
				try {
					return next(specifier, { ...context, parentURL: here });
				} catch {}
			return next(specifier, context);
		},
		load(url, context, next) {
			if (!/^file:.*\/node_modules\/.*\.m?ts$/.test(url)) return next(url, context);
			return { format: "module", source: stripTypeScriptTypes(readFileSync(fileURLToPath(url), "utf8")), shortCircuit: true };
		},
	});
}

/** The folders this process has run a package's code from, each with the package's name in Pi's settings. */
const ran = new Map();

/** Whether any package's code is in this process. */
export const packagesRan = () => ran.size > 0;

/**
 * The package an error came from, as Pi's settings name it: the one whose folder is in the error's stack. `undefined`
 * when the stack names none, which says little: a promise that a package left to fail often has no frame of its own.
 */
export function packageOf(error) {
	const stack = typeof error?.stack === "string" ? error.stack : "";
	for (const [root, source] of ran) if (stack.includes(`${pathToFileURL(root).href}/`) || stack.includes(`${root}${sep}`)) return source;
	return undefined;
}

/** What the package in a folder adds, from each of its Pi Durable entries. `undefined` when it has no such entry. */
async function loadPackage(root, source) {
	let manifest;
	try {
		manifest = JSON.parse((await readFile(join(root, "package.json"), "utf8")).replace(/^\uFEFF/, ""));
	} catch (error) {
		throw new Error(error.code === "ENOENT" ? "There is no package.json in that folder" : `Its package.json could not be read: ${error.message}`);
	}
	const entries = manifest?.pi?.durable;
	if (entries === undefined) return undefined;
	if (!Array.isArray(entries) || !entries.every((entry) => typeof entry === "string")) throw new Error('"pi.durable" in its package.json must list files');
	const made = { extensions: [], providers: [] };
	// Node names a module by where its file really is, so that is the name a stack holds.
	let real = root;
	try {
		real = realpathSync(root);
	} catch {}
	ran.set(real, source);
	for (const entry of entries) {
		const { default: setup } = await import(pathToFileURL(resolve(root, entry)).href);
		if (typeof setup !== "function") throw new Error(`${entry} has no default export to call`);
		const { extensions = [], providers = [] } = (await setup()) ?? {};
		made.extensions.push(...extensions);
		made.providers.push(...providers);
	}
	return made;
}

/**
 * Loads the Pi Durable parts of the packages in Pi's settings (`packages` in `settings.json`).
 *
 * Only a package in a local folder is loaded. A package that throws, takes longer than `limit`, or returns something
 * the host cannot install is left out, with the reason; the others still load. A setup that never gives the thread
 * back, such as an endless loop, cannot be timed out: it holds the whole host. What a package leaves uncaught
 * outside its setup, such as a promise nobody awaits, is main.mjs's to survive (`packageOf`, `packagesRan`).
 *
 * @param piDir Pi's folder
 * @param options.limit milliseconds each package has
 * @param options.reservedTools tool names a package may not use: bzz's own
 * @returns `extensions` and `providers` in the order of the settings; `report`, one entry per package, as bzz is
 *   told: `{ source, loaded, extensions?, providers?, error? }`; `withoutEntry`, the local packages that have no Pi
 *   Durable entry, as `{ source, root }`
 */
export async function loadPackages(piDir, { limit = PACKAGE_MS, reservedTools = () => false } = {}) {
	let settings;
	try {
		// As Pi does, with the byte order mark an editor may save.
		settings = JSON.parse((await readFile(join(piDir, "settings.json"), "utf8")).replace(/^\uFEFF/, ""));
	} catch {
		return noPackages();
	}
	const sources = Array.isArray(settings?.packages) ? settings.packages : [];
	if (sources.length === 0) return noPackages();
	hookPackages();

	const attempts = sources.map((item) => {
		const source = typeof item === "string" ? item : item?.source;
		if (typeof source !== "string" || !source || /^(?:npm|git|https?|ssh):|^git@/.test(source))
			return { source: typeof source === "string" ? source : JSON.stringify(item), made: Promise.reject(new Error("Only Pi packages in a local folder are loaded")) };
		const root = resolve(piDir, source.replace(/^~(?=$|[\\/])/, homedir()));
		// A timer the host waits for: a setup that never settles may hold nothing else that keeps Node running.
		let timer;
		const late = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`It took over ${limit / 1000} seconds to load`)), limit); });
		return { source, root, made: Promise.race([loadPackage(root, source), late]).finally(() => clearTimeout(timer)) };
	});
	// Every attempt is awaited below; until then a rejection is not "unhandled".
	for (const attempt of attempts) attempt.made.catch(() => undefined);

	// Extensions are tried against what every agent installs, so that one that cannot install is left out here
	// instead of stopping the agent from opening. bzz's own extension goes in last, under `bzz`.
	const registry = createRegistry();
	registry.install(CodingTools);
	const loaded = noPackages();
	for (const { source, root, made } of attempts) {
		const installed = [];
		try {
			const parts = await made;
			if (parts === undefined) {
				loaded.withoutEntry.push({ source, root });
				loaded.report.push({ source, loaded: false, error: NO_ENTRY });
				continue;
			}
			const { extensions, providers } = parts;
			if (!Array.isArray(extensions) || !Array.isArray(providers)) throw new Error("`extensions` and `providers` must be lists");
			for (const extension of extensions) {
				if (typeof extension?.name !== "string" || !extension.name) throw new Error("An extension it returns has no name");
				if (extension.name === "bzz" || registry.snapshot().extension(extension.name)) throw new Error(`Another extension is named ${extension.name}`);
				const taken = (extension.tools ?? []).find((tool) => reservedTools(tool?.name));
				if (taken) throw new Error(`Its tool ${taken.name} has the name of one of bzz's own tools`);
				registry.install(extension);
				installed.push(extension);
			}
			if (!providers.every(isProvider)) throw new Error("A provider it returns needs an id, name, auth and getModels");
			// The host never refreshes models, so a provider whose models come only from `fetchModels` would have none.
			const empty = providers.find((provider) => !provider.getModels().length);
			if (empty) throw new Error(`Its provider ${empty.id} lists no models, and this host does not fetch a provider's models`);
			loaded.extensions.push(...extensions);
			loaded.providers.push(...providers);
			loaded.report.push({ source, loaded: true, extensions: extensions.map((extension) => extension.name), providers: providers.map((provider) => provider.id) });
		} catch (error) {
			for (const extension of installed) registry.uninstall(extension);
			loaded.report.push({ source, loaded: false, error: String(error instanceof Error ? error.message : error).split("\n")[0].slice(0, 300) });
		}
	}
	return loaded;
}

const SOURCE = new Set([".ts", ".mts", ".js", ".mjs", ".cjs", ".json"]);
const MAX_FILES = 200;
const MAX_BYTES = 1024 * 1024;

/**
 * The packages without a Pi Durable entry whose own files name a provider: its id as a quoted string, as a `pi`
 * extension that registers a provider holds it. The files are read as text and nothing in them is run. It is a guess
 * from the text, and the message built from it says "names", not "provides".
 * @returns their sources, as Pi's settings name them
 */
export async function packagesNaming(provider, withoutEntry) {
	const quoted = [`"${provider}"`, `'${provider}'`, `\`${provider}\``];
	const found = [];
	for (const { source, root } of withoutEntry) {
		let budget = MAX_FILES;
		const names = async (dir) => {
			for (const entry of await readdir(dir, { withFileTypes: true }).catch(() => [])) {
				if (budget <= 0) return false;
				if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
				const path = join(dir, entry.name);
				if (entry.isDirectory()) {
					if (await names(path)) return true;
				} else if (entry.isFile() && SOURCE.has(extname(entry.name))) {
					budget--;
					if (((await stat(path).catch(() => undefined))?.size ?? Infinity) > MAX_BYTES) continue;
					const text = await readFile(path, "utf8").catch(() => "");
					if (quoted.some((form) => text.includes(form))) return true;
				}
			}
			return false;
		};
		if (await names(root)) found.push(source);
	}
	return found;
}
