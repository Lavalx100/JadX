#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")" && pwd)"
user_home="${HOME:?HOME is required on macOS}"
lms_path="${JADX_LMS_PATH:-$user_home/.lmstudio/bin/lms}"

echo "JadX setup"
echo "Project: $project_dir"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This setup currently targets macOS." >&2
  exit 1
fi
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Warning: Apple Silicon was not detected; Tauri can still build, but MLX validation is unavailable."
fi

command -v node >/dev/null || { echo "Node.js 20+ is required." >&2; exit 1; }
command -v npm >/dev/null || { echo "npm is required." >&2; exit 1; }
if ! command -v rustc >/dev/null; then
  echo "Rust is missing. Install it with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi
if [[ ! -x "$lms_path" ]]; then
  echo "LM Studio lms CLI was not found at $lms_path"
  echo "Install LM Studio or set JADX_LMS_PATH to the correct CLI path."
  exit 1
fi

echo "Node: $(node --version)"
echo "Rust: $(rustc --version)"
echo "Xcode: $(xcode-select -p 2>/dev/null || echo unavailable)"
echo "Hardware memory: $(sysctl -n hw.memsize) bytes"
echo "LM Studio models:"
"$lms_path" ls || true

if curl -fsS --max-time 3 http://127.0.0.1:1234/api/v1/models >/tmp/jadx-models.json; then
  echo "LM Studio API: available"
  echo "Discovered models:"
  node -e 'const fs=require("fs"); const d=JSON.parse(fs.readFileSync("/tmp/jadx-models.json","utf8")); for (const m of (d.models||[])) console.log(`- ${m.key||m.id} (${m.architecture||m.arch||"unknown"})`);'
else
  echo "LM Studio API: offline"
  echo "Start it with: $lms_path server start"
fi

echo "Installing JavaScript dependencies"
cd "$project_dir"
npm install
echo "Building the frontend"
npm run build
echo "Setup checks complete. Launch with: npm run tauri:dev"
