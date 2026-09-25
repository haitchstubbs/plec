import { useState } from '@plec/core';
import type { Instrument, StressEvent, Summary } from '../stress-feed';

/** Recognized by Plec's route compiler as a host-owned keyed collection.
 * The route is always compiled, so this declaration emits no JavaScript. */
declare function useCollection<Row>(name: string): Row[];

export function RuntimeStressPage() {
  const instruments = useCollection<Instrument>('instruments');
  const summary = useCollection<Summary>('summary');
  const events = useCollection<StressEvent>('events');
  const [selected, setSelected] = useState<Instrument | null>(null);
  return (
    <main className="min-h-[calc(100svh-4rem)] bg-[#090d14] px-4 py-5 font-mono text-slate-200 sm:px-6">
      <div className="mx-auto grid max-w-[110rem] gap-4">
        <header className="flex flex-wrap items-end justify-between gap-3 border-b border-slate-800 pb-4">
          <div>
            <p className="m-0 text-[11px] font-bold uppercase tracking-[0.22em] text-cyan-400">
              Plec runtime stress fixture
            </p>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight text-slate-50">
              Realtime market terminal
            </h1>
          </div>
          <p className="m-0 text-xs text-slate-500">
            Mounted bindings stay resident; only keyed deltas move.
          </p>
        </header>
        <section className="grid gap-px overflow-hidden rounded border border-slate-800 bg-slate-800 sm:grid-cols-3 xl:grid-cols-6">
          {summary.map((item) => (
            <div key={item.id} className="bg-[#0d131d] px-3 py-2.5">
              <p className="m-0 text-[10px] uppercase tracking-wider text-slate-500">
                Active instruments
              </p>
              <p className="mt-1 text-lg font-semibold text-slate-100">
                {item.active}
              </p>
              <p className="m-0 text-[10px] text-slate-500">
                {item.updatesPerSecond} ticks / sec
              </p>
              <p className="mt-3 text-[10px] uppercase tracking-wider text-slate-500">
                Average value
              </p>
              <p className="mt-1 text-sm text-slate-300">
                {item.averageValue}
              </p>
              <p className="mt-3 text-[10px] uppercase tracking-wider text-slate-500">
                Total volume
              </p>
              <p className="mt-1 text-sm text-slate-300">
                {item.totalVolume}
              </p>
              <p className="mt-3 text-[10px] uppercase tracking-wider text-slate-500">
                Movers
              </p>
              <p className="mt-1 text-sm">
                <span className="text-emerald-400">
                  +{item.positiveMovers}
                </span>{' '}
                <span className="text-rose-400">
                  −{item.negativeMovers}
                </span>
              </p>
            </div>
          ))}
        </section>
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]">
          <section className="min-w-0 overflow-hidden rounded border border-slate-800 bg-[#0d131d]">
            <div className="grid grid-cols-[5.5rem_4.75rem_repeat(6,minmax(4.25rem,1fr))_5.5rem_5rem_4.5rem] gap-x-2 border-b border-slate-800 bg-[#101926] px-3 py-2 text-[10px] font-bold uppercase tracking-wide text-slate-500">
              <span>Symbol</span>
              <span>Venue</span>
              <span>Bid</span>
              <span>Ask</span>
              <span>Last</span>
              <span>Change</span>
              <span>Change %</span>
              <span>Volume</span>
              <span>Status</span>
              <span>Updated</span>
            </div>
            <div className="max-h-[66svh] overflow-auto">
              {instruments.map((instrument) => (
                <button
                  key={instrument.id}
                  type="button"
                  onClick={() => setSelected(instrument)}
                  className="grid w-full grid-cols-[5.5rem_4.75rem_repeat(6,minmax(4.25rem,1fr))_5.5rem_5rem_4.5rem] gap-x-2 border-b border-slate-900 px-3 py-1.5 text-left text-xs tabular-nums hover:bg-slate-800/70 focus:bg-slate-800/70 focus:outline-none"
                >
                  <span className="font-semibold text-slate-100">
                    {instrument.symbol}
                  </span>
                  <span className="text-slate-500">
                    {instrument.venue}
                  </span>
                  <span>{instrument.bid}</span>
                  <span>{instrument.ask}</span>
                  <span className="text-slate-100">
                    {instrument.last}
                  </span>
                  <span
                    className={
                      instrument.change >= 0
                        ? 'text-emerald-400'
                        : 'text-rose-400'
                    }
                  >
                    {instrument.change}
                  </span>
                  <span
                    className={
                      instrument.changePct >= 0
                        ? 'text-emerald-400'
                        : 'text-rose-400'
                    }
                  >
                    {instrument.changePct}
                  </span>
                  <span>{instrument.volume}</span>
                  <span
                    className={
                      instrument.status === 'active'
                        ? 'text-cyan-400'
                        : 'text-amber-400'
                    }
                  >
                    ● {instrument.status}
                  </span>
                  <span className="text-slate-500">
                    {instrument.updatedAt}
                  </span>
                </button>
              ))}
            </div>
          </section>
          <aside className="grid content-start gap-4">
            <section className="rounded border border-cyan-900/70 bg-[#0d131d] p-3">
              {selected ? (
                <div>
                  <p className="m-0 text-[10px] font-bold uppercase tracking-wider text-cyan-400">
                    Selected instrument
                  </p>
                  <p className="mt-2 text-lg font-semibold text-slate-100">
                    {selected.symbol}
                  </p>
                  <dl className="mt-3 grid grid-cols-2 gap-x-3 gap-y-2 text-xs">
                    <dt className="text-slate-500">Venue</dt>
                    <dd className="m-0 text-right">{selected.venue}</dd>
                    <dt className="text-slate-500">Last</dt>
                    <dd className="m-0 text-right">{selected.last}</dd>
                    <dt className="text-slate-500">Bid / ask</dt>
                    <dd className="m-0 text-right">
                      {selected.bid} / {selected.ask}
                    </dd>
                    <dt className="text-slate-500">Volume</dt>
                    <dd className="m-0 text-right">
                      {selected.volume}
                    </dd>
                    <dt className="text-slate-500">Status</dt>
                    <dd className="m-0 text-right">
                      {selected.status}
                    </dd>
                  </dl>
                </div>
              ) : (
                <p className="m-0 text-xs text-slate-500">
                  Select a row to inspect its current snapshot.
                </p>
              )}
            </section>
            <section className="rounded border border-slate-800 bg-[#0d131d] p-3">
              <p className="m-0 text-[10px] font-bold uppercase tracking-wider text-slate-400">
                Runtime telemetry
              </p>
              {summary.map((item) => (
                <dl
                  key={item.id}
                  className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1.5 text-[11px] tabular-nums"
                >
                  <dt className="text-slate-500">Rows</dt>
                  <dd className="m-0 text-right">{item.rowCount}</dd>
                  <dt className="text-slate-500">Bindings</dt>
                  <dd className="m-0 text-right">
                    ≈ {item.bindingCount}
                  </dd>
                  <dt className="text-slate-500">Interval</dt>
                  <dd className="m-0 text-right">
                    {item.tickInterval} ms
                  </dd>
                  <dt className="text-slate-500">Changed / tick</dt>
                  <dd className="m-0 text-right">
                    {item.changedRecords}
                  </dd>
                  <dt className="text-slate-500">Latest update</dt>
                  <dd className="m-0 text-right">
                    {item.latestUpdateMs} ms
                  </dd>
                  <dt className="text-slate-500">Rolling average</dt>
                  <dd className="m-0 text-right">
                    {item.rollingUpdateMs} ms
                  </dd>
                  <dt className="text-slate-500">DOM operations</dt>
                  <dd className="m-0 text-right">
                    {item.domOperations}
                  </dd>
                  <dt className="text-slate-500">Bindings touched</dt>
                  <dd className="m-0 text-right">
                    {item.bindingsTouched}
                  </dd>
                  <dt className="text-slate-500">Prop writes</dt>
                  <dd className="m-0 text-right">{item.propWrites}</dd>
                  <dt className="text-slate-500">
                    Row inserts / moves
                  </dt>
                  <dd className="m-0 text-right">
                    {item.rowInserts} / {item.rowMoves}
                  </dd>
                  <dt className="text-slate-500">WASM DOM</dt>
                  <dd className="m-0 text-right">
                    {item.wasmDomUs} μs
                  </dd>
                </dl>
              ))}
              <p className="mb-0 mt-3 text-[10px] leading-relaxed text-slate-600">
                Duration measures host delta delivery through the
                runtime, not browser paint.
              </p>
            </section>
            <section className="rounded border border-slate-800 bg-[#0d131d] p-3">
              <p className="m-0 text-[10px] font-bold uppercase tracking-wider text-slate-400">
                Recent events
              </p>
              <ul className="mt-3 grid max-h-64 gap-1 overflow-auto p-0 text-[11px]">
                {events.map((event) => (
                  <li
                    key={event.id}
                    className="grid grid-cols-[3.8rem_1fr] gap-2 border-b border-slate-900 pb-1"
                  >
                    <span className="text-slate-600">{event.time}</span>
                    <span className={event.tone}>{event.message}</span>
                  </li>
                ))}
              </ul>
            </section>
          </aside>
        </div>
      </div>
    </main>
  );
}
