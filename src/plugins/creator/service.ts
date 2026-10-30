import { invoke, isTauri } from '@tauri-apps/api/core';

export interface StudioNote {
  projectId: string;
  projectName: string;
  style: string;
  bpm: number | null;
  musicalKey: string;
  notes: string;
  updatedAt: string;
}
export interface StreamDeckStatus { available: boolean; paired: boolean; port: number; error: string | null }
export interface StreamDeckPairingCode { code: string; expiresInSeconds: number; port: number }

const nativeOnly = <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  if (!isTauri()) return Promise.reject(new Error('Cette fonction nécessite l’application ORBIT OS sous Windows.'));
  return invoke<T>(command, args);
};

export const studioNotesService = {
  available: isTauri(),
  get: (projectId: string) => nativeOnly<StudioNote | null>('creator_plugins_studio_get_note', { projectId }),
  save: (note: Omit<StudioNote, 'projectName' | 'updatedAt'>) => nativeOnly<StudioNote>('creator_plugins_studio_save_note', { note }),
  remove: (projectId: string) => nativeOnly<void>('creator_plugins_studio_delete_note', { projectId }),
};

export const streamDeckService = {
  available: isTauri(),
  status: () => nativeOnly<StreamDeckStatus>('creator_streamdeck_pairing_status'),
  startPairing: () => nativeOnly<StreamDeckPairingCode>('creator_streamdeck_pairing_start'),
  revokePairing: () => nativeOnly<void>('creator_streamdeck_revoke'),
  installCompanion: () => nativeOnly<void>('creator_streamdeck_install'),
};
