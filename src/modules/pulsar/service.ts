import { isTauri, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { PulsarDraftResult, PulsarDraftSpec, PulsarService } from '../../shared/contracts';
import type { PulsarPort, RunLog, Scenario, SimulationResult } from './types';

const inDesktop = isTauri();

export const pulsarPort: PulsarPort = {
  available: inDesktop,
  unavailableReason: inDesktop ? undefined : 'Les automatisations nécessitent ORBIT OS sous Windows.',
  async list() { return (await requireDesktop<Scenario[]>('pulsar_list')).map(normalizeScenario); },
  save: scenario => requireDesktop('pulsar_save_scenario', { scenario }),
  createDraft: draft => requireDesktop<PulsarDraftResult>('pulsar_create_draft', { draft }),
  remove: id => requireDesktop('pulsar_delete_scenario', { id }),
  setEnabled: (id, enabled) => requireDesktop('pulsar_set_enabled', { id, enabled }),
  simulate: id => requireDesktop<SimulationResult>('pulsar_simulate', { id }),
  run: (id, confirmed) => {
    if (!confirmed) return Promise.reject(new Error('Confirmez l’exécution de ce scénario.'));
    return requireDesktop('pulsar_run', { id, confirmed });
  },
  logs: (limit = 40) => requireDesktop<RunLog[]>('pulsar_logs', { limit }),
  globalPauseStatus: () => requireDesktop<boolean>('pulsar_global_pause_status'),
  setGlobalPause: (paused: boolean) => requireDesktop<boolean>('pulsar_global_pause_set', { paused }),
  async subscribeGlobalPause(listener) {
    if (!inDesktop) return () => undefined;
    return listen<boolean>('pulsar.global-pause', event => listener(event.payload));
  },
  async pickFolder() {
    if (!inDesktop) throw new Error('Le sélecteur de dossiers est disponible dans l’application Windows.');
    const path = await open({ directory: true, multiple: false, title: 'Choisir un dossier surveillé' });
    return typeof path === 'string' ? path : null;
  },
  async pickFile() {
    if (!inDesktop) throw new Error('Le sélecteur de fichiers est disponible dans l’application Windows.');
    const path = await open({ directory: false, multiple: false, title: 'Choisir un fichier surveillé' });
    return typeof path === 'string' ? path : null;
  },
  async pickExecutable() {
    if (!inDesktop) throw new Error('Le sélecteur d’applications est disponible dans l’application Windows.');
    const path = await open({ directory: false, multiple: false, title: 'Choisir le fichier .exe à surveiller', filters: [{ name: 'Application Windows', extensions: ['exe'] }] });
    return typeof path === 'string' ? path : null;
  },
  fireEvent: name => requireDesktop('pulsar_fire_event', { name }),
};

function requireDesktop<T = void>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!inDesktop) return Promise.reject(new Error('Cette action nécessite la version Windows native.'));
  return invoke<T>(command, args);
}

export const pulsarService: PulsarService = {
  async list() {
    return (await pulsarPort.list()).map(({ id, name, enabled, lastRunAt }) => ({ id, name, enabled, lastRunAt }));
  },
  createDraft: (input: PulsarDraftSpec) => pulsarPort.createDraft({ ...input, allowScheduledFileChanges: false }),
  run: (id, options) => pulsarPort.run(id, options.confirmed),
};

function normalizeScenario(scenario: Scenario): Scenario {
  const triggers = scenario.triggers?.length ? scenario.triggers : scenario.trigger ? [scenario.trigger] : [{ kind: 'manual' as const }];
  return { ...scenario, triggers, trigger: triggers[0] };
}

export type { RunLog, Scenario, SimulationResult };
