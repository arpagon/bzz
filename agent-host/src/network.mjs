// The only way out to the network for the agent: `git` and the shell's
// `curl` both send through `request` below, which lets a request through
// only to a host the owner listed, at every hop of a redirect.
const REDIRECT = new Set([301, 302, 303, 307, 308]);
const LOOPBACK = new Set(["localhost", "127.0.0.1", "[::1]"]);
const MAX_HOPS = 5;

export class NetworkRefused extends Error {}

/**
 * @param entries hosts as bzz sends them: `github.com`, or `host:port` for a port that is not the scheme's own.
 *   A name covers that host only, not the names below it.
 */
export function network(entries = []) {
	const hosts = new Set();
	for (const entry of entries) {
		if (typeof entry !== "string" || !/^[a-z0-9.-]+(:\d+)?$|^\[[0-9a-f:]+\](:\d+)?$/i.test(entry)) throw new Error(`the network allow-list takes host names, got ${JSON.stringify(entry)}`);
		hosts.add(entry.toLowerCase());
	}

	/** Throws unless the agent may send to `url`. HTTPS only, except to this machine itself, where a test server lives. */
	function check(url) {
		let parsed;
		try {
			parsed = new URL(url);
		} catch {
			throw new NetworkRefused(`network: ${JSON.stringify(String(url).slice(0, 80))} is not a URL this sandbox can reach (https only, no SSH remotes)`);
		}
		const secure = parsed.protocol === "https:" || (parsed.protocol === "http:" && LOOPBACK.has(parsed.hostname));
		if (!secure) throw new NetworkRefused(`network: ${parsed.protocol}//${parsed.host} is refused: only https is allowed${parsed.protocol === "ssh:" ? ", SSH remotes are not supported" : ""}`);
		// `host` leaves out the scheme's own port, so `github.com` means port 443 and nothing else.
		if (!hosts.has(parsed.host)) throw new NetworkRefused(`network: ${parsed.host} is not on this agent's allow-list${hosts.size ? ` (${[...hosts].join(", ")})` : " (it is empty)"}. The owner sets it in bzz.`);
	}

	/** One request, following redirects by hand so that each new address is checked before anything is sent to it. */
	async function request(url, { method = "GET", headers = {}, body, signal, follow = true } = {}) {
		const chain = [];
		for (let hop = 0; ; hop++) {
			check(url);
			const response = await fetch(url, { method, headers, body, signal, redirect: "manual" });
			const location = response.headers.get("location");
			if (!follow || !REDIRECT.has(response.status) || !location) {
				return { url, status: response.status, statusText: response.statusText, headers: Object.fromEntries(response.headers), body: new Uint8Array(await response.arrayBuffer()), chain };
			}
			if (hop >= MAX_HOPS) throw new NetworkRefused(`network: more than ${MAX_HOPS} redirects from ${new URL(url).host}`);
			chain.push({ status: response.status, statusText: response.statusText, headers: Object.fromEntries(response.headers) });
			await response.arrayBuffer().catch(() => undefined);
			const next = new URL(location, url);
			// Credentials are for the host they were given for.
			if (next.host !== new URL(url).host) headers = Object.fromEntries(Object.entries(headers).filter(([name]) => !/^(authorization|cookie)$/i.test(name)));
			if (response.status === 303 || ((response.status === 301 || response.status === 302) && method === "POST")) {
				method = "GET";
				body = undefined;
			}
			url = next.href;
		}
	}

	return {
		hosts: [...hosts],
		check,
		request,
		/** The `http` client isomorphic-git takes. */
		git: {
			async request({ url, method, headers, body }) {
				const chunks = [];
				if (body) for await (const chunk of body) chunks.push(chunk);
				const response = await request(url, { method, headers, body: chunks.length ? Buffer.concat(chunks) : undefined });
				return { url: response.url, method, statusCode: response.status, statusMessage: response.statusText, headers: response.headers, body: [response.body] };
			},
		},
		/** The fetch just-bash's `curl` calls. */
		async shell(url, options = {}) {
			const headers = options.headers instanceof Headers ? Object.fromEntries(options.headers) : (options.headers ?? {});
			const timeout = AbortSignal.timeout(options.timeoutMs ?? 30_000);
			const response = await request(url, {
				method: options.method, headers, body: options.body, follow: options.followRedirects !== false,
				signal: options.signal ? AbortSignal.any([options.signal, timeout]) : timeout,
			});
			return { status: response.status, statusText: response.statusText, headers: response.headers, body: response.body, url: response.url, redirectChain: response.chain };
		},
	};
}
