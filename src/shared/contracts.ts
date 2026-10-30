import type React from 'react';
import type { LucideIcon } from 'lucide-react';

export type PluginRouteId = 'project-shelf' | 'usage-analytics' | 'stream-deck' | 'workspaces' | 'game-sessions' | 'downloads-organizer';
export type ModuleId = 'mission-control' | 'echo' | 'pulsar' | 'core' | 'timeline' | 'settings' | 'plugins' | PluginRouteId;
export interface ModulePageProps { navigate: (to: ModuleId) => void }
export interface ModuleContribution {
  id: ModuleId;
  label: string;
  description: string;
  icon?: string;
  Page: React.ComponentType<ModulePageProps>;
}

export type MetricAvailability = 'available' | 'unavailable' | 'unsupported';
export interface Metric<T> {
  value: T | null;
  unit: string;
  availability: MetricAvailability;
  detail?: string;
  sampledAt: string | null;
}
export interface CpuMetrics { name: Metric<string>; usagePercent: Metric<number>; cores: Metric<number>; threads: Metric<number> }
export interface MemoryMetrics { totalBytes: Metric<number>; usedBytes: Metric<number>; availableBytes: Metric<number> }
export interface DiskMetric { mount: string; totalBytes: Metric<number>; freeBytes: Metric<number>; usedPercent: Metric<number> }
export interface NetworkMetrics { receivedBytesPerSecond: Metric<number>; sentBytesPerSecond: Metric<number> }
export interface GpuMetrics { name: Metric<string>; usagePercent: Metric<number>; memoryUsedBytes: Metric<number>; memoryTotalBytes: Metric<number> }
export interface SystemSnapshot {
  capturedAt: string;
  cpu: CpuMetrics;
  memory: MemoryMetrics;
  disks: DiskMetric[];
  network: NetworkMetrics;
  gpu: GpuMetrics;
  uptimeSeconds: Metric<number>;
}
export interface CoreHistoryPoint { at: string; cpuPercent: number | null; memoryPercent: number | null }
export interface CoreService {
  snapshot(): Promise<SystemSnapshot>;
  history(): Promise<CoreHistoryPoint[]>;
  subscribe(listener: (value: SystemSnapshot) => void, intervalMs?: number): () => void;
}

export interface OrbitEventMap {
  'core.snapshot': SystemSnapshot;
  'core.alert': { id: string; metric: 'cpu'|'memory'|'disk'; message: string; value: number; threshold: number; at: string };
  'echo.entry-added': { id: string; kind: 'text'|'url'|'image'; createdAt: string };
  'pulsar.run': { id: string; scenarioId: string; status: 'started'|'succeeded'|'failed'|'cancelled'; detail?: string; at: string };
  'pulsar.global-pause': boolean;
  'timeline.record': { id: string; category: 'echo'|'pulsar'|'core'|'system'; title: string; detail?: string; at: string };
}
export interface OrbitEventBus {
  emit<K extends keyof OrbitEventMap>(type: K, payload: OrbitEventMap[K]): void;
  on<K extends keyof OrbitEventMap>(type: K, handler: (payload: OrbitEventMap[K]) => void): () => void;
}

export interface OrbitSettings {
  version: 1;
  locale: 'fr';
  onboardingComplete: boolean;
  sidebarCompact: boolean;
  immersive: boolean;
  reduceMotion: boolean;
  theme: OrbitThemeId;
  accentColor: string;
  /** Empty means use the selected theme's card surface. */
  panelColor: string;
  /** Empty means use the selected theme's raised/secondary surface. */
  raisedColor: string;
  /** Keeps valid user colors with their theme so switching appearance never discards custom work. */
  themeColorProfiles: Partial<Record<OrbitThemeId, { accentColor: string; panelColor: string; raisedColor: string }>>;
  orbitalVisualization: OrbitalVisualization;
  orbitalEnabled: boolean;
  orbitalRotationSpeed: 0.5 | 1 | 1.5;
  orbitalParticlesEnabled: boolean;
  orbitalIntensity: 'low' | 'balanced' | 'high';
  coreRefreshSeconds: 2 | 5 | 10;
  coreHistoryMinutes: 5 | 15 | 60;
  timelineRetentionDays: 7 | 30 | 90;
  alerts: { cpuPercent: number; memoryPercent: number; diskFreePercent: number };
  widgetOrder: string[];
  hiddenWidgets: string[];
}
export type OrbitalVisualization = 'earth' | 'ringed-planet' | 'lunar-station' | 'nebula' | 'core' | 'wireframe' | 'constellations';
export type OrbitThemeId = 'sidereal' | 'nebula' | 'obsidian' | 'lunar' | 'aurora' | 'solar' | 'abyss';
export const ORBIT_ACCENT_PRESETS = [
  { value: '', label: 'Couleur du thème' },
  { value: '#D6B978', label: 'Laiton' },
  { value: '#75DDD0', label: 'Menthe' },
  { value: '#8FCDF3', label: 'Glace' },
  { value: '#E59B7D', label: 'Corail' },
  { value: '#C5A6FF', label: 'Violet' },
] as const;
export interface SettingsService { read(): Promise<OrbitSettings>; update(patch: Partial<OrbitSettings>): Promise<OrbitSettings>; reset(): Promise<OrbitSettings> }

