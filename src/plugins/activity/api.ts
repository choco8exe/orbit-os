import { invoke, isTauri } from '@tauri-apps/api/core';

export type ActivityRange = 'day' | 'week' | 'month';
export type ActivityCategory = 'game' | 'development' | 'creative' | 'browser' | 'other';

export interface ActivityAppUsage {
  appId: string;
  name: string;
  category: ActivityCategory;
  totalSeconds: number;
  backgroundSeconds: number;
  sessionCount: number;
  dailySeconds: Record<string, number>;
  dailyBackgroundSeconds: Record<string, number>;
  dailyTargetMinutes: number | null;
}

export interface ActivityDailyGoal {
  appId: string;
  appName: string;
  dailyTargetMinutes: number;
}

export interface ActivityDayUsage {
  date: string;
  totalSeconds: number;
  backgroundSeconds: number;
  categories: Record<ActivityCategory, number>;
  applications: Record<string, number>;
}

export interface ActivityPreferences {
  monitoringEnabled: boolean;
  countBackgroundApps: boolean;
  idleThresholdSeconds: number;
  retentionDays: number;
  excludedApps: string[];
}

export interface ActivityPulseSnapshot {
  available: boolean;
  unavailableReason: string | null;
  capturedAt: string;
  range: ActivityRange;
  totalSeconds: number;
  backgroundSeconds: number;
  apps: ActivityAppUsage[];
  dailyGoals: ActivityDailyGoal[];
  days: ActivityDayUsage[];
  preferences: ActivityPreferences;
}

export interface ActivityPulseService {
  available(): boolean;
  snapshot(range: ActivityRange): Promise<ActivityPulseSnapshot>;
  setMonitoring(enabled: boolean): Promise<ActivityPulseSnapshot>;
  setPreferences(preferences: Pick<ActivityPreferences, 'idleThresholdSeconds' | 'retentionDays' | 'excludedApps' | 'countBackgroundApps'>): Promise<ActivityPulseSnapshot>;
  setCategory(appId: string, category: ActivityCategory): Promise<ActivityPulseSnapshot>;
  setAppDailyTarget(appId: string, targetMinutes: number | null): Promise<ActivityPulseSnapshot>;
  clearHistory(before?: string): Promise<ActivityPulseSnapshot>;
  exportCsv(): Promise<string>;
}

function requireNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error('ORBIT Pulse mesure les applications dans l’application Windows native. L’aperçu navigateur ne collecte aucune activité.'));
  return invoke<T>(command, args);
}

export const activityPulseService: ActivityPulseService = {
  available: () => isTauri(),
  snapshot: (range) => requireNative<ActivityPulseSnapshot>('activity_pulse_snapshot', { range }),
  setMonitoring: (enabled) => requireNative<ActivityPulseSnapshot>('activity_pulse_set_monitoring', { enabled }),
  setPreferences: (preferences) => requireNative<ActivityPulseSnapshot>('activity_pulse_set_preferences', { preferences }),
  setCategory: (appId, category) => requireNative<ActivityPulseSnapshot>('activity_pulse_set_category', { appId, category }),
  setAppDailyTarget: (appId, targetMinutes) => requireNative<ActivityPulseSnapshot>('activity_pulse_set_app_goal', { appId, targetMinutes }),
  clearHistory: (before) => requireNative<ActivityPulseSnapshot>('activity_pulse_clear_history', { before: before ?? null }),
  exportCsv: () => requireNative<string>('activity_pulse_export_csv'),
};
