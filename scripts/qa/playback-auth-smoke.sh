#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'required command not found: %s\n' "$1" >&2
    exit 127
  fi
}

for command_name in python3 curl cmp gst-launch-1.0 mpv grep seq; do
  require_command "$command_name"
done

keep_artifacts="${FERREX_QA_KEEP_ARTIFACTS:-0}"
if [[ -n "${FERREX_QA_WORK_DIR:-}" ]]; then
  work_dir="$FERREX_QA_WORK_DIR"
  mkdir -p "$work_dir"
else
  work_dir="$(mktemp -d -t ferrex-playback-auth-qa.XXXXXX)"
fi

server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi

  rm -f "$work_dir/mpv.raw.log"
  if [[ "$keep_artifacts" != "1" ]]; then
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT
rm -f \
  "$work_dir/fetched.wav" \
  "$work_dir/mpv.raw.log" \
  "$work_dir/mpv.redacted.log" \
  "$work_dir/port" \
  "$work_dir/server.log" \
  "$work_dir/unauthorized.body"

python3 - "$work_dir" <<'PY'
import math
import struct
import sys
import wave
from pathlib import Path

root = Path(sys.argv[1])
rate = 44_100
frames = int(rate * 0.25)
with wave.open(str(root / "ticketed.wav"), "wb") as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(rate)
    for i in range(frames):
        value = int(0.15 * 32_767 * math.sin(2 * math.pi * 440 * i / rate))
        wav.writeframes(struct.pack("<h", value))

(root / "ticket_server.py").write_text(
    r'''
import argparse
import contextlib
import http.server
import pathlib
import urllib.parse


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        message = (fmt % args).replace(self.server.token, "<redacted>")
        print(message, flush=True)

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        params = urllib.parse.parse_qs(parsed.query)
        if parsed.path != "/ticketed.wav" or params.get("access_token") != [self.server.token]:
            self.send_response(401)
            self.end_headers()
            self.wfile.write(b"unauthorized")
            return

        data = pathlib.Path(self.server.root, "ticketed.wav").read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "audio/wav")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        self.wfile.write(data)


parser = argparse.ArgumentParser()
parser.add_argument("--root", required=True)
parser.add_argument("--port-file", required=True)
parser.add_argument("--token", required=True)
args = parser.parse_args()
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.root = args.root
server.token = args.token
pathlib.Path(args.port_file).write_text(str(server.server_port))
with contextlib.suppress(KeyboardInterrupt):
    server.serve_forever()
''',
    encoding="utf-8",
)
PY

token="ferrex-qa-ticket-${RANDOM}-${RANDOM}"
python3 "$work_dir/ticket_server.py" \
  --root "$work_dir" \
  --port-file "$work_dir/port" \
  --token "$token" \
  >"$work_dir/server.log" 2>&1 &
server_pid=$!

for _ in $(seq 1 50); do
  [[ -s "$work_dir/port" ]] && break
  sleep 0.1
done

if [[ ! -s "$work_dir/port" ]]; then
  printf 'ticketed smoke server did not publish a port\n' >&2
  exit 1
fi

port="$(<"$work_dir/port")"
base_url="http://127.0.0.1:${port}/ticketed.wav"
ticketed_url="${base_url}?access_token=${token}"

unauthorized_status="$(curl -sS -o "$work_dir/unauthorized.body" -w '%{http_code}' "${base_url}?access_token=wrong")"
if [[ "$unauthorized_status" != "401" ]]; then
  printf 'expected unauthorized ticket request to return 401, got %s\n' "$unauthorized_status" >&2
  exit 1
fi

curl -fsS "$ticketed_url" -o "$work_dir/fetched.wav"
cmp "$work_dir/ticketed.wav" "$work_dir/fetched.wav" >/dev/null

gst-launch-1.0 -q playbin \
  uri="$ticketed_url" \
  audio-sink=fakesink \
  video-sink=fakesink

mpv \
  --no-config \
  --really-quiet \
  --no-video \
  --ao=null \
  --force-window=no \
  --log-file="$work_dir/mpv.raw.log" \
  "$ticketed_url"

python3 - "$work_dir/mpv.raw.log" "$work_dir/mpv.redacted.log" "$token" <<'PY'
import re
import sys

raw_path, redacted_path, token = sys.argv[1:]
with open(raw_path, encoding="utf-8", errors="replace") as raw_file:
    text = raw_file.read()
text = re.sub(r"access_token=[^\s&]+", "access_token=<redacted>", text)
text = text.replace(token, "<redacted>")
with open(redacted_path, "w", encoding="utf-8") as redacted_file:
    redacted_file.write(text)
if token in text:
    raise SystemExit("raw ticket leaked into redacted mpv log")
PY
rm -f "$work_dir/mpv.raw.log"

if grep -R -n -- "$token" "$work_dir/server.log" "$work_dir/mpv.redacted.log" "$work_dir/unauthorized.body" >/dev/null; then
  printf 'raw ticket leaked into retained playback auth smoke artifacts\n' >&2
  exit 1
fi

printf 'Playback auth smoke passed\n'
printf ' - unauthorized ticket rejected with HTTP 401\n'
printf ' - ticketed bytes fetched and matched fixture\n'
printf ' - GStreamer playbin completed with fakesinks\n'
printf ' - MPV completed with --no-config --ao=null\n'
printf ' - retained logs are redacted\n'
if [[ "$keep_artifacts" == "1" ]]; then
  printf 'Artifacts: %s\n' "$work_dir"
fi
