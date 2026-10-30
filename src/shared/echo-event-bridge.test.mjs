import test from 'node:test';
import assert from 'node:assert/strict';
import { connectEchoEvents } from './echo-event-bridge.ts';

const startup = Date.parse('2026-10-28T12:00:00.000Z');
const snapshot = entries => ({ entries, collections: [], preferences: { monitoringEnabled: true, captureImages: true, retentionDays: 30, maxEntryBytes: 1_000_000, excludedApps: [] }, available: true });
const entry = (id, createdAt, text) => ({ id, kind: 'text', createdAt, text, preview: text, byteLength: text.length, favorite: false, pinned: false, collectionIds: [] });

test('existing Echo history is baseline and is never replayed into Pulsar', async () => {
  let onSnapshot;
  const emitted = [];
  const stop = connectEchoEvents({
    subscribe: listener => { onSnapshot = listener; return () => {}; },
    getSnapshot: async () => snapshot([entry('old', '2026-10-27T12:00:00.000Z', 'private old text')]),
  }, { emit: (type, payload) => emitted.push({ type, payload }) }, () => startup);

  await Promise.resolve();
  onSnapshot(snapshot([entry('old', '2026-10-27T12:00:00.000Z', 'private old text')]));
  assert.deepEqual(emitted, []);
  stop();
});

test('Echo changes during baseline are delivered with metadata only', async () => {
  let onSnapshot;
  let resolveBaseline;
  const emitted = [];
  const old = entry('old', '2026-10-27T12:00:00.000Z', 'private old text');
  const fresh = entry('new', '2026-10-28T12:00:01.000Z', 'private new text');
  const stop = connectEchoEvents({
    subscribe: listener => { onSnapshot = listener; return () => {}; },
    getSnapshot: () => new Promise(resolve => { resolveBaseline = resolve; }),
  }, { emit: (type, payload) => emitted.push({ type, payload }) }, () => startup);

  onSnapshot(snapshot([old, fresh]));
  resolveBaseline(snapshot([old, fresh]));
  await Promise.resolve();

  assert.deepEqual(emitted, [{
    type: 'echo.entry-added',
    payload: { id: 'new', kind: 'text', createdAt: fresh.createdAt },
  }]);
  assert.equal(JSON.stringify(emitted).includes('private'), false);
  stop();
});
