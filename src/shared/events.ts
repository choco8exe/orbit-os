import type { OrbitEventBus, OrbitEventMap, TimelineRecord, TimelineService } from './contracts';
import { settingsService } from './settings';

const TIMELINE_KEY = 'orbit.timeline.v1';
const MAX_RECORDS = 500;
const timelineListeners = new Set<(records: TimelineRecord[]) => void>();

function readRecords(): TimelineRecord[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(TIMELINE_KEY) ?? '[]');
    return Array.isArray(value) ? value.filter(isTimelineRecord) : [];
  } catch { return []; }
}
function isTimelineRecord(value: unknown): value is TimelineRecord {
  if (!value || typeof value !== 'object') return false;
  const row = value as Partial<TimelineRecord>;
  return typeof row.id === 'string' && typeof row.title === 'string' && typeof row.at === 'string'
    && ['echo', 'pulsar', 'core', 'system'].includes(String(row.category));
}
function broadcast() { const rows = readRecords(); for (const listener of [...timelineListeners]) listener(rows); }
function persist(record: TimelineRecord) {
  try {
    const next = [record, ...readRecords()].slice(0, MAX_RECORDS);
    localStorage.setItem(TIMELINE_KEY, JSON.stringify(next));
    broadcast();
  } catch { /* Timeline storage failure does not stop its originating module. */ }
}

export const timelineService: TimelineService = {
  async list(options = {}) {
    const settings = await settingsService.read();
    const cutoff = Date.now() - settings.timelineRetentionDays * 86_400_000;
    const retained = readRecords().filter(row => Date.parse(row.at) >= cutoff);
    try { localStorage.setItem(TIMELINE_KEY, JSON.stringify(retained)); } catch { /* Reading remains available if cleanup cannot persist. */ }
    return retained
      .filter(row => !options.category || row.category === options.category)
      .filter(row => !options.query || `${row.title} ${row.detail ?? ''}`.toLocaleLowerCase('fr').includes(options.query.toLocaleLowerCase('fr')))
      .slice(0, options.limit ?? 200);
  },
  subscribe(listener) { timelineListeners.add(listener); listener(readRecords()); return () => timelineListeners.delete(listener); },
  async clear() { localStorage.removeItem(TIMELINE_KEY); broadcast(); },
};

function asRecord<K extends keyof OrbitEventMap>(type: K, payload: OrbitEventMap[K]): TimelineRecord | null {
  if (type === 'timeline.record') return payload as TimelineRecord;
  if (type === 'core.snapshot') return null;
  if (type === 'core.alert') {
    const value = payload as OrbitEventMap['core.alert'];
    return { id: value.id, category: 'core', title: value.message, detail: `${value.metric}: ${value.value}% (seuil ${value.threshold}%)`, at: value.at };
  }
  if (type === 'pulsar.run') {
    const value = payload as OrbitEventMap['pulsar.run'];
    return { id: value.id, category: 'pulsar', title: `Pulsar : ${value.status}`, detail: value.detail, at: value.at };
  }
  if (type === 'echo.entry-added') {
    const value = payload as OrbitEventMap['echo.entry-added'];
    return { id: value.id, category: 'echo', title: 'Nouvelle entrée Echo', detail: value.kind, at: value.createdAt };
  }
  return null;
}

export function createEventBus(): OrbitEventBus {
  const listeners = new Map<keyof OrbitEventMap, Set<(payload: never) => void>>();
  return {
    emit(type, payload) {
      for (const listener of [...(listeners.get(type) ?? [])]) {
        try { listener(payload as never); } catch { /* One consumer cannot block other modules. */ }
      }
      const record = asRecord(type, payload);
      if (record) persist(record);
    },
    on(type, handler) {
      const bucket = listeners.get(type) ?? new Set<(payload: never) => void>();
      bucket.add(handler as (payload: never) => void);
      listeners.set(type, bucket);
      return () => { bucket.delete(handler as (payload: never) => void); if (!bucket.size) listeners.delete(type); };
    },
  };
}
