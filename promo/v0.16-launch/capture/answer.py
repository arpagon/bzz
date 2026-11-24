#!/usr/bin/env python3
"""answer.py <answer> <passphrase file> -- <command…>: run a command on a
pseudo-terminal; type <answer> + Enter at each yes/no question and the
disposable passphrase at each passphrase prompt (never echoed here). For
the setup only, never for a tape."""
import os, pty, re, select, sys, time

i = sys.argv.index("--")
answer, passfile, command = sys.argv[1], sys.argv[2], sys.argv[i + 1:]
secret = open(passfile).read().strip()
pid, fd = pty.fork()
if pid == 0:
    os.execvp(command[0], command)
out, answered, start = b"", 0, time.time()
while True:
    r, _, _ = select.select([fd], [], [], 0.5)
    if r:
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        out += data
        sys.stdout.write(data.decode("utf-8", "replace")); sys.stdout.flush()
    pending = out[answered:].decode("utf-8", "replace")
    if re.search(r"passphrase[^\n]*:\s*$", pending, re.I):
        time.sleep(0.3); os.write(fd, (secret + "\r").encode()); answered = len(out)
    elif re.search(r"(\[y/N\]|\[Y/n\]|\(y/n\))[^\n]*$", pending, re.I):
        time.sleep(0.3); os.write(fd, (answer + "\r").encode()); answered = len(out)
    if time.time() - start > 300:
        print("answer.py: timed out", file=sys.stderr); os.kill(pid, 9); sys.exit(2)
_, status = os.waitpid(pid, 0)
sys.exit(os.waitstatus_to_exitcode(status))
