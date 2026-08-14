import {
  isPlecValue,
  plecValueTypeEquals,
  validatePlecGraphArtifact,
  type GraphCommand,
  type GraphInterface,
  type GraphOutput,
  type OutletChildContract,
  type PlecGraphArtifact,
} from '../../plec-ir/dist/index.js';

export type GraphInstanceId = string;
export type OutputWiring = Record<
  string,
  (message: {
    instanceId: GraphInstanceId;
    outputId: string;
    payload: unknown;
  }) => void
>;
export interface MountGraphRequest {
  parentInstanceId: GraphInstanceId;
  outletId: string;
  graphId: string;
  revision: string;
  artifactUrl?: string;
  effectsUrl?: string;
  inputs: Record<string, unknown>;
  outputWiring: OutputWiring;
}
export interface ReplaceGraphRequest {
  parentInstanceId: GraphInstanceId;
  outletId: string;
  next: MountGraphRequest;
}
export interface GraphRuntimeAdapter {
  mount(instance: {
    instanceId: GraphInstanceId;
    artifact: PlecGraphArtifact;
    parentInstanceId?: GraphInstanceId;
    outletId?: string;
    inputs: Record<string, unknown>;
  }): void;
  dispose(instanceId: GraphInstanceId): void;
  setInput(
    instanceId: GraphInstanceId,
    inputId: string,
    value: unknown,
  ): void;
  requestCommand?(message: {
    instanceId: GraphInstanceId;
    commandId: string;
    capability: string;
    payload: unknown;
  }): void;
}
export type GraphArtifactLoader = (
  request: Pick<
    MountGraphRequest,
    'graphId' | 'revision' | 'artifactUrl' | 'effectsUrl'
  >,
) => Promise<unknown>;

export class GraphContractError extends Error {
  constructor(
    public readonly code: string,
    public readonly context: Record<string, string>,
  ) {
    super(
      `${code}: ${Object.entries(context)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, value]) => `${key}=${value}`)
        .join(', ')}`,
    );
    this.name = 'GraphContractError';
  }
}

type Instance = {
  id: GraphInstanceId;
  artifact: PlecGraphArtifact;
  parentId?: GraphInstanceId;
  outletId?: string;
  contract?: OutletChildContract;
  wiring: OutputWiring;
  children: Map<string, GraphInstanceId>;
};

/** Coordinator-owned composition boundary. The runtime sees isolated graph
 * instances and explicit messages only; it never exposes graph internals. */
export class PlecGraphCoordinator {
  private readonly instances = new Map<GraphInstanceId, Instance>();
  private nextId = 1;
  constructor(
    private readonly runtime: GraphRuntimeAdapter,
    private readonly load: GraphArtifactLoader,
  ) {}

  mountRoot(
    artifactValue: unknown,
    inputs: Record<string, unknown> = {},
  ): GraphInstanceId {
    const artifact = validatePlecGraphArtifact(artifactValue);
    this.validateInputs(artifact.interface, inputs, {
      graphId: artifact.graphId,
      revision: artifact.revision,
    });
    const instanceId = this.allocateId();
    this.instances.set(instanceId, {
      id: instanceId,
      artifact,
      wiring: {},
      children: new Map(),
    });
    this.runtime.mount({
      instanceId,
      artifact,
      inputs: this.resolveInputs(artifact.interface, inputs),
    });
    return instanceId;
  }

  async mountGraph(
    request: MountGraphRequest,
  ): Promise<GraphInstanceId> {
    const parent = this.instances.get(request.parentInstanceId);
    if (!parent)
      throw this.error('UNKNOWN_OUTLET', request, {
        parentInstanceId: request.parentInstanceId,
      });
    const outlet = parent.artifact.interface.outlets.find(
      (entry) => entry.id === request.outletId,
    );
    if (!outlet)
      throw this.error('UNKNOWN_OUTLET', request, {
        parentInstanceId: parent.id,
      });
    if (parent.children.has(outlet.id))
      throw this.error('DUPLICATE_INSTANCE_PORT', request, {
        parentInstanceId: parent.id,
      });
    const artifact = validatePlecGraphArtifact(
      await this.load(request),
    );
    if (
      artifact.graphId !== request.graphId ||
      artifact.revision !== request.revision
    )
      throw this.error('STALE_GRAPH_REVISION', request, {
        fetchedGraphId: artifact.graphId,
        fetchedRevision: artifact.revision,
      });
    this.validateCompatibility(
      artifact.interface,
      outlet.accepts,
      request,
    );
    const instanceId = this.allocateId();
    const child: Instance = {
      id: instanceId,
      artifact,
      parentId: parent.id,
      outletId: outlet.id,
      contract: outlet.accepts,
      wiring: request.outputWiring,
      children: new Map(),
    };
    this.instances.set(instanceId, child);
    parent.children.set(outlet.id, instanceId);
    this.runtime.mount({
      instanceId,
      artifact,
      parentInstanceId: parent.id,
      outletId: outlet.id,
      inputs: this.resolveInputs(artifact.interface, request.inputs),
    });
    return instanceId;
  }

