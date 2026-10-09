# Production deployment

## Standalone output

Build the application and deploy the complete `dist/` directory. Its
`plec-server.json` manifest, `server/` artifacts and application entry,
`client/` assets, and `public/` files are resolved relative to that manifest.
The output may be copied elsewhere and served without the source checkout.

Run it with Node.js **22.20.0 or newer** and the explicit runtime dependency
`@plec/node` (including its N-API native binding for the host platform):

```sh
plec serve dist
```

`plec serve` is the JS package shim and runs `@plec/node`, which crosses N-API
to `plec-server-engine`. The Rust CLI does not own or launch production HTTP
serving. Node is the only first-party production HTTP host; there is no Axum
fallback and no sidecar/private HTTP protocol. Native bindings are provided
for Linux x64 GNU/musl, Linux arm64 GNU, macOS x64/arm64, and Windows x64 MSVC.

The default bind host is `127.0.0.1`; set `--host 0.0.0.0` for container
ingress. The port is `--port <PORT>`, then the `PORT` environment variable,
then `3000`. `SIGINT` and `SIGTERM` stop admission, drain requests for up to
five seconds, close remaining connections, and close the native application.

## Reverse proxies

Forwarding headers are ignored by default: scheme is taken from the direct
transport and authority from the validated `Host` header. Enable trusted proxy
mode with `plec serve --trust-proxy` only behind a trusted reverse proxy that
strips/replaces client-supplied forwarding headers. In that mode the first
`X-Forwarded-Proto` value (`http` or `https`) and first `X-Forwarded-Host`
value may supply the canonical scheme and authority; absent proto falls back
to transport and absent host falls back to `Host`. Malformed forwarded hosts
are rejected. This canonical URL is used consistently for
`requestContext().url`, document SSR, and action origin handling. Blindly
trusting client-supplied forwarding headers lets clients spoof URL authority
and scheme.

## Generic container

Build `dist/` before the image build. The runtime image needs Node and the
application's `@plec/node` runtime dependency, but no Cargo/Rust toolchain.

```dockerfile
FROM node:22.20.0-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production
COPY package.json yarn.lock ./
RUN corepack enable && yarn install --immutable --production
COPY dist/ ./dist/
EXPOSE 3000
CMD ["yarn", "plec", "serve", "dist", "--host", "0.0.0.0"]
```

Set `PORT` for the desired listening port. Add `--trust-proxy` only when the
container is behind a trusted proxy that replaces forwarding headers.
Writable/persistent storage is application-owned and is not provided by Plec.
