// The host's ACP face (docs/agent-host-protocol.md). One process serves one
// agent: its memory is Pi Durable on memory.sqlite, its disk is files.sqlite.
// bzz, or Buzz's `buzz-acp` for a remote copy, is the client on the pipe.
import { randomUUID } from "node:crypto";
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { createRegistry, defineDoc, defineExtension, defineTool, GenerationTask, Harness, hook, section, ToolTask } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import { normalize, SqliteFs } from "./disk.mjs";
import { BzzVfsEnv, calling, createShell, HOME } from "./env.mjs";
import { defaultPiDir, ModelUnavailable, openModels } from "./model.mjs";
import { network } from "./network.mjs";
import { loadPackages, noPackages } from "./packages.mjs";
import { buzzTool, readAgentFile, readBuzzPrompt } from "./remote.mjs";
import { createRun, RUN_UNAVAILABLE } from "./run.mjs";
import { connect, RpcError } from "./rpc.mjs";
import { Vfs } from "./vfs.mjs";

export const VERSION = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
/** bzz allows 2 s between closing our input and TERM. */
const CLOSE_MS = 1_500;
/** The model sees the chat tools under this prefix: Pi's own file tools already own the name `read`. */
const PREFIX = "buzz_";
const INVALID = -32602;
/**
 * A turn that failed. A code of this host's own, in the range JSON-RPC leaves to a server: ACP gives -32000 the
 * meaning "sign in" and -32002 "not found", and a turn fails for other reasons too. `data.kind` says which.
 */
export const TURN_FAILED = -32010;
/** bzz's own tool names: a Pi package may not take them. */
const reservedTool = (name) => typeof name === "string" && (name.startsWith(PREFIX) || name === "run");

/**
 * Whether a model can be used, asked without an agent: what `node main.mjs --check-model` answers, for `bzz agent
 * new --model` before anything exists. It is the check a turn makes, with the providers a turn would have: Pi's own
 * and, unless `packages` is `false`, those of the Pi packages in Pi's settings, whose code is run to learn them.
 * @param wanted `{ provider?, modelId?, thinkingLevel? }`, as in `_meta.bzz.model`
 * @returns `{ ok: true, provider, modelId, thinkingLevel, packages }`, or `{ ok: false, kind, message, package?,
 *   packages }`, where `package` names a package without a Pi Durable entry that names the provider, and
 *   `packages` is the report `initialize` would give
 */
export async function checkModel({ piDir, wanted = {}, packages: on = true, env = process.env }) {
	let report = [];
	try {
		const packages = on === false ? noPackages(true) : await loadPackages(piDir, { reservedTools: reservedTool });
		report = packages.report;
		const { choose } = await openModels({ piDir, env, packages });
		const chosen = await choose(wanted);
		return { ok: true, provider: chosen.model.provider, modelId: chosen.model.modelId, thinkingLevel: chosen.thinkingLevel ?? null, packages: report };
	} catch (error) {
		const known = error instanceof ModelUnavailable;
		return { ok: false, kind: known ? error.kind : "error", message: String(error?.message ?? error), ...(known && error.data.package !== undefined && { package: error.data.package }), packages: report };
	}
}

const checkpointWhen = (_value, _ops, { deltasSinceBase }) => deltasSinceBase >= 64;
const doc = (kind) => defineDoc({ kind, version: 1, scope: "session", initial: () => ({}), checkpointWhen });
/** place -> conversation. This is what finds a conversation again after any restart, whatever the session id. */
const Places = doc("bzz.places");
/** conversation -> the request its current turn answers, and for a remote copy where to post. */
const Turns = doc("bzz.turns");
/** Remote copy only: request -> the final reply posted for it, kept until the turn settles. */
const Replies = doc("bzz.replies");

