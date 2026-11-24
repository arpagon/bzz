#!/usr/bin/env python3
"""make-vtt.py <out dir>: the film's two WebVTT tracks, from the same timing
the picture uses (timing/film.json, timing/narration.json):

  film-apps.en.vtt        captions: each burned-in caption, at its measured
                          film time (the narration's own speech and pauses)
  film-apps.chapters.vtt  chapters: each chapter's title, start to end
and chapters.json: the chapter list with times, for the page's links."""
import json, pathlib, re, sys

FILM = pathlib.Path(__file__).resolve().parent.parent
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
t = json.loads((FILM / "timing/film.json").read_text())
NAMES = {0: "Apps in bzz", 1: "Beside the chat", 2: "Shared, live", 3: "At the same moment", 4: "Three apps ship",
         5: "Ask your agent", 6: "Read the exact bytes", 7: "It changes in place", 8: "Offer it to the channel",
         9: "Agents use the records", 10: "And on Buzz desktop", 11: "bzz: apps in the channel"}


def ts(s):
    s = max(0.0, s); h = int(s // 3600); m = int(s % 3600 // 60)
    return f"{h:02d}:{m:02d}:{s % 60:06.3f}"


def plain(h): return re.sub(r"<[^>]+>", "", h).replace("&nbsp;", " ")


cues = ["WEBVTT", ""]
for i, c in enumerate(t["captions"], 1):
    cues += [str(i), f"{ts(c['start'] + 0.15)} --> {ts(c['end'])}", plain(c["html"]), ""]
end = t["chapters"][-1]
cues += [str(len(t["captions"]) + 1), f"{ts(end['start'] + 0.5)} --> {ts(end['start'] + 3.0)}", "bzz. Apps in the channel.", ""]
(out / "film-apps.en.vtt").write_text("\n".join(cues))
chap = ["WEBVTT", ""]
listing = []
for c in t["chapters"]:
    chap += [f"ch{c['n']}", f"{ts(c['start'])} --> {ts(c['end'])}", NAMES[c["n"]], ""]
    listing.append({"n": c["n"], "title": NAMES[c["n"]], "start": c["start"], "end": c["end"],
                    "at": f"{int(c['start'] // 60)}:{int(c['start'] % 60):02d}"})
(out / "film-apps.chapters.vtt").write_text("\n".join(chap))
(out / "chapters.json").write_text(json.dumps({"total_s": t["total_s"], "chapters": listing}, indent=1) + "\n")
print(f"{len(t['captions']) + 1} caption cues, {len(listing)} chapters, {t['total_s']} s")
