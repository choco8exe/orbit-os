import { useEffect, useId, useMemo, useState } from 'react';
import { Activity, Check, ChevronRight, CircleAlert, Database, History as HistoryIcon, Orbit, Pause, Play, Plus, Trash2, Zap } from 'lucide-react';
import type { Action, Condition, Scenario, SimulationResult, Trigger } from './types';
import { EVENT_TRIGGERS } from './types';
import { pulsarPort } from './service';
import './pulsar.css';

type Tab = 'scenarios' | 'history';

const nowIso = () => new Date().toISOString();
const newScenario = (): Scenario => ({
  id: crypto.randomUUID(), name: 'Nouvelle automatisation', description: '', enabled: false,
  triggers: [{ kind: 'manual' }], conditions: [], actions: [{ kind: 'log', message: 'Scénario exécuté.' }],
  conditionMode: 'all',
  allowScheduledFileChanges: false, createdAt: nowIso(), updatedAt: nowIso(),
});

function localDateTime(iso: string) {
  const date = new Date(iso);
  const offset = date.getTimezoneOffset();
  return new Date(date.getTime() - offset * 60_000).toISOString().slice(0, 16);
}

function triggerSummary(trigger: Trigger) {
  switch (trigger.kind) {
    case 'manual': return 'Déclenchement manuel';
    case 'startup': return 'Au démarrage d’ORBIT';
    case 'interval': return `Toutes les ${trigger.everySeconds} s`;
    case 'at-time': return `Le ${new Intl.DateTimeFormat('fr-FR', { dateStyle: 'short', timeStyle: 'short' }).format(new Date(trigger.at))}`;
    case 'file-created': return `Nouveau fichier · ${trigger.directory}`;
    case 'file-modified': return `Fichier modifié · ${trigger.path}`;
    case 'event': return EVENT_TRIGGERS.find(item => item.value === trigger.name)?.label ?? trigger.name;
    case 'process-started': return `Au lancement de ${trigger.executable.split(/[\\/]/).pop() || 'l’application'}`;
  }
}

function triggersSummary(triggers: Trigger[]) { return triggers.map(triggerSummary).join(' OU '); }

function actionSummary(action: Action) {
  switch (action.kind) {
    case 'notify': return `Notification : ${action.title || 'sans titre'}`;
    case 'open-folder': return `Ouvrir le dossier · ${action.path || 'à choisir'}`;
    case 'open-app': return `Ouvrir ${({ notepad: 'Bloc-notes', calculator: 'Calculatrice', 'file-explorer': 'Explorateur de fichiers' })[action.app]}`;
    case 'copy-file': return `Copier · ${action.source || 'source'} → ${action.destination || 'destination'}`;
    case 'move-file': return `Déplacer · ${action.source || 'source'} → ${action.destination || 'destination'}`;
    case 'rename-file': return `Renommer · ${action.path || 'fichier'} → ${action.newName || 'nouveau nom'}`;
    case 'log': return `Ajouter au journal · ${action.message || 'événement'}`;
  }
}

const writesFiles = (actions: Action[]) => actions.some(action => action.kind === 'copy-file' || action.kind === 'move-file' || action.kind === 'rename-file');

