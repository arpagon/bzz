// The host as a remote copy, under Buzz's `buzz-acp` on another machine.
// There bzz is absent: the place and the request are read from the text of
// buzz-acp's prompt, and messages go out through the `buzz` command, which
// has the agent's key in its environment because Buzz puts it there.
// tests/fixtures/agents/buzz-acp-prompts holds prompts recorded at the pinned
// Buzz; the test that reads them is what tells us Buzz reworded its prompt.
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

const UUID = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const EVENT = "[0-9a-f]{64}";

/** Name, model and instructions of a moved agent, written by `agent move` and `agent launch-kit`. */
export async function readAgentFile(folder) {
	let text;
	try {
		text = await readFile(join(folder, "agent.json"), "utf8");
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
		throw new Error("This folder has no agent.json: it was not prepared by `bzz agent move` or `bzz agent launch-kit`, and the client sent no settings.");
	}
	const parsed = JSON.parse(text.replace(/^\uFEFF/, ""));
	if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("agent.json is not a JSON object");
	return {
		agent: { name: typeof parsed.name === "string" && parsed.name ? parsed.name : "agent" },
		model: parsed.model && typeof parsed.model === "object" ? parsed.model : undefined,
		instructions: typeof parsed.instructions === "string" ? parsed.instructions : "",
	};
}

/**
 * The place and the request in a prompt of `buzz-acp`.
 *
 * The place comes only from the `<context>` section, which Buzz writes itself and escapes. The request is the event
 * id under the last event header. Buzz does not escape message text inside the event section, so a message can carry
 * lines shaped like a header: it can point this reading at another event of the same channel, never at another channel.
 *
 * @returns `{ place, request, channel, root }`, or undefined when the text is not such a prompt
 */
export function readBuzzPrompt(text) {
	const context = /^<context>\n([\s\S]*?)\n<\/context>$/m.exec(text)?.[1];
	if (!context) return undefined;
	const scope = /^Scope: (\w+)$/m.exec(context)?.[1];
	// A channel's name comes before its id and may itself hold text in brackets: the id is what ends the line.
	const channel = new RegExp(`^Channel: (?:.* \\(#(${UUID})\\)|(${UUID}))$`, "m").exec(context);
	const headers = [...text.matchAll(new RegExp(`^(?:<buzz-event [^>\\n]*>|--- Event \\d+ \\([^\\n]*\\) ---)\\nEvent ID: (${EVENT})$`, "gm"))];
	const request = headers.at(-1)?.[1];
	if (!channel || !request) return undefined;
	const id = channel[1] ?? channel[2];
	const root = new RegExp(`^Thread root: (${EVENT})$`, "m").exec(context)?.[1];
	// As bzz names places: a DM is its channel; a thread is the channel and its root; a top-level mention starts a thread.
	const place = scope === "dm" ? id : `${id}/${root ?? request}`;
	return { place, request, channel: id, root: scope === "dm" ? root : (root ?? request) };
}

/** The command to run: `buzz` on the PATH, or what the launch kit names, as a path or a JSON array with first arguments. */
function buzzCommand(env) {
	const named = env.BZZ_AGENT_HOST_BUZZ;
	if (!named) return ["buzz"];
	return named.trimStart().startsWith("[") ? JSON.parse(named) : [named];
}

/** Runs `buzz` with this process's environment and returns what it printed. The environment is never logged. */
function buzz(env, args, input) {
	const [program, ...first] = buzzCommand(env);
	return new Promise((resolve, reject) => {
		const child = execFile(program, [...first, ...args], { env, encoding: "utf8", maxBuffer: 4 * 1024 * 1024, timeout: 60_000, windowsHide: true }, (error, stdout, stderr) => {
			if (!error) return resolve(stdout);
			const why = error.code === "ENOENT" ? "the `buzz` command was not found" : (stderr.trim().split("\n").at(-1) || error.message);
			reject(new Error(`buzz ${args[0]} ${args[1]} failed: ${why}`));
		});
		child.stdin.on("error", () => undefined);
		child.stdin.end(input ?? "");
	});
}

const refuse = (text) => ({ text, isError: true });

/**
 * The three chat tools of a remote copy, with the answers bzz gives locally (protocol, section 5).
 * @param turn `{ request, channel, root }` of the conversation's current turn
 * @param sent the final reply already recorded for this request, if any
 * @returns `{ text, final?, isError?, event? }`
 */
export async function buzzTool(env, name, args, turn, sent) {
	if (name === "send") {
		const text = args.text ?? args.message ?? args.content;
		if (typeof text !== "string" || !text) return refuse("send needs `text`: the message to post, as a string.");
		if (args.final !== undefined && typeof args.final !== "boolean") return refuse("`final` must be true or false.");
		if (args.mentions !== undefined && !(Array.isArray(args.mentions) && args.mentions.every((key) => typeof key === "string"))) return refuse("`mentions` must be a list of pubkeys.");
		// One final reply per request: a turn cut off after posting asks again and gets the first one back.
		if (args.final && sent) return { text: `already sent as ${sent}`, final: true };
		// The text goes through standard input, so it never appears in a process list.
		const out = await buzz(env, ["messages", "send", "--channel", turn.channel, "--reply-to", turn.request, "--content", "-", ...(args.mentions ?? []).flatMap((key) => ["--mention", key])], text);
		let event;
		try {
			event = JSON.parse(out).event_id;
		} catch {}
		event ??= /[0-9a-f]{64}/.exec(out)?.[0] ?? "unknown";
		return { text: `sent as ${event}`, event, ...(args.final ? { final: true } : {}) };
	}
	if (name === "read") {
		const limit = args.limit ?? 20;
		if (!Number.isInteger(limit) || limit < 1 || limit > 100) return refuse("`limit` must be a whole number from 1 to 100.");
		if (args.before !== undefined) return refuse("`before` is not available on this machine: the `buzz` command reads the newest messages only. Call read without it.");
		const where = turn.root ? ["messages", "thread", "--channel", turn.channel, "--event", turn.root] : ["messages", "get", "--channel", turn.channel];
		return { text: await buzz(env, [...where, "--limit", String(limit)]) };
	}
	if (name === "react") {
		if (typeof args.emoji !== "string" || !args.emoji) return refuse("react needs `emoji`: one emoji, as a string.");
		const event = args.event ?? turn.request;
		if (typeof event !== "string" || !new RegExp(`^${EVENT}$`).test(event)) return refuse("`event` must be an event id: 64 hex characters.");
		await buzz(env, ["reactions", "add", "--event", event, "--emoji", args.emoji]);
		return { text: `reacted to ${event}` };
	}
	return refuse(`There is no chat tool named ${name}.`);
}
