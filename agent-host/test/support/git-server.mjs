// A git remote for the tests: bare repositories served over HTTP on this
// machine by the real `git http-backend`. What the agent's git pushes there
// is read back with the real `git`, which is the judge.
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";

export const git = (cwd, ...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
const OWNER = ["-c", "user.name=owner", "-c", "user.email=owner@example.invalid"];

/** Why the git tests cannot run here, or undefined when they can. */
export function gitMissing() {
	try {
		const exec = git(".", "--exec-path");
		const backend = ["git-http-backend", "git-http-backend.exe"].some((name) => existsSync(join(exec, name)));
		return backend ? undefined : "this git has no http-backend";
	} catch {
		return "git is not installed";
	}
}

/**
 * @returns `origin` (the URL), `host` (for the allow-list), `bare` (the repository on disk), `ownerPush(file, text)`
 */
export async function startGitServer(t, root) {
	mkdirSync(root, { recursive: true });
	git(root, "init", "-q", "--bare", "-b", "main", "project.git");
	const bare = join(root, "project.git");
	git(bare, "config", "http.receivepack", "true");
	const seed = join(root, "seed");
	git(root, "init", "-q", "-b", "main", "seed");
	const ownerPush = (file, text, message) => {
		execFileSync(process.execPath, ["-e", "require('fs').writeFileSync(process.argv[1], process.argv[2])", join(seed, file), text]);
		git(seed, "add", file);
		git(seed, ...OWNER, "commit", "-q", "-m", message);
		git(seed, "push", "-q", bare, "main");
	};
	ownerPush("notes.txt", "alpha\nbeta\ngamma\n", "add notes");
	const server = createServer((request, response) => {
		const url = new URL(request.url, "http://localhost");
		const cgi = spawn("git", ["http-backend"], { env: { ...process.env, GIT_PROJECT_ROOT: root, GIT_HTTP_EXPORT_ALL: "1", PATH_INFO: url.pathname,
			QUERY_STRING: url.search.slice(1), REQUEST_METHOD: request.method, CONTENT_TYPE: request.headers["content-type"] ?? "", REMOTE_ADDR: "127.0.0.1" } });
		request.pipe(cgi.stdin);
		let head = Buffer.alloc(0);
		let sent = false;
		cgi.stdout.on("data", (chunk) => {
			if (sent) return void response.write(chunk);
			head = Buffer.concat([head, chunk]);
			const end = head.indexOf("\r\n\r\n");
			if (end < 0) return;
			const headers = Object.fromEntries(head.subarray(0, end).toString().split("\r\n").map((line) => [line.slice(0, line.indexOf(":")), line.slice(line.indexOf(":") + 1).trim()]));
			const status = Number((headers.Status ?? "200").split(" ")[0]);
			delete headers.Status;
			response.writeHead(status, headers);
			response.write(head.subarray(end + 4));
			sent = true;
		});
		cgi.on("close", () => response.end());
	});
	await new Promise((done) => server.listen(0, "127.0.0.1", done));
	t.after(() => new Promise((done) => {
		server.closeAllConnections();
		server.close(done);
	}));
	const host = `127.0.0.1:${server.address().port}`;
	return { origin: `http://${host}/project.git`, host, bare, seed, ownerPush, pullSeed: () => git(seed, "pull", "-q", bare, "main") };
}
