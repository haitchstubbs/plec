# `@plec/node`

The Node.js host and native Rust bindings for Plec applications.

The package requires Node.js 22.20.0 or later and installs the matching
platform binding as an optional dependency. Supported targets:

- Linux x64 GNU and musl
- Linux arm64 GNU
- macOS x64 and arm64
- Windows x64 MSVC

The public API is `createPlecHandler()` and `serve()`. Applications are built
with `plec build` and served from the resulting distribution directory.

The package metadata and optional platform dependencies target all six
platforms. The package-install/native-load smoke test runs on the active Linux
GNU runner; this repository does not currently run native-load smoke tests on
macOS or Windows.

`build:native` builds the current host; `build:native:target` and
`build:native:cross` accept NAPI-RS target triples. `package:stage` collects
the completed target binaries into the generated platform-package directories.
`test:package` packs the wrapper and the active host's platform package, then
installs and loads them in an isolated temporary npm install. These commands
build and test packages locally; they do not publish them.

Use `build:native:target` on a host that can build the selected Rust target,
or `build:native:cross` when its NAPI-RS cross toolchain supports that target.
`package:stage` requires binaries for all six configured targets.
