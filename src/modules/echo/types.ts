export type EchoEntryKind = "text" | "url" | "image";

/** Clipboard item captured locally. Source is omitted unless Windows reports it reliably. */
export interface EchoEntry {
  id: string;
  kind: EchoEntryKind;
  createdAt: string;
  text?: string;
  imageDataUrl?: string;
  preview: string;
  byteLength: number;
  sourceApp?: string;
  favorite: boolean;
  pinned: boolean;
  collectionIds: string[];
}

/** Content for one explicitly opened entry. It is fetched on demand, never in list snapshots. */
export interface EchoEntryDetail {
  id: string;
  kind: EchoEntryKind;
  text?: string;
  imageDataUrl?: string;
  byteLength: number;
  truncated: boolean;
}

export interface EchoCollection {
  id: string;
  name: string;
  createdAt: string;
}

export interface EchoPreferences {
  monitoringEnabled: boolean;
  captureImages: boolean;
  retentionDays: number;
  maxEntryBytes: number;
  excludedApps: string[];
}

export interface EchoSnapshot {
  entries: EchoEntry[];
  collections: EchoCollection[];
  preferences: EchoPreferences;
  available: boolean;
  unavailableReason?: string;
}

export interface EchoSearchHit { id: string; kind: EchoEntryKind; preview: string; createdAt: string; favorite: boolean; pinned: boolean; collectionIds: string[] }
export interface EchoDuplicateSet { ids: string[]; kind: EchoEntryKind; preview: string; count: number }

/** Host implementation must use the OS clipboard and local persistent storage. */
export interface EchoPort {
  getSnapshot(): Promise<EchoSnapshot>;
  getEntry(id: string): Promise<EchoEntryDetail>;
  search(query: string, options?: { since?: string; limit?: number }): Promise<EchoSearchHit[]>;
  subscribe(listener: (snapshot: EchoSnapshot) => void): () => void;
  setMonitoring(enabled: boolean): Promise<void>;
  setPreferences(patch: Partial<EchoPreferences>): Promise<void>;
  copyEntry(id: string): Promise<void>;
  setFavorite(id: string, favorite: boolean): Promise<void>;
  setPinned(id: string, pinned: boolean): Promise<void>;
  assignCollection(id: string, collectionId: string | null): Promise<void>;
  createCollection(name: string): Promise<void>;
  deleteEntry(id: string): Promise<void>;
  deleteEntries(ids: string[]): Promise<void>;
  findDuplicates(): Promise<EchoDuplicateSet[]>;
  removeDuplicates(): Promise<number>;
  clearHistory(): Promise<void>;
}

export const defaultEchoPreferences: EchoPreferences = {
  monitoringEnabled: false,
  captureImages: false,
  retentionDays: 30,
  maxEntryBytes: 1_000_000,
  excludedApps: [],
};
