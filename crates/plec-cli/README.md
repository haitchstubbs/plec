# Plec CLI

Command-line tooling for compiling and inspecting Plec applications.

The CLI exposes the Plec compiler pipeline directly, making it possible to inspect executable IR, route manifests, and compiler state without starting a browser runtime or development server.

## Dev CLI vs app CLI

The `plec` binary contains two frontends: the **dev CLI** (compiler inspection: `inspect`, `raw`, `routes`, `build`) and the **app CLI**. Which frontend handles a command is selected at **compile time**, not process startup:

- `crates/plec-cli/build.rs` lifts `PLEC_CLI_VERSION` from `.env.plec` at the workspace root (parsed with `dotenvy`) and bakes it into the crate with `cargo:rustc-env`.
- `lib.rs` selects the frontend with `option_env!("PLEC_CLI_VERSION")`: `release` selects the app CLI, any other value (or an absent variable) keeps the dev CLI.

Because the selection is baked in, changing `.env.plec` triggers a rebuild of the CLI through the build script's `rerun-if-changed` directive, and an explicitly exported `PLEC_CLI_VERSION` in the build environment wins over the file:

```bash
# .env.plec
PLEC_CLI_VERSION='release'

# pick up the change in the binary
cargo build -p plec-cli
```

## Usage

```bash
plec <command>
```

Available commands:

```text
inspect   Query the compiled application
raw       Print the compiled executable application
routes    Print the application's route manifest
```

## `raw`

Compile a Plec application and print its executable component graph as JSON.

```bash
plec raw <source>
```

Example:

```bash
plec raw apps/fullstack/src/main.tsx
```

The command runs the application through the normal compiler pipeline:

```text
source graph
    ↓
semantic graph
    ↓
root component discovery
    ↓
application HIR
    ↓
executable IR
```

The resulting `ComponentApplication` is written to stdout as formatted JSON.

This is useful when debugging lowering, inspecting generated IR, or verifying compiler output directly.

---

## `inspect`

Compile an application and query its executable graph through the Plec inspector.

```bash
plec inspect <source> '<query>'
```

Example:

```bash
plec inspect apps/fullstack/src/main.tsx '
{
  components {
    id
  }
}'
```

The inspector operates directly against the in-memory compiled application. It does not start an HTTP or GraphQL server.

Query results are emitted as JSON:

```json
{
  "components": [
    {
      "id": "App"
    }
  ]
}
```

Inspector/query errors are written to stderr.

This command is intended for targeted compiler debugging where dumping the entire IR with `raw` would be unnecessarily noisy.

---

## `routes`

Parse an application and print its compiled route manifest.

```bash
plec routes <source>
```

Example:

```bash
plec routes apps/fullstack/src/main.tsx
```

The command performs source discovery and semantic analysis before lowering route declarations into Plec's route representation and serializing the resulting manifest.

Example output:

```json
{
  "routes": [
    {
      "path": "/"
    },
    {
      "path": "/todos"
    }
  ]
}
```

The exact manifest structure is defined by the current Plec route IR.

---

## Running from the workspace

During development, the CLI can be run directly through Cargo:

```bash
cargo run -p plec-cli -- raw apps/fullstack/src/main.tsx
```

```bash
cargo run -p plec-cli -- routes apps/fullstack/src/main.tsx
```

```bash
cargo run -p plec-cli -- inspect apps/fullstack/src/main.tsx '{ components { id } }'
```

To see Clap's generated help:

```bash
cargo run -p plec-cli -- --help
```

or:

```bash
plec --help
```

## Compiler pipeline

The CLI intentionally uses the same compiler crates as the rest of Plec rather than maintaining a separate inspection path.

For application compilation it performs:

1. `read_source_graph`
2. `build_semantic_graph`
3. `discover_root_component`
4. `lower_application`
5. `lower_application_to_executable`

For route inspection it performs:

1. `read_source_graph`
2. `build_semantic_graph`
3. `lower_routes`
4. `lower_route_manifest`

This makes the CLI useful as a thin debugging surface over the canonical compiler implementation.

## Commands at a glance

| Command        | Input                | Output              | Purpose                            |
| -------------- | -------------------- | ------------------- | ---------------------------------- |
| `plec raw`     | Source entry         | Executable IR JSON  | Inspect complete compiler output   |
| `plec inspect` | Source entry + query | Query result JSON   | Targeted inspection of compiled IR |
| `plec routes`  | Source entry         | Route manifest JSON | Inspect router compilation         |

## Status

The CLI currently focuses on compiler inspection and diagnostics.

It is expected to grow alongside Plec's toolchain, with higher-level commands such as application compilation, development workflows, and build orchestration remaining separate concerns until their interfaces are stable.
