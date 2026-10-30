import { isTauri, invoke } from '@tauri-apps/api/core';
import type { CoreHistoryPoint, CoreService, Metric, SystemSnapshot } from '../../shared/contracts';
import { settingsService } from '../../shared/settings';

const HISTORY_KEY = 'orbit.core.history.v1';

function readHistory(): CoreHistoryPoint[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]');
    return Array.isArray(value) ? value.filter((point): point is CoreHistoryPoint => Boolean(point && typeof point.at === 'string' && (point.cpuPercent === null || typeof point.cpuPercent === 'number') && (point.memoryPercent === null || typeof point.memoryPercent === 'number'))) : [];
  } catch { return []; }
}

async function saveHistory(snapshot: SystemSnapshot) {
  const settings = await settingsService.read();
  const cutoff = Date.now() - settings.coreHistoryMinutes * 60_000;
  const point: CoreHistoryPoint = {
    at: snapshot.capturedAt,
    cpuPercent: snapshot.cpu.usagePercent.availability === 'available' ? snapshot.cpu.usagePercent.value : null,
    memoryPercent: snapshot.memory.totalBytes.value && snapshot.memory.usedBytes.value !== null
      ? snapshot.memory.usedBytes.value / snapshot.memory.totalBytes.value * 100 : null,
  };
  const next = [...readHistory().filter(item => Date.parse(item.at) >= cutoff), point].slice(-600);
  try { localStorage.setItem(HISTORY_KEY, JSON.stringify(next)); } catch { /* Metrics remain live if local history cannot be written. */ }
}

const unavailable = <T,>(unit = '', detail = 'Les métriques système nécessitent la version Windows native.'): Metric<T> => ({
  value: null, unit, availability: 'unavailable', detail, sampledAt: null,
});

export function browserSnapshot(): SystemSnapshot {
  const time = new Date().toISOString();
  return {
    capturedAt: time,
    cpu: {
      name: unavailable('', 'Le navigateur ne fournit pas le modèle du processeur.'),
      usagePercent: unavailable('%'), cores: unavailable(''), threads: unavailable(''),
    },
    memory: { totalBytes: unavailable('octets'), usedBytes: unavailable('octets'), availableBytes: unavailable('octets') },
    disks: [],
    network: { receivedBytesPerSecond: unavailable('octets/s'), sentBytesPerSecond: unavailable('octets/s') },
    gpu: { name: unavailable(''), usagePercent: unavailable('%'), memoryUsedBytes: unavailable('octets'), memoryTotalBytes: unavailable('octets') },
    uptimeSeconds: unavailable('s'),
  };
}

export const coreService: CoreService = {
  async snapshot() {
    if (!isTauri()) return browserSnapshot();
    try { return await invoke<SystemSnapshot>('get_system_snapshot'); }
    catch (error) {
      const fallback = browserSnapshot();
      fallback.cpu.usagePercent.detail = `Collecte native indisponible : ${String(error)}`;
      return fallback;
    }
  },
  async history() {
    const settings = await settingsService.read();
    const cutoff = Date.now() - settings.coreHistoryMinutes * 60_000;
    const retained = readHistory().filter(point => Date.parse(point.at) >= cutoff).slice(-600);
    try { localStorage.setItem(HISTORY_KEY, JSON.stringify(retained)); } catch { /* Keep the in-memory result. */ }
    return retained;
  },
  subscribe(listener, intervalMs = 2000) {
    let active = true;
    let timer: ReturnType<typeof setInterval> | undefined;
    const interval = Math.max(1000, Math.min(10_000, intervalMs));
    const tick = async () => { const value = await coreService.snapshot(); if (active) { await saveHistory(value); listener(value); } };
    void tick();
    const start = () => { if (timer) clearInterval(timer); timer = setInterval(() => void tick(), interval); };
    start();
    return () => { active = false; if (timer) clearInterval(timer); };
  },
};
