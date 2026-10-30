import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  Activity, Bot, Check, ChevronLeft, Command, Database, Download,
  FolderOpen, HardDrive, History, Orbit, Sparkles, Trash2,
} from 'lucide-react';
import {
  activityPulseService,
  type ActivityAppUsage,
  type ActivityCategory,
  type ActivityPulseSnapshot,
  type ActivityRange,
} from './api';
import { dailyGoalsWithoutVisibleAppUsage } from './daily-goals';
import './activity-pulse.css';

type Props = { navigate: (id: 'plugins') => void };
const categoryNames: Record<ActivityCategory, string> = {
  game: 'Jeux', development: 'Développement', creative: 'Création', browser: 'Navigateur', other: 'Autres',
};
const ranges: Array<{ id: ActivityRange; label: string }> = [
  { id: 'day', label: 'Aujourd’hui' }, { id: 'week', label: '7 jours' }, { id: 'month', label: 'Ce mois' },
];

function duration(seconds: number): string {
  const minutes = Math.max(0, Math.round(seconds / 60));
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return `${rest} min`;
  return rest ? `${hours} h ${rest.toString().padStart(2, '0')}` : `${hours} h`;
}

const applicationColors = ['#4CC9F0', '#F29E4C', '#A78BFA', '#55D6A2', '#F06E8A', '#E4D65A'];

function appIcon(app: ActivityAppUsage) {
  if (app.category === 'game') return Orbit;
  if (app.category === 'development') return Command;
  if (app.category === 'browser') return FolderOpen;
  if (app.category === 'creative') return Sparkles;
  return Database;
}