const CHAT_TOOLS = [
	{
		name: "send",
		description: "Post a message in this conversation. People see only what you post with this tool. Set `final` to true on the message that ends your turn.",
		parameters: {
			type: "object",
			properties: {
				text: { type: "string", description: "The message, in Markdown." },
				final: { type: "boolean", description: "True when this message ends your turn." },
				mentions: { type: "array", items: { type: "string" }, description: "Pubkeys of people to notify." },
			},
			required: ["text"],
		},
		// Models often call the text `message` or `content` (seen with Codex in M0).
		prepareArguments(args) {
			if (!args || typeof args !== "object" || args.text !== undefined) return args;
			const { message, content, ...rest } = args;
			const text = message ?? content;
			return text === undefined ? args : { ...rest, text };
		},
	},
	{
		name: "read",
		description: "Read recent messages of this conversation, newest last.",
		parameters: {
			type: "object",
			properties: {
				limit: { type: "integer", minimum: 1, maximum: 100, description: "How many messages. Default 20." },
				before: { type: "string", description: "An event id: read the messages before it." },
			},
		},
	},
	{
		name: "react",
		description: "Add an emoji reaction to a message.",
		parameters: {
			type: "object",
			properties: {
				emoji: { type: "string", description: "One emoji." },
				event: { type: "string", description: "The event id of the message. Default: the message you are answering." },
			},
			required: ["emoji"],
		},
	},
];
const TOOL_KIND = { read: "read", write: "edit", edit: "edit", bash: "execute", run: "execute" };

/**
 * Serves one agent on `input` and `output` until the input ends.
 * @param folder the agent's folder: memory.sqlite, files.sqlite and, for a remote copy, agent.json
 * @param env this process's environment. It is read for Pi's folder and handed to `buzz`; never logged
 * @returns `closed`, settled once storage is closed
 */
