import { invoke, isTauri } from '@tauri-apps/api/core';
export type GameProcess = { executable: string; running: boolean };
export type GameUsage = { executable: string; seconds: number; sessions: number };
export type GameDay = { date: string; seconds: number; applications: Record<string, number> };
export type GameSnapshot = { enabled: boolean; tracked: string[]; running: GameProcess[]; week: GameDay[]; totals: GameUsage[] };
export const gameSessionsService = {
  available: isTauri,
  snapshot: () => invoke<GameSnapshot>('game_sessions_snapshot'),
  setEnabled: (enabled: boolean) => invoke<GameSnapshot>('game_sessions_set_enabled', { enabled }),
  add: (executable: string) => invoke<GameSnapshot>('game_sessions_add', { executable }),
  remove: (executable: string) => invoke<GameSnapshot>('game_sessions_remove', { executable }),
  clear: () => invoke<GameSnapshot>('game_sessions_clear'),
};
