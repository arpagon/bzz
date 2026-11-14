// A `git` command for the agent's shell, on isomorphic-git: no git program
// runs. It covers the everyday subset. Anything else answers "not supported"
// and names what is, so the model knows instead of guessing.
import { createTwoFilesPatch } from "diff";
import git from "isomorphic-git";
import { defineCommand } from "just-bash";
import { nodeFs } from "./adapters.mjs";
import { normalize } from "./disk.mjs";

const MISSING = {
	rebase: "Merge instead, or make the change again on top of the new commit.",
	stash: "Commit to a branch of your own instead.",
	"cherry-pick": "Make the change again by hand.",
	submodule: "Clone the other repository beside this one.",
	reset: "Use `git checkout <ref>`, or write the file again.",
};

/**
 * @param vfs the agent's file tree
 * @param options.http the allow-listed client of network.mjs
 * @param options.author who commits when the repository's configuration does not say
 */
export function gitCommand(vfs, { http, author }) {
	const fs = nodeFs(vfs);
	const ok = (stdout = "") => ({ stdout, stderr: "", exitCode: 0 });
	const fail = (message, exitCode = 1) => ({ stdout: "", stderr: `git: ${message}\n`, exitCode });
	/** Takes `--name value` out of `args`. */
	const flag = (args, ...names) => {
		const at = args.findIndex((arg) => names.includes(arg));
		return at < 0 ? undefined : args.splice(at, 2)[1];
	};
	const has = (args, ...names) => {
		const at = args.findIndex((arg) => names.includes(arg));
		if (at >= 0) args.splice(at, 1);
		return at >= 0;
	};
	const status = async (dir) => (await git.statusMatrix({ fs, dir })).filter(([, head, work, stage]) => !(head === 1 && work === 1 && stage === 1));
	const code = ([, head, work, stage]) =>
		head === 0 ? (stage === 0 ? "??" : stage === 2 ? "A " : "AM")
		: work === 0 ? (stage === 0 ? "D " : " D")
		: stage === 1 ? " M" : stage === 2 ? "M " : stage === 3 ? "MM" : "D ";
	const text = (bytes) => new TextDecoder().decode(bytes);
	const who = async (dir) => ({
		name: (await git.getConfig({ fs, dir, path: "user.name" })) ?? author.name,
		email: (await git.getConfig({ fs, dir, path: "user.email" })) ?? author.email,
	});
	const positional = (args) => args.filter((arg) => !arg.startsWith("-"));

	const commands = {
		async init(args, cwd) {
			const branch = flag(args, "-b", "--initial-branch") ?? "main";
			const [name] = positional(args);
			const dir = name ? normalize(name.startsWith("/") ? name : `${cwd}/${name}`) : cwd;
			vfs.mkdir(dir, { recursive: true });
			await git.init({ fs, dir, defaultBranch: branch });
			return ok(`Initialized empty Git repository in ${dir}/.git/\n`);
		},
		async clone(args, cwd) {
			const depth = flag(args, "--depth");
			const ref = flag(args, "-b", "--branch");
			const [url, name = url?.replace(/\/$/, "").split("/").pop().replace(/\.git$/, "")] = positional(args);
			if (!url) return fail("clone needs a URL");
			const dir = normalize(name.startsWith("/") ? name : `${cwd}/${name}`);
			const existed = vfs.exists(dir);
			try {
				await git.clone({ fs, http, dir, url, singleBranch: true, ...(ref && { ref }), ...(depth && { depth: Number(depth) }) });
			} catch (error) {
				// A refused or failed clone leaves no half-made repository behind.
				if (!existed) vfs.rm(dir, { recursive: true, force: true });
				throw error;
			}
			return ok(`Cloned into '${dir}'\n`);
		},
		async status(args, _cwd, dir) {
			const rows = await status(dir);
			if (has(args, "--short", "-s", "--porcelain")) return ok(rows.map((row) => `${code(row)} ${row[0]}\n`).join(""));
			const branch = (await git.currentBranch({ fs, dir })) ?? "(detached)";
			return ok(`On branch ${branch}\n${rows.length ? rows.map((row) => `\t${code(row)} ${row[0]}\n`).join("") : "nothing to commit, working tree clean\n"}`);
		},
		async add(args, cwd, dir) {
			const all = has(args, "-A", "--all") || args.includes(".");
			const rows = await status(dir);
			const relative = (path) => normalize(path.startsWith("/") ? path : `${cwd}/${path}`).slice(dir.length + 1);
			const wanted = all ? rows.map(([path]) => path) : args.map(relative);
			for (const filepath of wanted) {
				const row = rows.find(([path]) => path === filepath);
				if (row && row[2] === 0) await git.remove({ fs, dir, filepath });
				else await git.add({ fs, dir, filepath });
			}
			return ok();
		},
		async rm(args, cwd, dir) {
			for (const path of positional(args)) {
				const at = normalize(path.startsWith("/") ? path : `${cwd}/${path}`);
				vfs.rm(at, { force: true });
				await git.remove({ fs, dir, filepath: at.slice(dir.length + 1) });
			}
			return ok();
		},
		async commit(args, _cwd, dir) {
			const both = args.indexOf("-am");
			if (both >= 0) args.splice(both, 1, "-a", "-m");
			const message = flag(args, "-m", "--message");
			if (!message) return fail("commit needs -m <message>: there is no editor in this sandbox");
			if (has(args, "-a", "--all"))
				for (const [filepath, head, work] of await status(dir))
					if (head === 1) await (work === 0 ? git.remove({ fs, dir, filepath }) : git.add({ fs, dir, filepath }));
			const oid = await git.commit({ fs, dir, message, author: await who(dir) });
			return ok(`[${(await git.currentBranch({ fs, dir })) ?? "detached"} ${oid.slice(0, 7)}] ${message.split("\n")[0]}\n`);
		},
		async log(args, _cwd, dir) {
			const oneline = has(args, "--oneline");
			const depth = Number(flag(args, "-n", "--max-count") ?? args.find((arg) => /^-\d+$/.test(arg))?.slice(1) ?? 20);
			const entries = await git.log({ fs, dir, depth });
			return ok(entries.map(({ oid, commit }) => oneline
				? `${oid.slice(0, 7)} ${commit.message.split("\n")[0]}\n`
				: `commit ${oid}\nAuthor: ${commit.author.name} <${commit.author.email}>\nDate:   ${new Date(commit.author.timestamp * 1000).toISOString()}\n\n    ${commit.message.trimEnd().replace(/\n/g, "\n    ")}\n\n`).join(""));
		},
		async diff(args, _cwd, dir) {
			const head = await git.resolveRef({ fs, dir, ref: "HEAD" });
			const only = positional(args);
			let out = "";
			for (const [filepath, inHead, work] of await status(dir)) {
				if (only.length && !only.includes(filepath)) continue;
				const before = inHead ? text((await git.readBlob({ fs, dir, oid: head, filepath })).blob) : "";
				const after = work ? text(vfs.readFile(`${dir}/${filepath}`)) : "";
				if (before !== after) out += createTwoFilesPatch(`a/${filepath}`, `b/${filepath}`, before, after, "", "", { context: 3 });
			}
			return ok(out);
		},
		async branch(args, _cwd, dir) {
			const remove = flag(args, "-d", "-D", "--delete");
			if (remove) return void (await git.deleteBranch({ fs, dir, ref: remove })) ?? ok();
			if (has(args, "--show-current")) return ok(`${(await git.currentBranch({ fs, dir })) ?? ""}\n`);
			const [name] = positional(args);
			if (name) return void (await git.branch({ fs, dir, ref: name })) ?? ok();
			const current = await git.currentBranch({ fs, dir });
			return ok((await git.listBranches({ fs, dir })).map((branch) => `${branch === current ? "*" : " "} ${branch}\n`).join(""));
		},
		async checkout(args, _cwd, dir) {
			const create = flag(args, "-b");
			if (create) await git.branch({ fs, dir, ref: create, checkout: true });
			else if (!args[0]) return fail("checkout needs a branch or a commit");
			else await git.checkout({ fs, dir, ref: args[0] });
			return ok(`Switched to '${create ?? args[0]}'\n`);
		},
		async remote(args, _cwd, dir) {
			if (args[0] === "add") return void (await git.addRemote({ fs, dir, remote: args[1], url: args[2] })) ?? ok();
			if (args[0] === "remove" || args[0] === "rm") return void (await git.deleteRemote({ fs, dir, remote: args[1] })) ?? ok();
			if (args[0] === "set-url") return void (await git.addRemote({ fs, dir, remote: args[1], url: args[2], force: true })) ?? ok();
			return ok((await git.listRemotes({ fs, dir })).map(({ remote, url }) => `${remote}\t${url}\n`).join(""));
		},
		async config(args, _cwd, dir) {
			has(args, "--local");
			if (has(args, "--global")) return fail("there is no global configuration in this sandbox: set it in the repository, without --global");
			const [path, value] = positional(args);
			if (!path) return fail("config needs a name, such as user.name");
			if (value === undefined) return ok(`${(await git.getConfig({ fs, dir, path })) ?? ""}\n`);
			await git.setConfig({ fs, dir, path, value });
			return ok();
		},
		async fetch(args, _cwd, dir) {
			const [remote = "origin", ref] = positional(args);
			await git.fetch({ fs, http, dir, remote, ...(ref && { ref, singleBranch: true }) });
			return ok(`Fetched ${remote}\n`);
		},
		async merge(args, _cwd, dir) {
			const [theirs] = positional(args);
			if (!theirs) return fail("merge needs a branch");
			// isomorphic-git stops at a conflict and leaves the tree as it was: there is no way to resolve one here.
			const result = await git.merge({ fs, dir, theirs, author: await who(dir), abortOnConflict: true });
			if (!result.alreadyMerged) await git.checkout({ fs, dir, ref: (await git.currentBranch({ fs, dir })) ?? "HEAD" });
			return ok(result.alreadyMerged ? "Already up to date.\n" : `Merged ${theirs}${result.fastForward ? " (fast-forward)" : ""}\n`);
		},
		async push(args, _cwd, dir) {
			const force = has(args, "-f", "--force");
			has(args, "-u", "--set-upstream");
			const [remote = "origin", ref = await git.currentBranch({ fs, dir })] = positional(args);
			const result = await git.push({ fs, http, dir, remote, ref, force });
			return result.ok ? ok(`To ${remote}\n   ${ref} -> ${ref}\n`) : fail(`push rejected: ${JSON.stringify(result.refs)}`);
		},
		async pull(args, _cwd, dir) {
			const [remote = "origin", ref = await git.currentBranch({ fs, dir })] = positional(args);
			await git.pull({ fs, http, dir, remote, ref, singleBranch: true, fastForwardOnly: true, author: await who(dir) });
			return ok(`Updated ${ref} from ${remote}\n`);
		},
		async "rev-parse"(args, _cwd, dir) {
			if (has(args, "--show-toplevel")) return ok(`${dir}\n`);
			if (has(args, "--abbrev-ref")) return ok(`${(await git.currentBranch({ fs, dir })) ?? "HEAD"}\n`);
			return ok(`${await git.resolveRef({ fs, dir, ref: positional(args)[0] ?? "HEAD" })}\n`);
		},
	};

	return defineCommand("git", async (argv, ctx) => {
		const args = [...argv];
		const elsewhere = flag(args, "-C");
		const cwd = elsewhere ? normalize(elsewhere.startsWith("/") ? elsewhere : `${ctx.cwd}/${elsewhere}`) : ctx.cwd;
		const name = args.shift();
		const run = commands[name];
		if (!run) {
			const hint = MISSING[name] ? ` ${MISSING[name]}` : "";
			return fail(`'${name ?? ""}' is not supported in this sandbox.${hint} Supported: ${Object.keys(commands).join(", ")}`, 129);
		}
		const quiet = has(args, "-q", "--quiet");
		try {
			const dir = name === "init" || name === "clone" ? undefined : await git.findRoot({ fs, filepath: cwd });
			const result = (await run(args, cwd, dir)) ?? ok();
			return quiet && result.exitCode === 0 ? ok() : result;
		} catch (error) {
			if (error.code === "NotFoundError" && /\.git|git root/.test(error.message)) return fail("not a git repository (or any of the parent directories)", 128);
			if (error.code === "MergeConflictError" || error.code === "MergeNotSupportedError") return fail(`${error.message} Conflicts cannot be resolved in this sandbox: nothing was changed.`);
			if (error.code === "UnknownTransportError") return fail("only https remotes work in this sandbox: SSH remotes are not supported");
			return fail(error.message);
		}
	});
}
