// The victim of kill.test.mjs: writes, appends, renames, deletes and commits
// on a disk without pause until it is killed.
//   node kill-writer.mjs <files.sqlite> <seed>
import git from "isomorphic-git";
import { nodeFs } from "../../src/adapters.mjs";
import { SqliteFs } from "../../src/disk.mjs";
import { Vfs } from "../../src/vfs.mjs";

const vfs = new Vfs(new SqliteFs(process.argv[2]));
const fs = nodeFs(vfs);
const dir = "/home/agent/w";
let seed = Number(process.argv[3]);
const random = () => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed / 2147483648;
};
vfs.mkdir(dir, { recursive: true });
if (!vfs.exists(`${dir}/.git`)) await git.init({ fs, dir, defaultBranch: "main" });
const author = { name: "agent", email: "agent@bzz.invalid" };
process.stdout.write("ready\n");
for (let n = 0; ; n++) {
	const path = `${dir}/f${Math.floor(random() * 40)}.bin`;
	const roll = random();
	if (roll < 0.55) vfs.writeFile(path, new Uint8Array(Math.floor(random() * 600_000)).fill(n % 251));
	else if (roll < 0.7) vfs.appendFile(path, new Uint8Array(Math.floor(random() * 50_000)).fill(7));
	else if (roll < 0.8 && vfs.exists(path)) vfs.rename(path, `${dir}/f${Math.floor(random() * 40)}.bin`);
	else if (roll < 0.9) vfs.rm(path, { force: true });
	else {
		for (const [filepath, , workdir] of await git.statusMatrix({ fs, dir }))
			if (workdir === 0) await git.remove({ fs, dir, filepath });
			else await git.add({ fs, dir, filepath });
		await git.commit({ fs, dir, message: `step ${n}`, author });
	}
}
