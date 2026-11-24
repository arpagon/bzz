#!/usr/bin/env python3
"""scan.py [--state state.json] [--passphrase file]... <tape.jsonl>...: search
every byte a tape holds (each raw line, and each output chunk decoded) for
anything of this machine or its owner (names, paths, the Modal workspace),
the fixture's secret keys and every disposable passphrase, and the shapes of
keys, tokens, e-mail addresses and sign-in files. Prints what it found; exit 1
on a hit. Lists every 64-hex string and /tmp path seen, so a person can tell
public keys and hashes from secrets.
(After promo/v0.16-launch/capture/scan.py, with several passphrases.)"""
import argparse, base64, json, re, sys

p = argparse.ArgumentParser()
p.add_argument("--state"); p.add_argument("--passphrase", action="append", default=[]); p.add_argument("tapes", nargs="+")
a = p.parse_args()
secrets = {}
if a.state:
    for who, key in json.load(open(a.state)).get("keys", {}).items():
        secrets[f"secret key of {who}"] = key
for i, path in enumerate(a.passphrase):
    secrets[f"disposable passphrase {i + 1}"] = open(path).read().strip()
import os, getpass
# Of the machine and its owner: generic places, plus this machine's own names, from the
# environment (SCAN_TERMS, comma-separated: the login, the owner's name, account names).
machine = ["/home/", "/opt/", "/media/", "/run/user", ".pi/", "/.pi", ".codex", ".claude", ".grok", "Workspace",
           "mise/installs", "archive-data", getpass.getuser(), os.path.expanduser("~")]
machine += [t for t in os.environ.get("SCAN_TERMS", "").split(",") if t]
shapes = {"nostr secret key": r"nsec1[02-9ac-hj-np-z]{20,}", "encrypted nostr key": r"ncryptsec1",
          "JWT / OAuth token": r"eyJ[A-Za-z0-9_-]{12,}\.", "API key": r"\bsk-[A-Za-z0-9_-]{16,}",
          "bearer header": r"Bearer [A-Za-z0-9._-]{12,}", "token field": r"(access|refresh|id)_token",
          "e-mail address": r"[\w.+-]+@[A-Za-z0-9-]+\.[A-Za-z]{2,}", "private key block": r"PRIVATE KEY",
          "auth file": r"auth\.json"}
status = 0
for path in a.tapes:
    raw = open(path, "rb").read()
    text = raw.decode("utf-8", "replace")
    for line in raw.splitlines():
        event = json.loads(line)
        if isinstance(event.get("o"), str):
            text += "\n" + base64.b64decode(event["o"]).decode("utf-8", "replace")
    hits = [m for m in machine if m in text]
    hits += [name for name, rx in shapes.items() if re.search(rx, text)]
    hits += [name for name, value in secrets.items() if value and value in text]
    hexes = sorted(set(re.findall(r"(?<![0-9a-f])[0-9a-f]{64}(?![0-9a-f])", text)))
    tmp = sorted(set(re.findall(r"/tmp/[\w./-]+", text)))
    print(f"{path}: {len(raw)} bytes; hits: {hits or 'none'}")
    print(f"  64-hex strings: {len(hexes)}: " + ", ".join(h[:12] + "…" for h in hexes))
    print(f"  /tmp paths: {tmp or 'none'}")
    status |= bool(hits)
sys.exit(status)
