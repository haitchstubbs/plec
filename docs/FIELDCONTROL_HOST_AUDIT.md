# FieldControl host-operation audit

This records the reachable operations before extending the host protocol. It
does not prescribe component-specific behavior.

| Source | Construct | Bounded capability | Classification | Reactive/cleanup |
| --- | --- | --- | --- | --- |
| `field/control/FieldControl.mjs` | `validation.inputRef.current?.value` | `element.property(ref, "value")` | read | layout dependency; feeds `setFilled` |
| `field/control/FieldControl.mjs` | `inputRef.current === activeElement(ownerDocument(inputRef.current))` | `element.isActive(ref)` | query | layout dependency; no cleanup |
| `field/control/FieldControl.mjs` | `event.currentTarget.value`, `.tagName`, `.key` | typed event fields | read | event-local; no cleanup |
| `internals/field-register-control/useRegisterFieldControl.mjs` | `registerFieldControl(source, registration)` | `resource.upsert(controller, source, record)` | registration | layout rerun updates in place |
| same | `registerFieldControl(source, undefined)` | `resource.remove(controller, source)` | unregister | layout cleanup/disposal |
| `internals/labelable-provider/useLabelableId.mjs` | `controlRef.current`, `elem.closest('label')` | `element.closest(ref, "label")` | query | layout dependency; no cleanup |
| same | `registerControlId(source, nextId)` | `resource.upsert(controller, source, value)` | registration | normal-effect cleanup unregisters |
| `internals/useRenderElement.mjs` | ref arrays and forwarded refs | `ref.attach` / `ref.detach` fan-out | resource attachment | detach on replacement/disposal |

The closure does **not** require dynamic property names or arbitrary method
calls for these paths. `Map` and `Symbol` in the source are implementation
details of registry identity; the compiled representation needs only a stable
opaque resource source id and keyed upsert/remove semantics.

The next IR surface is therefore deliberately finite: opaque host element ref
ids; properties `value`, `name`, `disabled`, `checked`, and `tagName`; the
`isActive` and static-selector `closest` queries; typed event fields; resource
upsert/remove; and `layout` lifecycle descriptors.