  async replaceGraph(
    request: ReplaceGraphRequest,
  ): Promise<GraphInstanceId> {
    if (
      request.parentInstanceId !== request.next.parentInstanceId ||
      request.outletId !== request.next.outletId
    )
      throw this.error('UNKNOWN_OUTLET', request.next, {
        replacement: 'parent/outlet mismatch',
      });
    const parent = this.instances.get(request.parentInstanceId);
    if (
      !parent ||
      !parent.artifact.interface.outlets.some(
        (outlet) => outlet.id === request.outletId,
      )
    )
      throw this.error('UNKNOWN_OUTLET', request.next, {
        parentInstanceId: request.parentInstanceId,
      });
    const existing = parent.children.get(request.outletId);
    if (existing) this.dispose(existing);
    return this.mountGraph(request.next);
  }

  setInput(
    instanceId: GraphInstanceId,
    inputId: string,
    value: unknown,
  ): void {
    const instance = this.instances.get(instanceId);
    if (!instance)
      throw new GraphContractError('INVALID_INPUT_VALUE', {
        instanceId,
        inputId,
      });
    const input = instance.artifact.interface.inputs.find(
      (entry) => entry.id === inputId,
    );
    if (!input || !isPlecValue(value, input.type))
      throw new GraphContractError('INVALID_INPUT_VALUE', {
        graphId: instance.artifact.graphId,
        inputId,
        instanceId,
        revision: instance.artifact.revision,
      });
    this.runtime.setInput(
      instanceId,
      inputId,
      copyBoundaryValue(value),
    );
  }

  emitOutput(
    instanceId: GraphInstanceId,
    outputId: string,
    payload: unknown,
  ): void {
    const instance = this.require(instanceId);
    const output = instance.artifact.interface.outputs.find(
      (entry) => entry.id === outputId,
    );
    if (!output || !isPlecValue(payload, output.payloadType))
      throw new GraphContractError('OUTPUT_CONTRACT_MISMATCH', {
        graphId: instance.artifact.graphId,
        instanceId,
        outputId,
        revision: instance.artifact.revision,
      });
    if (
      instance.contract &&
      !matchesOutput(output, instance.contract.outputs)
    )
      throw new GraphContractError('OUTPUT_CONTRACT_MISMATCH', {
        graphId: instance.artifact.graphId,
        instanceId,
        outputId,
        revision: instance.artifact.revision,
      });
    const handler = instance.wiring[outputId];
    if (!handler)
      throw new GraphContractError('INVALID_OUTPUT_WIRING', {
        graphId: instance.artifact.graphId,
        instanceId,
        outputId,
        revision: instance.artifact.revision,
      });
    handler({
      instanceId,
      outputId,
      payload: copyBoundaryValue(payload),
    });
  }

  requestCommand(
    instanceId: GraphInstanceId,
    commandId: string,
    payload: unknown,
  ): void {
    const instance = this.require(instanceId);
    const command = instance.artifact.interface.commands.find(
      (entry) => entry.id === commandId,
    );
    if (
      !command ||
      !isPlecValue(payload, command.payloadType) ||
      (instance.contract &&
        !matchesCommand(command, instance.contract.commands))
    )
      throw new GraphContractError('UNSUPPORTED_COMMAND', {
        commandId,
        graphId: instance.artifact.graphId,
        instanceId,
        revision: instance.artifact.revision,
      });
    this.runtime.requestCommand?.({
      instanceId,
      commandId,
      capability: command.capability,
      payload: copyBoundaryValue(payload),
    });
  }

  dispose(instanceId: GraphInstanceId): void {
    const instance = this.require(instanceId);
    for (const childId of [...instance.children.values()])
      this.dispose(childId);
    instance.children.clear();
    this.runtime.dispose(instanceId);
    this.instances.delete(instanceId);
    if (instance.parentId && instance.outletId)
      this.instances
        .get(instance.parentId)
        ?.children.delete(instance.outletId);
  }

