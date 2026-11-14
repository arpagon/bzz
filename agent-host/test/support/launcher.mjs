// Starts a host the way that leaves it orphaned with its input still open:
// the host's input is a pipe, and a second process, the holder, is given the
// writing end too. A test then kills this process. The pipe stays open in the
// holder, so the host sees no end of input: only the loss of its parent can
// end it.
//   node launcher.mjs <pid file> <main.mjs> <agent folder>
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const [pidFile, main, folder] = process.argv.slice(2);
const host = spawn(process.execPath, [main, folder], { stdio: ["pipe", "inherit", "inherit"] });
const holder = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: ["ignore", "ignore", "ignore", host.stdin] });
writeFileSync(pidFile, JSON.stringify({ host: host.pid, holder: holder.pid }));
setInterval(() => {}, 1000);
