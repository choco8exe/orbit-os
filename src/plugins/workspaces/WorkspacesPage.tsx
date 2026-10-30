import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { Activity, CircleHelp, FolderOpen, Plus, Trash2, X, Zap } from 'lucide-react';
import { isTauri } from '@tauri-apps/api/core';
import { orbitServices } from '../../shared/services';
import { workspacesService, type Workspace } from './service';
import './workspaces.css';

const PLUGIN = 'com.orbitos.workspaces';

export function WorkspacesPage({ navigate }: { navigate: () => void }) {
  const [groups, setGroups] = useState<Workspace[]>([]);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [manage, setManage] = useState(false);
  const [canLaunch, setCanLaunch] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [helpOpen, setHelpOpen] = useState(false);

  const refresh = async () => setGroups(await workspacesService.list());
  useEffect(() => {
    let active = true;
    void Promise.all([workspacesService.list(), orbitServices.plugins.listStates()]).then(([items, states]) => {
      if (!active) return;
      setGroups(items);
      const plugin = states.find(item => item.pluginId === PLUGIN);
      setManage(Boolean(plugin?.enabled && plugin.grantedPermissions.includes('workspaces.manage')));
      setCanLaunch(Boolean(plugin?.enabled && plugin.grantedPermissions.includes('workspaces.launch')));
    }).catch(cause => { if (active) setError(cause instanceof Error ? cause.message : String(cause)); });
    return () => { active = false; };
  }, []);

  const mutate = async (operation: () => Promise<Workspace[]>) => {
    setBusy(true); setError(''); setNotice('');
    try { setGroups(await operation()); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const createGroup = async (event: FormEvent) => {
    event.preventDefault();
    const draft = name.trim();
    if (!draft) return;
    setBusy(true); setError('');
    try { setGroups(await workspacesService.create(draft)); setName(''); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const addApplication = async (group: Workspace) => {
    setBusy(true); setError('');
    try {
      const path = await workspacesService.chooseExecutable();
      if (path) setGroups(await workspacesService.addApp(group.id, path));
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const launch = async (group: Workspace) => {
    setBusy(true); setError(''); setNotice('');
    try { const result = await workspacesService.launch(group.id); setNotice(`${result.launchedCount} application${result.launchedCount === 1 ? '' : 's'} lancée${result.launchedCount === 1 ? '' : 's'} pour « ${result.workspaceName} ».`); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const totalApps = useMemo(() => groups.reduce((count, group) => count + group.applications.length, 0), [groups]);

  if (!isTauri()) return <section className="page orbit-module-page workspaces-page" aria-labelledby="workspaces-title">
    <header className="workspaces-heading"><div><p className="eyebrow">PLUGIN LOCAL · SESSION LAUNCHER</p><h1 id="workspaces-title">Espaces de travail</h1><p className="page-subtitle">Regroupe tes applications par session et lance-les en un clic.</p></div></header>
    <div className="workspaces-preview panel"><FolderOpen size={27} aria-hidden="true" /><h2>Aperçu indisponible dans le navigateur</h2><p>Cette fonction utilise le sélecteur de fichiers et le lancement natif Windows. Ouvre la version ORBIT installée pour créer tes espaces.</p></div>
  </section>;

  return <section className="page orbit-module-page workspaces-page" aria-labelledby="workspaces-title">
    <header className="workspaces-heading"><div><p className="eyebrow">PLUGIN LOCAL · SESSION LAUNCHER</p><h1 id="workspaces-title">Espaces de travail</h1><p className="page-subtitle">Regroupe tes applications par session et lance-les en un clic.</p></div><button className="secondary-button" type="button" onClick={() => setHelpOpen(value => !value)} aria-expanded={helpOpen} aria-controls="workspaces-help"><CircleHelp size={16} />Comment ça marche</button></header>
    {helpOpen && <aside id="workspaces-help" className="workspaces-help panel"><button type="button" className="icon-button" aria-label="Fermer l’aide" onClick={() => setHelpOpen(false)}><X size={16} /></button><h2>Une session, un groupe d’applications</h2><p>Crée un espace, ajoute des fichiers .exe avec le sélecteur Windows, puis lance les applications ensemble. ORBIT mémorise uniquement les chemins sélectionnés. Le lancement est désactivé jusqu’à l’autorisation « Lancer les applications des espaces » dans le Plugin Center.</p></aside>}
    <div className="workspaces-summary"><div><strong>{groups.length}</strong><span>espaces</span></div><div><strong>{totalApps}</strong><span>applications</span></div><button type="button" className="text-button" onClick={() => void refresh()} disabled={busy}><Activity size={14} />Actualiser</button></div>
    <form className="workspaces-create panel" onSubmit={event => void createGroup(event)}><label htmlFor="workspace-name">Nouvel espace</label><div><input id="workspace-name" value={name} onChange={event => setName(event.target.value)} maxLength={48} placeholder="Ex. Studio, Dev, Gaming" disabled={busy || !manage} /><button className="primary-button" type="submit" disabled={busy || !manage || !name.trim()}><Plus size={16} />Créer</button></div>{!manage && <p className="workspaces-permission">Autorise la gestion des espaces dans le Plugin Center pour commencer.</p>}</form>
    {error && <p className="plugin-error" role="alert">{error}</p>}{notice && <p className="workspaces-notice" role="status">{notice}</p>}
    {groups.length === 0 ? <div className="workspaces-empty panel"><FolderOpen size={24} /><h2>Prêt pour ta première session ?</h2><p>Crée un espace puis ajoute les applications que tu veux ouvrir ensemble.</p></div> : <div className="workspaces-grid">{groups.map(group => <article className="workspaces-card panel" key={group.id}>
      <header><div className="workspace-card-mark"><FolderOpen size={18} aria-hidden="true" /></div><div className="workspace-card-title"><h2>{group.name}</h2><p>{group.applications.length} / 20 applications</p></div><button className="icon-button workspace-delete" type="button" aria-label={`Supprimer l’espace ${group.name}`} title="Supprimer cet espace" disabled={busy || !manage} onClick={() => { if (window.confirm(`Supprimer l’espace « ${group.name} » et sa liste d’applications ?`)) void mutate(() => workspacesService.remove(group.id)); }}><Trash2 size={16} /></button></header>
      {group.applications.length ? <ul className="workspace-app-list">{group.applications.map(application => <li key={application.id}><span className="workspace-app-icon" aria-hidden="true">EXE</span><div><strong>{application.name}</strong><span title={application.path}>{application.path}</span></div><button className="icon-button" type="button" aria-label={`Retirer ${application.name} de ${group.name}`} disabled={busy || !manage} onClick={() => void mutate(() => workspacesService.removeApp(group.id, application.id))}><X size={15} /></button></li>)}</ul> : <p className="workspace-empty-apps">Aucune application. Ajoute des fichiers .exe depuis ton PC.</p>}
      <footer><button className="secondary-button" type="button" onClick={() => void addApplication(group)} disabled={busy || !manage || group.applications.length >= 20}><Plus size={15} />Ajouter une application</button><button className="primary-button" type="button" onClick={() => void launch(group)} disabled={busy || !canLaunch || !group.applications.length} title={!canLaunch ? 'Autorise le lancement dans le Plugin Center.' : undefined}><Zap size={15} />Lancer l’espace</button></footer>
      {!canLaunch && <p className="workspaces-permission">Le lancement nécessite une autorisation distincte dans le Plugin Center.</p>}
    </article>)}</div>}
    <button className="text-button" type="button" onClick={navigate}>← Retour au Plugin Center</button>
  </section>;
}
