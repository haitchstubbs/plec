type LoweringTrace = {
  seq: number;
  stage: string;
  file?: string;
  symbol?: string;
  input?: unknown;
  output?: unknown;
  reason?: string;
};

let traces: LoweringTrace[] = [];
let seq = 0;
let filterSymbol: string | undefined;

export function trace(entry: Omit<LoweringTrace, 'seq'>) {
  if (!process.env.PLEC_TRACE_LOWERING) return;
  if (filterSymbol && entry.symbol !== filterSymbol) return;
  traces.push({ seq: seq++, ...entry });
}

export function clearTrace(filter?: string) {
  traces = [];
  seq = 0;
  filterSymbol = filter;
}

export function getTraces() {
  return traces;
}
