"""Run a program in a pseudo-terminal and drive it like a user (stdlib only)."""
import errno
import fcntl
import os
import select
import signal
import struct
import subprocess
import termios
import time

from vt import Screen

KEYS = {
    "enter": b"\r", "esc": b"\x1b", "tab": b"\t", "backspace": b"\x7f",
    "ctrl-c": b"\x03", "space": b" ",
}
ARROWS = {"up": "A", "down": "B", "right": "C", "left": "D"}


def set_winsize(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def session_pids(sid):
    """Live processes whose session id is ``sid`` (zombies excluded)."""
    out = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat", "rb") as f:
                stat = f.read().decode(errors="replace")
        except OSError:
            continue
        rest = stat[stat.rfind(")") + 2:].split()
        state, session = rest[0], int(rest[3])
        if session == sid and state not in ("Z", "X"):
            out.append(int(name))
    return out


def tree_rss_kib(sid):
    total = 0
    for pid in session_pids(sid):
        try:
            with open(f"/proc/{pid}/status") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        total += int(line.split()[1])
        except OSError:
            pass
    return total


class Session:
    def __init__(self, argv, cols, rows, env, cwd=None):
        self.argv = argv
        self.master, self.slave = os.openpty()
        set_winsize(self.slave, rows, cols)
        attrs = termios.tcgetattr(self.slave)
        if hasattr(termios, "IUTF8"):
            attrs[0] |= termios.IUTF8
        termios.tcsetattr(self.slave, termios.TCSANOW, attrs)
        self.initial_attrs = termios.tcgetattr(self.slave)
        self.screen = Screen(cols, rows)
        self.raw = bytearray()
        self.t_start = time.monotonic()
        self.last_output = self.t_start
        self.proc = subprocess.Popen(
            argv, stdin=self.slave, stdout=self.slave, stderr=self.slave,
            env=env, cwd=cwd, start_new_session=True, close_fds=True,
            preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
        )
        self.sid = self.proc.pid
        os.set_blocking(self.master, False)

    # --------------------------------------------------------- reading
    def pump(self, timeout):
        """Read whatever arrives within ``timeout`` seconds. Return bytes read."""
        r, _, _ = select.select([self.master], [], [], max(0.0, timeout))
        if not r:
            return 0
        n = 0
        while True:
            try:
                chunk = os.read(self.master, 65536)
            except BlockingIOError:
                break
            except OSError as e:
                if e.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            n += len(chunk)
            self.raw += chunk
            self.screen.feed(chunk)
        if n:
            self.last_output = time.monotonic()
        if self.screen.responses:
            os.write(self.master, bytes(self.screen.responses))
            self.screen.responses.clear()
        return n

    def wait_for(self, pred, timeout=3.0):
        end = time.monotonic() + timeout
        while True:
            try:
                if pred(self):
                    return True
            except Exception:
                pass
            now = time.monotonic()
            if self.proc.poll() is not None and now - self.last_output > 0.3:
                end = now  # the program is gone and silent: no point waiting
            if now >= end:
                self.pump(0)
                try:
                    return bool(pred(self))
                except Exception:
                    return False
            self.pump(min(0.02, end - now))

    def settle(self, quiet=0.25, limit=3.0):
        """Wait until the program has been silent for ``quiet`` seconds."""
        end = time.monotonic() + limit
        self.pump(0)
        while time.monotonic() < end:
            if time.monotonic() - self.last_output >= quiet:
                return True
            self.pump(0.02)
        return False

    def sleep(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.pump(end - time.monotonic())

    # --------------------------------------------------------- input
    def send(self, data: bytes):
        os.write(self.master, data)

    def key(self, name, settle=0.08):
        if name in ARROWS:
            data = ("\x1bO" if self.screen.app_cursor else "\x1b[") + ARROWS[name]
            self.send(data.encode())
        elif name in KEYS:
            self.send(KEYS[name])
        else:
            self.send(name.encode())
        self.sleep(settle)

    def type(self, text, delay=0.03):
        for ch in text:
            self.send(ch.encode())
            self.sleep(delay)

    def paste(self, text):
        data = text.encode()
        if self.screen.bracketed_paste:
            data = b"\x1b[200~" + data + b"\x1b[201~"
        self.send(data)

    def resize(self, cols, rows):
        self.pump(0)
        set_winsize(self.master, rows, cols)
        self.screen.resize(cols, rows)
        # The kernel sends SIGWINCH to the foreground process group.

    # --------------------------------------------------------- state
    def alive(self):
        return self.proc.poll() is None

    def text(self):
        return self.screen.text()

    def last_row(self):
        return self.screen.lines()[-1]

    def termios_restored(self):
        now = termios.tcgetattr(self.slave)
        lflag_bits = termios.ECHO | termios.ICANON | termios.ISIG
        iflag_bits = termios.ICRNL
        oflag_bits = termios.OPOST
        return ((now[3] & lflag_bits) == (self.initial_attrs[3] & lflag_bits)
                and (now[0] & iflag_bits) == (self.initial_attrs[0] & iflag_bits)
                and (now[1] & oflag_bits) == (self.initial_attrs[1] & oflag_bits))

    def wait_exit(self, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            self.pump(0.02)
            if self.proc.poll() is not None:
                self.pump(0.05)
                return True
        return False

    def wait_session_gone(self, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if not session_pids(self.sid):
                return True
            time.sleep(0.02)
        return not session_pids(self.sid)

    def close(self):
        """Kill everything left in the program's session and free the pty."""
        for sig in (signal.SIGTERM, signal.SIGKILL):
            pids = session_pids(self.sid)
            if not pids:
                break
            for pid in pids:
                try:
                    os.kill(pid, sig)
                except ProcessLookupError:
                    pass
            end = time.monotonic() + 1.0
            while time.monotonic() < end and session_pids(self.sid):
                self.pump(0.02)
        try:
            self.proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        for fd in (self.master, self.slave):
            try:
                os.close(fd)
            except OSError:
                pass
