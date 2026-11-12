#!/usr/bin/env bash
# Pixel stills of a tape at chosen seconds, via HyperFrames (xterm.js + Sixel).
# Usage: preview-tape.sh <tape.jsonl> <seconds,comma,separated> [out-dir]
# Prints the contact-sheet path. Needs the vendored libs in $FILM/lib.
set -euo pipefail
skill=$(cd "$(dirname "$0")/.." && pwd)
tape=$(readlink -f "$1"); times=$2
out=$(readlink -m "${3:-$(dirname "$tape")/preview}")
film=${FILM:?set FILM to a film project with lib/ and assets/fonts/}
work=$(mktemp -d "${TMPDIR:-/tmp}/tape-preview.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/assets"
cp -r "$film/lib" "$work/lib"; cp "$skill/assets/tape-replay.js" "$work/lib/"
cp -r "$film/assets/fonts" "$work/assets/fonts"; cp "$tape" "$work/assets/tape.jsonl"
end=$(python3 -c "import json,sys; print(int(json.loads(open(sys.argv[1]).readlines()[-1])['t'])+1)" "$tape")
cat > "$work/index.html" <<HTML
<!doctype html><html><head><meta charset="UTF-8">
<link rel="stylesheet" href="lib/xterm.css"><script src="lib/gsap.min.js"></script>
<script src="lib/xterm.js"></script><script src="lib/addon-image.js"></script><script src="lib/addon-unicode11.js"></script><script src="lib/tape-replay.js"></script>
<style>@font-face{font-family:"JetBrains Mono";src:url("assets/fonts/jetbrains-mono-latin-400-normal.woff2")}
@font-face{font-family:"JetBrains Mono";font-weight:700;src:url("assets/fonts/jetbrains-mono-latin-700-normal.woff2")}
html,body{margin:0;width:1920px;height:1080px;overflow:hidden;background:#14110d}
#term .xterm-viewport{overflow:hidden!important}</style></head><body>
<div id="root" data-composition-id="main" data-start="0" data-duration="$end" data-width="1920" data-height="1080">
<div id="term" class="clip" data-start="0" data-duration="$end" data-track-index="0" data-layout-ignore style="position:absolute;left:0;top:0"></div></div>
<script>mountTape({id:"t",el:"#term",url:"assets/tape.jsonl",filmStart:0,tapeStart:0,width:1920,height:1080});
const tl=gsap.timeline({paused:true});tl.to({}, {duration:$end});window.__timelines["main"]=tl;</script></body></html>
HTML
cd "$work"
HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1 npx -y hyperframes@0.9.13 snapshot --at "$times" > snap.log 2>&1 || { cat snap.log; exit 1; }
mkdir -p "$out"
cp .hyperframes/artifacts/snapshots/*/*.png .hyperframes/artifacts/snapshots/*/contact-sheet*.jpg "$out/" 2>/dev/null
ls "$out"/contact-sheet*.jpg
