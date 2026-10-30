import streamDeck, { action, DidReceiveSettingsEvent, KeyDownEvent, SingletonAction, WillAppearEvent } from '@elgato/streamdeck';
import { invokeHelper, type CompanionAction } from './secure-helper.js';
import { formatPulseSummaryTitle } from './pulse-title.js';

type Settings = { action?: CompanionAction; scenarioId?: string };

function safeHelperErrorCategory(error?: string): string {
  const allowed = new Set([
    'orbit-unavailable', 'orbit-rejected', 'credential-write', 'credential-store',
    'permission', 'pair', 'response', 'input', 'input-size', 'helper-timeout',
  ]);
  return error && allowed.has(error) ? error : error ? 'other' : 'none';
}

@action({ UUID: 'com.orbitos.creator.orbit-control' })
class OrbitControl extends SingletonAction<Settings> {
  override async onWillAppear(ev: WillAppearEvent<Settings>) {
    await this.updateActionTitle(ev.action, ev.payload.settings.action);
  }

  override async onDidReceiveSettings(ev: DidReceiveSettingsEvent<Settings>) {
    // A Stream Deck action title can be overridden after an action runs (for
    // example, Pulse's weekly summary). Restore the selected action's title
    // whenever its settings change so the old summary does not stick around.
    await this.updateActionTitle(ev.action, ev.payload.settings.action);
  }

  override async onKeyDown(ev: KeyDownEvent<Settings>) {
    const action = this.safeAction(ev.payload.settings.action);
    if (!action) { await ev.action.showAlert(); return; }
    try {
      const result = await invokeHelper('action', { action, scenarioId: action === 'pulsar.runScenario' ? ev.payload.settings.scenarioId : undefined });
      if (!result.ok) throw new Error(result.error || 'ORBIT a refusé cette action.');
      const data = result.data;
      if (action === 'echo.toggleMonitoring' && typeof data?.monitoringEnabled === 'boolean') {
        await ev.action.setTitle(data.monitoringEnabled ? 'Echo\nCapture active' : 'Echo\nEn pause');
      } else if (action === 'pulse.toggleMonitoring' && typeof data?.monitoringEnabled === 'boolean') {
        await ev.action.setTitle(data.monitoringEnabled ? 'Pulse\nSuivi actif' : 'Pulse\nEn pause');
      } else if (action === 'pulsar.togglePause' && typeof data?.globalPaused === 'boolean') {
        await ev.action.setTitle(data.globalPaused ? 'Pulsar\nEn pause' : 'Pulsar\nActif');
      } else if (action === 'pulse.summary' && typeof data?.summarySeconds === 'number') {
        await ev.action.setTitle(formatPulseSummaryTitle(data.summarySeconds));
      } else {
        await ev.action.showOk();
      }
    } catch {
      await ev.action.showAlert();
    }
  }

  override async onSendToPlugin(ev: import('@elgato/streamdeck').SendToPluginEvent<import('@elgato/utils').JsonValue, Settings>) {
    const message = ev.payload as { event?: string; code?: string; port?: number; requestId?: string; settings?: Settings };
    if (message.event === 'settings' && message.settings) {
      await ev.action.setSettings(message.settings);
      // The PI relays changes through this custom event, so update the physical
      // key title here as well as in onDidReceiveSettings.
      await this.updateActionTitle(ev.action, message.settings.action);
      return;
    }
    if (message.event === 'list-scenarios') {
      const result = await invokeHelper('scenarios');
      await streamDeck.ui.sendToPropertyInspector({ event: 'scenario-list', ok: result.ok, error: result.error, scenarios: result.data?.scenarios ?? [] });
      return;
    }
    if (message.event === 'pair') {
      const requestId = typeof message.requestId === 'string' && message.requestId.length <= 80 ? message.requestId : undefined;
      if (typeof message.code !== 'string' || typeof message.port !== 'number') {
        streamDeck.logger.warn('ORBIT pairing stage=plugin_received result=invalid_input');
        await streamDeck.ui.sendToPropertyInspector({ event: 'pair-result', ...(requestId ? { requestId } : {}), ok: false, error: 'Code d’appairage invalide.' });
        return;
      }
      streamDeck.logger.info('ORBIT pairing stage=plugin_received');
      try {
        streamDeck.logger.info('ORBIT pairing stage=helper_started');
        const result = await invokeHelper('pair', { code: message.code, port: message.port });
        streamDeck.logger.info(`ORBIT pairing stage=helper_returned ok=${result.ok} error=${safeHelperErrorCategory(result.error)}`);
        await streamDeck.ui.sendToPropertyInspector({ event: 'pair-result', ...(requestId ? { requestId } : {}), ok: result.ok, error: result.error });
      } catch {
        streamDeck.logger.error('ORBIT pairing stage=helper_threw');
        await streamDeck.ui.sendToPropertyInspector({ event: 'pair-result', ...(requestId ? { requestId } : {}), ok: false, error: 'Le compagnon ORBIT n’a pas pu terminer l’appairage.' });
      }
    } else if (message.event === 'unpair') {
      const requestId = typeof message.requestId === 'string' && message.requestId.length <= 80 ? message.requestId : undefined;
      try {
        const result = await invokeHelper('unpair');
        await streamDeck.ui.sendToPropertyInspector({ event: 'unpair-result', ...(requestId ? { requestId } : {}), ok: result.ok, error: result.error, remoteRevoked: result.data?.remoteRevoked });
      } catch {
        await streamDeck.ui.sendToPropertyInspector({ event: 'unpair-result', ...(requestId ? { requestId } : {}), ok: false, error: 'Le compagnon ORBIT n’a pas pu terminer la dissociation.' });
      }
    }
  }

  private safeAction(value: unknown): CompanionAction | undefined {
    return value === 'orbit.open' || value === 'mission.open' || value === 'echo.open' || value === 'pulsar.open' || value === 'pulsar.togglePause' || value === 'pulsar.runScenario' || value === 'core.open' || value === 'timeline.open' || value === 'settings.open' || value === 'echo.copyLatest' || value === 'echo.copyPrevious' || value === 'echo.toggleMonitoring' || value === 'pulse.open' || value === 'pulse.toggleMonitoring' || value === 'pulse.summary' || value === 'workspaces.open'
      ? value : undefined;
  }

  private async updateActionTitle(action: { setTitle(title: string): Promise<void> }, value: unknown) {
    const selected = this.safeAction(value);
    await action.setTitle(selected ? this.title(selected) : 'Choisir une action');
  }

  private title(action: CompanionAction) {
    return ({
      'orbit.open': 'Ouvrir\nORBIT', 'mission.open': 'Mission\nControl',
      'echo.open': 'Ouvrir\nEcho', 'pulsar.open': 'Ouvrir\nPulsar', 'core.open': 'Ouvrir\nCore',
      'timeline.open': 'Ouvrir\nTimeline', 'settings.open': 'Ouvrir\nParamètres', 'workspaces.open': 'Ouvrir\nEspaces',
      'pulsar.togglePause': 'Pulsar\nPause / reprise', 'pulsar.runScenario': 'Lancer\nPulsar', 'echo.copyLatest': 'Copier\nEcho récent', 'echo.copyPrevious': 'Echo\nPrécédent',
      'echo.toggleMonitoring': 'Echo\nPause / reprise', 'pulse.open': 'Ouvrir\nPulse',
      'pulse.toggleMonitoring': 'Pulse\nPause / reprise', 'pulse.summary': 'Pulse\nSemaine',
    } as const)[action];
  }
}

streamDeck.logger.setLevel('info');
streamDeck.actions.registerAction(new OrbitControl());
streamDeck.connect();
