# Plec _(pl-eh-ch)_

> **Experimental.** Plec is an early-stage experiment validating its architecture, not a production framework. Some capabilities described here are design targets, not shipped features — see [AGENTS.md](AGENTS.md) for goals and explicit non-goals.

A full-stack framework for building data-intensive web applications around a compiled semantic application graph.

Plec treats application UI differently from most web frameworks.

Instead of compiling components into application-specific JavaScript that owns rendering, reconciliation, and runtime behavior, Plec compiles application semantics into a portable graph executed by a generic runtime.

The result is closer to a UI ABI than a traditional frontend bundle.

## Why Plec?

Modern frameworks tend to optimize one layer of the application stack.

React optimizes the authoring model. Svelte and Solid optimize UI execution. Blazor, Leptos, and Dioxus explore alternative runtime models. Meta's LightSpeed demonstrated the value of representing significant application behavior as structured data rather than handwritten UI logic.

Plec attempts to combine these ideas into one architecture.

| Technology                                        | Client/server semantic drift | Runtime reconciliation overhead | Shipping app logic/runtime to client | UI portability across hosts | Static validation before execution | Capability / security boundaries | Versioned UI compatibility | SSR ↔ interactive boundary | Inspectable / transformable app semantics | Familiar productive UI authoring |
| ------------------------------------------------- | ---------------------------: | ------------------------------: | -----------------------------------: | --------------------------: | ---------------------------------: | -------------------------------: | -------------------------: | -------------------------: | ----------------------------------------: | -------------------------------: |
| **React** ([React][1])                            |                           ❌ |                              ❌ |                                   ❌ |                          ❌ |                                 ❌ |                               ❌ |                         ❌ |                         ❌ |                                        ❌ |                               ✅ |
| **React Server Components / Flight** ([React][2]) |                           ✅ |                              ❌ |                                   ✅ |                          ❌ |                                 ❌ |                               ❌ |                         ❌ |                         ✅ |                                        ✅ |                               ✅ |
| **Svelte / SvelteKit** ([Svelte][3])              |                           ❌ |                              ✅ |                                   ✅ |                          ❌ |                                 ✅ |                               ❌ |                         ❌ |                         ✅ |                                        ❌ |                               ✅ |
| **Solid / SolidStart** ([Solid Documentation][4]) |                           ❌ |                              ✅ |                                   ❌ |                          ❌ |                                 ❌ |                               ❌ |                         ❌ |                         ✅ |                                        ❌ |                               ✅ |
| **Meta LightSpeed** ([Engineering at Meta][5])    |                           ✅ |                              ✅ |                                   ✅ |                          ✅ |                                 ✅ |                               ✅ |                         ✅ |                         ✅ |                                        ✅ |                               ❌ |
| **Blazor WebAssembly** ([Microsoft Learn][6])     |                           ✅ |                              ❌ |                                   ❌ |                          ✅ |                                 ✅ |                               ✅ |                         ❌ |                         ✅ |                                        ❌ |                               ✅ |
| **Leptos** ([Leptos][7])                          |                           ✅ |                              ✅ |                                   ❌ |                          ✅ |                                 ✅ |                               ✅ |                         ❌ |                         ✅ |                                        ❌ |                               ❌ |
| **Dioxus** ([Dioxus Labs][8])                     |                           ✅ |                              ❌ |                                   ❌ |                          ✅ |                                 ✅ |                               ✅ |                         ❌ |                         ✅ |                                        ❌ |                               ❌ |
| **Plec**                                          |                           ✅ |                              ✅ |                                   ✅ |                          🚧 |                                 ✅ |                               🚧 |                         🚧 |                         🚧 |                                        ✅ |                               ✅ |

The matrix is intentionally strict. A ✅ means the architecture directly addresses the problem, not merely that a solution could be built on top of it. A 🚧 marks a design target the architecture is built around but that is not yet implemented in Plec.

### Client/server semantic drift

In many full-stack applications, client and server are separate programs connected by conventions: route names, payload shapes, serialization rules, permissions, and assumptions that both sides must keep in sync.

Some frameworks reduce this problem. Dioxus server functions, for example, derive the request boundary from a typed function definition. React Server Components establish an explicit serialization boundary between server and client components.

