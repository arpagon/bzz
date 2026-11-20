"""Text-only properties of the Panel's sparkline and gauge (spec/panel.md).

Written against what stock Ratatui 0.30 `Sparkline` and `Gauge` print (see
f2/probe/), but they are properties, not glyph-exact pictures, so a hand-drawn
equivalent passes too.
"""
import re

SPARK = "▁▂▃▄▅▆▇█"          # U+2581..U+2588: level 1..8 of one cell
FULL = "█"                  # U+2588
LEVEL = {c: i + 1 for i, c in enumerate(SPARK)}


def read_sparkline(screen, name):
    """Return the column heights (in eighths) of the sparkline under
    `History: <name>`, left to right, or None if there is no such label or
    no glyph row right below it. Also returns the x positions."""
    rows = screen.lines()
    label = re.compile(r"History: " + re.escape(name) + r"(?!\w)")
    for y in range(len(rows) - 1):
        if not label.search(rows[y]):
            continue
        heights = {}
        yy = y + 1
        while yy < screen.rows - 1:
            text, xs = screen.row_cells(yy)
            cells = [(x, LEVEL[c]) for c, x in zip(text, xs) if c in LEVEL]
            if not cells:
                break
            for x, lv in cells:
                heights[x] = heights.get(x, 0) + lv
            yy += 1
        if not heights:
            return None
        order = sorted(heights)
        return {"xs": order, "heights": [heights[x] for x in order], "rows": yy - y - 1}
    return None


def sparkline_problem(screen, name, history):
    """None if the sparkline of `name` meets the spec for `history`, else why not."""
    sp = read_sparkline(screen, name)
    if sp is None:
        return f"a row with 'History: {name}' directly above sparkline glyphs (U+2581-U+2588)"
    xs, h = sp["xs"], sp["heights"]
    if len(h) != len(history):
        return (f"one sparkline cell per history value under 'History: {name}': "
                f"{len(history)} values {history}, saw {len(h)} drawn cells (heights in eighths {h})")
    if xs != list(range(xs[0], xs[0] + len(xs))):
        return f"sparkline cells side by side with no gap, saw them at columns {xs}"
    for i, a in enumerate(history):
        for j, b in enumerate(history):
            if a > b and h[i] < h[j]:
                return (f"a higher value never drawn lower: value {a} (cell {i + 1}) drawn {h[i]}/8 "
                        f"but value {b} (cell {j + 1}) drawn {h[j]}/8; heights {h} for {history}")
    top = history.index(max(history))
    if h[top] != max(h):
        return f"the largest value {max(history)} drawn with the tallest cell; heights {h} for {history}"
    low = history.index(min(history))
    if min(history) * 4 <= max(history) * 3 and h[low] >= h[top]:
        return (f"the largest value {max(history)} drawn taller than the smallest {min(history)}; "
                f"heights {h} for {history}")
    return None


GAUGE_LABEL = re.compile(r"(?<![\w/.%])(\d{1,3})%")


def read_gauge(screen):
    """Return {'pct', 'full', 'y'} for the first row (not the status row) with an
    `N%` label, preferring a row that also has full-block cells."""
    rows = screen.lines()[:-1]
    found = None
    for y, line in enumerate(rows):
        m = GAUGE_LABEL.search(line)
        if not m:
            continue
        g = {"pct": int(m.group(1)), "full": line.count(FULL), "y": y}
        if g["full"]:
            return g
        found = found or g
    return found


def gauge_problem(screen, value, maximum):
    """None if a gauge labelled round(100*value/max)% is shown, else why not."""
    want = int(value * 100 / maximum + 0.5)
    g = read_gauge(screen)
    if g is None:
        return f"a gauge row with the label '{want}%' ({value} of {maximum})"
    if g["pct"] != want:
        return f"the gauge label '{want}%' ({value} of {maximum}), saw '{g['pct']}%'"
    if want >= 10 and g["full"] == 0:
        return f"the gauge row with '{want}%' has filled cells drawn with '█' (U+2588)"
    return None