export interface EchoService {
  search(query: string, options?: { since?: string; limit?: number }): Promise<Array<{ id: string; kind: 'text'|'url'|'image'; preview: string; createdAt: string }>>;
  copy(id: string): Promise<void>;
  status(): Promise<{ enabled: boolean; paused: boolean; itemCount: number }>;
}
export type PulsarTrigger =
  | { kind: 'manual' }
  | { kind: 'startup' }
  | { kind: 'interval'; everySeconds: number }
  | { kind: 'at-time'; at: string }
  | { kind: 'file-created'; directory: string }
  | { kind: 'file-modified'; path: string }
  | { kind: 'event'; name: 'core.alert'|'echo.entry-added' }
  | { kind: 'process-started'; executable: string };
/** Draft-only relative trigger; Pulsar converts it to a concrete one-shot time before persistence. */
export type PulsarDraftTrigger = PulsarTrigger | { kind: 'after-delay'; seconds: number };
export type PulsarCondition =
  | { kind: 'extension'; value: string }
  | { kind: 'name-contains'; value: string }
  | { kind: 'size-at-least'; bytes: number }
  | { kind: 'path-exists'; path: string }
  | { kind: 'time-window'; from: string; to: string };
export type PulsarAction =
  | { kind: 'notify'; title: string; body: string }
  | { kind: 'open-folder'; path: string }
  | { kind: 'open-app'; app: 'notepad'|'calculator'|'file-explorer' }
  | { kind: 'copy-file'; source: string; destination: string }
  | { kind: 'move-file'; source: string; destination: string }
  | { kind: 'rename-file'; path: string; newName: string }
  | { kind: 'log'; message: string };
/** Input for user-created Pulsar scenarios. Identity, timestamps and enabled state are backend-owned. */
export interface PulsarDraftSpec {
  name: string;
  description: string;
  triggers: PulsarDraftTrigger[];
  conditions: PulsarCondition[];
  actions: PulsarAction[];
  allowScheduledFileChanges: boolean;
}
export interface PulsarDraftResult {
  id: string;
  /** Authoritative native schedule preview when a draft contains an after-delay trigger. */
  schedulePreview?: { delaySeconds: number; firesAt: string };
}
export interface PulsarService {
  list(): Promise<Array<{ id: string; name: string; enabled: boolean; lastRunAt?: string }>>;
  createDraft(input: PulsarDraftSpec): Promise<PulsarDraftResult>;
  run(id: string, options: { confirmed: boolean }): Promise<void>;
}
export interface TimelineRecord { id: string; category: 'echo'|'pulsar'|'core'|'system'; title: string; detail?: string; at: string }
export interface TimelineService {
  list(options?: { category?: TimelineRecord['category']; query?: string; limit?: number }): Promise<TimelineRecord[]>;
  subscribe(listener: (records: TimelineRecord[]) => void): () => void;
  clear(): Promise<void>;
}
export type PluginId = 'com.orbitos.fl-studio' | 'com.orbitos.activity-pulse' | 'com.orbitos.creator-suite' | 'com.orbitos.echo-streamdeck' | 'com.orbitos.workspaces' | 'com.orbitos.game-sessions' | 'com.orbitos.downloads-organizer';
export type PluginPermissionId =
  | 'project-shelf.local-library' | 'project-shelf.select-project-files' | 'project-shelf.open-project' | 'fl-studio.session-notes'
  | 'activity-pulse.track-active-apps'
  | 'creator.streamdeck.bridge'
  | 'pulsar.streamdeck.control'
  | 'echo.clipboard.copy-from-streamdeck' | 'echo.clipboard.toggle-capture-from-streamdeck'
  | 'game-sessions.track-selected-processes' | 'downloads-organizer.manage-authorized-folder'
  | 'workspaces.manage' | 'workspaces.launch';
