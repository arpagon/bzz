#!/usr/bin/env python3
"""scan.py [--state state.json --passphrase file] <tape.jsonl>…: search every
byte a tape holds (raw line, and each output chunk decoded) for anything of
this machine or its owner: paths, the account names, the fixture's secrets
(when the profile still exists), and the shapes of keys, tokens and
sign-ins. Prints what it found; exit 1 on a hit. Lists every 64-hex string
and /tmp path seen, so a person can tell public keys and hashes apart."""
import argparse, base64, json, re, sys

p = argparse.ArgumentParser()
p.add_argument("--state"); p.add_argument("--passphrase"); p.add_argument("tapes", nargs="+")
a = p.parse_args()
secrets = {}
if a.state:
    for who, key in json.load(open(a.state)).get("keys", {}).items():
        secrets[f"secret key of {who}"] = key
if a.passphrase:
    secrets["the disposable passphrase"] = open(a.passphrase).read().strip()
# Of the machine and its owner (account names: the login, the Modal workspace).
machine = ["arpagon", "Arpagon", "/home/", "/opt/", "/media/", "Workspace", "/run/user", ".pi/", "/.pi",
           ".codex", ".claude", ".grok", "s-28179", "mise/installs", "archive-data"]
# Shapes of keys, tokens and sign-ins.
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
