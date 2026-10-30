# ORBIT internal contracts

Version 1 of the in-process contract. Module owners may implement these interfaces in their own
`src/modules/<module>` folder. Shared code is owned by Core; changes to these definitions must be
coordinated with the supervisor before integration.

## Module identity and contribution

```ts
export type PluginRouteId = 'project-shelf' | 'usage-analytics' | 'stream-deck' | 'workspaces' | 'game-sessions' | 'downloads-organizer';
export type ModuleId = 'mission-control' | 'echo' | 'pulsar' | 'core' | 'timeline' | 'settings' | 'plugins' | PluginRouteId;
export interface ModulePageProps { navigate(to: ModuleId): void }
export interface ModuleContribution {
  id: ModuleId;
  label: string;
  description: string;
  icon: string; // short textual symbol; use a native accessible name in the UI
  Page: React.ComponentType<ModulePageProps>;
}
```

Core owns the app shell, routing, navigation, shared components, and tokens. A module exports a
page contribution and does not create its own application window or global navigation.

## Core metrics

```ts
export type MetricAvailability = 'available' | 'unavailable' | 'unsupported';
export interface Metric<T> {
  value: T | null;
  unit: string;
  availability: MetricAvailability;
  detail?: string;
  sampledAt: string | null; // ISO 8601 UTC; null when unavailable
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
```

Unavailable values are `null` and must be rendered as “Non disponible” with the supplied reason.
Zero is a real measurement and must never be used as a substitute for unavailable data. Disk paths
must be local mount identifiers only; do not infer disk capacity from browser storage quota.

## Typed event bus

```ts
export interface OrbitEventMap {
  'core.snapshot': SystemSnapshot;
  'core.alert': { id: string; metric: 'cpu'|'memory'|'disk'; message: string; value: number; threshold: number; at: string };
  'echo.entry-added': { id: string; kind: 'text'|'url'|'image'; createdAt: string };
  'pulsar.run': { id: string; scenarioId: string; status: 'started'|'succeeded'|'failed'|'cancelled'; detail?: string; at: string };
  'timeline.record': { id: string; category: 'echo'|'pulsar'|'core'|'system'; title: string; detail?: string; at: string };
}
export interface OrbitEventBus {
  emit<K extends keyof OrbitEventMap>(type: K, payload: OrbitEventMap[K]): void;
  on<K extends keyof OrbitEventMap>(type: K, handler: (payload: OrbitEventMap[K]) => void): () => void;
}
```

Events stay in process, handlers must be isolated so one listener failure cannot block others.
Never put secrets, clipboard previews or contents, prompt text, or file contents in events or timeline records.

## Settings and persistence

```ts
export interface OrbitSettings {
  version: 1;
  locale: 'fr';
  onboardingComplete: boolean;
  sidebarCompact: boolean;
  immersive: boolean;
  reduceMotion: boolean;
  theme: 'sidereal'|'nebula'|'obsidian';
  /** Empty selects the theme accent; custom values are #RRGGBB with >=4.5:1 contrast. */
  accentColor: string;
  /** Empty inherits --color-bg-panel from the selected theme; custom colors keep all theme text >=4.5:1. */
  panelColor: string;
  /** Empty inherits --color-bg-raised from the selected theme; custom colors keep all theme text >=4.5:1. */
  raisedColor: string;
  orbitalVisualization: 'earth'|'nebula'|'wireframe';
  coreRefreshSeconds: 2 | 5 | 10;
  coreHistoryMinutes: 5 | 15 | 60;
  timelineRetentionDays: 7 | 30 | 90;
  alerts: { cpuPercent: number; memoryPercent: number; diskFreePercent: number };
  widgetOrder: string[];
  hiddenWidgets: string[];
}
export interface SettingsService { read(): Promise<OrbitSettings>; update(patch: Partial<OrbitSettings>): Promise<OrbitSettings>; reset(): Promise<OrbitSettings> }
```

Settings stay local. Implementations must validate values and migrate by version. Until a native
secure store is available, secrets must not be persisted through this service.

## Cross-module services