  private validateCompatibility(
    child: GraphInterface,
    contract: OutletChildContract,
    request: MountGraphRequest,
  ): void {
    for (const input of child.inputs) {
      const accepted = contract.inputs.find(
        (entry) => entry.id === input.id,
      );
      if (!accepted || !plecValueTypeEquals(input.type, accepted.type))
        throw this.error('INPUT_CONTRACT_MISMATCH', request, {
          inputId: input.id,
        });
    }
    this.validateInputs(child, request.inputs, {
      graphId: request.graphId,
      revision: request.revision,
      outletId: request.outletId,
    });
    for (const output of child.outputs) {
      if (!matchesOutput(output, contract.outputs))
        throw this.error('OUTPUT_CONTRACT_MISMATCH', request, {
          outputId: output.id,
        });
      if (!request.outputWiring[output.id])
        throw this.error('INVALID_OUTPUT_WIRING', request, {
          outputId: output.id,
        });
    }
    for (const command of child.commands)
      if (!matchesCommand(command, contract.commands))
        throw this.error('COMMAND_CONTRACT_MISMATCH', request, {
          commandId: command.id,
        });
  }

  private validateInputs(
    graph: GraphInterface,
    values: Record<string, unknown>,
    context: Record<string, string>,
  ): void {
    for (const id of Object.keys(values))
      if (!graph.inputs.some((input) => input.id === id))
        throw new GraphContractError('INVALID_INPUT_VALUE', {
          ...context,
          inputId: id,
        });
    for (const input of graph.inputs) {
      const present = Object.hasOwn(values, input.id);
      if (input.required && !present && input.default === undefined)
        throw new GraphContractError('MISSING_REQUIRED_INPUT', {
          ...context,
          inputId: input.id,
        });
      if (present && !isPlecValue(values[input.id], input.type))
        throw new GraphContractError('INVALID_INPUT_VALUE', {
          ...context,
          inputId: input.id,
        });
    }
  }
  private resolveInputs(
    graph: GraphInterface,
    values: Record<string, unknown>,
  ): Record<string, unknown> {
    return Object.fromEntries(
      graph.inputs.flatMap((input) => {
        if (Object.hasOwn(values, input.id))
          return [[input.id, copyBoundaryValue(values[input.id])]];
        return input.default === undefined
          ? []
          : [[input.id, copyBoundaryValue(input.default)]];
      }),
    );
  }
  private require(id: string): Instance {
    const instance = this.instances.get(id);
    if (!instance)
      throw new GraphContractError('UNKNOWN_GRAPH_INSTANCE', {
        instanceId: id,
      });
    return instance;
  }
  private allocateId(): string {
    return `gi${this.nextId++}`;
  }
  private error(
    code: string,
    request: MountGraphRequest,
    extra: Record<string, string>,
  ): GraphContractError {
    return new GraphContractError(code, {
      graphId: request.graphId,
      outletId: request.outletId,
      parentInstanceId: request.parentInstanceId,
      revision: request.revision,
      ...extra,
    });
  }
}

function matchesOutput(
  output: GraphOutput,
  accepted: GraphOutput[],
): boolean {
  const match = accepted.find((entry) => entry.id === output.id);
  return Boolean(
    match && plecValueTypeEquals(match.payloadType, output.payloadType),
  );
}
function matchesCommand(
  command: GraphCommand,
  accepted: GraphCommand[],
): boolean {
  const match = accepted.find((entry) => entry.id === command.id);
  return Boolean(
    match &&
    match.capability === command.capability &&
    plecValueTypeEquals(match.payloadType, command.payloadType),
  );
}

/** Enforces the no-shared-identity boundary without requiring a physical codec. */
function copyBoundaryValue(value: unknown): any {
  if (
    value === null ||
    typeof value === 'boolean' ||
    typeof value === 'number' ||
    typeof value === 'string'
  )
    return value;
  if (Array.isArray(value)) return value.map(copyBoundaryValue);
  if (
    typeof value === 'object' &&
    Object.getPrototypeOf(value) === Object.prototype
  )
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(
        ([key, item]) => [key, copyBoundaryValue(item)],
      ),
    );
  throw new GraphContractError('CROSS_INSTANCE_REFERENCE', {
    valueType: typeof value,
  });
}