export function ActivityPulsePage({ navigate }: Props) {
  const [snapshot, setSnapshot] = useState<ActivityPulseSnapshot | null>(null);
  const [range, setRange] = useState<ActivityRange>('week');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [clearError, setClearError] = useState('');
  const [selectedAppId, setSelectedAppId] = useState<string | null>(null);
  const [goalMinutesDraft, setGoalMinutesDraft] = useState('');
  const clearDialog = useRef<HTMLDialogElement>(null);
  const selectedApp = snapshot?.apps.find(app => app.appId === selectedAppId);
  const selectedGoalMinutes = selectedApp?.dailyTargetMinutes ?? null;

  useEffect(() => {
    setGoalMinutesDraft(selectedGoalMinutes ? String(selectedGoalMinutes) : '');
  }, [selectedAppId, selectedGoalMinutes]);

  const load = useCallback(async (requestedRange = range) => {
    if (!activityPulseService.available()) { setLoading(false); return; }
    try {
      const result = await activityPulseService.snapshot(requestedRange);
      setSnapshot(result);
      setError('');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally { setLoading(false); }
  }, [range]);

  useEffect(() => { void load(range); }, [load, range]);
  useEffect(() => {
    if (!activityPulseService.available()) return;
    let unlisten: (() => void) | undefined;
    let mounted = true;
    void listen('activity-pulse://changed', () => { if (mounted) void load(); }).then(stop => {
      if (mounted) unlisten = stop; else stop();
    }).catch(() => undefined);
    return () => { mounted = false; unlisten?.(); };
  }, [load]);

  const mutate = async (operation: () => Promise<ActivityPulseSnapshot>, success: string) => {
    setBusy(true); setError(''); setNotice('');
    try { setSnapshot(await operation()); setNotice(success); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  const exportCsv = async () => {
    setBusy(true); setError(''); setNotice('');
    try {
      const csv = await activityPulseService.exportCsv();
      const url = URL.createObjectURL(new Blob([`\ufeff${csv}`], { type: 'text/csv;charset=utf-8' }));
      const anchor = document.createElement('a');
      anchor.href = url; anchor.download = 'orbit-pulse-activity.csv'; anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      setNotice('Export CSV préparé sur cet appareil.');
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  const clearHistory = async () => {
    if (busy) return;
    setBusy(true);
    setClearError('');
    setError('');
    setNotice('');
    try {
      const result = await activityPulseService.clearHistory();
      setSnapshot(result);
      setSelectedAppId(null);
      setNotice('Historique supprimé. Le suivi reste actif et reprendra lors des prochaines applications utilisées.');
      clearDialog.current?.close();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setClearError(message);
      setError(message);
    } finally {
      setBusy(false);
    }
  };

  const updateExcluded = (appId: string, excluded: boolean) => {
    if (!snapshot) return;
    const excludedApps = new Set(snapshot.preferences.excludedApps);
    if (excluded) excludedApps.add(appId); else excludedApps.delete(appId);
    void mutate(() => activityPulseService.setPreferences({
      countBackgroundApps: snapshot.preferences.countBackgroundApps,
      idleThresholdSeconds: snapshot.preferences.idleThresholdSeconds,
      retentionDays: snapshot.preferences.retentionDays,
      excludedApps: [...excludedApps],
    }), excluded ? 'Application exclue du suivi.' : 'Application réincluse dans le suivi.');
  };

  const chartMax = useMemo(() => Math.max(1, ...((snapshot?.days ?? []).map(day => day.totalSeconds))), [snapshot]);
  const chartApps = useMemo(() => (snapshot?.apps ?? [])
    .filter(app => app.totalSeconds > 0)
    .sort((a, b) => b.totalSeconds - a.totalSeconds || (a.appId.toLowerCase() < b.appId.toLowerCase() ? -1 : a.appId.toLowerCase() > b.appId.toLowerCase() ? 1 : 0))
    .slice(0, 5), [snapshot]);
  const chartColors = useMemo(() => new Map(chartApps.map((app, index) => [app.appId, applicationColors[index]])), [chartApps]);
  const savedGoals = useMemo(() => dailyGoalsWithoutVisibleAppUsage(snapshot?.dailyGoals ?? [], snapshot?.apps ?? []), [snapshot]);
  const native = activityPulseService.available();
  const preferences = snapshot?.preferences;

  return <section className="page orbit-module-page activity-pulse-page" aria-labelledby="activity-pulse-title">
    <div className="page-heading orbit-module-heading activity-pulse-heading">
      <div><p className="eyebrow">PLUGIN LOCAL · TEMPS D’ACTIVITÉ</p><h1 id="activity-pulse-title">ORBIT Pulse</h1><p className="page-subtitle">Comprends le temps passé dans tes applications et tes jeux.</p></div>
      <button type="button" className="secondary-button" onClick={() => navigate('plugins')}><ChevronLeft size={16} />Plugin Center</button>
    </div>

    {!native && <div className="activity-pulse-note" role="status"><HardDrive size={17} /><span>Le suivi des fenêtres actives est disponible dans l’application Windows. Cet aperçu navigateur ne collecte aucune donnée.</span></div>}
    {snapshot?.unavailableReason && <div className="activity-pulse-note" role="status"><HardDrive size={17} /><span>{snapshot.unavailableReason}</span></div>}
    {error && <p className="plugin-error" role="alert">{error}</p>}
    {notice && <p className="plugin-success" role="status"><Check size={14} />{notice}</p>}

    <div className="activity-pulse-toolbar panel">
      <div className="activity-range" role="group" aria-label="Période du rapport">
        {ranges.map(item => <button key={item.id} type="button" className={range === item.id ? 'selected' : ''} aria-pressed={range === item.id} onClick={() => setRange(item.id)}>{item.label}</button>)}
      </div>
      <button className="icon-button" type="button" onClick={() => void load()} disabled={loading || busy} aria-label="Actualiser les statistiques" title="Actualiser"><Activity size={16} /></button>
    </div>

    <div className="activity-summary-grid">
      <article className="activity-summary-card panel"><span>Temps au premier plan</span><strong>{loading ? '—' : duration(snapshot?.totalSeconds ?? 0)}</strong><small>Application focalisée avec activité récente</small></article>
      <article className="activity-summary-card panel"><span>Fenêtres ouvertes en arrière-plan</span><strong>{loading ? '—' : duration(snapshot?.backgroundSeconds ?? 0)}</strong><small>Estimation distincte, non additionnée au temps au premier plan</small></article>
      <article className="activity-summary-card panel"><span>Applications</span><strong>{loading ? '—' : snapshot?.apps.length ?? 0}</strong><small>Applications suivies sur la période</small></article>
      <article className="activity-summary-card panel"><span>État du suivi</span><strong>{preferences?.monitoringEnabled ? 'En cours' : 'En pause'}</strong><small>{preferences?.monitoringEnabled ? 'Les données restent sur cet appareil' : 'Active-le quand tu es prêt'}</small></article>
    </div>

    <div className="activity-pulse-grid">
      <article className="activity-panel panel" aria-labelledby="activity-chart-title">
        <div className="activity-panel-heading"><div><p className="eyebrow">RÉPARTITION</p><h2 id="activity-chart-title">Temps au premier plan par jour</h2></div><History size={18} aria-hidden="true" /></div>
        {chartApps.length > 0 && <div className="activity-app-legend" aria-label="Légende des applications du graphique">{chartApps.map(app => <span key={app.appId}><i style={{ backgroundColor: chartColors.get(app.appId) }} />{app.name}</span>)}{(snapshot?.apps.length ?? 0) > chartApps.length && <span><i className="activity-other-color" />Autres applications</span>}</div>}
        {(snapshot?.days.length ?? 0) > 0 ? <div className="activity-chart" role="img" aria-label={`Temps au premier plan par jour et par application. ${(snapshot?.days ?? []).map(day => {
          const named = chartApps.map(app => ({ name: app.name, seconds: day.applications[app.appId] ?? 0 })).filter(item => item.seconds > 0);
          const other = Math.max(0, day.totalSeconds - named.reduce((sum, item) => sum + item.seconds, 0));
          return `${day.date} : ${duration(day.totalSeconds)} ; ${named.map(item => `${item.name} ${duration(item.seconds)}`).join(', ')}${other ? `, autres applications ${duration(other)}` : ''}`;
        }).join('; ')}`}>
          {(snapshot?.days ?? []).map(day => {
            const segments = chartApps.map(app => ({ id: app.appId, name: app.name, seconds: day.applications[app.appId] ?? 0, color: chartColors.get(app.appId)! })).filter(segment => segment.seconds > 0);
            const listedSeconds = segments.reduce((total, segment) => total + segment.seconds, 0);
            const otherSeconds = Math.max(0, day.totalSeconds - listedSeconds);
            if (otherSeconds > 0) segments.push({ id: 'other', name: 'Autres applications', seconds: otherSeconds, color: '#728096' });
            return <div className="activity-chart-column" key={day.date} title={`${day.date} · ${duration(day.totalSeconds)}`}>
            <span className="activity-chart-value">{day.totalSeconds ? duration(day.totalSeconds) : '—'}</span>
            <div className="activity-chart-track"><div className="activity-chart-bar" style={{ height: `${Math.max(day.totalSeconds ? 4 : 0, day.totalSeconds / chartMax * 100)}%` }} aria-label={`${day.date} : ${segments.map(segment => `${segment.name}, ${duration(segment.seconds)}`).join('; ')}`}>{segments.map(segment => <i key={segment.id} title={`${segment.name} · ${duration(segment.seconds)}`} aria-hidden="true" style={{ height: `${segment.seconds / Math.max(1, day.totalSeconds) * 100}%`, backgroundColor: segment.color }} />)}</div></div>
            <span className="activity-chart-label">{new Intl.DateTimeFormat('fr-FR', { weekday: 'short', day: 'numeric' }).format(new Date(`${day.date}T12:00:00`))}</span>
          </div>;})}
        </div> : <div className="activity-empty"><Activity size={22} /><p>{loading ? 'Chargement des statistiques…' : 'Pas encore d’activité enregistrée sur cette période.'}</p><small>Les durées des fenêtres ouvertes en arrière-plan sont affichées séparément.</small></div>}
      </article>

      <article className="activity-panel panel" aria-labelledby="activity-apps-title">
        <div className="activity-panel-heading"><div><p className="eyebrow">TES APPLICATIONS</p><h2 id="activity-apps-title">Temps utilisé</h2></div><span className="activity-app-count">{snapshot?.apps.length ?? 0}</span></div>
        {(snapshot?.apps.length ?? 0) > 0 ? <div className="activity-app-list">{snapshot!.apps.map(app => {
          const Icon = appIcon(app);
          return <div className="activity-app-row" key={app.appId}>
            <div className={`activity-app-icon category-${app.category}`} aria-hidden="true"><Icon size={18} /></div>
            <button className="activity-app-main activity-app-drilldown" type="button" aria-expanded={selectedAppId === app.appId} onClick={() => setSelectedAppId(current => current === app.appId ? null : app.appId)}><strong title={app.name}>{app.name}</strong><small>{app.sessionCount} session{app.sessionCount === 1 ? '' : 's'} · premier plan {duration(app.totalSeconds)} · arrière-plan {duration(app.backgroundSeconds)}</small></button>
            <select aria-label={`Catégorie de ${app.name}`} value={app.category} disabled={busy} onChange={event => {
              const category = event.target.value as ActivityCategory;
              void mutate(() => activityPulseService.setCategory(app.appId, category), `Catégorie de ${app.name} mise à jour.`);
            }}>{Object.entries(categoryNames).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select>
            <strong className="activity-app-duration">{duration(app.totalSeconds)}</strong>
          </div>;
        })}{selectedApp && (() => {
          const dayRows = snapshot!.days.map(day => ({ date: day.date, seconds: selectedApp.dailySeconds[day.date] ?? 0 }));
          const max = Math.max(1, ...dayRows.map(day => day.seconds));
          const today = new Date();
          const todayKey = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
          const todaySeconds = selectedApp.dailySeconds[todayKey] ?? 0;
          const targetSeconds = (selectedApp.dailyTargetMinutes ?? 0) * 60;
          const progress = targetSeconds > 0 ? Math.min(100, Math.round(todaySeconds / targetSeconds * 100)) : 0;
          return <div className="activity-app-detail" aria-label={`Détail de ${selectedApp.name}`}>
            <strong>{selectedApp.name} · premier plan {duration(selectedApp.totalSeconds)} · arrière-plan {duration(selectedApp.backgroundSeconds)}</strong>
            <div>{dayRows.map(day => <span key={day.date} title={`${day.date} · ${duration(day.seconds)}`}><i style={{ height: `${Math.max(day.seconds ? 5 : 0, day.seconds / max * 100)}%` }} /><small>{new Intl.DateTimeFormat('fr-FR', { weekday: 'short', day: 'numeric' }).format(new Date(`${day.date}T12:00:00`))}</small></span>)}</div>
            <form className="activity-goal-form" onSubmit={event => {
              event.preventDefault();
              const minutes = goalMinutesDraft.trim() ? Number(goalMinutesDraft) : null;
              if (minutes === null || !Number.isInteger(minutes) || minutes < 5 || minutes > 1440) return;
              void mutate(() => activityPulseService.setAppDailyTarget(selectedApp.appId, minutes), 'Objectif quotidien enregistré.');
            }}>
              <label htmlFor="activity-daily-goal">Objectif quotidien · temps au premier plan</label>
              <div className="activity-goal-controls">
                <input id="activity-daily-goal" type="number" inputMode="numeric" min={5} max={1440} step={1} value={goalMinutesDraft} placeholder="Sans objectif" aria-describedby="activity-goal-help" onChange={event => setGoalMinutesDraft(event.target.value)} />
                <span>minutes</span>
                <button type="submit" className="secondary-button" disabled={busy || !goalMinutesDraft.trim() || !Number.isInteger(Number(goalMinutesDraft)) || Number(goalMinutesDraft) < 5 || Number(goalMinutesDraft) > 1440}>Enregistrer</button>
                {selectedGoalMinutes !== null && <button type="button" className="text-button" disabled={busy} onClick={() => { setGoalMinutesDraft(''); void mutate(() => activityPulseService.setAppDailyTarget(selectedApp.appId, null), 'Objectif quotidien supprimé.'); }}>Supprimer</button>}
              </div>
              <small id="activity-goal-help">Entre 5 minutes et 24 heures. L’objectif reste sur cet appareil. Les estimations d’arrière-plan ne comptent pas dans la progression.</small>
            </form>
            {selectedGoalMinutes !== null && <div className={`activity-goal-progress${todaySeconds >= targetSeconds ? ' reached' : ''}`}>
              <div><strong>{todaySeconds >= targetSeconds ? 'Objectif atteint' : 'Progression aujourd’hui'}</strong><span>{duration(todaySeconds)} / {duration(targetSeconds)} · {progress}%</span></div>
              <div className="activity-goal-track" role="progressbar" aria-label={`Objectif quotidien de ${selectedApp.name}, temps au premier plan`} aria-valuemin={0} aria-valuemax={targetSeconds} aria-valuenow={Math.min(todaySeconds, targetSeconds)} aria-valuetext={`${duration(todaySeconds)} sur ${duration(targetSeconds)} au premier plan`}><i style={{ width: `${progress}%` }} /></div>
              {todaySeconds > targetSeconds && <small>Dépassé de {duration(todaySeconds - targetSeconds)}.</small>}
              {todaySeconds < targetSeconds && <small>Il reste {duration(targetSeconds - todaySeconds)} au premier plan pour atteindre l’objectif.</small>}
            </div>}
          </div>;
        })()}</div> : null}
      </article>
    </div>

    {savedGoals.length > 0 && <article className="activity-panel activity-saved-goals panel" aria-labelledby="activity-saved-goals-title">
      <div className="activity-panel-heading"><div><p className="eyebrow">RÉGLAGES CONSERVÉS</p><h2 id="activity-saved-goals-title">Objectifs sans activité sur cette période</h2><p>Ces applications n’ont pas de ligne dans la période choisie. Leurs objectifs restent enregistrés localement.</p></div><History size={18} aria-hidden="true" /></div>
      <ul className="activity-saved-goal-list">{savedGoals.map(goal => <li key={goal.appId}>
        <span><strong>{goal.appName}</strong><small>{goal.appId}</small></span>
        <span className="activity-saved-goal-target">{duration(goal.dailyTargetMinutes * 60)} / jour</span>
        <button type="button" className="text-button danger-action" disabled={busy} aria-label={`Supprimer l’objectif de ${goal.appName}`} onClick={() => void mutate(() => activityPulseService.setAppDailyTarget(goal.appId, null), `Objectif de ${goal.appName} supprimé.`)}><Trash2 size={14} />Supprimer</button>
      </li>)}</ul>
    </article>}

    <article className="activity-settings panel" aria-labelledby="activity-settings-title">
      <div className="activity-panel-heading"><div><p className="eyebrow">LOCAL ET PRIVÉ</p><h2 id="activity-settings-title">Réglages du suivi</h2></div><Activity size={18} aria-hidden="true" /></div>
      <div className="activity-setting-row"><div><strong>Suivre les applications au premier plan</strong><small>Pulse estime le temps d’utilisation à partir de l’application au premier plan et d’une activité récente. Une vidéo ou une lecture sans interaction peut donc être marquée inactive. Seul le nom du programme et sa durée estimée sont conservés ; aucun titre de fenêtre, texte saisi ni capture n’est collecté.</small></div><label className="activity-switch"><span className="sr-only">Activer le suivi des applications</span><input type="checkbox" checked={preferences?.monitoringEnabled ?? false} disabled={!native || busy || !snapshot?.available} onChange={event => void mutate(() => activityPulseService.setMonitoring(event.target.checked), event.target.checked ? 'Suivi activé.' : 'Suivi mis en pause.')} /><i aria-hidden="true" /></label></div>
      <div className="activity-setting-row"><div><strong>Estimer les applications ouvertes en arrière-plan</strong><small>Compte séparément les applications avec une fenêtre visible (y compris réduite) tant que tu utilises le PC. C’est une estimation de processus ouvert : elle ne confirme pas qu’une vidéo joue. Chrome/Edge restera identifié comme navigateur, jamais comme YouTube. Les durées de plusieurs applications peuvent se chevaucher et ne s’ajoutent pas au temps au premier plan.</small></div><label className="activity-switch"><span className="sr-only">Estimer les applications ouvertes en arrière-plan</span><input type="checkbox" checked={preferences?.countBackgroundApps ?? false} disabled={!native || busy || !snapshot?.available || !preferences?.monitoringEnabled} onChange={event => preferences && void mutate(() => activityPulseService.setPreferences({ ...preferences, countBackgroundApps: event.target.checked }), event.target.checked ? 'Estimation des fenêtres en arrière-plan activée.' : 'Estimation des fenêtres en arrière-plan désactivée.')} /><i aria-hidden="true" /></label></div>
      <div className="activity-preference-grid">
        <label><span>Déclarer l’inactivité après</span><select value={preferences?.idleThresholdSeconds ?? 300} disabled={!native || busy || !preferences} onChange={event => preferences && void mutate(() => activityPulseService.setPreferences({ ...preferences, idleThresholdSeconds: Number(event.target.value) }), 'Seuil d’inactivité enregistré.')}><option value={30}>30 secondes</option><option value={60}>1 minute</option><option value={180}>3 minutes</option><option value={300}>5 minutes</option></select></label>
        <label><span>Conserver les données</span><select value={preferences?.retentionDays ?? 30} disabled={!native || busy || !preferences} onChange={event => preferences && void mutate(() => activityPulseService.setPreferences({ ...preferences, retentionDays: Number(event.target.value) }), 'Durée de conservation enregistrée.')}><option value={7}>7 jours</option><option value={30}>30 jours</option><option value={90}>90 jours</option><option value={365}>1 an</option></select></label>
      </div>
      {(snapshot?.apps.length ?? 0) > 0 && <details className="activity-exclusions"><summary>Exclure des applications</summary><div className="activity-exclusion-list">{snapshot!.apps.map(app => <label key={app.appId}><input type="checkbox" checked={preferences?.excludedApps.includes(app.appId) ?? false} disabled={busy || !preferences} onChange={event => updateExcluded(app.appId, event.target.checked)} /><span>{app.name}</span></label>)}</div></details>}
      <div className="activity-data-actions"><button className="secondary-button" type="button" disabled={!native || busy} onClick={() => void exportCsv()}><Download size={15} />Exporter CSV</button><button className="text-button danger-action" type="button" disabled={!native || busy || !snapshot?.apps.length} onClick={() => { setClearError(''); if (clearDialog.current && !clearDialog.current.open) clearDialog.current.showModal(); }}><Trash2 size={15} />Effacer l’historique</button></div>
    </article>

    <div className="activity-site-note"><Bot size={15} /><span>Les pictogrammes indiquent la catégorie ; Pulse n’utilise pas encore les logos propres à chaque application. Il suit les applications Windows, pas les sites visités. Le suivi des domaines demanderait une extension de navigateur distincte et une autorisation séparée.</span></div>

    <dialog className="plugin-dialog activity-clear-dialog" ref={clearDialog} aria-labelledby="activity-clear-title" onCancel={() => setClearError('')} onClose={() => setClearError('')}>
      <div>
        <h2 id="activity-clear-title">Effacer l’historique Pulse ?</h2>
        <p>Les statistiques locales seront supprimées. Le suivi restera actif et reprendra lorsque tu utiliseras de nouveau une application.</p>
        {clearError && <p className="plugin-error" role="alert">{clearError}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" type="button" disabled={busy} onClick={() => clearDialog.current?.close()}>Annuler</button>
          <button className="danger-button" type="button" disabled={busy} onClick={() => void clearHistory()}>{busy ? 'Effacement…' : 'Effacer l’historique'}</button>
        </div>
      </div>
    </dialog>
    <div className="activity-live-region" role="status" aria-live="polite">{loading ? 'Chargement des statistiques.' : ''}</div>
  </section>;
}