```ts
export interface EchoService {
  search(query: string, options?: { since?: string; limit?: number }): Promise<Array<{ id: string; kind: 'text'|'url'|'image'; preview: string; createdAt: string }>>;
  copy(id: string): Promise<void>;
  status(): Promise<{ enabled: boolean; paused: boolean; itemCount: number }>;
}
export interface PulsarService {
  list(): Promise<Array<{ id: string; name: string; enabled: boolean; lastRunAt?: string }>>;
  createDraft(input: PulsarDraftSpec): Promise<PulsarDraftResult>;
  run(id: string, options: { confirmed: boolean }): Promise<void>;
}
export interface PulsarDraftSpec {
  name: string; description: string; triggers: PulsarDraftTrigger[];
  conditions: PulsarCondition[]; actions: PulsarAction[];
  allowScheduledFileChanges: boolean;
}
export interface PulsarDraftResult {
  id: string;
  schedulePreview?: { delaySeconds: number; firesAt: string };
}
export interface TimelineRecord { id: string; category: 'echo'|'pulsar'|'core'|'system'; title: string; detail?: string; at: string }
export interface TimelineService {
  list(options?: { category?: TimelineRecord['category']; query?: string; limit?: number }): Promise<TimelineRecord[]>;
  subscribe(listener: (records: TimelineRecord[]) => void): () => void;
  clear(): Promise<void>;
}
export interface OrbitServices { core: CoreService; events: OrbitEventBus; settings: SettingsService; timeline: TimelineService; echo?: EchoService; pulsar?: PulsarService }
```

`PulsarTrigger`, `PulsarCondition`, and `PulsarAction` are defined once in
`src/shared/contracts.ts` and imported by the module; feature folders must not redefine their
wire shapes. `PulsarDraftTrigger` adds the draft-only `{ kind: 'after-delay', seconds }` variant.
The service sends `{ draft }` to `pulsar_create_draft`; the native handler validates the full spec,
generates identity/timestamps, and forces the saved draft disabled with file changes unauthorized.
It converts `after-delay` using the native UTC clock into a future one-shot `at-time` and returns an
authoritative UTC `firesAt` for local-time preview. Enabled state is never accepted from draft input.
Timed drafts in the past or invalid dates are rejected, and activation rechecks the schedule so an
expired reminder cannot fire unexpectedly.

`OrbitServicesProvider` and `useOrbitServices()` expose app-wide service ports to module pages.
Service absence is explicit (`undefined`) and must produce a disabled control with an explanation.
Local data stays local unless a user explicitly invokes a feature that sends it elsewhere.

## Design tokens

The theme id is stored in `OrbitSettings.theme`: `sidereal` (default near-black with warm ivory),
`nebula` (the violet/plasma palette), or `obsidian` (high-contrast black and white). Existing
preferences are preserved; older settings without `theme` validate to the Sidereal default.
`accentColor` may be empty to use the theme accent, one of the published presets, or an accessible
custom `#RRGGBB` value. Core validates custom accents against a 4.5:1 contrast ratio on the darkest
supported canvas. Status/data colors are semantic tokens and are not overwritten by accent choice.
`panelColor` and `raisedColor` customize the two global card levels separately. Empty values restore
the selected theme's surface colors. Custom surface values are rejected unless primary and secondary
text remain at least 4.5:1 across every built-in theme; cards in all modules consume the same semantic
`--color-bg-panel` and `--color-bg-raised` tokens.

The shared CSS semantic tokens remain stable across all three themes:

```css
--color-bg-canvas --color-bg-rail --color-bg-panel --color-bg-raised
--color-bg-selected --color-bg-hover --color-bg-stage --color-bg-input
--color-text-primary --color-text-secondary --color-border-subtle --color-border-control
--color-rule --color-action --color-action-fill --color-action-ink
--color-success --color-warning --color-error --color-data-primary --color-data-secondary
```

Module pages should use `.orbit-module-page` for width/gutters, `.orbit-module-heading` for the
shared page header, and `.orbit-module-content` for the content area. These classes define shared
spacing and typography while module styles can retain their own layout.

The cyan accent is for selected navigation, active data, and primary actions only. Use flat surfaces,
fine dividers, readable text, system-native keyboard focus, tabular figures for metrics, and honor
`prefers-reduced-motion`. Avoid decorative charts and glows. The desktop reference's density should
inform the layout without copying its language or drawing fake statistics.
