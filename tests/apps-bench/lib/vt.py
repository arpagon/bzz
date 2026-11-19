"""Minimal xterm-compatible screen emulator for the acceptance bench.

Standard library only. It keeps the text of every cell (no colours), the
cursor, the modes a test cares about (alternate screen, cursor visibility,
bracketed paste, mouse, application cursor keys), and two overflow counters:

- ``wraps``: a character was written past the last column and autowrapped;
- ``lost_lines``: a non-blank line was scrolled off the top of the screen.

Either one means some text did not fit inside the viewport.
"""
import codecs
import unicodedata


def char_width(ch):
    o = ord(ch)
    if o < 32 or 0x7F <= o < 0xA0:
        return 0
    if unicodedata.combining(ch) or o in (0x200B, 0x200C, 0x200D, 0xFE0E, 0xFE0F):
        return 0
    if unicodedata.east_asian_width(ch) in ("W", "F"):
        return 2
    return 1


def text_width(s):
    return sum(char_width(c) for c in s)


MOUSE_MODES = {9, 1000, 1001, 1002, 1003, 1005, 1006, 1015, 1016}


class Screen:
    def __init__(self, cols, rows):
        self.cols, self.rows = cols, rows
        self.main = self._blank(cols, rows)
        self.alt = self._blank(cols, rows)
        self.grid = self.main
        self.alt_active = False
        self.x = self.y = 0
        self.pending_wrap = False
        self.saved = (0, 0)
        self.saved_main = (0, 0)
        self.top, self.bottom = 0, rows - 1
        self.cursor_visible = True
        self.bracketed_paste = False
        self.app_cursor = False
        self.autowrap = True
        self.focus_events = False
        self.mouse = set()
        self.wraps = 0
        self.lost_lines = 0
        self.lost_samples = []
        self.forgiven = 0
        self.responses = bytearray()
        self._dec = codecs.getincrementaldecoder("utf-8")("replace")
        self._state = "ground"
        self._buf = ""
        self.changed = 0  # bumps on any cell write or erase

    # ------------------------------------------------------------ helpers
    @staticmethod
    def _blank(cols, rows):
        return [[" "] * cols for _ in range(rows)]

    def _forgive(self):
        """A full-screen erase hides anything that overflowed before it."""
        self.forgiven += self.wraps + self.lost_lines
        self.wraps = 0
        self.lost_lines = 0
        self.lost_samples = []

    def reset_counters(self):
        self.wraps = 0
        self.lost_lines = 0
        self.lost_samples = []

    def lines(self):
        return ["".join(c for c in row if c != "").rstrip() for row in self.grid]

    def text(self):
        return "\n".join(self.lines())

    def row_cells(self, y):
        """Return (text, xs) where xs[i] is the cell column of text[i]."""
        chars, xs = [], []
        for x, c in enumerate(self.grid[y]):
            if c == "":
                continue
            for ch in c:
                chars.append(ch)
                xs.append(x)
        return "".join(chars), xs

    def dump(self, title=""):
        top = f"+{'-' * self.cols}+ {self.cols}x{self.rows} {title}"
        out = [top]
        for i, row in enumerate(self.grid):
            s = "".join(c for c in row if c != "")
            out.append(f"|{s}| {i:2d}")
        out.append(f"+{'-' * self.cols}+")
        return "\n".join(out)

    def resize(self, cols, rows):
        for name in ("main", "alt"):
            g = getattr(self, name)
            g = [r[:cols] + [" "] * max(0, cols - len(r)) for r in g[:rows]]
            while len(g) < rows:
                g.append([" "] * cols)
            for r in g:  # do not leave half a wide char at the edge
                if r and r[-1] != "" and len(r) >= 1 and char_width(r[-1][:1] or " ") == 2:
                    r[-1] = " "
            setattr(self, name, g)
        self.grid = self.alt if self.alt_active else self.main
        self.cols, self.rows = cols, rows
        self.x = min(self.x, cols - 1)
        self.y = min(self.y, rows - 1)
        self.top, self.bottom = 0, rows - 1
        self.pending_wrap = False
        self.changed += 1

    # ------------------------------------------------------------ feed
    def feed(self, data: bytes):
        for ch in self._dec.decode(data):
            self._char(ch)

    def _char(self, ch):
        st = self._state
        if st == "ground":
            if ch == "\x1b":
                self._state = "esc"
            elif ord(ch) < 32 or ch == "\x7f":
                self._control(ch)
            else:
                self._put(ch)
        elif st == "esc":
            self._esc(ch)
        elif st == "csi":
            if "\x40" <= ch <= "\x7e":
                self._state = "ground"
                self._csi(self._buf, ch)
            elif ch == "\x1b":
                self._state = "esc"
            else:
                self._buf += ch
        elif st == "osc":
            if ch == "\x07":
                self._state = "ground"
            elif ch == "\x1b":
                self._state = "str_esc"
        elif st == "str":
            if ch == "\x1b":
                self._state = "str_esc"
        elif st == "str_esc":
            self._state = "ground" if ch == "\\" else "str"
        elif st == "charset":
            self._state = "ground"

    def _control(self, ch):
        if ch == "\r":
            self.x = 0
            self.pending_wrap = False
        elif ch in "\n\x0b\x0c":
            self._linefeed()
            self.pending_wrap = False
        elif ch == "\x08":
            if self.x > 0:
                self.x -= 1
            self.pending_wrap = False
        elif ch == "\t":
            self.x = min(self.cols - 1, (self.x // 8 + 1) * 8)
            self.pending_wrap = False

    def _esc(self, ch):
        self._state = "ground"
        if ch == "[":
            self._state, self._buf = "csi", ""
        elif ch == "]":
            self._state = "osc"
        elif ch in "P_^X":
            self._state = "str"
        elif ch in "()*+-./#%":
            self._state = "charset"
        elif ch == "7":
            self.saved = (self.x, self.y)
        elif ch == "8":
            self.x, self.y = self.saved
            self.pending_wrap = False
        elif ch == "M":
            if self.y == self.top:
                self._scroll_down(1)
            elif self.y > 0:
                self.y -= 1
        elif ch == "D":
            self._linefeed()
        elif ch == "E":
            self.x = 0
            self._linefeed()
        elif ch == "c":
            self.__init__(self.cols, self.rows)

    # ------------------------------------------------------------ drawing
    def _put(self, ch):
        w = char_width(ch)
        if w == 0:
            if unicodedata.combining(ch) or ord(ch) in (0x200D, 0xFE0F, 0xFE0E):
                px = self.x - 1 if not self.pending_wrap else self.x
                if 0 <= px < self.cols:
                    if self.grid[self.y][px] == "" and px > 0:
                        px -= 1
                    self.grid[self.y][px] += ch
            return
        if self.pending_wrap:
            if self.autowrap:
                self.wraps += 1
                self.x = 0
                self._linefeed()
            self.pending_wrap = False
        if w == 2 and self.x == self.cols - 1:
            if self.autowrap:
                self.wraps += 1
                self._set(self.x, self.y, " ")
                self.x = 0
                self._linefeed()
            else:
                return
        self._set(self.x, self.y, ch)
        if w == 2:
            self._set(self.x + 1, self.y, "")
        self.changed += 1
        if self.x + w >= self.cols:
            self.x = self.cols - 1
            self.pending_wrap = True
        else:
            self.x += w

    def _set(self, x, y, val):
        row = self.grid[y]
        if row[x] == "" and x > 0:  # overwriting the tail of a wide char
            row[x - 1] = " "
        if val != "" and x + 1 < self.cols and row[x + 1] == "" and row[x] not in ("", " "):
            row[x + 1] = " "  # overwriting the head of a wide char
        row[x] = val

    def _linefeed(self):
        if self.y == self.bottom:
            self._scroll_up(1)
        elif self.y < self.rows - 1:
            self.y += 1

    def _scroll_up(self, n):
        for _ in range(n):
            line = self.grid.pop(self.top)
            text = "".join(c for c in line if c != "").strip()
            if self.top == 0 and text:
                self.lost_lines += 1
                if len(self.lost_samples) < 5:
                    self.lost_samples.append(text)
            self.grid.insert(self.bottom, [" "] * self.cols)
        self.changed += 1

    def _scroll_down(self, n):
        for _ in range(n):
            self.grid.pop(self.bottom)
            self.grid.insert(self.top, [" "] * self.cols)
        self.changed += 1

    def _erase(self, y, x0, x1):
        row = self.grid[y]
        for x in range(max(0, x0), min(self.cols, x1)):
            row[x] = " "
        if x0 > 0 and x0 < self.cols and row[x0 - 1] != "" and char_width(row[x0 - 1][:1] or " ") == 2:
            row[x0 - 1] = " "
        self.changed += 1

    # ------------------------------------------------------------ CSI
    def _csi(self, body, final):
        prefix = ""
        if body and body[0] in "?>=<":
            prefix, body = body[0], body[1:]
        inter = ""
        while body and body[-1] in " !\"#$%&'()*+,-./":
            inter = body[-1] + inter
            body = body[:-1]
        params = []
        for p in body.split(";") if body else []:
            p = p.split(":")[0]
            params.append(int(p) if p.isdigit() else 0)

        def arg(i, default=1):
            v = params[i] if i < len(params) else 0
            return v if v else default

        if inter:
            return  # DECSCUSR, DECRQM and friends: ignore
        if prefix == "?":
            if final in "hl":
                for p in params:
                    self._mode(p, final == "h")
            return  # ?u (kitty query), ?n etc. are not answered
        if prefix == ">":
            if final == "c":
                self.responses += b"\x1b[>0;0;0c"
            return
        if prefix:
            return
        mv = final in "ABCDEFGHfd`ae"
        if mv:
            self.pending_wrap = False
        if final == "A":
            self.y = max(self.top if self.y >= self.top else 0, self.y - arg(0))
        elif final in "Be":
            self.y = min(self.bottom if self.y <= self.bottom else self.rows - 1, self.y + arg(0))
        elif final in "Ca":
            self.x = min(self.cols - 1, self.x + arg(0))
        elif final == "D":
            self.x = max(0, self.x - arg(0))
        elif final == "E":
            self.y = min(self.rows - 1, self.y + arg(0))
            self.x = 0
        elif final == "F":
            self.y = max(0, self.y - arg(0))
            self.x = 0
        elif final in "G`":
            self.x = min(self.cols - 1, arg(0) - 1)
        elif final in "Hf":
            self.y = min(self.rows - 1, arg(0) - 1)
            self.x = min(self.cols - 1, arg(1) - 1)
        elif final == "d":
            self.y = min(self.rows - 1, arg(0) - 1)
        elif final == "J":
            m = params[0] if params else 0
            if m == 0:
                self._erase(self.y, self.x, self.cols)
                for y in range(self.y + 1, self.rows):
                    self._erase(y, 0, self.cols)
            elif m == 1:
                for y in range(0, self.y):
                    self._erase(y, 0, self.cols)
                self._erase(self.y, 0, self.x + 1)
            else:
                for y in range(self.rows):
                    self._erase(y, 0, self.cols)
            if m in (2, 3) or (m == 0 and self.x == 0 and self.y == 0):
                self._forgive()
        elif final == "K":
            m = params[0] if params else 0
            if m == 0:
                self._erase(self.y, self.x, self.cols)
            elif m == 1:
                self._erase(self.y, 0, self.x + 1)
            else:
                self._erase(self.y, 0, self.cols)
            self.pending_wrap = False
        elif final == "X":
            self._erase(self.y, self.x, self.x + arg(0))
        elif final == "P":
            row = self.grid[self.y]
            n = min(arg(0), self.cols - self.x)
            del row[self.x:self.x + n]
            row.extend([" "] * n)
            self.changed += 1
        elif final == "@":
            row = self.grid[self.y]
            n = min(arg(0), self.cols - self.x)
            for _ in range(n):
                row.insert(self.x, " ")
            del row[self.cols:]
            self.changed += 1
        elif final == "L":
            if self.top <= self.y <= self.bottom:
                for _ in range(arg(0)):
                    self.grid.pop(self.bottom)
                    self.grid.insert(self.y, [" "] * self.cols)
                self.changed += 1
        elif final == "M":
            if self.top <= self.y <= self.bottom:
                for _ in range(arg(0)):
                    self.grid.pop(self.y)
                    self.grid.insert(self.bottom, [" "] * self.cols)
                self.changed += 1
        elif final == "S":
            self._scroll_up(arg(0))
        elif final == "T":
            self._scroll_down(arg(0))
        elif final == "r":
            top = arg(0) - 1
            bottom = (params[1] if len(params) > 1 and params[1] else self.rows) - 1
            if 0 <= top < bottom < self.rows:
                self.top, self.bottom = top, bottom
                self.x, self.y = 0, 0
        elif final == "s":
            self.saved = (self.x, self.y)
        elif final == "u":
            self.x, self.y = self.saved
        elif final == "n":
            if params and params[0] == 6:
                self.responses += f"\x1b[{self.y + 1};{self.x + 1}R".encode()
            elif params and params[0] == 5:
                self.responses += b"\x1b[0n"
        elif final == "c":
            if not params or params[0] == 0:
                self.responses += b"\x1b[?62;22c"
        elif final == "t":
            if params and params[0] == 18:
                self.responses += f"\x1b[8;{self.rows};{self.cols}t".encode()
        # 'm' (colours) and everything else: ignored

    def _mode(self, p, on):
        if p in (47, 1047, 1049):
            if on and not self.alt_active:
                if p == 1049:
                    self.saved_main = (self.x, self.y)
                self.alt = self._blank(self.cols, self.rows)
                self.grid = self.alt
                self.alt_active = True
            elif not on and self.alt_active:
                self.grid = self.main
                self.alt_active = False
                if p == 1049:
                    self.x, self.y = self.saved_main
            self.pending_wrap = False
            self.changed += 1
        elif p == 25:
            self.cursor_visible = on
        elif p == 2004:
            self.bracketed_paste = on
        elif p == 1:
            self.app_cursor = on
        elif p == 7:
            self.autowrap = on
        elif p == 1004:
            self.focus_events = on
        elif p in MOUSE_MODES:
            if on:
                self.mouse.add(p)
            else:
                self.mouse.discard(p)
