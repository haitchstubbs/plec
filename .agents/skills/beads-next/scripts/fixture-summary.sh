#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <fixture.json>" >&2
  exit 1
fi

FILE="$1"

python3 - "$FILE" <<'PY'
import json
import sys

path = sys.argv[1]

with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

components = data.get("components")

if components is None:
    print("No top-level 'components' array found.")
    print("Top-level keys:", ", ".join(data.keys()))
    sys.exit(1)

for ci, component in enumerate(components):
    print(f"component {ci}")

    name = component.get("name")
    if name is not None:
        print(f"  name: {name}")

    nodes = component.get("nodes", [])

    print("  nodes:")

    for ni, node in enumerate(nodes):
        op = node.get("op", "?")

        details = []

        for key in (
            "tag",
            "component",
            "parent",
            "binding",
            "expression",
            "loop",
            "conditional",
        ):
            if key in node:
                details.append(f"{key}={node[key]!r}")

        print(
            f"    {ni:>3}  {op}"
            + (f"  {' '.join(details)}" if details else "")
        )

    bindings = component.get("bindings", [])

    if bindings:
        print("  bindings:")

        for bi, binding in enumerate(bindings):
            print(
                f"    {bi:>3}"
                f" target={binding.get('target')!r}"
                f" sink={binding.get('sink')!r}"
                f" expression={binding.get('expression')!r}"
            )

    expressions = component.get("expressions", [])

    if expressions:
        print("  expressions:")

        for ei, expression in enumerate(expressions):
            compact = json.dumps(
                expression,
                separators=(",", ":"),
                ensure_ascii=False,
            )

            print(f"    {ei:>3}  {compact}")

    print()
PY