export function PulsarPage() {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [draft, setDraft] = useState<Scenario>();
  const [logs, setLogs] = useState<Awaited<ReturnType<typeof pulsarPort.logs>>>([]);
  const [tab, setTab] = useState<Tab>('scenarios');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [simulation, setSimulation] = useState<SimulationResult>();
  const [saved, setSaved] = useState(false);
  const [guideOpen, setGuideOpen] = useState(false);
  const [creationStatus, setCreationStatus] = useState('');
  const [globallyPaused, setGloballyPaused] = useState(false);

  const selected = scenarios.find(scenario => scenario.id === selectedId);
  const enabledCount = scenarios.filter(scenario => scenario.enabled).length;
  const canSave = Boolean(draft?.name.trim() && draft?.actions.length && pulsarPort.available);

  const refresh = async (preferredId?: string) => {
    if (!pulsarPort.available) { setLoading(false); return; }
    setError('');
    try {
      const [nextScenarios, nextLogs, pauseStatus] = await Promise.all([pulsarPort.list(), pulsarPort.logs(30), pulsarPort.globalPauseStatus()]);
      setScenarios(nextScenarios);
      setLogs(nextLogs);
      setGloballyPaused(pauseStatus);
      const nextId = preferredId ?? selectedId;
      const active = nextScenarios.find(item => item.id === nextId) ?? nextScenarios[0];
      setSelectedId(active?.id);
      if (active && !draft) setDraft(active);
      if (active && draft && active.id === draft.id && active.updatedAt !== draft.updatedAt) setDraft(active);
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Impossible de charger Pulsar.'); }
    finally { setLoading(false); }
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void pulsarPort.subscribeGlobalPause(paused => { if (active) setGloballyPaused(paused); })
      .then(stop => { if (active) unsubscribe = stop; else stop(); })
      .catch(() => undefined);
    return () => { active = false; unsubscribe?.(); };
  }, []);
  useEffect(() => {
    if (!selected || draft?.id === selected.id) return;
    setDraft(selected);
    setSimulation(undefined);
    setSaved(false);
  }, [selectedId, selected?.id]);

  const updateDraft = (patch: Partial<Scenario>) => {
    setSaved(false);
    setSimulation(undefined);
    setDraft(current => current ? { ...current, ...patch, updatedAt: nowIso() } : current);
  };

  const create = () => {
    setGuideOpen(true);
    setDraft(undefined);
    setSimulation(undefined);
    setCreationStatus('');
  };

  const installScenario = async (scenario: Scenario) => {
    setBusy(true); setError(''); setCreationStatus('');
    try {
      // Templates and guided scenarios use the same persisted schema as the editor.
      await pulsarPort.save(scenario);
      setGuideOpen(false);
      setDraft(scenario);
      setSelectedId(scenario.id);
      setCreationStatus(`« ${scenario.name} » est enregistré et désactivé. Vérifiez-le avant de l’activer.`);
      await refresh(scenario.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Impossible d’enregistrer cette automatisation.');
    } finally { setBusy(false); }
  };

  const save = async () => {
    if (!draft) return;
    const validationError = validateDraft(draft);
    if (validationError) { setError(validationError); return; }
    setBusy(true); setError('');
    try {
      await pulsarPort.save({ ...draft, updatedAt: nowIso() });
      setSaved(true);
      await refresh(draft.id);
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Impossible d’enregistrer le scénario.'); }
    finally { setBusy(false); }
  };

  const remove = async () => {
    if (!draft || !window.confirm(`Supprimer « ${draft.name} » et son journal d’exécution ? Cette action est définitive.`)) return;
    setBusy(true); setError('');
    try {
      await pulsarPort.remove(draft.id);
      setDraft(undefined); setScenarios(current => current.filter(item => item.id !== draft.id));
      setSelectedId(undefined); setSimulation(undefined); await refresh();
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Impossible de supprimer ce scénario.'); }
    finally { setBusy(false); }
  };

  const toggleEnabled = async (scenario: Scenario, enabled: boolean) => {
    if (!enabled && !scenario.enabled) return;
    let allowScheduledFileChanges = scenario.allowScheduledFileChanges;
    if (enabled && writesFiles(scenario.actions)) {
      if (!window.confirm(`Autoriser « ${scenario.name} » à copier, déplacer ou renommer des fichiers sans demander à chaque déclenchement ? Les destinations existantes ne seront jamais remplacées.`)) return;
      allowScheduledFileChanges = true;
    }
    setBusy(true); setError('');
    try {
      const updated = { ...(draft?.id === scenario.id ? draft : scenario), enabled, allowScheduledFileChanges, updatedAt: nowIso() };
      await pulsarPort.save(updated);
      setDraft(updated);
      await pulsarPort.setEnabled(scenario.id, enabled);
      await refresh(scenario.id);
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Impossible de modifier l’état du scénario.'); }
    finally { setBusy(false); }
  };

  const simulate = async () => {
    if (!draft) return;
    setBusy(true); setError('');
    try {
      await pulsarPort.save(draft);
      setSimulation(await pulsarPort.simulate(draft.id));
      await refresh(draft.id);
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'La simulation a échoué.'); }
    finally { setBusy(false); }
  };

  const setGlobalPaused = async (paused: boolean) => {
    setBusy(true); setError('');
    try {
      setGloballyPaused(await pulsarPort.setGlobalPause(paused));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Impossible de modifier la suspension de Pulsar.');
    } finally { setBusy(false); }
  };

  const runNow = async () => {
    if (!draft) return;
    const validationError = validateDraft(draft);
    if (validationError) { setError(validationError); return; }
    if (!window.confirm(`Exécuter « ${draft.name} » maintenant ? Les actions définies seront lancées sur cet appareil.`)) return;
    setBusy(true); setError('');
    try { await pulsarPort.save(draft); await pulsarPort.run(draft.id, true); await refresh(draft.id); }
    catch (reason) { setError(reason instanceof Error ? reason.message : 'L’exécution a échoué.'); }
    finally { setBusy(false); }
  };

  const triggerDetails = useMemo(() => triggersSummary(draft?.triggers ?? [{ kind: 'manual' }]), [draft?.triggers]);

  if (loading) return <section className="orbit-module-page pulsar-page" aria-labelledby="pulsar-title"><header className="orbit-module-heading pulsar-heading"><div><h1 id="pulsar-title">Pulsar</h1><p className="page-subtitle">Chargement des automatisations locales.</p></div></header><div className="orbit-module-content pulsar-module-content"><p role="status">Chargement de Pulsar…</p></div></section>;

  return <section className="orbit-module-page pulsar-page" aria-labelledby="pulsar-title">
    <header className="orbit-module-heading pulsar-heading">
      <div><p className="eyebrow">PROJECT NEBULA <span>·</span> AUTOMATISATION LOCALE</p><h1 id="pulsar-title">Pulsar</h1><p className="page-subtitle">Définissez des déclencheurs, des conditions et des actions exécutés sur cet appareil.</p></div>
      <div className="pulsar-heading-actions">
        <div className="pulsar-overview" aria-label={`${enabledCount} scénarios actifs sur ${scenarios.length}`}><Orbit size={17} /><strong>{enabledCount.toString().padStart(2, '0')}</strong><span>scénarios actifs</span></div>
        <button type="button" className={`pulsar-button ${globallyPaused ? 'resume' : 'pause'}`} aria-pressed={globallyPaused} disabled={!pulsarPort.available || busy} onClick={() => void setGlobalPaused(!globallyPaused)}>
          {globallyPaused ? <Play size={15} /> : <Pause size={15} />}{globallyPaused ? 'Reprendre Pulsar' : 'Tout suspendre'}
        </button>
      </div>
    </header>

    <div className="orbit-module-content pulsar-module-content">

    {!pulsarPort.available && <div className="pulsar-banner" role="status"><CircleAlert size={17} /><span>{pulsarPort.unavailableReason}</span></div>}
    {globallyPaused && <div className="pulsar-paused-banner" role="status"><Pause size={16} /><span><strong>Automatisations suspendues.</strong> Les déclencheurs manqués ne seront pas rejoués. Une action déjà en cours finit son étape.</span><button type="button" onClick={() => void setGlobalPaused(false)} disabled={busy || !pulsarPort.available}>Reprendre</button></div>}
    {error && <div className="pulsar-error" role="alert"><CircleAlert size={16} /><span>{error}</span><button type="button" onClick={() => setError('')}>Fermer</button></div>}

    <div className="pulsar-tabs" role="group" aria-label="Sections Pulsar">
      <button type="button" aria-pressed={tab === 'scenarios'} onClick={() => setTab('scenarios')}>Scénarios <span>{scenarios.length}</span></button>
      <button type="button" aria-pressed={tab === 'history'} onClick={() => setTab('history')}>Historique <span>{logs.length}</span></button>
      <span className={`pulsar-local-state${pulsarPort.available ? ' available' : ''}`}><span />{pulsarPort.available ? 'Moteur local' : 'Aperçu limité'}</span>
    </div>
    {creationStatus && <p className="pulsar-created-status" role="status">{creationStatus}</p>}

    {tab === 'history' ? <History logs={logs} onRefresh={() => void refresh()} /> : <div className="pulsar-workspace">
      <aside className="pulsar-list" aria-label="Scénarios">
        <div className="pulsar-list-heading"><div><h2>Automatisations</h2><span>{scenarios.length} scénario{scenarios.length === 1 ? '' : 's'}</span></div><button type="button" onClick={create} disabled={!pulsarPort.available} aria-label="Créer une automatisation" title="Créer une automatisation"><Plus size={17} /></button></div>
        {scenarios.length ? <div className="scenario-items">{scenarios.map(scenario => <button key={scenario.id} type="button" className={`scenario-item${scenario.id === selectedId ? ' selected' : ''}`} onClick={() => setSelectedId(scenario.id)} aria-current={scenario.id === selectedId ? 'true' : undefined}>
          <span className={`scenario-status${scenario.enabled ? ' enabled' : ''}`} aria-hidden="true" /><span className="scenario-item-copy"><strong>{scenario.name}</strong><span>{triggersSummary(scenario.triggers)}</span><small>{scenario.actions.length} action{scenario.actions.length === 1 ? '' : 's'} · {scenario.lastRunAt ? `Dernier lancement ${shortDate(scenario.lastRunAt)}` : 'Jamais exécuté'}</small></span><ChevronRight size={15} />
        </button>)}</div> : <div className="pulsar-list-empty"><strong>Aucun scénario</strong><p>Créez une automatisation locale.</p><button type="button" className="pulsar-text-action" onClick={create} disabled={!pulsarPort.available}>Créer un scénario</button></div>}
        <div className="pulsar-list-foot"><span><span className="pulsar-dot active" />Actif</span><span><span className="pulsar-dot" />Désactivé</span></div>
      </aside>

      <div className="pulsar-editor">
        {!draft ? <TemplatePicker
          guideOpen={guideOpen}
          onStartGuide={() => { setGuideOpen(true); setCreationStatus(''); }}
          onCancelGuide={() => setGuideOpen(false)}
          onChoose={scenario => void installScenario(scenario)}
          onCompleteGuide={scenario => void installScenario(scenario)}
          disabled={!pulsarPort.available || busy}
        /> : <>
          <div className="editor-topline"><div><span>ÉDITEUR DE SCÉNARIO</span><strong>{draft.enabled ? 'Activé' : 'Désactivé'}</strong></div><div className="editor-actions">
            <button type="button" className="pulsar-icon-action" onClick={() => void remove()} disabled={busy || !pulsarPort.available} aria-label="Supprimer le scénario" title="Supprimer le scénario"><Trash2 size={16} /></button>
            <button type="button" className="pulsar-button quiet" onClick={() => void simulate()} disabled={busy || !pulsarPort.available}><Activity size={15} />Simuler</button>
            <button type="button" className="pulsar-button quiet" onClick={() => void runNow()} disabled={busy || !pulsarPort.available}><Zap size={15} />Exécuter</button>
            <button type="button" className="pulsar-button primary" onClick={() => void save()} disabled={!canSave || busy}><Check size={15} />{saved ? 'Enregistré' : 'Enregistrer'}</button>
          </div></div>
          <p className="execution-help"><strong>Simuler</strong> vérifie le scénario sans lancer ses actions. <strong>Exécuter</strong> lance les actions après confirmation. Un scénario activé attend son déclencheur.</p>

          <div className="scenario-properties">
            <label>Nom du scénario<input value={draft.name} maxLength={64} onChange={event => updateDraft({ name: event.target.value })} placeholder="Ex. Rappel de pause" disabled={!pulsarPort.available} /></label>
            <label>Description<input value={draft.description} maxLength={180} onChange={event => updateDraft({ description: event.target.value })} placeholder="Décrivez ce que fait ce scénario" disabled={!pulsarPort.available} /></label>
          </div>

          <div className="flow-heading"><h2>Flux d’exécution</h2><p>Un déclencheur, des conditions facultatives, puis les actions.</p></div>
          <TriggersEditor triggers={draft.triggers} onChange={triggers => updateDraft({ triggers, trigger: triggers[0] })} />
          <div className="flow-connector" aria-hidden="true"><span /></div>
          <ConditionsEditor conditions={draft.conditions} mode={draft.conditionMode ?? 'all'} onChange={(conditions, conditionMode) => updateDraft({ conditions, conditionMode })} />
          <div className="flow-connector" aria-hidden="true"><span /></div>
          <ActionsEditor actions={draft.actions} onChange={actions => updateDraft({ actions })} />

          <div className="scenario-footer"><label className="pulsar-switch"><input type="checkbox" checked={draft.enabled} onChange={event => void toggleEnabled(draft, event.target.checked)} disabled={busy || !pulsarPort.available} /><span className="switch-track" /><span><strong>Activer le scénario</strong><small>{draft.enabled ? 'Les déclencheurs s’exécutent en arrière-plan.' : 'Le scénario reste désactivé.'}</small></span></label>
            {draft.lastRunStatus && <span className={`last-run ${draft.lastRunStatus}`}>{statusLabel(draft.lastRunStatus)} · {draft.lastRunAt ? shortDate(draft.lastRunAt) : ''}</span>}
          </div>
          {simulation && <SimulationPanel result={simulation} onClose={() => setSimulation(undefined)} />}
          <p className="privacy-note"><span className="pulsar-dot active" />Les scénarios et leurs journaux sont conservés localement. Aucun terminal ni script arbitraire n’est exécuté.</p>
        </>}
      </div>
    </div>}
    <div className="pulsar-statusbar"><span><HistoryIcon size={13} />{triggerDetails}</span><span>{pulsarPort.available ? 'Les actions restent sur cet appareil.' : 'Activez Pulsar dans l’application Windows native.'}</span></div>
    </div>
  </section>;
}

function TriggersEditor({ triggers, onChange }: { triggers: Trigger[]; onChange: (triggers: Trigger[]) => void }) {
  const makeTrigger = (kind: Trigger['kind']): Trigger => {
    switch (kind) {
      case 'manual': return { kind };
      case 'startup': return { kind };
      case 'interval': return { kind, everySeconds: 300 };
      case 'at-time': return { kind, at: new Date(Date.now() + 3_600_000).toISOString() };
      case 'file-created': return { kind, directory: '' };
      case 'file-modified': return { kind, path: '' };
      case 'event': return { kind, name: 'core.alert' };
      case 'process-started': return { kind, executable: '' };
    }
  };
  const add = (kind: Trigger['kind']) => onChange([...triggers, makeTrigger(kind)]);
  const update = (index: number, trigger: Trigger) => onChange(triggers.map((item, position) => position === index ? trigger : item));
  return <section className="flow-node trigger-node" aria-labelledby="trigger-title">
    <div className="flow-node-head"><span className="flow-node-number">01</span><div><h3 id="trigger-title">Déclencheurs</h3><p>Un seul suffit pour lancer le scénario (OU).</p></div><select aria-label="Ajouter un déclencheur" value="" onChange={event => event.target.value && add(event.target.value as Trigger['kind'])} disabled={!pulsarPort.available || triggers.length >= 12}><option value="">Ajouter…</option><option value="manual">Manuel</option><option value="startup">Démarrage d’ORBIT</option><option value="interval">Intervalle programmé</option><option value="at-time">À une date et heure</option><option value="file-created">Nouveau fichier dans un dossier</option><option value="file-modified">Modification d’un fichier</option><option value="event">Événement ORBIT</option><option value="process-started">Quand je lance une application</option></select></div>
    <div className="trigger-list">{triggers.map((trigger, index) => <div className="trigger-instance" key={`${trigger.kind}-${index}`}>
      <div className="trigger-instance-head"><strong>Déclencheur {index + 1}</strong><span>{triggerSummary(trigger)}</span><button type="button" onClick={() => onChange(triggers.filter((_, position) => position !== index))} aria-label={`Supprimer le déclencheur ${index + 1}`} disabled={!pulsarPort.available || triggers.length <= 1}>×</button></div>
      <label className="field-label">Type<select value={trigger.kind} onChange={event => update(index, makeTrigger(event.target.value as Trigger['kind']))} disabled={!pulsarPort.available}>
        <option value="manual">Manuel</option><option value="startup">Démarrage d’ORBIT</option><option value="interval">Intervalle programmé</option><option value="at-time">À une date et heure</option><option value="file-created">Nouveau fichier dans un dossier</option><option value="file-modified">Modification d’un fichier</option><option value="event">Événement ORBIT</option><option value="process-started">Quand je lance une application</option>
      </select></label>
      {trigger.kind === 'interval' && <label className="field-label">Répéter toutes les<input type="number" min={30} max={86400} step={30} value={trigger.everySeconds} onChange={event => update(index, { ...trigger, everySeconds: Number(event.target.value) })} disabled={!pulsarPort.available} /><small>Secondes · minimum 30 · maximum 24 heures</small></label>}
      {trigger.kind === 'at-time' && <label className="field-label">Date et heure<input type="datetime-local" value={localDateTime(trigger.at)} onChange={event => { if (event.target.value) update(index, { ...trigger, at: new Date(event.target.value).toISOString() }); }} disabled={!pulsarPort.available} /><small>Choisissez une heure future. ORBIT doit rester ouvert à ce moment-là.</small></label>}
      {trigger.kind === 'file-created' && <PathField label="Dossier surveillé" value={trigger.directory} onChange={directory => update(index, { ...trigger, directory })} onPick={() => pulsarPort.pickFolder()} buttonLabel="Choisir un dossier" />}
      {trigger.kind === 'file-modified' && <PathField label="Fichier surveillé" value={trigger.path} onChange={path => update(index, { ...trigger, path })} onPick={() => pulsarPort.pickFile()} buttonLabel="Choisir un fichier" />}
      {trigger.kind === 'event' && <label className="field-label">Événement<select value={trigger.name} onChange={event => update(index, { ...trigger, name: event.target.value as typeof trigger.name })} disabled={!pulsarPort.available}>{EVENT_TRIGGERS.map(item => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>}
      {trigger.kind === 'process-started' && <><PathField label="Fichier exécutable (.exe)" value={trigger.executable} onChange={executable => update(index, { ...trigger, executable })} onPick={() => pulsarPort.pickExecutable()} buttonLabel="Choisir une application" /><p className="trigger-note">Pulsar surveille le démarrage de ce programme; il ne le lance pas. ORBIT doit rester ouvert.</p></>}
    </div>)}</div>
  </section>;
}

function ConditionsEditor({ conditions, mode, onChange }: { conditions: Condition[]; mode: 'all' | 'any'; onChange: (conditions: Condition[], mode: 'all' | 'any') => void }) {
  const add = (kind: Condition['kind']) => {
    const next: Condition = kind === 'extension' ? { kind, value: '.pdf' } : kind === 'name-contains' ? { kind, value: '' } : kind === 'size-at-least' ? { kind, bytes: 1_000_000 } : kind === 'path-exists' ? { kind, path: '' } : { kind, from: '09:00', to: '17:00' };
    onChange([...conditions, next], mode);
  };
  const updateAt = (index: number, condition: Condition) => onChange(conditions.map((item, position) => position === index ? condition : item), mode);
  const removeAt = (index: number) => onChange(conditions.filter((_, position) => position !== index), mode);
  return <section className="flow-node condition-node" aria-labelledby="condition-title">
    <div className="flow-node-head"><span className="flow-node-number">02</span><div><h3 id="condition-title">Conditions</h3><p>{conditions.length ? mode === 'all' ? 'Toutes doivent être vraies (ET).' : 'Au moins une doit être vraie (OU).' : 'Facultatives · aucune condition requise'}</p></div><div className="condition-head-actions">{conditions.length > 1 && <label className="condition-logic">Continuer si<select aria-label="Logique entre les conditions" value={mode} onChange={event => onChange(conditions, event.target.value as 'all' | 'any')} disabled={!pulsarPort.available}><option value="all">Toutes sont vraies (ET)</option><option value="any">Au moins une est vraie (OU)</option></select></label>}<select aria-label="Ajouter une condition" value="" onChange={event => event.target.value && add(event.target.value as Condition['kind'])} disabled={!pulsarPort.available || conditions.length >= 12}><option value="">Ajouter…</option><option value="extension">Extension</option><option value="name-contains">Nom contient</option><option value="size-at-least">Taille minimale</option><option value="path-exists">Le chemin existe</option><option value="time-window">Plage horaire</option></select></div></div>
    {conditions.length === 0 ? <p className="node-empty">Sans condition, chaque déclenchement lance les actions.</p> : <div className="condition-list">{conditions.map((condition, index) => <div className="condition-row" key={`${condition.kind}-${index}`}>
      {condition.kind === 'extension' && <><span>Extension</span><input aria-label="Extension de fichier" value={condition.value} onChange={event => updateAt(index, { ...condition, value: event.target.value })} placeholder=".pdf" disabled={!pulsarPort.available} /></>}
      {condition.kind === 'name-contains' && <><span>Nom contient</span><input aria-label="Texte du nom de fichier" value={condition.value} onChange={event => updateAt(index, { ...condition, value: event.target.value })} placeholder="rapport" disabled={!pulsarPort.available} /></>}
      {condition.kind === 'size-at-least' && <><span>Taille d’au moins</span><input type="number" min={0} aria-label="Taille minimale en octets" value={condition.bytes} onChange={event => updateAt(index, { ...condition, bytes: Number(event.target.value) })} /><span>octets</span></>}
      {condition.kind === 'path-exists' && <><span>Le chemin existe</span><input aria-label="Chemin à vérifier" value={condition.path} onChange={event => updateAt(index, { ...condition, path: event.target.value })} placeholder="C:\\Dossier\\fichier.txt" disabled={!pulsarPort.available} /></>}
      {condition.kind === 'time-window' && <><span>Entre</span><input type="time" aria-label="Heure de début" value={condition.from} onChange={event => updateAt(index, { ...condition, from: event.target.value })} /><span>et</span><input type="time" aria-label="Heure de fin" value={condition.to} onChange={event => updateAt(index, { ...condition, to: event.target.value })} /></>}
      <button type="button" className="remove-condition" onClick={() => removeAt(index)} aria-label={`Supprimer la condition ${index + 1}`} disabled={!pulsarPort.available}>×</button>
    </div>)}</div>}
  </section>;
}

function ActionsEditor({ actions, onChange }: { actions: Action[]; onChange: (actions: Action[]) => void }) {
  const add = (kind: Action['kind']) => {
    const action: Action = kind === 'notify' ? { kind, title: 'ORBIT', body: 'Une automatisation a été exécutée.' }
      : kind === 'open-folder' ? { kind, path: '' } : kind === 'open-app' ? { kind, app: 'file-explorer' }
      : kind === 'copy-file' ? { kind, source: '', destination: '' } : kind === 'move-file' ? { kind, source: '', destination: '' }
      : kind === 'rename-file' ? { kind, path: '', newName: '' } : { kind, message: 'Scénario exécuté.' };
    onChange([...actions, action]);
  };
  const updateAt = (index: number, action: Action) => onChange(actions.map((item, position) => position === index ? action : item));
  return <section className="flow-node action-node" aria-labelledby="actions-title">
    <div className="flow-node-head"><span className="flow-node-number">03</span><div><h3 id="actions-title">Actions</h3><p>Exécutées dans l’ordre ci-dessous · {actions.length}/12</p></div><select aria-label="Ajouter une action" value="" onChange={event => event.target.value && add(event.target.value as Action['kind'])} disabled={!pulsarPort.available || actions.length >= 12}><option value="">{actions.length >= 12 ? 'Limite atteinte' : 'Ajouter…'}</option><option value="notify">Notification Windows</option><option value="open-folder">Ouvrir un dossier</option><option value="open-app">Ouvrir une application ORBIT</option><option value="copy-file">Copier un fichier</option><option value="move-file">Déplacer un fichier</option><option value="rename-file">Renommer un fichier</option><option value="log">Ajouter au journal</option></select></div>
    {actions.length === 0 ? <p className="node-empty">Ajoutez au moins une action pour enregistrer ce scénario.</p> : <div className="action-list">{actions.map((action, index) => <div className="action-config" key={`${action.kind}-${index}`}>
      <div className="action-config-head"><span>{String(index + 1).padStart(2, '0')}</span><strong>{actionSummary(action)}</strong><button type="button" onClick={() => onChange(actions.filter((_, position) => position !== index))} aria-label="Supprimer l’action" disabled={!pulsarPort.available}>×</button></div>
      {action.kind === 'notify' && <div className="action-fields"><label>Titre<input value={action.title} maxLength={80} onChange={event => updateAt(index, { ...action, title: event.target.value })} disabled={!pulsarPort.available} /></label><label>Message<input value={action.body} maxLength={180} onChange={event => updateAt(index, { ...action, body: event.target.value })} disabled={!pulsarPort.available} /></label></div>}
      {action.kind === 'open-folder' && <PathField label="Dossier à ouvrir" value={action.path} onChange={path => updateAt(index, { ...action, path })} onPick={() => pulsarPort.pickFolder()} buttonLabel="Choisir un dossier" />}
      {action.kind === 'open-app' && <label className="field-label">Application autorisée<select value={action.app} onChange={event => updateAt(index, { ...action, app: event.target.value as typeof action.app })} disabled={!pulsarPort.available}><option value="file-explorer">Explorateur de fichiers</option><option value="notepad">Bloc-notes</option><option value="calculator">Calculatrice</option></select></label>}
      {(action.kind === 'copy-file' || action.kind === 'move-file') && <div className="action-fields"><PathField label="Fichier source" value={action.source} onChange={source => updateAt(index, { ...action, source })} onPick={() => pulsarPort.pickFile()} buttonLabel="Choisir un fichier" /><PathField label="Dossier de destination" value={action.destination} onChange={destination => updateAt(index, { ...action, destination })} onPick={() => pulsarPort.pickFolder()} buttonLabel="Choisir un dossier" /></div>}
      {action.kind === 'rename-file' && <div className="action-fields"><PathField label="Fichier" value={action.path} onChange={path => updateAt(index, { ...action, path })} onPick={() => pulsarPort.pickFile()} buttonLabel="Choisir un fichier" /><label>Nouveau nom<input value={action.newName} maxLength={180} onChange={event => updateAt(index, { ...action, newName: event.target.value })} placeholder="nouveau-nom.ext" disabled={!pulsarPort.available} /></label></div>}
      {action.kind === 'log' && <label className="field-label">Message interne<input value={action.message} maxLength={180} onChange={event => updateAt(index, { ...action, message: event.target.value })} disabled={!pulsarPort.available} /><small>N’incluez pas de contenu du presse-papiers ni de secret dans les journaux.</small></label>}
    </div>)}</div>}
  </section>;
}

function PathField({ label, value, onChange, onPick, buttonLabel }: { label: string; value: string; onChange: (value: string) => void; onPick: () => Promise<string | null>; buttonLabel: string }) {
  const inputId = useId();
  const [error, setError] = useState('');
  const pick = async () => { setError(''); try { const path = await onPick(); if (path) onChange(path); } catch (reason) { setError(reason instanceof Error ? reason.message : 'Le sélecteur de fichiers est indisponible.'); } };
  return <div className="field-label"><label htmlFor={inputId}>{label}</label><div className="path-field"><input id={inputId} value={value} onChange={event => onChange(event.target.value)} placeholder="Ex. C:\\Dossier\\fichier" disabled={!pulsarPort.available} /><button type="button" onClick={() => void pick()} disabled={!pulsarPort.available}><Database size={14} />{buttonLabel}</button></div>{error && <small role="alert" className="field-error">{error}</small>}</div>;
}

function SimulationPanel({ result, onClose }: { result: SimulationResult; onClose: () => void }) {
  return <section className={`simulation-panel${result.eligible ? ' eligible' : ''}`} aria-labelledby="simulation-title" role="status">
    <div className="simulation-heading"><div><strong id="simulation-title">Résultat de simulation</strong><span>{result.eligible ? 'Conditions réunies' : 'Aucune action ne serait lancée'}</span></div><button type="button" onClick={onClose} aria-label="Fermer le résultat">×</button></div>
    <p>{result.message}</p>
    {result.conditions.length > 1 && <p className="simulation-logic">Logique : {result.conditionMode === 'all' ? 'toutes les conditions doivent être vraies (ET).' : 'au moins une condition doit être vraie (OU).'}</p>}
    {result.conditions.map((condition, index) => <div className="simulation-condition" key={`${condition.label}-${index}`}><span className={condition.passed ? 'passed' : 'failed'}>{condition.passed ? '✓' : '–'}</span><span>{condition.label}</span><small>{condition.detail}</small></div>)}
    <ul>{result.actions.map((action, index) => <li key={`${action}-${index}`}>{action}</li>)}</ul>
    <small className="simulation-footnote">La simulation n’a ouvert aucun fichier, envoyé aucune notification et modifié aucune donnée.</small>
  </section>;
}

function TemplatePicker({ guideOpen, onStartGuide, onCancelGuide, onChoose, onCompleteGuide, disabled }: { guideOpen: boolean; onStartGuide: () => void; onCancelGuide: () => void; onChoose: (scenario: Scenario) => void; onCompleteGuide: (scenario: Scenario) => void; disabled: boolean }) {
  const [templateError, setTemplateError] = useState('');
  const templates: Array<{ title: string; description: string; build: () => Scenario }> = [
    { title: 'Rappel programmé', description: 'Affiche une notification à un moment choisi.', build: () => ({ ...newScenario(), name: 'Rappel programmé', description: 'Un rappel ponctuel avec notification Windows.', triggers: [{ kind: 'at-time', at: new Date(Date.now() + 86_400_000).toISOString() }], actions: [{ kind: 'notify', title: 'Rappel ORBIT', body: 'Votre rappel programmé est arrivé.' }] }) },
    { title: 'Surveiller un dossier', description: 'Réagit à l’apparition d’un nouveau fichier.', build: () => ({ ...newScenario(), name: 'Surveiller un dossier', description: 'Notification locale lorsqu’un fichier apparaît.', triggers: [{ kind: 'file-created', directory: '' }], actions: [{ kind: 'notify', title: 'Nouveau fichier', body: 'Un fichier a été ajouté au dossier surveillé.' }] }) },
    { title: 'Au démarrage d’ORBIT', description: 'Lance une action simple au démarrage du logiciel.', build: () => ({ ...newScenario(), name: 'Action au démarrage', description: 'Journalise le démarrage de la session ORBIT.', triggers: [{ kind: 'startup' }], actions: [{ kind: 'log', message: 'Session ORBIT démarrée.' }] }) },
  ];
  const chooseTemplate = async (build: () => Scenario) => {
    setTemplateError('');
    const scenario = build();
    if (scenario.triggers.some(trigger => trigger.kind === 'file-created')) {
      try {
        const directory = await pulsarPort.pickFolder();
        if (!directory) return;
        scenario.triggers = scenario.triggers.map(trigger => trigger.kind === 'file-created' ? { kind: 'file-created', directory } : trigger);
        scenario.trigger = scenario.triggers[0];
      } catch (reason) {
        setTemplateError(reason instanceof Error ? reason.message : 'Impossible de choisir le dossier à surveiller.');
        return;
      }
    }
    onChoose(scenario);
  };
  return <div className="pulsar-first-run">
    <div className="first-run-mark"><Orbit size={22} /></div>
    <h2>{guideOpen ? 'Votre première automatisation' : 'Que souhaitez-vous automatiser ?'}</h2>
    <p>{guideOpen ? 'Choisissez un déclencheur, une condition facultative et une action. Vous pourrez tout modifier avant l’activation.' : 'Commencez avec le guide pas à pas ou installez un modèle déjà préparé.'}</p>
    {!guideOpen ? <>
      <div className="guide-intro" aria-label="Les trois étapes de Pulsar">
        <span><b>1</b><strong>Déclencheur</strong><small>Quand agir ?</small></span>
        <span><b>2</b><strong>Condition</strong><small>Facultative : quand continuer ?</small></span>
        <span><b>3</b><strong>Action</strong><small>Que faire sur ce PC ?</small></span>
      </div>
      <button type="button" className="pulsar-button primary guide-start" disabled={disabled} onClick={onStartGuide}><Plus size={15} />Créer avec le guide</button>
      <h3 className="templates-title">Ou installer un modèle</h3>
      <div className="template-grid">{templates.map(template => <button type="button" key={template.title} className="template-card" disabled={disabled} onClick={() => void chooseTemplate(template.build)}><span>{template.title}</span><small>{template.description}</small><ChevronRight size={15} /></button>)}</div>
      {templateError && <p role="alert" className="field-error guide-error">{templateError}</p>}
      <p className="guide-prerequisites">Pré requis : ORBIT doit rester ouvert pour les horaires et la surveillance. Choisissez uniquement des dossiers auxquels votre compte Windows a accès. Les modèles sont enregistrés désactivés.</p>
    </> : <GuidedScenarioWizard disabled={disabled} onCancel={onCancelGuide} onComplete={onCompleteGuide} />}
  </div>;
}

function GuidedScenarioWizard({ disabled, onCancel, onComplete }: { disabled: boolean; onCancel: () => void; onComplete: (scenario: Scenario) => void }) {
  const [step, setStep] = useState(1);
  const [name, setName] = useState('Rappel de pause');
  const [triggerKind, setTriggerKind] = useState<'manual' | 'startup' | 'interval' | 'at-time' | 'file-created'>('interval');
  const [interval, setInterval] = useState(1800);
  const [at, setAt] = useState(localDateTime(new Date(Date.now() + 86_400_000).toISOString()));
  const [directory, setDirectory] = useState('');
  const [conditionKind, setConditionKind] = useState<'none' | 'extension' | 'name-contains'>('none');
  const [conditionValue, setConditionValue] = useState('.pdf');
  const [actionKind, setActionKind] = useState<'notify' | 'log' | 'open-folder'>('notify');
  const [folder, setFolder] = useState('');
  const [fieldError, setFieldError] = useState('');
  const conditions: Condition[] = conditionKind === 'none' ? [] : conditionKind === 'extension' ? [{ kind: 'extension', value: conditionValue }] : [{ kind: 'name-contains', value: conditionValue }];
  const canContinue = step === 1 ? Boolean(name.trim() && (triggerKind !== 'file-created' || directory.trim()) && (triggerKind !== 'at-time' || Number.isFinite(new Date(at).getTime())) && (triggerKind !== 'interval' || Number.isInteger(interval) && interval >= 30 && interval <= 86400)) : step === 2 ? conditionKind === 'none' || Boolean(conditionValue.trim()) : actionKind !== 'open-folder' || Boolean(folder.trim());
  const makeScenario = (): Scenario => {
    const timestamp = nowIso();
    const trigger: Trigger = triggerKind === 'manual' ? { kind: 'manual' }
      : triggerKind === 'startup' ? { kind: 'startup' }
      : triggerKind === 'interval' ? { kind: 'interval', everySeconds: interval }
      : triggerKind === 'at-time' ? { kind: 'at-time', at: new Date(at).toISOString() }
      : { kind: 'file-created', directory };
    const action: Action = actionKind === 'notify' ? { kind: 'notify', title: 'Rappel ORBIT', body: 'Votre automatisation est déclenchée.' }
      : actionKind === 'log' ? { kind: 'log', message: `Automatisation « ${name.trim()} » déclenchée.` }
      : { kind: 'open-folder', path: folder };
    return { ...newScenario(), name: name.trim(), description: 'Créée avec le guide Pulsar.', enabled: false, triggers: [trigger], trigger, conditions, actions: [action], createdAt: timestamp, updatedAt: timestamp };
  };
  const continueStep = () => {
    if (!canContinue) { setFieldError(step === 1 ? triggerKind === 'interval' ? 'Saisissez un intervalle entre 30 et 86 400 secondes.' : triggerKind === 'file-created' ? 'Saisissez un nom et choisissez le dossier à surveiller.' : triggerKind === 'at-time' ? 'Choisissez une date et une heure valides.' : 'Saisissez un nom pour cette automatisation.' : step === 2 ? 'Saisissez la valeur de la condition.' : 'Choisissez le dossier à ouvrir.'); return; }
    setFieldError(''); setStep(current => Math.min(3, current + 1));
  };
  return <section className="guided-wizard" aria-labelledby="guide-step-title">
    <div className="guide-progress" aria-label={`Étape ${step} sur 3`}><span>Étape {step} sur 3</span><div>{[1, 2, 3].map(item => <i key={item} className={item <= step ? 'current' : ''} />)}</div></div>
    {step === 1 && <div className="guide-step"><h3 id="guide-step-title">1. Choisissez quand agir</h3><p>Le déclencheur lance le scénario. Il reste local à ORBIT.</p>
      <label className="field-label">Nom de l’automatisation<input value={name} maxLength={64} onChange={event => setName(event.target.value)} /></label>
      <label className="field-label">Déclencheur<select value={triggerKind} onChange={event => setTriggerKind(event.target.value as typeof triggerKind)}><option value="interval">Toutes les 30 minutes</option><option value="at-time">À une date et une heure</option><option value="startup">Au démarrage d’ORBIT</option><option value="file-created">Quand un fichier apparaît</option><option value="manual">Quand je la lance manuellement</option></select></label>
      {triggerKind === 'interval' && <label className="field-label">Répéter toutes les (secondes)<input type="number" min={30} max={86400} step={30} value={interval} onChange={event => setInterval(Number(event.target.value))} /><small>ORBIT doit rester ouvert. Minimum 30 secondes.</small></label>}
      {triggerKind === 'at-time' && <label className="field-label">Date et heure<input type="datetime-local" value={at} onChange={event => setAt(event.target.value)} /><small>Cette date est ponctuelle. ORBIT doit être ouvert au moment choisi.</small></label>}
      {triggerKind === 'file-created' && <PathField label="Dossier à surveiller" value={directory} onChange={setDirectory} onPick={pulsarPort.pickFolder} buttonLabel="Choisir un dossier" />}
    </div>}
    {step === 2 && <div className="guide-step"><h3 id="guide-step-title">2. Ajoutez une condition (facultatif)</h3><p>Si elle est ajoutée, toutes les conditions doivent être vraies pour continuer.</p>
      <label className="field-label">Condition<select value={conditionKind} onChange={event => setConditionKind(event.target.value as typeof conditionKind)}><option value="none">Aucune condition</option><option value="extension">L’extension correspond</option><option value="name-contains">Le nom contient un texte</option></select></label>
      {conditionKind !== 'none' && <label className="field-label">{conditionKind === 'extension' ? 'Extension (par exemple .pdf)' : 'Texte recherché dans le nom'}<input value={conditionValue} onChange={event => setConditionValue(event.target.value)} placeholder={conditionKind === 'extension' ? '.pdf' : 'facture'} /></label>}
      <p className="guide-hint">Sans condition, chaque déclenchement lance l’action choisie.</p>
    </div>}
    {step === 3 && <div className="guide-step"><h3 id="guide-step-title">3. Choisissez l’action</h3><p>L’action sera proposée en aperçu avant d’être activée.</p>
      <label className="field-label">Action<select value={actionKind} onChange={event => setActionKind(event.target.value as typeof actionKind)}><option value="notify">Afficher une notification Windows</option><option value="log">Ajouter un événement au journal ORBIT</option><option value="open-folder">Ouvrir un dossier</option></select></label>
      {actionKind === 'open-folder' && <PathField label="Dossier à ouvrir" value={folder} onChange={setFolder} onPick={pulsarPort.pickFolder} buttonLabel="Choisir un dossier" />}
      <div className="guide-summary"><strong>Résumé</strong><span><b>Quand</b>{triggersSummary(makeScenario().triggers)}</span><span><b>Condition</b>{conditionKind === 'none' ? 'Aucune' : conditionKind === 'extension' ? `Extension ${conditionValue}` : `Nom contenant « ${conditionValue} »`}</span><span><b>Action</b>{actionSummary(makeScenario().actions[0])}</span></div>
      <p className="guide-hint">Le scénario sera enregistré désactivé. Simulez-le, puis activez-le après vérification. Les actions sur fichiers nécessitent une confirmation distincte dans l’éditeur.</p>
    </div>}
    {fieldError && <p role="alert" className="field-error guide-error">{fieldError}</p>}
    <div className="guide-controls"><button type="button" className="pulsar-button quiet" onClick={step === 1 ? onCancel : () => { setFieldError(''); setStep(current => current - 1); }}> {step === 1 ? 'Annuler' : 'Précédent'}</button>{step < 3 ? <button type="button" className="pulsar-button primary" onClick={continueStep}>Continuer <ChevronRight size={14} /></button> : <button type="button" className="pulsar-button primary" disabled={disabled || !canContinue} onClick={() => { if (!canContinue) { setFieldError('Choisissez un dossier à ouvrir.'); return; } onComplete(makeScenario()); }}>Enregistrer le scénario</button>}</div>
  </section>;
}

function History({ logs, onRefresh }: { logs: Awaited<ReturnType<typeof pulsarPort.logs>>; onRefresh: () => void }) {
  return <section className="pulsar-history panel" aria-labelledby="history-title"><div className="panel-heading"><div><h2 id="history-title">Historique d’exécution</h2><p>Événements locaux récents, sans contenu sensible.</p></div><button type="button" className="pulsar-button quiet" onClick={onRefresh}><Activity size={14} />Actualiser</button></div>
    {logs.length ? <div className="history-table-wrap"><table><thead><tr><th>Scénario</th><th>Résultat</th><th>Détail</th><th>Heure</th></tr></thead><tbody>{logs.map(log => <tr key={log.id}><td>{log.scenarioName}</td><td><span className={`history-status ${log.status}`}>{statusLabel(log.status)}</span></td><td>{log.summary}</td><td>{shortDate(log.at)}</td></tr>)}</tbody></table></div> : <div className="history-empty"><HistoryIcon size={17} /><strong>Aucune exécution enregistrée</strong><span>Les simulations et les lancements apparaîtront ici.</span></div>}
  </section>;
}

function validateDraft(scenario: Scenario): string | undefined {
  if (!scenario.name.trim()) return 'Donnez un nom à cette automatisation.';
  if (!scenario.actions.length) return 'Ajoutez au moins une action.';
  if (!scenario.triggers?.length || scenario.triggers.length > 12) return 'Ajoutez entre 1 et 12 déclencheurs.';
  if (new Set(scenario.triggers.map(trigger => JSON.stringify(trigger).toLowerCase())).size !== scenario.triggers.length) return 'Supprimez les déclencheurs en double.';
  for (const trigger of scenario.triggers) {
    if (trigger.kind === 'interval' && (!Number.isInteger(trigger.everySeconds) || trigger.everySeconds < 30 || trigger.everySeconds > 86400)) return 'L’intervalle doit être compris entre 30 secondes et 24 heures.';
    if (trigger.kind === 'at-time' && (!Number.isFinite(new Date(trigger.at).getTime()) || new Date(trigger.at).getTime() <= Date.now())) return 'Choisissez une date et une heure futures.';
    if (trigger.kind === 'file-created' && !trigger.directory.trim()) return 'Choisissez le dossier à surveiller.';
    if (trigger.kind === 'file-modified' && !trigger.path.trim()) return 'Choisissez le fichier à surveiller.';
    if (trigger.kind === 'process-started' && (!trigger.executable.trim() || !trigger.executable.toLowerCase().endsWith('.exe'))) return 'Choisissez un fichier exécutable .exe existant.';
  }
  for (const action of scenario.actions) {
    if (action.kind === 'notify' && (!action.title.trim() || !action.body.trim())) return 'Ajoutez un titre et un message à la notification.';
    if (action.kind === 'open-folder' && !action.path.trim()) return 'Choisissez le dossier à ouvrir.';
    if ((action.kind === 'copy-file' || action.kind === 'move-file') && (!action.source.trim() || !action.destination.trim())) return 'Choisissez un fichier source et un dossier de destination.';
    if (action.kind === 'rename-file' && (!action.path.trim() || !action.newName.trim())) return 'Choisissez le fichier et son nouveau nom.';
    if (action.kind === 'log' && !action.message.trim()) return 'Saisissez un message pour le journal.';
  }
  return undefined;
}

function shortDate(value: string) { return new Intl.DateTimeFormat('fr-FR', { dateStyle: 'short', timeStyle: 'short' }).format(new Date(value)); }
function statusLabel(status: string) { return ({ succeeded: 'Réussi', failed: 'Échec', skipped: 'Ignoré', simulated: 'Simulé', started: 'Démarré', cancelled: 'Arrêté', paused: 'Suspendu', action: 'Action terminée' } as Record<string, string>)[status] ?? status; }