Plec takes the same principle further: application boundaries are part of the compiled contract and can therefore be validated before execution.

### Runtime reconciliation

React preserves a highly productive component model by reconciling runtime representations of the UI.

Svelte and Solid attack the same problem differently, moving more knowledge into compilation and generating targeted updates rather than repeatedly diffing a component tree.

Plec similarly moves work out of the hot runtime path, but does not compile each application into bespoke rendering code. The compiler produces a semantic graph that a generic executor can update directly.

### Application logic as code versus semantics

This is the central architectural distinction.

React, Solid, Svelte, Leptos, Dioxus, and Blazor ultimately ship or execute application programs.

Plec instead compiles the relevant application semantics into structured data:

```text
Application source
       │
       ▼
┌────────────────────────────┐
│ Semantic application graph │
│                            │
│ UI structure               │
│ state                      │
│ dependencies               │
│ actions                    │
│ events                     │
│ effects                    │
│ capabilities               │
│ ownership                  │
│ routing / outlets          │
│ interfaces                 │
└─────────────┬──────────────┘
              │
              ▼
        generic executor
```

The graph is still a build artifact, but it is not simply another JavaScript program.

It is a machine-readable description of what the application is allowed to do and how its pieces relate.

That distinction makes application behavior inspectable before it runs. A graph can be validated, versioned, transformed, optimized, capability-restricted, compared for compatibility, or potentially executed by different hosts without requiring the original TypeScript source.

## The architectural families

| Family                   | Primarily optimizes                                    |
| ------------------------ | ------------------------------------------------------ |
| React                    | Authoring model                                        |
| Svelte / Solid           | UI execution                                           |
| Blazor / Leptos / Dioxus | Runtime and platform                                   |
| Meta LightSpeed          | Application representation                             |
| **Plec**                 | **Application representation + execution + authoring** |

Plec is not an attempt to replace JavaScript with WebAssembly.

WebAssembly is an implementation detail of the runtime. Today both halves are Rust: application source compiles through the Rust pipeline in `crates/*`, and the executor ships as the Rust runtime compiled to WebAssembly in `packages/plec/dist/runtime`.

The larger idea is to change the boundary between application source and application execution:

```text
Traditional framework

source
  ↓
application-specific executable code
  ↓
framework/runtime
  ↓
browser
```

```text
Plec

source
  ↓
semantic application contract
  ↓
generic executor
  ↓
host capabilities
  ↓
browser
```

## Standing on existing ideas

None of the individual ideas behind Plec are unprecedented.

Compiler-driven UI frameworks have demonstrated that much of rendering can be resolved ahead of time.

- Typed full-stack frameworks have shown that client/server boundaries can be generated rather than manually coordinated.
- WebAssembly provides a portable execution target.
- Server-driven UI systems have demonstrated that interface behavior can be represented as structured data.

Meta's Project LightSpeed is a particularly useful precedent.

In rebuilding Messenger, Meta moved significant application behavior into a compact, data-driven architecture backed by SQLite and reusable UI templates. Meta reported reducing Messenger's core codebase from more than 1.7 million lines to roughly 360,000 while substantially improving startup performance and application size.

Plec generalizes the underlying architectural idea.

Rather than building a semantic application system for one product, Plec makes the semantic contract itself the framework primitive.

Developers continue to author familiar application code. The compiler extracts its semantics. The runtime executes the resulting contract.

**Plec preserves familiar authoring while giving the application a concrete, inspectable execution contract.**

[1]: https://react.dev/ 'React'
[2]: https://react.dev/reference/rsc/server-components 'Server Components'
[3]: https://svelte.dev/ 'Svelte'
[4]: https://docs.solidjs.com/concepts/understanding-jsx 'Understanding JSX - SolidJS Documentation'
[5]: https://engineering.fb.com/2020/03/02/data-infrastructure/messenger/ 'Project LightSpeed: Rewriting Messenger to be faster, smaller and simpler'
[6]: https://learn.microsoft.com/en-us/aspnet/core/blazor/webassembly-build-tools-and-aot?view=aspnetcore-10.0 'ASP.NET Core Blazor WebAssembly'
[7]: https://leptos.dev/ 'Leptos'
[8]: https://dioxuslabs.com/learn/0.7/essentials/fullstack/ 'Dioxus'