export type PluginSecretId = 'creator.streamdeck.pair-token';
export type PluginCapabilityId = 'local-project-library' | 'local-activity-history' | 'studio-project-notes' | 'streamdeck-control' | 'echo-copy-via-streamdeck' | 'echo-streamdeck-capture-control' | 'local-workspace-launcher' | 'local-game-session-history' | 'local-download-organization';
export type PluginConnectionMode = 'local' | 'credentials' | 'oauth' | 'companion';
export interface PluginPermissionManifest {
  id: PluginPermissionId;
  label: string;
  description: string;
  required: boolean;
  scope: 'local';
}
export interface PluginPageContribution {
  routeId: PluginRouteId;
  label: string;
  description: string;
  icon: string;
}
export interface PluginSecretSlot {
  id: PluginSecretId;
  label: string;
  required: boolean;
  /** Provider-managed slots are written by a native pairing/OAuth flow, never by an arbitrary secret input. */
  entryMode: 'user-input' | 'provider-managed';
}
export interface PluginConnectionContribution {
  mode: PluginConnectionMode;
  summary: string;
  secretSlots: PluginSecretSlot[];
}
/** Static page component compiled into ORBIT; manifests cannot provide executable code. */
export interface PluginPageProvider {
  pluginId: PluginId;
  routeId: PluginPageContribution['routeId'];
  label: string;
  icon: LucideIcon;
  Page: React.ComponentType<ModulePageProps>;
}
export interface PluginManifest {
  id: PluginId;
  version: string;
  apiVersion: number;
  name: string;
  publisher: 'ORBIT OS';
  trust: 'first-party-prepackaged';
  description: string;
  capabilities: PluginCapabilityId[];
  demandedPermissions: PluginPermissionManifest[];
  contributions: { pages: PluginPageContribution[] };
  connection: PluginConnectionContribution;
}
export interface PluginState {
  pluginId: PluginId;
  installed: boolean;
  enabled: boolean;
  grantedPermissions: PluginPermissionId[];
  installedVersion: string | null;
}
export interface PluginService {
  listCatalog(): Promise<PluginManifest[]>;
  listStates(): Promise<PluginState[]>;
  install(pluginId: PluginId): Promise<PluginState>;
  enable(pluginId: PluginId): Promise<PluginState>;
  disable(pluginId: PluginId): Promise<PluginState>;
  uninstall(pluginId: PluginId, options: { deleteData: boolean }): Promise<void>;
  setPermissions(pluginId: PluginId, grantedPermissions: PluginPermissionId[]): Promise<PluginState>;
  /** Returns only whether a declared secret slot exists in the OS keyring; secret values never cross into JS. */
  secretStatus(pluginId: PluginId, secretId: PluginSecretId): Promise<boolean>;
  setSecret(pluginId: PluginId, secretId: PluginSecretId, value: string): Promise<void>;
  removeSecret(pluginId: PluginId, secretId: PluginSecretId): Promise<void>;
  retiredCredentialStatus(): Promise<{ obs: boolean; github: boolean; youtube: boolean }>;
  cleanupRetiredCredentials(): Promise<void>;
}
export interface ProjectShelfEntry {
  id: string;
  name: string;
  path: string;
  pinned: boolean;
  addedAt: string;
  modifiedAt: string | null;
  sizeBytes: number | null;
  exists: boolean;
  tags: string[];
}
export interface ProjectShelfService {
  list(): Promise<ProjectShelfEntry[]>;
  add(path: string): Promise<ProjectShelfEntry[]>;
  setPinned(id: string, pinned: boolean): Promise<ProjectShelfEntry[]>;
  open(id: string): Promise<void>;
  remove(id: string): Promise<ProjectShelfEntry[]>;
  addTag(id: string, tag: string): Promise<ProjectShelfEntry[]>;
  removeTag(id: string, tag: string): Promise<ProjectShelfEntry[]>;
}
export interface OrbitServices {
  core: CoreService;
  events: OrbitEventBus;
  settings: SettingsService;
  timeline: TimelineService;
  plugins: PluginService;
  projectShelf?: ProjectShelfService;
  echo?: EchoService;
  pulsar?: PulsarService;
}

export const defaultSettings: OrbitSettings = {
  version: 1, locale: 'fr', onboardingComplete: false, sidebarCompact: false, immersive: false, reduceMotion: false, theme: 'sidereal', accentColor: '', panelColor: '', raisedColor: '', themeColorProfiles: {}, orbitalVisualization: 'earth', orbitalEnabled: true, orbitalRotationSpeed: 1, orbitalParticlesEnabled: true, orbitalIntensity: 'balanced',
  coreRefreshSeconds: 2, coreHistoryMinutes: 15,
  timelineRetentionDays: 30,
  alerts: { cpuPercent: 90, memoryPercent: 90, diskFreePercent: 10 },
  widgetOrder: ['system', 'orbit', 'modules', 'activity'], hiddenWidgets: [],
};
