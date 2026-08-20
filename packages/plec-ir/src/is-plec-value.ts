import type { PlecGraphArtifact, PlecValueType } from './index';

export function isPlecValue(
  value: unknown,
  type: PlecValueType,
): boolean {
  if (type.kind === 'null') return value === null;
  if (
    type.kind === 'boolean' ||
    type.kind === 'number' ||
    type.kind === 'string'
  )
    return typeof value === type.kind;
  if (type.kind === 'array')
    return (
      Array.isArray(value) &&
      value.every((item) => isPlecValue(item, type.item))
    );
  if (
    value === null ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.getPrototypeOf(value) !== Object.prototype
  )
    return false;
  const object = value as Record<string, unknown>;
  return (
    Object.keys(object).length === Object.keys(type.fields).length &&
    Object.entries(type.fields).every(
      ([key, field]) =>
        Object.hasOwn(object, key) && isPlecValue(object[key], field),
    )
  );
}

export function plecValueTypeEquals(
  left: PlecValueType,
  right: PlecValueType,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function validatePlecGraphArtifact(
  artifact: PlecGraphArtifact,
): PlecGraphArtifact {
  const unique = (kind: string, values: Array<{ id: string }>) => {
    const seen = new Set<string>();

    for (const entry of values) {
      if (seen.has(entry.id)) {
        throw new Error(
          `DUPLICATE_GRAPH_INTERFACE_ID: ${kind}:${entry.id}`,
        );
      }

      seen.add(entry.id);
    }
  };

  unique('input', artifact.interface.inputs);
  unique('output', artifact.interface.outputs);
  unique('command', artifact.interface.commands);
  unique('outlet', artifact.interface.outlets);

  for (const input of artifact.interface.inputs) {
    if (
      input.default !== undefined &&
      !isPlecValue(input.default, input.type)
    ) {
      throw new Error(`INVALID_INPUT_DEFAULT: ${input.id}`);
    }
  }

  return artifact;
}

export type { PlecValueType, PlecGraphArtifact };
export type {
  GraphCommand,
  GraphInterface,
  GraphInput,
  GraphOutput,
  OutletChildContract,
} from './index';
