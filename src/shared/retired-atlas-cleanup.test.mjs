import assert from 'node:assert/strict';
import test from 'node:test';
import { withoutRetiredAtlasTimelineRecords } from './retired-atlas-cleanup.ts';

test('filters only retired Atlas Timeline records', () => {
  const kept = withoutRetiredAtlasTimelineRecords([
    { id: 'a', category: 'atlas' },
    { id: 'e', category: 'echo' },
    { id: 'p', category: 'pulsar' },
    null,
  ]);
  assert.deepEqual(kept, [
    { id: 'e', category: 'echo' },
    { id: 'p', category: 'pulsar' },
    null,
  ]);
});

test('preserves malformed Timeline data by rejecting before migration writes', () => {
  assert.throws(() => withoutRetiredAtlasTimelineRecords({ invalid: true }));
});
