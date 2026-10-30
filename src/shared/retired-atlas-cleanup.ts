const CLEANUP_MARKER = 'orbit.retired-atlas-cleanup.v1';
const RETIRED_DATABASE = 'orbit-os-atlas';
const TIMELINE_MARKER = 'orbit.retired-atlas-timeline-cleanup.v1';
const TIMELINE_KEY = 'orbit.timeline.v1';

export function withoutRetiredAtlasTimelineRecords(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error('Timeline data is not a list');
  return value.filter(item => !item || typeof item !== 'object' || (item as { category?: unknown }).category !== 'atlas');
}

/** Deletes only retired Atlas stores once; each independent cleanup keeps its own retry marker. */
export async function clearRetiredAtlasDatabaseOnce(): Promise<void> {
  const operations: Promise<void>[] = [];
  if (localStorage.getItem(TIMELINE_MARKER) !== 'done') {
    operations.push((async () => {
      const raw = localStorage.getItem(TIMELINE_KEY);
      if (raw !== null) {
        const value: unknown = JSON.parse(raw);
        const retained = withoutRetiredAtlasTimelineRecords(value);
        if (retained.length !== (value as unknown[]).length) localStorage.setItem(TIMELINE_KEY, JSON.stringify(retained));
      }
      localStorage.setItem(TIMELINE_MARKER, 'done');
    })());
  }
  if (localStorage.getItem(CLEANUP_MARKER) !== 'done' && typeof indexedDB !== 'undefined') {
    operations.push(new Promise<void>((resolve, reject) => {
      const request = indexedDB.deleteDatabase(RETIRED_DATABASE);
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error ?? new Error('IndexedDB cleanup failed'));
      request.onblocked = () => reject(new Error('IndexedDB cleanup is blocked'));
    }).then(() => { localStorage.setItem(CLEANUP_MARKER, 'done'); }));
  }
  const results = await Promise.allSettled(operations);
  if (results.some(result => result.status === 'rejected')) throw new Error('Retired local data cleanup will retry on the next launch');
}
