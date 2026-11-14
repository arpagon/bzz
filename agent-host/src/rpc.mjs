// JSON-RPC 2.0, one message per line, as the Agent Client Protocol uses it
// over a child's standard input and output.
import { createInterface } from "node:readline";

export class RpcError extends Error {
	constructor(code, message, data) {
		super(message);
		this.code = code;
		this.data = data;
	}
}

/**
 * @param methods requests this side answers: `name -> async (params) => result`
 * @param notifications notifications this side takes: `name -> (params) => void`
 * @returns `request(method, params)`, `notify(method, params)`, and `closed`, settled when the input ends or `end()` is called
 */
export function connect({ input, output, methods, notifications = {}, log = () => undefined }) {
	const pending = new Map();
	let next = 0;
	let open = true;
	const write = (message) => {
		if (open) output.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
	};

	async function receive(line) {
		if (!line.trim()) return;
		let message;
		try {
			message = JSON.parse(line);
		} catch {
			return write({ id: null, error: { code: -32700, message: "Parse error: a message is one line of JSON" } });
		}
		if (message.method === undefined) {
			const call = pending.get(message.id);
			pending.delete(message.id);
			if (call) message.error ? call.reject(new RpcError(message.error.code, message.error.message, message.error.data)) : call.resolve(message.result);
			return;
		}
		if (message.id === undefined) {
			try {
				await notifications[message.method]?.(message.params ?? {});
			} catch (error) {
				log(`notification ${message.method} failed: ${error?.message ?? error}`);
			}
			return;
		}
		const run = methods[message.method];
		if (!run) return write({ id: message.id, error: { code: -32601, message: `Method not found: ${message.method}` } });
		try {
			write({ id: message.id, result: await run(message.params ?? {}) });
		} catch (error) {
			if (!(error instanceof RpcError)) log(`${message.method} failed: ${error?.stack ?? error}`);
			write({ id: message.id, error: { code: error instanceof RpcError ? error.code : -32603, message: String(error?.message ?? error), ...(error?.data !== undefined && { data: error.data }) } });
		}
	}

	const lines = createInterface({ input });
	const closed = new Promise((resolve) => {
		lines.on("line", receive).on("close", () => {
			for (const call of pending.values()) call.reject(new Error("the client is gone"));
			pending.clear();
			resolve();
		});
	});

	return {
		closed,
		request(method, params) {
			return new Promise((resolve, reject) => {
				if (!open) return reject(new Error("the client is gone"));
				const id = `h${++next}`;
				pending.set(id, { resolve, reject });
				write({ id, method, params });
			});
		},
		notify: (method, params) => write({ method, params }),
		/** Stops writing: whatever still runs has nobody to answer. */
		stop() {
			open = false;
		},
		/** Ends the conversation from this side, as if the input had ended. */
		end: () => lines.close(),
	};
}
