import type { EchoPort, EchoSnapshot } from '../modules/echo/types';
import type { OrbitEventBus } from './contracts';

/** Convert local Echo snapshots into payload-free Orbit events, without replaying old history. */
export function connectEchoEvents(
  port: Pick<EchoPort, 'getSnapshot' | 'subscribe'>,
  events: Pick<OrbitEventBus, 'emit'>,
  now: () => number = Date.now,
): () => void {
  const startup = now();
  let seen = new Set<string>();
  let ready = false;
  let disposed = false;
  let buffered: EchoSnapshot[] = [];
  const process = (snapshot: EchoSnapshot) => {
    for (const entry of snapshot.entries) {
      if (seen.has(entry.id)) continue;
      if (Date.parse(entry.createdAt) >= startup) {
        events.emit('echo.entry-added', { id: entry.id, kind: entry.kind, createdAt: entry.createdAt });
      }
      seen.add(entry.id);
    }
    if (seen.size > 1000) seen = new Set(snapshot.entries.map(entry => entry.id));
  };
  const stop = port.subscribe(snapshot => {
    if (!ready) { buffered.push(snapshot); return; }
    process(snapshot);
  });
  void port.getSnapshot().then(snapshot => {
    if (disposed) return;
    seen = new Set(snapshot.entries.filter(entry => Date.parse(entry.createdAt) < startup).map(entry => entry.id));
    ready = true;
    for (const pending of buffered) process(pending);
    buffered = [];
  }).catch(() => {
    if (disposed) return;
    // If the baseline cannot be read, use the first buffered snapshot as baseline to avoid replaying history.
    const fallback = buffered.shift();
    if (fallback) seen = new Set(fallback.entries.map(entry => entry.id));
    ready = true;
    for (const pending of buffered) process(pending);
    buffered = [];
  });
  return () => { disposed = true; buffered = []; stop(); };
}
