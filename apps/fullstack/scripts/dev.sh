#!/usr/bin/env bash
set -euo pipefail

yarn check:no-react
yarn build:css:public
yarn copy:fonts:public

tailwindcss --watch -i src/styles.css -o public/assets/styles.css &
css_pid=$!
cleanup() {
  kill "$css_pid" 2>/dev/null || true
  wait "$css_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

plec dev src/app.tsx --out-dir dist
