import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

export type CompanionAction = 'orbit.open' | 'mission.open' | 'echo.open' | 'pulsar.open' | 'pulsar.togglePause' | 'pulsar.runScenario' | 'core.open' | 'timeline.open' | 'settings.open' | 'workspaces.open' | 'echo.copyLatest' | 'echo.copyPrevious' | 'echo.toggleMonitoring' | 'pulse.open' | 'pulse.toggleMonitoring' | 'pulse.summary';
export type HelperOperation = 'pair' | 'status' | 'unpair' | 'action' | 'scenarios';
type HelperResult = { ok: boolean; error?: string; data?: { summarySeconds?: number; monitoringEnabled?: boolean; globalPaused?: boolean; remoteRevoked?: boolean; scenarios?: Array<{ id: string; name: string }> } };

const helperPath = join(dirname(fileURLToPath(import.meta.url)), 'orbit-credential-helper.exe');
const allowed = new Set<CompanionAction>(['orbit.open', 'mission.open', 'echo.open', 'pulsar.open', 'pulsar.togglePause', 'pulsar.runScenario', 'core.open', 'timeline.open', 'settings.open', 'workspaces.open', 'echo.copyLatest', 'echo.copyPrevious', 'echo.toggleMonitoring', 'pulse.open', 'pulse.toggleMonitoring', 'pulse.summary']);

/** Secret values never cross into the Stream Deck process or settings; helper IPC is stdin/stdout only. */
export function invokeHelper(operation: HelperOperation, input: Record<string, unknown> = {}): Promise<HelperResult> {
  if (!['pair', 'status', 'unpair', 'action', 'scenarios'].includes(operation)) return Promise.resolve({ ok: false, error: 'Opération refusée.' });
  if (operation === 'action' && !allowed.has(input.action as CompanionAction)) return Promise.resolve({ ok: false, error: 'Action ORBIT non autorisée.' });
  return new Promise(resolve => {
    const child = spawn(helperPath, [operation], { shell: false, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    let stdout = '';
    let done = false;
    const finish = (result: HelperResult) => { if (!done) { done = true; resolve(result); } };
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => { if (stdout.length < 2048) stdout += chunk; });
    child.on('error', () => finish({ ok: false, error: 'Le compagnon ORBIT sécurisé est introuvable.' }));
    child.on('close', code => {
    try { finish(JSON.parse(stdout) as HelperResult); }
      catch { finish({ ok: false, error: code === 0 ? 'Réponse du compagnon invalide.' : 'Le compagnon ORBIT a refusé l’opération.' }); }
    });
    child.stdin.end(JSON.stringify(input));
    const timeoutMs = operation === 'action' && input.action === 'orbit.open' ? 20000 : 9000;
    const timeout = setTimeout(() => { child.kill(); finish({ ok: false, error: 'Délai du compagnon dépassé.' }); }, timeoutMs);
    child.once('close', () => clearTimeout(timeout));
  });
}
