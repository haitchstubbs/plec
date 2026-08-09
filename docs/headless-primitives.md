# Headless semantic primitives

Headless-library adapters lower public component calls into small runtime semantics. Do not compile a third-party component's hooks, effects, refs, or package-private DOM implementation.

## Toggle

`Toggle` is the first semantic primitive. It owns a native checkbox input, an authored root part, and an optional authored indicator part. Its state is `false`, `true`, or `mixed`.

The compiler test fixture imports `Toggle` from `@wasm-runtime/internal-toggle`; this is not a supported application package. The accepted shape is:

```tsx
<Toggle.Root checked={todo.done} onCheckedChange={update} className="root">
  <Toggle.Indicator className="indicator">✓</Toggle.Indicator>
</Toggle.Root>
```

`checked` creates a controlled toggle. `defaultChecked` creates runtime-owned state, restored by native form reset. `indeterminate`, `disabled`, `readOnly`, `required`, `name`, `value`, and `form` are supported. The runtime writes native input properties and `aria-checked`, while Root and Indicator receive `data-state`; indicator presence is represented with `hidden`.

Unsupported: prop spreads with unknown shape, refs, render props, arbitrary child composition, groups, and library-specific event bubbling. The runtime records only direct state-driven DOM writes in its existing mutation metrics.

## Adding an adapter

1. Add one explicit import alias in the compiler semantic registry; never inspect the package implementation.
2. Map its root props and parts to `Toggle`, retaining authored classes, styles, and children.
3. Add deterministic compiler snapshots for a supported example and diagnostics for unsupported API shapes.
4. Validate native interaction, controlled synchronization, form/reset behavior, indicator visibility, and DOM-operation counts in the runtime.
5. Add the library/version and supported prop subset to this document's support matrix.

Planned order: Radix Checkbox, then Switch. Checkbox Group, portals, overlays, focus scopes, roving focus, and positioned layers require separate primitives and are intentionally deferred.

## Support matrix

| Source surface | Status | Notes |
| --- | --- | --- |
| Internal Toggle fixture | supported | Compiler/runtime contract only. |
| Base UI Checkbox | supported | Root + Indicator map to Toggle; groups and render callbacks remain unsupported. |
| Radix Checkbox | planned | Portability proof after Base UI. |
| Switch | planned | Reuses Toggle where semantics fit. |
