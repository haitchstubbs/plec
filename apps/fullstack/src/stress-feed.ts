import type {
  CompiledInputProducer,
  CompiledQueryUpdate,
  RuntimeDelta,
} from 'plec-browser';

export type Instrument = {
  id: string;
  symbol: string;
  venue: string;
  bid: number;
  ask: number;
  last: number;
  change: number;
  changePct: number;
  volume: number;
  status: string;
  updatedAt: string;
};

export type Summary = {
  id: string;
  active: number;
  updatesPerSecond: number;
  averageValue: number;
  totalVolume: number;
  positiveMovers: number;
  negativeMovers: number;
  rowCount: number;
  bindingCount: number;
  tickInterval: number;
  changedRecords: number;
  latestUpdateMs: number;
  rollingUpdateMs: number;
  domOperations: number;
  bindingsTouched: number;
  nodesTouched: number;
  propWrites: number;
  rowInserts: number;
  rowRemoves: number;
  rowMoves: number;
  wasmDomUs: number;
};

export type StressEvent = {
  id: string;
  time: string;
  message: string;
  tone: string;
};

const ROW_COUNT = 1_000;
const CHANGED_RECORDS = 20;
const TICK_INTERVAL = 100;
const EVENT_LIMIT = 40;
const VENUES = ['NASDAQ', 'NYSE', 'ARCA', 'BATS'];

type DeltaCollection<Row extends { id: string }> = {
  input: CompiledInputProducer<Row[]>;
  values(): Row[];
  publish(deltas: RuntimeDelta[]): void;
};

function createDeltaCollection<Row extends { id: string }>(
  initial: Row[],
): DeltaCollection<Row> {
  let rows = initial;
  const listeners = new Set<(deltas: RuntimeDelta[]) => void>();
  return {
    input: {
      getSnapshot: () => rows,
      subscribeDeltas: (listener) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
    },
    values: () => rows,
    publish: (deltas) =>
      listeners.forEach((listener) => listener(deltas)),
  };
}

function nextRandom(seed: number): number {
  let value = seed >>> 0;
  value ^= value << 13;
  value ^= value >>> 17;
  value ^= value << 5;
  return value >>> 0;
}

function money(value: number): number {
  return Math.round(value * 100) / 100;
}

function clock(tick: number): string {
  const seconds = String(Math.floor(tick / 10) % 60).padStart(2, '0');
  const minutes = String(Math.floor(tick / 600) % 60).padStart(2, '0');
  return `14:${minutes}:${seconds}`;
}

function initialInstrument(index: number): Instrument {
  const symbol = `PX${String(index + 1).padStart(4, '0')}`;
  const last = money(24 + ((index * 73) % 21_000) / 100);
  const change = money((((index * 19) % 500) - 250) / 100);
  return {
    id: symbol,
    symbol,
    venue: VENUES[index % VENUES.length]!,
    bid: money(last - 0.02),
    ask: money(last + 0.02),
    last,
    change,
    changePct: money((change / last) * 100),
    volume: 1_000_000 + index * 84_211,
    status: index % 17 === 0 ? 'halted' : 'active',
    updatedAt: '14:00:00',
  };
}

function makeSummary(rows: Instrument[]): Summary {
  const totalValue = rows.reduce((total, row) => total + row.last, 0);
  const totalVolume = rows.reduce(
    (total, row) => total + row.volume,
    0,
  );
  return {
    id: 'summary',
    active: rows.filter((row) => row.status === 'active').length,
    updatesPerSecond: 1_000 / TICK_INTERVAL,
    averageValue: money(totalValue / rows.length),
    totalVolume,
    positiveMovers: rows.filter((row) => row.change >= 0).length,
    negativeMovers: rows.filter((row) => row.change < 0).length,
    rowCount: ROW_COUNT,
    bindingCount: ROW_COUNT * 12,
    tickInterval: TICK_INTERVAL,
    changedRecords: CHANGED_RECORDS,
    latestUpdateMs: 0,
    rollingUpdateMs: 0,
    domOperations: 0,
    bindingsTouched: 0,
    nodesTouched: 0,
    propWrites: 0,
    rowInserts: 0,
    rowRemoves: 0,
    rowMoves: 0,
    wasmDomUs: 0,
  };
}

/** A small host-owned synthetic source. It deliberately publishes the same
 * keyed delta shape expected by production collection adapters. */
