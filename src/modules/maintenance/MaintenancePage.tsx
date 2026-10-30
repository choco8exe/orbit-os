import { useEffect, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { Archive, CheckCircle2, CircleAlert, Database, Download, ExternalLink, FolderOpen, HardDrive, RefreshCw, ShieldCheck, XCircle } from 'lucide-react';
import './maintenance.css';

type DatabaseHealth = { name: string; present: boolean; readable: boolean; integrity: string };
type DiagnosticReport = { generatedAt: string; appVersion: string; operatingSystem: string; architecture: string; databases: DatabaseHealth[]; privacy: string };
type BackupPreview = { appVersion: string; createdAt: string; databases: string[]; includesEcho: boolean; settingsIncluded: boolean; timelineEntries: number; bytes: number };
type ReleaseCheck = { latestVersion: string; currentVersion: string; updateAvailable: boolean; releaseUrl: string; publishedAt?: string; prerelease: boolean };
type PendingRestoreStatus = { pending: boolean; quarantined: boolean; error: string | null };

const REPOSITORY_KEY = 'orbit.maintenance.githubRepository';
const backupFilter = [{ name: 'ORBIT sauvegarde', extensions: ['orbitbackup'] }];
function readableSize(bytes: number) {
  if (bytes < 1024) return bytes + ' o';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(0) + ' Ko';
  return (bytes / (1024 * 1024)).toFixed(1) + ' Mo';
}
function settingsSnapshot(): Record<string, unknown> | undefined {
  try { const value = localStorage.getItem('orbit.settings.v1'); return value ? JSON.parse(value) as Record<string, unknown> : undefined; } catch { return undefined; }
}
function localJsonSnapshot(key: string): unknown {
  try { return JSON.parse(localStorage.getItem(key) ?? 'null'); } catch { return null; }
}

export function MaintenancePage() {
  const native = isTauri();
  const [health, setHealth] = useState<DiagnosticReport | null>(null);
  const [restoreStatus, setRestoreStatus] = useState<PendingRestoreStatus | null>(null);
  const [repo, setRepo] = useState(() => localStorage.getItem(REPOSITORY_KEY) ?? '');
  const [release, setRelease] = useState<ReleaseCheck | null>(null);
  const [preview, setPreview] = useState<BackupPreview | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [includeEcho, setIncludeEcho] = useState(false);
  const [includeTimeline, setIncludeTimeline] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');

  async function run<T>(action: () => Promise<T>, success?: string): Promise<T | undefined> {
    setBusy(true); setError(''); setStatus('');
    try { const value = await action(); if (success) setStatus(success); return value; }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); return undefined; }
    finally { setBusy(false); }
  }
  async function refreshHealth() {
    const value = await run(async () => ({
      restore: await invoke<PendingRestoreStatus>('maintenance_pending_restore_status'),
      report: await invoke<DiagnosticReport>('maintenance_diagnostics'),
    }), 'État des données vérifié.');
    if (value) { setRestoreStatus(value.restore); setHealth(value.report); }
  }
  useEffect(() => { if (native) void refreshHealth(); }, [native]);

  async function createBackup() {
    const accepted = window.confirm('Cette sauvegarde peut contenir les scénarios Pulsar, l’historique Pulse et des références de fichiers. ' + (includeTimeline ? 'Timeline est inclus. ' : '') + (includeEcho ? 'Echo est inclus et peut contenir des éléments très privés. ' : '') + 'Les secrets du coffre Windows ne sont jamais exportés. Continuer ?');
    if (!accepted) return;
    const date = new Date().toISOString().slice(0, 10);
    const outputPath = await save({ defaultPath: 'ORBIT-sauvegarde-' + date + '.orbitbackup', filters: backupFilter });
    if (!outputPath) return;
    const clientData = await run(async () => ({
      timeline: includeTimeline ? localJsonSnapshot('orbit.timeline.v1') : null,
    }));
    if (!clientData) return;
    await run(() => invoke('maintenance_backup_create', {
      outputPath, includeEcho, settings: settingsSnapshot(), includeTimeline, timeline: clientData.timeline,
    }), 'Sauvegarde créée. Conserve le fichier dans un emplacement sûr.');
  }
  async function inspectBackup() {
    const backupPath = await open({ multiple: false, filters: backupFilter });
    if (typeof backupPath !== 'string') return;
    const value = await run(() => invoke<BackupPreview>('maintenance_backup_preview', { backupPath }), 'Sauvegarde validée.');
    if (value) { setPreview(value); setPreviewPath(backupPath); }
  }
  async function stageRestore() {
    if (!preview) return;
    const accepted = window.confirm('Cette restauration sera préparée pour le prochain démarrage. Elle remplacera des données locales. Crée une sauvegarde avant de continuer. Continuer ?');
    if (!accepted) return;
    if (!previewPath) return;
    await run(() => invoke<BackupPreview>('maintenance_backup_stage_restore', { backupPath: previewPath, confirmed: true }), 'Restauration préparée. Redémarre ORBIT pour appliquer les données; les secrets Windows restent sur cet appareil.');
  }
  async function exportDiagnostics() {
    const outputPath = await save({ defaultPath: 'ORBIT-diagnostic-' + new Date().toISOString().slice(0, 10) + '.json', filters: [{ name: 'Rapport JSON', extensions: ['json'] }] });
    if (!outputPath) return;
    await run(() => invoke('maintenance_diagnostics_export', { outputPath }), 'Rapport local exporté. Il ne contient ni chemins personnels ni contenu Echo.');
  }
  async function checkRelease() {
    const [owner, repository, ...extra] = repo.trim().split('/');
    if (!owner || !repository || extra.length) { setError('Saisis un dépôt public au format propriétaire/dépôt.'); setStatus(''); return; }
    localStorage.setItem(REPOSITORY_KEY, owner + '/' + repository);
    setRepo(owner + '/' + repository);
    const value = await run(() => invoke<ReleaseCheck>('maintenance_check_updates', { owner, repository }));
    if (value) { setRelease(value); setStatus(value.updateAvailable ? 'La version ' + value.latestVersion + ' est publiée. Télécharge son installeur depuis GitHub.' : 'ORBIT ' + value.currentVersion + ' correspond à la dernière version détectée.'); }
  }

  return <section className="maintenance-page" aria-labelledby="maintenance-title">
    <header className="maintenance-header">
      <div><p className="maintenance-eyebrow">ORBIT · DONNÉES LOCALES</p><h1 id="maintenance-title">Sauvegarde et diagnostic</h1><p>Protège tes données, vérifie leur état et retrouve les versions publiques d’ORBIT.</p></div>
      <button type="button" className="maintenance-secondary" onClick={() => void refreshHealth()} disabled={!native || busy}><RefreshCw size={16} aria-hidden="true" /> Vérifier l’état</button>
    </header>
    {!native && <p className="maintenance-notice" role="status">Les commandes de sauvegarde et les diagnostics sont disponibles dans l’application Windows installée.</p>}
    {native && restoreStatus?.pending && <p className="maintenance-notice" role="status">Une restauration est en attente. Elle sera appliquée au prochain démarrage, avant l’ouverture des bases ORBIT.</p>}
    {native && restoreStatus?.error && <p className="maintenance-error" role="alert">La restauration n’a pas été appliquée. ORBIT a démarré avec les données locales existantes. {restoreStatus.quarantined ? 'Le paquet en échec a été mis en quarantaine.' : 'Le paquet en échec est désactivé pour éviter de bloquer les prochains démarrages.'} Détail : {restoreStatus.error}</p>}
    <p className="maintenance-notice">Les fichiers .orbitbackup ne sont pas chiffrés : conserve-les dans un emplacement de confiance.</p>
    <div className="maintenance-grid">
      <section className="maintenance-card" aria-labelledby="maintenance-backup-title">
        <div className="maintenance-card-icon"><Archive size={19} aria-hidden="true" /></div><h2 id="maintenance-backup-title">Sauvegardes</h2>
        <p>Crée une copie cohérente des bases locales : scénarios, notes FL Studio, historiques Pulse et préférences. Les états d’installation et permissions des plugins restent sur cet appareil. Les secrets du coffre Windows ne sont jamais exportés.</p>
        <label className="maintenance-check"><input type="checkbox" checked={includeTimeline} onChange={event => setIncludeTimeline(event.target.checked)} /><span><strong>Inclure Timeline</strong><small>L’historique des événements peut mentionner des scénarios et chemins de fichiers.</small></span></label>
        <label className="maintenance-check"><input type="checkbox" checked={includeEcho} onChange={event => setIncludeEcho(event.target.checked)} /><span><strong>Inclure Echo</strong><small>L’historique peut contenir des textes ou images privés. Désactivé par défaut.</small></span></label>
        <div className="maintenance-actions"><button type="button" className="maintenance-primary" disabled={!native || busy} onClick={() => void createBackup()}><Download size={16} aria-hidden="true" /> Créer une sauvegarde</button><button type="button" className="maintenance-secondary" disabled={!native || busy} onClick={() => void inspectBackup()}><FolderOpen size={16} aria-hidden="true" /> Vérifier un fichier</button></div>
        {preview && <div className="maintenance-preview" aria-live="polite"><b>Fichier valide · {readableSize(preview.bytes)}</b><span>Version {preview.appVersion} · {preview.databases.length} bases · Préférences {preview.settingsIncluded ? 'incluses' : 'absentes'} · Timeline {preview.timelineEntries} · Echo {preview.includesEcho ? 'inclus' : 'absent'}</span><details><summary>Voir les bases incluses</summary><ul>{preview.databases.map(name => <li key={name}>{name}</li>)}</ul></details><button type="button" className="maintenance-secondary" disabled={!native || busy} onClick={() => void stageRestore()}>Préparer la restauration au prochain démarrage</button></div>}
      </section>

      <section className="maintenance-card" aria-labelledby="maintenance-updates-title">
        <div className="maintenance-card-icon"><RefreshCw size={19} aria-hidden="true" /></div><h2 id="maintenance-updates-title">Mises à jour</h2>
        <p>Vérifie une publication publique GitHub. ORBIT n’installe ni ne lance d’exécutable téléchargé automatiquement. La recherche reste désactivée tant qu’aucun dépôt n’est configuré.</p>
        <label className="maintenance-field" htmlFor="maintenance-repo">Dépôt public GitHub</label><input id="maintenance-repo" className="maintenance-input" value={repo} onChange={event => { setRepo(event.target.value); setRelease(null); }} placeholder="propriétaire/dépôt" autoComplete="off" />
        <button type="button" className="maintenance-primary" disabled={!native || busy || !repo.trim()} onClick={() => void checkRelease()}><RefreshCw size={16} aria-hidden="true" /> Rechercher une version</button>
        {release && <div className="maintenance-preview"><b>{release.updateAvailable ? 'Version ' + release.latestVersion + ' disponible' : 'Version ' + release.latestVersion + ' détectée'}</b><span>{release.prerelease ? 'Publication de préversion.' : 'Publication stable.'} La mise à jour nécessite le téléchargement manuel de l’installeur.</span><a href={release.releaseUrl} target="_blank" rel="noreferrer">Ouvrir la publication GitHub <ExternalLink size={14} aria-hidden="true" /></a></div>}
      </section>

      <section className="maintenance-card maintenance-health" aria-labelledby="maintenance-health-title">
        <div className="maintenance-card-icon"><Database size={19} aria-hidden="true" /></div><h2 id="maintenance-health-title">État des données</h2>
        {!health && <p>{native ? 'Lance une vérification pour contrôler les bases locales.' : 'État disponible depuis l’application Windows.'}</p>}
        {health && <><p>Version {health.appVersion} · {health.operatingSystem} · {health.architecture}</p><ul className="maintenance-db-list">{health.databases.map(db => <li key={db.name}><span className="maintenance-db-status" aria-hidden="true">{db.present && db.readable && db.integrity === 'ok' ? <CheckCircle2 size={16} /> : db.present ? <CircleAlert size={16} /> : <HardDrive size={16} />}</span><span>{db.name}</span><small>{db.integrity}</small></li>)}</ul></>}
      </section>

      <section className="maintenance-card" aria-labelledby="maintenance-privacy-title">
        <div className="maintenance-card-icon"><ShieldCheck size={19} aria-hidden="true" /></div><h2 id="maintenance-privacy-title">Données du rapport</h2>
        <p>Le diagnostic exporté contient la version, la plateforme et l’état d’intégrité des bases. Il n’inclut aucun chemin utilisateur, texte Echo, conversation, secret ou jeton.</p>
        <button type="button" className="maintenance-secondary" disabled={!native || busy} onClick={() => void exportDiagnostics()}><Download size={16} aria-hidden="true" /> Exporter un rapport local</button>
      </section>
    </div>
    <div className="maintenance-live" aria-live="polite" aria-atomic="true">{status}</div>
    {error && <p className="maintenance-error" role="alert"><XCircle size={16} aria-hidden="true" /> {error}</p>}
  </section>;
}
