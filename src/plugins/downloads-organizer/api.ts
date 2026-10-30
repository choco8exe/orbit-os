import { invoke, isTauri } from '@tauri-apps/api/core';
export type OrganizerRule = { id: string; name: string; extensions: string[]; contains: string | null; targetFolder: string; enabled: boolean };
export type OrganizerMove = { source: string; destination: string; bytes: number };
export type OrganizerPreview = { root: string; moves: OrganizerMove[]; skipped: number };
export type OrganizerRun = { id: string; moved: number; skipped: number; completedAt: string };
export type OrganizerSnapshot = { root: string | null; enabled: boolean; rules: OrganizerRule[]; latestRun: OrganizerRun | null };
export type RuleInput = Omit<OrganizerRule, 'id'> & { id?: string };
export const downloadsOrganizerService = {
  available: isTauri,
  snapshot: () => invoke<OrganizerSnapshot>('downloads_organizer_snapshot'),
  setRoot: (path: string) => invoke<OrganizerSnapshot>('downloads_organizer_set_root', { path }),
  setEnabled: (enabled: boolean) => invoke<OrganizerSnapshot>('downloads_organizer_set_enabled', { enabled }),
  saveRule: (input: RuleInput) => invoke<OrganizerSnapshot>('downloads_organizer_save_rule', { input }),
  deleteRule: (id: string) => invoke<OrganizerSnapshot>('downloads_organizer_delete_rule', { id }),
  preview: () => invoke<OrganizerPreview>('downloads_organizer_preview'),
  run: () => invoke<OrganizerRun | null>('downloads_organizer_run'),
  undo: (runId: string) => invoke<OrganizerSnapshot>('downloads_organizer_undo', { runId }),
};
