import { describe, expect, it, vi } from 'vitest';
import {
  GraphContractError,
  PlecGraphCoordinator,
} from './graph-coordinator';

const string = { kind: 'string' } as const;
const bool = { kind: 'boolean' } as const;
function graph(graphId: string, revision: string, interfaceValue: any) {
  return { graphId, revision, interface: interfaceValue, ir: {} };
}
const childInterface = {
  inputs: [{ id: 'title', type: string, required: true }],
  outputs: [{ id: 'navigate', payloadType: string }],
  commands: [
    { id: 'focus', capability: 'dom.focus', payloadType: bool },
  ],
  outlets: [],
};
const accepts = {
  inputs: [
    { id: 'title', type: string },
    { id: 'unused', type: bool },
  ],
  outputs: [
    { id: 'navigate', payloadType: string },
    { id: 'unused-output', payloadType: bool },
  ],
  commands: [
    { id: 'focus', capability: 'dom.focus', payloadType: bool },
    {
      id: 'unused-command',
      capability: 'cookie.write',
      payloadType: string,
    },
  ],
};
const parent = graph('layout', 'r1', {
  inputs: [],
  outputs: [],
  commands: [],
  outlets: [{ id: 'main', regionId: 'region-main', accepts }],
});
const child = graph('route', 'r2', childInterface);

describe('PlecGraphCoordinator', () => {
  function setup() {
    const runtime = {
      mount: vi.fn(),
      dispose: vi.fn(),
      setInput: vi.fn(),
      requestCommand: vi.fn(),
    };
    return {
      runtime,
      coordinator: new PlecGraphCoordinator(runtime, async () => child),
    };
  }
  async function mounted() {
    const { runtime, coordinator } = setup();
    const root = coordinator.mountRoot(parent);
    const navigate = vi.fn();
    const route = await coordinator.mountGraph({
      parentInstanceId: root,
      outletId: 'main',
      graphId: 'route',
      revision: 'r2',
      inputs: { title: 'Hello' },
      outputWiring: { navigate },
    });
    return { runtime, coordinator, root, route, navigate };
  }

  it('accepts a child that uses only part of an outlet permission envelope', async () => {
    const { runtime, route } = await mounted();
    expect(route).toBe('gi2');
    expect(runtime.mount).toHaveBeenCalledTimes(2);
  });
  it('isolates values and propagates inputs only to the addressed graph', async () => {
    const { runtime, coordinator, route } = await mounted();
    const value = { title: 'Nope' };
    coordinator.setInput(route, 'title', 'Updated');
    expect(runtime.setInput).toHaveBeenCalledWith(
      route,
      'title',
      'Updated',
    );
    expect(() => coordinator.setInput(route, 'title', value)).toThrow(
      'INVALID_INPUT_VALUE',
    );
  });
  it('dispatches permitted outputs through per-mount wiring', async () => {
    const { coordinator, route, navigate } = await mounted();
    coordinator.emitOutput(route, 'navigate', '/about');
    expect(navigate).toHaveBeenCalledWith({
      instanceId: route,
      outputId: 'navigate',
      payload: '/about',
    });
  });
  it('denies commands not permitted by the outlet before reaching the host', async () => {
    const { runtime, coordinator, route } = await mounted();
    coordinator.requestCommand(route, 'focus', true);
    expect(runtime.requestCommand).toHaveBeenCalledWith({
      instanceId: route,
      commandId: 'focus',
      capability: 'dom.focus',
      payload: true,
    });
    expect(() =>
      coordinator.requestCommand(route, 'missing', true),
    ).toThrow('UNSUPPORTED_COMMAND');
  });
  it('rejects missing inputs and invalid per-mount output wiring', async () => {
    const { coordinator } = setup();
    const root = coordinator.mountRoot(parent);
    await expect(
      coordinator.mountGraph({
        parentInstanceId: root,
        outletId: 'main',
        graphId: 'route',
        revision: 'r2',
        inputs: {},
        outputWiring: { navigate: vi.fn() },
      }),
    ).rejects.toMatchObject({ code: 'MISSING_REQUIRED_INPUT' });
    await expect(
      coordinator.mountGraph({
        parentInstanceId: root,
        outletId: 'main',
        graphId: 'route',
        revision: 'r2',
        inputs: { title: 'x' },
        outputWiring: {},
      }),
    ).rejects.toMatchObject({ code: 'INVALID_OUTPUT_WIRING' });
  });
  it('rejects an implicit replacement and disposes before explicit replacement', async () => {
    const { runtime, coordinator, root, route } = await mounted();
    const request = {
      parentInstanceId: root,
      outletId: 'main',
      graphId: 'route',
      revision: 'r2',
      inputs: { title: 'Again' },
      outputWiring: { navigate: vi.fn() },
    };
    await expect(coordinator.mountGraph(request)).rejects.toMatchObject(
      { code: 'DUPLICATE_INSTANCE_PORT' },
    );
    const next = await coordinator.replaceGraph({
      parentInstanceId: root,
      outletId: 'main',
      next: request,
    });
    expect(runtime.dispose).toHaveBeenCalledWith(route);
    expect(next).toBe('gi3');
  });
  it('reports deterministic contract diagnostics', async () => {
    const { coordinator } = setup();
    const root = coordinator.mountRoot(parent);
    await expect(
      coordinator.mountGraph({
        parentInstanceId: root,
        outletId: 'main',
        graphId: 'route',
        revision: 'wrong',
        inputs: { title: 'x' },
        outputWiring: { navigate: vi.fn() },
      }),
    ).rejects.toMatchObject({ code: 'STALE_GRAPH_REVISION' });
    expect(GraphContractError).toBeDefined();
  });
});