export function createRuntimeStressFeed(): {
  inputs: Record<string, CompiledInputProducer<any>>;
  start(): void;
  stop(): void;
  recordRuntimeUpdate(update: CompiledQueryUpdate): void;
} {
  const instruments = Array.from({ length: ROW_COUNT }, (_, index) =>
    initialInstrument(index),
  );
  const byId = new Map(instruments.map((row) => [row.id, row]));
  const instrumentInput = createDeltaCollection(instruments);
  const summaryInput = createDeltaCollection([
    makeSummary(instruments),
  ]);
  const events = Array.from({ length: 20 }, (_, index) => ({
    id: `boot-${index}`,
    time: '14:00:00',
    message: `PX${String(index + 1).padStart(4, '0')} feed connected`,
    tone: 'text-slate-400',
  }));
  const eventInput = createDeltaCollection(events);
  let seed = 0x8f3d_9a51;
  let tick = 0;
  let timer: number | undefined;
  let phase: 'instruments' | 'summary' | 'events' | undefined;
  let runtimeUpdate: CompiledQueryUpdate | undefined;
  let rollingTotal = 0;
  let rollingCount = 0;

  const publishSummary = (patch: Partial<Summary>) => {
    const current = summaryInput.values()[0]!;
    Object.assign(current, patch);
    phase = 'summary';
    summaryInput.publish([
      {
        type: 'update',
        inputId: 'summary',
        rowKey: current.id,
        changes: patch,
      },
    ]);
    phase = undefined;
  };

  const publishEvent = (message: string, tone: string) => {
    const next: StressEvent = {
      id: `event-${tick}`,
      time: clock(tick),
      message,
      tone,
    };
    const current = eventInput.values();
    current.unshift(next);
    const deltas: RuntimeDelta[] = [
      {
        type: 'insert',
        inputId: 'events',
        rowKey: next.id,
        row: next,
        beforeRowKey: current[1]?.id ?? null,
      },
    ];
    const removed =
      current.length > EVENT_LIMIT ? current.pop() : undefined;
    if (removed)
      deltas.push({
        type: 'remove',
        inputId: 'events',
        rowKey: removed.id,
      });
    phase = 'events';
    eventInput.publish(deltas);
    phase = undefined;
  };

  const runTick = () => {
    tick += 1;
    const summary = summaryInput.values()[0]!;
    const changed = new Set<number>();
    while (changed.size < CHANGED_RECORDS) {
      seed = nextRandom(seed);
      changed.add(seed % ROW_COUNT);
    }
    const deltas: RuntimeDelta[] = [];
    for (const index of changed) {
      const previous = instruments[index]!;
      const next = { ...previous };
      seed = nextRandom(seed);
      const variant = seed % 4;
      if (variant === 0) {
        next.bid = money(previous.bid + ((seed % 9) - 4) / 100);
      } else if (variant === 1) {
        next.ask = money(previous.ask + ((seed % 9) - 4) / 100);
      } else if (variant === 2) {
        next.last = money(previous.last + ((seed % 19) - 9) / 100);
        next.change = money(previous.change + ((seed % 13) - 6) / 100);
        next.changePct = money((next.change / next.last) * 100);
      } else {
        next.volume = previous.volume + 1_000 + (seed % 90_000);
        next.status = seed % 29 === 0 ? 'halted' : 'active';
        next.updatedAt = clock(tick);
      }
      const changes = Object.fromEntries(
        Object.entries(next).filter(
          ([key, value]) => value !== previous[key as keyof Instrument],
        ),
      );
      instruments[index] = next;
      byId.set(next.id, next);
      summary.averageValue = money(
        summary.averageValue + (next.last - previous.last) / ROW_COUNT,
      );
      summary.totalVolume += next.volume - previous.volume;
      summary.active +=
        Number(next.status === 'active') -
        Number(previous.status === 'active');
      summary.positiveMovers +=
        Number(next.change >= 0) - Number(previous.change >= 0);
      summary.negativeMovers +=
        Number(next.change < 0) - Number(previous.change < 0);
      deltas.push({
        type: 'update',
        inputId: 'instruments',
        rowKey: next.id,
        changes,
      });
    }
    runtimeUpdate = undefined;
    const started = performance.now();
    phase = 'instruments';
    instrumentInput.publish(deltas);
    phase = undefined;
    const elapsed = performance.now() - started;
    rollingCount = Math.min(rollingCount + 1, 50);
    rollingTotal =
      rollingTotal +
      elapsed -
      (rollingCount === 50 ? rollingTotal / 50 : 0);
    publishSummary({
      averageValue: summary.averageValue,
      totalVolume: summary.totalVolume,
      active: summary.active,
      positiveMovers: summary.positiveMovers,
      negativeMovers: summary.negativeMovers,
      latestUpdateMs: elapsed,
      rollingUpdateMs: rollingTotal / rollingCount,
      domOperations:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.domOperations ?? 0,
      bindingsTouched:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.bindingsTouched ?? 0,
      nodesTouched:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.nodesTouched ?? 0,
      propWrites:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.propWrites ?? 0,
      rowInserts:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.rowInserts ?? 0,
      rowRemoves:
        (runtimeUpdate as CompiledQueryUpdate | undefined)
          ?.rowRemoves ?? 0,
      rowMoves:
        (runtimeUpdate as CompiledQueryUpdate | undefined)?.rowMoves ??
        0,
      wasmDomUs:
        (runtimeUpdate as CompiledQueryUpdate | undefined)?.wasmDomUs ??
        0,
    });
    const firstDelta = deltas[0]!;
    const representative = byId.get(firstDelta.rowKey)!;
    publishEvent(
      `${representative.symbol} updated across ${firstDelta.type === 'update' ? Object.keys(firstDelta.changes).length : 0} field(s)`,
      representative.change >= 0 ? 'text-emerald-400' : 'text-rose-400',
    );
  };

  return {
    inputs: {
      instruments: instrumentInput.input,
      summary: summaryInput.input,
      events: eventInput.input,
    },
    start: () => {
      if (timer === undefined)
        timer = window.setInterval(runTick, TICK_INTERVAL);
    },
    stop: () => {
      if (timer !== undefined) window.clearInterval(timer);
      timer = undefined;
    },
    recordRuntimeUpdate: (update) => {
      if (phase === 'instruments') runtimeUpdate = update;
    },
  };
}