export function serve({ folder, input, output, env = process.env, log = () => undefined }) {
	mkdirSync(folder, { recursive: true, mode: 0o700 });
	/** Set by `initialize`: what bzz sent, or what agent.json holds. */
	let config;
	let disk;
	let vfs;
	let shell;
	let models;
	/** Built by `initialize`: what the tools and the prompt hold depends on what bzz grants. */
	let registry;
	/** The `run` tool, when the owner granted a folder with `exec`. Otherwise it does not exist. */
	let run;
	/** The open Harness, as a promise. A checkpoint closes it; the next turn opens it again. */
	let opening;
	const sessions = new Map();
	/** conversation -> the session its tool calls and updates travel on. */
	const live = new Map();
	const waiting = new Map();
	let running = 0;
	let whenIdle = [];
	let checkpointing = Promise.resolve();

	const need = () => {
		if (!config) throw new RpcError(-32600, "initialize comes first");
		return config;
	};

	function attach(conversationId, sessionId) {
		live.set(conversationId, sessionId);
		for (const resolve of waiting.get(conversationId) ?? []) resolve(sessionId);
		waiting.delete(conversationId);
	}

	/**
	 * The session a conversation's turn is on. After a restart a cut-off turn runs again before bzz has handed its
	 * request over once more: its tool calls wait here for that session.
	 */
	function sessionOf(conversationId, signal) {
		const now = live.get(conversationId);
		if (now && sessions.has(now)) return Promise.resolve(now);
		let waiter;
		const arrived = new Promise((resolve) => {
			waiter = resolve;
			if (!waiting.has(conversationId)) waiting.set(conversationId, new Set());
			waiting.get(conversationId).add(resolve);
		});
		return stoppable(arrived, signal).finally(() => waiting.get(conversationId)?.delete(waiter));
	}

	/** `promise`, or a refusal as soon as the turn is stopped: a cancelled turn does not wait for an answer from bzz. */
	const stoppable = (promise, signal) => (signal ? Promise.race([promise, new Promise((_, reject) => {
		if (signal.aborted) reject(new Error("the turn was stopped"));
		else signal.addEventListener("abort", () => reject(new Error("the turn was stopped")), { once: true });
	})]) : promise);

	const update = (conversationId, change) => {
		const sessionId = live.get(conversationId);
		if (sessionId) peer.notify("session/update", { sessionId, update: change });
	};

	/** One chat tool call: over the pipe to bzz, or with the `buzz` command in a remote copy. */
	async function chat(name, args, api, context) {
		let result;
		try {
			if (config.local) {
				const sessionId = await sessionOf(api.conversationId, context.abortSignal);
				// bzz finds the agent from this process and the turn from the session: the arguments never say which mention.
				result = await stoppable(peer.request("_bzz/tool", { sessionId, name, arguments: args }), context.abortSignal);
			} else {
				const harness = await opening;
				const turn = (await harness.snapshot(Turns, ctx))?.[api.conversationId];
				if (!turn) throw new Error("this conversation has no request to answer");
				result = await buzzTool(env, name, args, turn, (await harness.snapshot(Replies, ctx))?.[turn.request]);
				// Recorded after the post: a crash between the two can still repeat a reply. Under bzz, bzz's ledger closes that.
				if (name === "send" && args.final && result.event) await harness.commit(async (tx) => void ((await tx.doc(Replies))[turn.request] = result.event), ctx);
			}
		} catch (error) {
			result = { text: `${name} failed: ${error?.message ?? error}`, isError: true };
		}
		return {
			output: [{ type: "text", text: String(result?.text ?? "") }],
			...(result?.isError === true && { isError: true }),
			...(result?.final === true && { control: { terminate: true } }),
		};
	}

	/**
	 * Asked by the agent's `git` when a host on the allow-list wants to know who is calling. bzz answers for the
	 * session, as for a tool. The answer goes to that one git command and is kept nowhere: it is not logged, and it
	 * never enters what the model sees.
	 */
	async function credential(host) {
		const call = calling.getStore();
		if (!call?.conversation) throw new Error("git was not started from a conversation");
		const sessionId = await sessionOf(call.conversation, call.signal);
		return stoppable(peer.request("_bzz/credential", { sessionId, host }), call.signal);
	}

	/**
	 * Asked by `run` before a real command starts. bzz decides, and may put the command to the owner. What is sent
	 * is the command as it will be handed to the shell, and nothing else is started.
	 */
	async function approve({ conversationId, mount, command }, signal) {
		const sessionId = await sessionOf(conversationId, signal);
		return stoppable(peer.request("_bzz/approve", { sessionId, kind: "exec", mount, command }), signal);
	}

	/** Keeps a long output of `run` on the agent's disk and returns where. */
	function saveOutput(bytes) {
		const path = `/tmp/run-${randomUUID().slice(0, 8)}.log`;
		vfs.tx(() => {
			vfs.mkdir("/tmp", { recursive: true });
			vfs.writeFile(path, bytes);
		});
		return path;
	}

	/**
	 * Off unless BZZ_AGENT_HOST_RECORD names a folder: then a remote copy saves each prompt of buzz-acp there. It is
	 * how tests/fixtures/agents/buzz-acp-prompts is recorded again at a new Buzz pin, with disposable identities.
	 */
	let recorded = 0;
	function record(text) {
		if (!env.BZZ_AGENT_HOST_RECORD) return;
		mkdirSync(env.BZZ_AGENT_HOST_RECORD, { recursive: true });
		writeFileSync(join(env.BZZ_AGENT_HOST_RECORD, `prompt-${process.pid}-${++recorded}.txt`), text);
	}

	const title = (call) => {
		const hint = call.name === "bash" || call.name === "run" ? call.arguments?.command : call.arguments?.path;
		const where = call.name === "run" && typeof call.arguments?.folder === "string" ? ` in /mnt/${call.arguments.folder.replace(/^\/mnt\//, "")}` : "";
		return typeof hint === "string" ? `${call.name}${where}: ${hint.split("\n")[0].slice(0, 80)}` : call.name;
	};

	/** What the agent has, told to the model briefly and as it is: its own disk, which folders, which it may write, and whether it has `run`. */
	function sandbox() {
		const grants = vfs.describeGrants();
		const hosts = config.network;
		return [
			`You are ${config.agent.name ?? "an agent"}, an agent in a team chat. People see only what you post with \`${PREFIX}send\`: your plain text is not shown to anyone. Set \`final\` to true on the message that ends your turn.`,
			`The chat tools are \`${PREFIX}send\`, \`${PREFIX}read\` and \`${PREFIX}react\`. Where your instructions name a chat tool such as \`send\`, call \`${PREFIX}send\`.`,
			`You have a disk of your own: \`read\`, \`write\`, \`edit\` and \`bash\` work on it. ${HOME} is yours and is kept between conversations. \`bash\` is a sandbox: its commands are built in, no other program can be installed or run in it, and a command's output arrives when it ends.`,
			grants.length ? `Folders of the owner's machine granted to you, which the same tools reach: ${grants.join(", ")}. Nothing else of that machine is visible to them.` : "No folder of the owner's machine is granted to you.",
			...(run ? [`\`run\` starts a real command of the owner's machine in ${run.names.map((name) => `/mnt/${name}`).join(", ")}: use it for what the sandbox lacks, such as a build or the tests. The owner may be asked before a command starts and can refuse it; a refusal comes back as the tool's error. Copy what you want to keep to your own disk.`] : []),
			hosts.length ? `Network: \`git\` (https only) and \`curl\` reach only ${hosts.join(", ")}.` : "Network: no host is allowed, so `git clone`, `git push` and `curl` to the network are refused.",
			"`git` covers init, clone, status, add, commit, log, diff, branch, checkout, merge without conflicts, fetch, pull and push. It has no rebase, no stash and no SSH remotes.",
		].join("\n");
	}

	/**
	 * What every conversation of this agent runs with: Pi's file tools, then what the Pi packages bring, then bzz's
	 * own tools and section. A later tool or section replaces an earlier one of the same name, so bzz's are last.
	 */
	function buildRegistry(packages) {
		const made = createRegistry();
		made.install(CodingTools);
		for (const extension of packages.extensions) made.install(extension);
		made.install(defineExtension({
			name: "bzz",
			tools: [
				...CHAT_TOOLS.map(({ name, ...tool }) => defineTool({ ...tool, name: `${PREFIX}${name}`, execute: (args, api, context) => chat(name, args, api, context) })),
				// Without a folder granted for it, the tool is not there, and nothing tells the model of it.
				...(run ? [run.tool] : []),
			],
			sections: [section("bzz", sandbox)],
			hooks: [
				hook(GenerationTask, {
					// bzz keeps the last plain text of a turn: it is the reply when the model never calls send.
					afterResponse(message, api) {
						const text = message.content.filter((part) => part.type === "text").map((part) => part.text).join("");
						if (text.trim()) update(api.conversationId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text } });
					},
				}),
				hook(ToolTask, {
					beforeTool(call, api) {
						update(api.conversationId, { sessionUpdate: "tool_call", toolCallId: call.id, title: title(call), kind: TOOL_KIND[call.name] ?? "other", status: "in_progress" });
					},
					afterTool(call, result, api) {
						update(api.conversationId, { sessionUpdate: "tool_call_update", toolCallId: call.id, status: result.isError ? "failed" : "completed" });
					},
				}),
			],
		}));
		return made;
	}

	function ready() {
		opening ??= (async () => {
			const path = join(folder, "memory.sqlite");
			const harness = await Harness.open(await openNodeSqliteStorage(path), {
				models: models.models,
				registry,
				// One agent, one disk: every conversation sees the same files.
				env: ({ cwd, conversationId }) => new BzzVfsEnv(vfs, shell, { agent: "disk", cwd: cwd ?? HOME, conversation: conversationId }),
				// A message that arrives while a turn runs joins it. A failed turn does not leave later ones waiting for ever.
				settings: { steeringMode: "all", afterFailedRun: "withdraw" },
				onReport: (report) => log(`pi reported ${report.kind}: ${String(report.error?.message ?? "").slice(0, 300)}`),
			}, ctx);
			// The agent's memory and files are its owner's alone. Windows keeps its own rules; chmod changes little there.
			for (const file of [path, join(folder, "files.sqlite")]) try { chmodSync(file, 0o600); } catch {}
			// Work a kill cut off continues now, with or without a client asking.
			harness.resume();
			return harness;
		})();
		return opening;
	}

	async function close() {
		const harness = await opening?.catch(() => undefined);
		opening = undefined;
		await harness?.close(ctx);
	}

	/** How a settled submission ends its `session/prompt`. */
	function answer(settled, turn) {
		if (settled.status === "done") return { stopReason: "end_turn" };
		if (turn.cancelled || settled.reason === "aborted") return { stopReason: "cancelled" };
		if (settled.reason === "turn_limit") return { stopReason: "max_turn_requests" };
		const detail = typeof settled.detail === "string" ? settled.detail : settled.detail?.message ?? settled.detail?.errorMessage;
		throw new RpcError(TURN_FAILED, `The turn failed: ${detail ?? settled.reason}`, { kind: settled.reason });
	}

	async function cancel(turn) {
		if (!turn.submission) return;
		// A queued input is withdrawn; one a run already took is stopped with that run.
		if ((await turn.submission.abort(ctx)) === "already_placed") await turn.conversation.abort(ctx);
	}

	async function prompt(session, sessionId, where, text) {
		// Known to `session/cancel` from the first moment, before there is anything to stop.
		const turn = { cancelled: false };
		session.turns.add(turn);
		try {
			const harness = await ready();
			const known = (await harness.snapshot(Places, ctx))?.[where.place];
			let conversation = known !== undefined ? await harness.conversation(known, ctx) : undefined;
			// The request is the idempotency key: handed over again, it is the same submission and nothing is written for it.
			const before = conversation && (await harness.commit((tx) => tx.submissionByRequest(conversation.id, where.request), ctx));
			turn.conversation = conversation;
			if (before) {
				attach(conversation.id, sessionId);
				turn.submission = await harness.submission(before.id, ctx);
			} else {
				let chosen;
				try {
					chosen = await models.choose(config.model);
				} catch (error) {
					if (error instanceof ModelUnavailable) throw new RpcError(TURN_FAILED, error.message, error.data);
					throw error;
				}
				const agent = { model: chosen.model, thinkingLevel: chosen.thinkingLevel, instructions: config.instructions || null, cwd: session.cwd };
				const here = { request: where.request, ...(where.channel && { channel: where.channel }), ...(where.root && { root: where.root }) };
				if (conversation) await conversation.configure(agent, ctx);
				// Created and placed in one commit: a kill cannot leave a conversation no place leads to.
				else conversation = await harness.createConversation({ ownership: { kind: "ownerless" }, agent, init: async (tx, id) => void ((await tx.doc(Places))[where.place] = id) }, ctx);
				await harness.commit(async (tx) => void ((await tx.doc(Turns))[conversation.id] = here), ctx);
				turn.conversation = conversation;
				attach(conversation.id, sessionId);
				turn.submission = await conversation.submit({ type: "input", content: text, whenBusy: "steer", requestId: where.request }, ctx);
			}
			if (turn.cancelled) await cancel(turn);
			const settled = await turn.submission.wait(ctx);
			if (!config.local) await harness.commit(async (tx) => void delete (await tx.doc(Replies))[where.request], ctx).catch(() => undefined);
			log(`turn ${settled.status}${settled.reason ? ` (${settled.reason})` : ""}`);
			return answer(settled, turn);
		} finally {
			session.turns.delete(turn);
		}
	}

	async function initialize(params) {
		const bzz = params._meta?.bzz;
		// Without bzz's fields this is a remote copy under buzz-acp: the settings moved with the agent.
		const settings = bzz
			? { local: true, agent: bzz.agent ?? {}, model: bzz.model, instructions: bzz.instructions ?? "", mounts: bzz.mounts ?? [], network: bzz.network ?? [], piDir: bzz.piDir, packages: bzz.packages }
			: { local: false, ...(await readAgentFile(folder)), mounts: [] };
		let net;
		try {
			if (!Array.isArray(settings.network)) throw new Error("the network allow-list must be a list of host names");
			net = network(settings.network);
			disk = new SqliteFs(join(folder, "files.sqlite"));
			vfs = new Vfs(disk, settings.mounts);
			// Under buzz-acp there is nobody to ask for credentials: git then says that this copy has none.
			shell = createShell(vfs, { net, author: { name: settings.agent.name ?? "agent", email: "agent@bzz.invalid" }, ...(settings.local && { credential }) });
		} catch (error) {
			throw new RpcError(INVALID, error.message);
		}
		const piDir = settings.piDir ?? defaultPiDir(env);
		// On unless bzz, or agent.json for a remote copy, says `false`: a package's code runs in this process.
		const packages = settings.packages === false ? noPackages(true) : await loadPackages(piDir, { reservedTools: reservedTool });
		for (const entry of packages.report) log(entry.loaded ? `package ${entry.source} loaded: extensions [${entry.extensions.join(", ")}], providers [${entry.providers.join(", ")}]` : `package ${entry.source} was left out: ${entry.error}`);
		models = await openModels({ piDir, env, packages });
		config = { ...settings, network: net.hosts };
		// Hands on this machine: only under bzz, which is asked about each command, and only where the owner said so.
		const hands = settings.local ? vfs.execFolders() : new Map();
		if (hands.size && RUN_UNAVAILABLE) log(`run is not available: ${RUN_UNAVAILABLE}`);
		run = hands.size && !RUN_UNAVAILABLE ? createRun({ folders: hands, approve, save: saveOutput, env, log }) : undefined;
		registry = buildRegistry(packages);
		await ready();
		log(`ready: ${config.local ? "started by bzz" : "remote copy"}, ${settings.mounts.length} granted folder(s), ${net.hosts.length} allowed host(s), ${packages.off ? "packages off" : `${packages.report.filter((entry) => entry.loaded).length} package(s)`}, ${run ? `run in ${run.names.length} folder(s)` : "no run"}`);
		return {
			protocolVersion: 1,
			agentCapabilities: { loadSession: false, promptCapabilities: { image: false, audio: false, embeddedContext: false } },
			agentInfo: { name: "bzz-agent-host", title: "bzz Pi Durable host", version: VERSION },
			authMethods: [],
			_meta: { bzz: { host: VERSION, tools: config.local ? "pipe" : "buzz", packages: packages.report, run: run ? run.names : [] } },
		};
	}

	let initialized = false;
	const methods = {
		async initialize(params) {
			if (initialized) throw new RpcError(-32600, "initialize was already called");
			initialized = true;
			try {
				return await initialize(params);
			} catch (error) {
				// Refused settings can be sent again, corrected: nothing of the failed attempt stays open.
				initialized = false;
				config = undefined;
				registry = undefined;
				run = undefined;
				opening = undefined;
				try { disk?.close(); } catch {}
				throw error;
			}
		},

		async "session/new"(params) {
			const { local } = need();
			const place = params._meta?.["bzz/place"];
			if (local && (typeof place !== "string" || !place)) throw new RpcError(INVALID, 'session/new needs _meta["bzz/place"]');
			// Only bzz names a directory of the agent's disk. buzz-acp sends a path of its own machine, which must not be kept.
			let cwd = HOME;
			if (local && typeof params.cwd === "string" && params.cwd.startsWith("/")) {
				cwd = normalize(params.cwd);
				try { vfs.mkdir(cwd, { recursive: true }); } catch {}
			}
			const sessionId = randomUUID();
			sessions.set(sessionId, { place: local ? place : undefined, cwd, turns: new Set() });
			return { sessionId };
		},

		async "session/prompt"({ sessionId, prompt: blocks, _meta }) {
			const { local } = need();
			const session = sessions.get(sessionId);
			if (!session) throw new RpcError(INVALID, `There is no session ${sessionId}`);
			const text = (Array.isArray(blocks) ? blocks : []).map((block) => (block?.type === "text" ? block.text : "")).filter(Boolean).join("\n");
			let where;
			if (local) {
				const request = _meta?.["bzz/request"];
				if (typeof request !== "string" || !request) throw new RpcError(INVALID, 'session/prompt needs _meta["bzz/request"]');
				where = { place: session.place, request };
			} else {
				where = readBuzzPrompt(text);
				record(text);
				if (!where) throw new RpcError(INVALID, "This prompt names no channel or event. The host reads them from the text of buzz-acp's prompt: Buzz may have changed it.");
			}
			await checkpointing;
			running++;
			try {
				return await prompt(session, sessionId, where, text);
			} finally {
				running--;
				if (running === 0) for (const resolve of whenIdle.splice(0)) resolve();
			}
		},

		/** Before a move, and when the agent is idle: both files whole, no write-ahead file beside them. */
		async "_bzz/checkpoint"() {
			need();
			const previous = checkpointing;
			checkpointing = (async () => {
				await previous;
				// "Finish writes": the turns that run end first; ones that arrive meanwhile wait for the checkpoint.
				if (running > 0) await new Promise((resolve) => whenIdle.push(resolve));
				await close();
				disk.checkpoint();
			})();
			await checkpointing;
			return {};
		},
	};

	const notifications = {
		async "session/cancel"({ sessionId }) {
			for (const turn of sessions.get(sessionId)?.turns ?? []) {
				turn.cancelled = true;
				await cancel(turn);
			}
		},
	};

	const peer = connect({ input, output, methods, notifications, log });
	const closed = peer.closed.then(async () => {
		// The client is gone: nothing more is said to it. Unfinished turns are in storage and continue in the next process.
		peer.stop();
		// A real command does not outlive the host. Its keeper would see the host go; this is the same order, sooner.
		run?.stopAll();
		const timer = new Promise((resolve) => setTimeout(resolve, CLOSE_MS, "late"));
		if ((await Promise.race([close().catch((error) => log(`close failed: ${error?.message ?? error}`)), timer])) === "late") log("storage did not close in time");
		try { disk?.close(); } catch {}
	});
	return { closed, stop: () => peer.end() };
}
