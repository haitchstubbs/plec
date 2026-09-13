#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

find_chrome() {
  local candidates=()

  [[ -n "${PLEC_CHROME_EXECUTABLE:-}" ]] &&
    candidates+=("$PLEC_CHROME_EXECUTABLE")

  for cmd in google-chrome google-chrome-stable chromium chromium-browser; do
    if command -v "$cmd" >/dev/null 2>&1; then
      candidates+=("$(command -v "$cmd")")
    fi
  done

  while IFS= read -r file; do
    candidates+=("$file")
  done < <(
    find "$HOME/.cache/ms-playwright" \
      -maxdepth 4 \
      -type f \
      \( -name chrome -o -name chrome-headless-shell \) \
      2>/dev/null || true
  )

  for candidate in "${candidates[@]}"; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  return 1
}

find_driver() {
  local candidates=()

  [[ -n "${CHROMEDRIVER:-}" ]] &&
    candidates+=("$CHROMEDRIVER")

  candidates+=(
    "$ROOT/.tools/chromedriver-linux64/chromedriver"
    "$ROOT/.tools/chromedriver-win64/chromedriver.exe"
  )

  if command -v chromedriver >/dev/null 2>&1; then
    candidates+=("$(command -v chromedriver)")
  fi

  for candidate in "${candidates[@]}"; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  return 1
}

chrome="$(find_chrome || true)"
driver="$(find_driver || true)"

echo "platform: $(uname -s) $(uname -m)"

if [[ -n "$chrome" ]]; then
  echo "chrome: $chrome"
  "$chrome" --version || true

  if command -v ldd >/dev/null 2>&1; then
    missing="$(ldd "$chrome" 2>/dev/null | grep 'not found' || true)"

    if [[ -n "$missing" ]]; then
      echo
      echo "missing Chrome libraries:"
      echo "$missing"
    fi
  fi
else
  echo "chrome: NOT FOUND"
fi

echo

if [[ -n "$driver" ]]; then
  echo "chromedriver: $driver"
  "$driver" --version || true

  if command -v ldd >/dev/null 2>&1; then
    missing="$(ldd "$driver" 2>/dev/null | grep 'not found' || true)"

    if [[ -n "$missing" ]]; then
      echo
      echo "missing ChromeDriver libraries:"
      echo "$missing"
    fi
  fi
else
  echo "chromedriver: NOT FOUND"
fi

if [[ -n "$chrome" && -n "$driver" ]]; then
  chrome_major="$("$chrome" --version 2>/dev/null | grep -oE '[0-9]+' | head -1)"
  driver_major="$("$driver" --version 2>/dev/null | grep -oE '[0-9]+' | head -1)"

  echo
  echo "chrome major:       $chrome_major"
  echo "chromedriver major: $driver_major"

  if [[ "$chrome_major" != "$driver_major" ]]; then
    echo "ERROR: Chrome and ChromeDriver major versions do not match." >&2
    exit 2
  fi

  echo "browser pair: OK"
fi