import type { PulsarAction, PulsarCondition, PulsarDraftResult, PulsarDraftSpec, PulsarTrigger } from '../../shared/contracts';

export type Trigger = PulsarTrigger;
export type Condition = PulsarCondition;
export type Action = PulsarAction;
export type ConditionMode = 'all' | 'any';

export interface Scenario {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  triggers: Trigger[];
  /** Optional legacy field. Native migration converts it to a one-element triggers list. */
  trigger?: Trigger;
  conditions: Condition[];
  /** All (ET) is the compatibility default for scenarios created before this field existed. */
  conditionMode: ConditionMode;
  actions: Action[];
  allowScheduledFileChanges: boolean;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string;
  lastRunStatus?: 'started' | 'succeeded' | 'failed' | 'skipped' | 'cancelled' | 'paused';
}

export interface RunLog {
  id: string;
  scenarioId: string;
  scenarioName: string;
  status: 'started' | 'succeeded' | 'failed' | 'skipped' | 'simulated' | 'action' | 'paused' | 'cancelled';
  summary: string;
  at: string;
  simulation: boolean;
}

export interface SimulationResult {
  eligible: boolean;
  trigger: string;
  conditions: Array<{ label: string; passed: boolean; detail: string }>;
  conditionMode: ConditionMode;
  actions: string[];
  message: string;
}

export interface PulsarPort {
  available: boolean;
  unavailableReason?: string;
  list(): Promise<Scenario[]>;
  save(scenario: Scenario): Promise<void>;
  createDraft(draft: PulsarDraftSpec): Promise<PulsarDraftResult>;
  remove(id: string): Promise<void>;
  setEnabled(id: string, enabled: boolean): Promise<void>;
  simulate(id: string): Promise<SimulationResult>;
  run(id: string, confirmed: boolean): Promise<void>;
  logs(limit?: number): Promise<RunLog[]>;
  globalPauseStatus(): Promise<boolean>;
  setGlobalPause(paused: boolean): Promise<boolean>;
  subscribeGlobalPause(listener: (paused: boolean) => void): Promise<() => void>;
  pickFolder(): Promise<string | null>;
  pickFile(): Promise<string | null>;
  pickExecutable(): Promise<string | null>;
  fireEvent(name: 'core.alert' | 'echo.entry-added'): Promise<void>;
}

export const EVENT_TRIGGERS = [
  { value: 'core.alert', label: 'Alerte Core' },
  { value: 'echo.entry-added', label: 'Nouvelle entrée Echo' },
] as const;

export type { PulsarDraftResult, PulsarDraftSpec };
