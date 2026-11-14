// The stand-in model of the tests, loaded by the host when
// BZZ_AGENT_HOST_SCRIPTED_MODEL names this file. It reads its script from the
// last message a person sent: a line `do: [step, ...]`, one step per model
// call. Without a script it answers through the chat tool with how many
// messages from people its conversation holds, which shows memory.
//
// Steps: ["tool", {arguments}] calls a tool; "say: text" answers in plain
// text; ["sleep", ms] waits, then runs `true`; ["report"] sends the last tool
// result as the final message; ["error", "why"] fails as a provider would;
// ["describe"] sends what the model was given, as JSON: `{ system, tools }`,
// the text of its system prompt and the names of the tools it was offered.
const textOf = (message) => (typeof message.content === "string" ? message.content : message.content.map((part) => part.text ?? "").join(""));

export default async function respond(context, { call, message }, options) {
	const messages = context.messages;
	const lastUser = messages.findLastIndex((entry) => entry.role === "user");
	const people = messages.filter((entry) => entry.role === "user").length;
	const script = /^do: (.*)$/m.exec(textOf(messages[lastUser]))?.[1];
	if (!script) return call("buzz_send", { text: `pong: this conversation holds ${people} message(s) from people`, final: true });
	const steps = JSON.parse(script);
	const step = steps[messages.slice(lastUser + 1).filter((entry) => entry.role === "assistant").length];
	if (step === undefined) return message("done");
	if (typeof step === "string") return message(step.replace(/^say: /, ""));
	const [name, argument] = step;
	if (name === "sleep") {
		await new Promise((resolve) => {
			const timer = setTimeout(resolve, argument);
			options?.signal?.addEventListener("abort", () => (clearTimeout(timer), resolve()), { once: true });
		});
		return call("bash", { command: "true" });
	}
	if (name === "report") {
		const result = messages.findLast((entry) => entry.role === "toolResult");
		return call("buzz_send", { text: result ? textOf(result) : "(no tool result)", final: true });
	}
	if (name === "describe") {
		// Pi sends the prompt as sections and the tools as a list, and later only what changed: put together here.
		const sections = {};
		const tools = new Set();
		for (const entry of messages.filter((entry) => entry.role === "system")) {
			Object.assign(sections, entry.sections);
			for (const tool of entry.toolsAdded ?? []) tools.add(tool.name);
			for (const tool of entry.toolsRemoved ?? []) tools.delete(typeof tool === "string" ? tool : tool.name);
		}
		return call("buzz_send", { text: JSON.stringify({ system: Object.values(sections).join("\n"), tools: [...tools] }), final: true });
	}
	if (name === "error") return message("", { stopReason: "error", errorMessage: argument });
	return call(name, argument);
}
