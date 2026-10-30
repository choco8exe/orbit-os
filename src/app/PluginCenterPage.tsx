import { useEffect, useMemo, useRef, useState } from 'react';
import { Activity, Check, Clipboard, Command, Download, FolderOpen, Gamepad2, Info, LockKeyhole, Orbit, Power, Search, Settings2, Sparkles, Trash2, X, type LucideIcon } from 'lucide-react';
import type { ModuleId, PluginId, PluginManifest, PluginPermissionId, PluginSecretId, PluginState } from '../shared/contracts';
import { pluginCatalog } from '../shared/plugins';
import { orbitServices } from '../shared/services';
import './plugin-center.css';

type PluginView = 'all' | 'installed' | 'active' | 'available';
type PluginMark = LucideIcon;
const pluginMarks: Partial<Record<PluginId, PluginMark>> = {
  'com.orbitos.fl-studio': Sparkles,
  'com.orbitos.activity-pulse': Activity,
  'com.orbitos.creator-suite': Orbit,
  'com.orbitos.echo-streamdeck': Clipboard,
  'com.orbitos.game-sessions': Gamepad2,
  'com.orbitos.downloads-organizer': FolderOpen,
};
const pluginKinds: Partial<Record<PluginId, string>> = {
  'com.orbitos.fl-studio': 'FL STUDIO',
  'com.orbitos.activity-pulse': 'TEMPS D’ACTIVITÉ',
  'com.orbitos.creator-suite': 'COMPAGNON LOCAL',
  'com.orbitos.echo-streamdeck': 'STREAM DECK · ECHO',
  'com.orbitos.game-sessions': 'SUIVI LOCAL',
  'com.orbitos.downloads-organizer': 'CLASSEMENT LOCAL',
};
const pluginViews: Array<{ id: PluginView; label: string }> = [
  { id: 'all', label: 'Tout' },
  { id: 'installed', label: 'Installés' },
  { id: 'active', label: 'Actifs' },
  { id: 'available', label: 'Disponibles' },
];

export function PluginCenterPage({ native, states, onStatesChanged, navigate }: {
  native: boolean;
  states: PluginState[];
  onStatesChanged: (states: PluginState[]) => void;
  navigate: (id: ModuleId) => void;
}) {
  const [retiredCredentials, setRetiredCredentials] = useState({ obs: false, github: false, youtube: false });
  const [cleanupBusy, setCleanupBusy] = useState(false);
  const [cleanupError, setCleanupError] = useState('');
  const [cleanupMessage, setCleanupMessage] = useState('');
  const [query, setQuery] = useState('');
  const [view, setView] = useState<PluginView>('all');
  const [selectedPluginId, setSelectedPluginId] = useState<PluginId | null>(null);
  const tileRefs = useRef(new Map<PluginId, HTMLButtonElement>());
  useEffect(() => {
    if (!native) return;
    void orbitServices.plugins.retiredCredentialStatus().then(setRetiredCredentials).catch(cause => setCleanupError(cause instanceof Error ? cause.message : String(cause)));
  }, [native]);
  const cleanupRetiredCredentials = async () => {
    setCleanupBusy(true); setCleanupError(''); setCleanupMessage('');
    try {
      await orbitServices.plugins.cleanupRetiredCredentials();
      setRetiredCredentials({ obs: false, github: false, youtube: false });
      setCleanupMessage('Les anciens identifiants d’intégrations retirées ont été supprimés du coffre Windows.');
    } catch (cause) { setCleanupError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setCleanupBusy(false); }
  };
  const searchablePlugins = useMemo(() => pluginCatalog.map(manifest => {
    const state = states.find(item => item.pluginId === manifest.id)
      ?? { pluginId: manifest.id, installed: false, enabled: false, grantedPermissions: [], installedVersion: null };
    const terms = [
      manifest.name,
      manifest.description,
      manifest.connection.summary,
      ...manifest.capabilities,
      ...manifest.demandedPermissions.flatMap(permission => [permission.label, permission.description]),
      ...manifest.contributions.pages.flatMap(page => [page.label, page.description]),
    ].join(' ').toLocaleLowerCase('fr-FR');
    return { manifest, state, terms };
  }), [states]);
  const queryMatches = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase('fr-FR');
    return searchablePlugins.filter(item => !needle || item.terms.includes(needle));
  }, [query, searchablePlugins]);
  const viewCounts: Record<PluginView, number> = {
    all: queryMatches.length,
    installed: queryMatches.filter(item => item.state.installed).length,
    active: queryMatches.filter(item => item.state.installed && item.state.enabled).length,
    available: queryMatches.filter(item => !item.state.installed).length,
  };
  const visiblePlugins = queryMatches.filter(({ state }) => {
    if (view === 'installed') return state.installed;
    if (view === 'active') return state.installed && state.enabled;
    if (view === 'available') return !state.installed;
    return true;
  });
  const selectedPlugin = visiblePlugins.find(item => item.manifest.id === selectedPluginId);
  useEffect(() => {
    if (selectedPluginId && !visiblePlugins.some(item => item.manifest.id === selectedPluginId)) setSelectedPluginId(null);
  }, [selectedPluginId, visiblePlugins]);
  useEffect(() => {
    if (!selectedPluginId) return;
    window.requestAnimationFrame(() => {
      const heading = document.getElementById('plugin-detail-heading');
      if (heading instanceof HTMLElement) heading.focus();
    });
  }, [selectedPluginId]);
  const closeDetails = () => {
    const previous = selectedPluginId;
    setSelectedPluginId(null);
    window.requestAnimationFrame(() => {
      const tile = previous ? tileRefs.current.get(previous) : undefined;
      if (tile?.isConnected) tile.focus();
    });
  };
  return <section className="page orbit-module-page plugin-center-page" aria-labelledby="page-title">
    <div className="page-heading orbit-module-heading"><div><p className="eyebrow">EXTENSIONS LOCALES</p><h1 id="page-title">Plugin Center</h1><p className="page-subtitle">Installe les outils inclus dans ORBIT et choisis leurs autorisations.</p></div></div>
    {!native && <div className="plugin-native-note" role="status"><Info size={17} /><span>Le catalogue est visible en aperçu. L’installation, les permissions et les connexions exigent l’application Windows native.</span></div>}
    {native && (retiredCredentials.obs || retiredCredentials.github || retiredCredentials.youtube || cleanupMessage || cleanupError) && <section className="plugin-native-note" aria-label="Anciens identifiants d’intégrations retirées"><Info size={17} /><span>{retiredCredentials.obs || retiredCredentials.github || retiredCredentials.youtube ? <>Des identifiants d’intégrations retirées sont encore dans le coffre Windows. ORBIT ne les utilise plus. <button type="button" className="text-button danger-action" disabled={cleanupBusy} onClick={() => void cleanupRetiredCredentials()}><Trash2 size={14} />Supprimer ces anciens identifiants</button></> : null}{cleanupError && <small role="alert">{cleanupError}</small>}{cleanupMessage && <small role="status">{cleanupMessage}</small>}</span></section>}
    <div className={`plugin-catalog-layout${selectedPlugin ? ' has-selection' : ''}`}>
    <div className="plugin-catalog-controls">
      <label className="plugin-catalog-search" htmlFor="plugin-catalog-query">
        <Search size={17} aria-hidden="true" />
        <span className="sr-only">Rechercher dans les plugins</span>
        <input id="plugin-catalog-query" type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Nom, fonction ou permission" autoComplete="off" />
        {query && <button type="button" aria-label="Effacer la recherche" onClick={() => setQuery('')}><X size={15} /></button>}
      </label>
      <div className="plugin-catalog-filters" role="group" aria-label="Filtrer les plugins">
        {pluginViews.map(item => <button key={item.id} type="button" className={view === item.id ? 'selected' : ''} aria-pressed={view === item.id} onClick={() => setView(item.id)}>
          <span>{item.label}</span><span className="plugin-filter-count" aria-label={`${viewCounts[item.id]} plugins`}>{viewCounts[item.id]}</span>
        </button>)}
      </div>
      <p className="plugin-catalog-result" aria-live="polite">{visiblePlugins.length} plugin{visiblePlugins.length === 1 ? '' : 's'} · Code local inclus dans ORBIT</p>
    </div>
    {visiblePlugins.length > 0 ? <div className="plugin-catalog-grid" role="group" aria-label="Catalogue des plugins">
      {visiblePlugins.map(({ manifest, state }) => {
        const Mark = pluginMarks[manifest.id] ?? Command;
        const selected = manifest.id === selectedPluginId;
        return <button key={manifest.id} ref={element => { if (element) tileRefs.current.set(manifest.id, element); else tileRefs.current.delete(manifest.id); }} type="button" className={`plugin-tile plugin-tone-${manifest.id.split('.').at(-1)}${selected ? ' selected' : ''}`} aria-controls="plugin-selected-details" aria-expanded={selected} onClick={() => setSelectedPluginId(selected ? null : manifest.id)}>
          <span className="plugin-tile-mark" aria-hidden="true"><Mark size={20} strokeWidth={1.8} /></span>
          <span className="plugin-tile-copy"><span className="plugin-tile-kind">{pluginKinds[manifest.id] ?? 'PLUGIN ORBIT'}</span><strong>{manifest.name}</strong></span>
          <span className={`plugin-state ${state.enabled ? 'enabled' : state.installed ? 'installed' : 'available'}`}><i aria-hidden="true" />{state.enabled ? 'Actif' : state.installed ? 'Installé' : 'Disponible'}</span>
          <span className="plugin-tile-version">v{manifest.version}</span>
        </button>;
      })}
    </div> : <div className="plugin-catalog-empty panel" role="status">
      <h2>{query ? 'Aucun résultat' : view === 'active' ? 'Aucun plugin actif' : view === 'installed' ? 'Aucun plugin installé' : 'Aucun plugin disponible'}</h2>
      <p>{query ? `Aucun plugin ne correspond à « ${query.trim()} » dans ce filtre.` : 'Change le filtre pour voir les autres plugins inclus dans cette version d’ORBIT.'}</p>
      {(query || view !== 'all') && <button type="button" className="secondary-button" onClick={() => { setQuery(''); setView('all'); }}>Afficher tout le catalogue</button>}
    </div>}
    <div id="plugin-selected-details" className="plugin-selected-details" hidden={!selectedPlugin}>
      {selectedPlugin && <PluginDetails key={selectedPlugin.manifest.id} manifest={selectedPlugin.manifest} native={native} states={states} onStatesChanged={onStatesChanged} navigate={navigate} onClose={closeDetails} />}
    </div>
    </div>
  </section>;
}

function PluginDetails({ manifest, native, states, onStatesChanged, navigate, onClose }: {
  manifest: PluginManifest;
  native: boolean;
  states: PluginState[];
  onStatesChanged: (states: PluginState[]) => void;
  navigate: (id: ModuleId) => void;
  onClose: () => void;
}) {
  const state = states.find(item => item.pluginId === manifest.id) ?? { pluginId: manifest.id, installed: false, enabled: false, grantedPermissions: [], installedVersion: null };
  const requiredPermissions = useMemo(() => manifest.demandedPermissions.filter(permission => permission.required).map(permission => permission.id), [manifest]);
  const [checkedRequired, setCheckedRequired] = useState<PluginPermissionId[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [secretPresence, setSecretPresence] = useState<Record<string, boolean>>({});
  const installDialog = useRef<HTMLDialogElement>(null);
  const uninstallDialog = useRef<HTMLDialogElement>(null);
  const allRequired = requiredPermissions.every(id => state.grantedPermissions.includes(id));
  const page = manifest.contributions.pages[0];

  useEffect(() => {
    let live = true;
    if (!native || !state.installed || manifest.connection.secretSlots.length === 0) {
      setSecretPresence({});
      return () => { live = false; };
    }
    void Promise.all(manifest.connection.secretSlots.map(async slot => [slot.id, await orbitServices.plugins.secretStatus(manifest.id, slot.id)] as const))
      .then(entries => { if (live) setSecretPresence(Object.fromEntries(entries)); })
      .catch(cause => { if (live) setError(cause instanceof Error ? cause.message : String(cause)); });
    return () => { live = false; };
  }, [manifest, native, state.installed]);

  const refresh = async () => onStatesChanged(await orbitServices.plugins.listStates());
  const perform = async (operation: () => Promise<void>, success: string) => {
    setBusy(true); setError(''); setMessage('');
    try { await operation(); await refresh(); setMessage(success); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const install = async () => {
    if (!requiredPermissions.every(id => checkedRequired.includes(id))) return;
    await perform(async () => {
      await orbitServices.plugins.install(manifest.id);
      await orbitServices.plugins.setPermissions(manifest.id, checkedRequired);
      await orbitServices.plugins.enable(manifest.id);
      installDialog.current?.close();
    }, `${manifest.name} est installé et activé. Les permissions facultatives restent désactivées.`);
  };
  const togglePermission = async (permission: PluginPermissionId, checked: boolean) => {
    const next = new Set(state.grantedPermissions);
    if (checked) next.add(permission); else next.delete(permission);
    await perform(async () => { await orbitServices.plugins.setPermissions(manifest.id, [...next]); }, checked ? 'Autorisation accordée.' : 'Autorisation retirée.');
  };
  const removeSavedSecret = async (secretId: PluginSecretId) => {
    await perform(async () => {
      await orbitServices.plugins.removeSecret(manifest.id, secretId);
      setSecretPresence(current => ({ ...current, [secretId]: false }));
    }, 'Secret supprimé du coffre Windows.');
  };

  const trust = manifest.trust === 'first-party-prepackaged'
    ? 'Code ORBIT inclus dans cette version. Aucun code n’est téléchargé ou chargé depuis le catalogue.'
    : 'Cette contribution est empaquetée avec ORBIT.';
  const connectionLabel = manifest.connection.mode === 'local' ? 'Local · aucun compte' : manifest.connection.mode === 'companion' ? 'Application locale associée' : manifest.connection.mode === 'oauth' ? 'Connexion de compte' : 'Identifiant à configurer';

  return <article className="plugin-card panel" aria-labelledby="plugin-detail-heading">
    <div className="plugin-card-top"><div className={`plugin-icon plugin-tone-${manifest.id.split('.').at(-1)}`}>{(() => { const Mark = pluginMarks[manifest.id] ?? Command; return <Mark size={22} strokeWidth={1.8} aria-hidden="true" />; })()}</div><div className="plugin-title"><div><p className="plugin-detail-kind">{pluginKinds[manifest.id] ?? 'PLUGIN ORBIT'}</p><h2 id="plugin-detail-heading" tabIndex={-1}>{manifest.name}</h2><span className="plugin-publisher">{manifest.publisher} · v{manifest.version}</span></div><span className={`plugin-state ${state.enabled ? 'enabled' : state.installed ? 'installed' : 'available'}`}><i aria-hidden="true" />{state.enabled ? 'Activé' : state.installed ? 'Installé · désactivé' : 'Disponible'}</span><button type="button" className="plugin-detail-close" aria-label={`Fermer les détails de ${manifest.name}`} onClick={onClose}><X size={16} /></button></div></div>
    <p className="plugin-description">{manifest.description}</p>
    <p className="plugin-trust"><LockKeyhole size={15} /><span>{trust}</span></p>
    <p className="plugin-connection"><span className="plugin-connection-label">{connectionLabel}</span><span>{manifest.connection.summary}</span></p>
    {state.installed && manifest.connection.secretSlots.length > 0 && <section className="plugin-secret-management" aria-label={`Secrets enregistrés pour ${manifest.name}`}><strong>Secrets du coffre Windows</strong>{manifest.connection.secretSlots.map(slot => <div className="plugin-secret-row" key={slot.id}><span><b>{slot.label}</b><small>{secretPresence[slot.id] ? 'Enregistré dans le coffre Windows' : 'Non configuré'}</small></span>{secretPresence[slot.id] && <button type="button" className="text-button danger-action" aria-label={`Supprimer ${slot.label} du coffre Windows`} disabled={!native || busy} onClick={() => void removeSavedSecret(slot.id)}><Trash2 size={14} />Supprimer</button>}</div>)}</section>}
    <div className="plugin-contributions"><strong>Fonctions incluses</strong>{manifest.contributions.pages.map(contribution => <button key={contribution.routeId} type="button" className="plugin-contribution" onClick={() => state.enabled && navigate(contribution.routeId)} disabled={!state.enabled}><FolderOpen size={16} /><span><b>{contribution.label}</b><small>{contribution.description}</small></span>{state.enabled && <span className="contribution-link">Ouvrir</span>}</button>)}</div>
    {state.installed && <fieldset className="plugin-permissions"><legend>Permissions locales</legend>{manifest.demandedPermissions.map(permission => <label className="plugin-permission" key={permission.id}><span><b>{permission.label}{permission.required ? ' · requise' : ' · facultative'}</b><small>{permission.description}</small></span><input type="checkbox" checked={state.grantedPermissions.includes(permission.id)} disabled={!native || busy} onChange={event => void togglePermission(permission.id, event.target.checked)} aria-label={`Autoriser : ${permission.label}`} /></label>)}</fieldset>}
    <div className="plugin-actions">
      {!state.installed && <button type="button" className="primary-button" disabled={!native || busy} onClick={() => { setCheckedRequired([]); setError(''); installDialog.current?.showModal(); }}><Download size={16} />Installer localement</button>}
      {state.installed && !state.enabled && <button type="button" className="primary-button" disabled={!native || busy || !allRequired} onClick={() => void perform(async () => { await orbitServices.plugins.enable(manifest.id); }, `${manifest.name} est activé.`)}><Power size={16} />Activer</button>}
      {state.installed && state.enabled && page && <button type="button" className="secondary-button" onClick={() => navigate(page.routeId)}><Settings2 size={16} />Configurer dans ORBIT</button>}
      {state.installed && state.enabled && <button type="button" className="secondary-button" disabled={!native || busy} onClick={() => void perform(async () => { await orbitServices.plugins.disable(manifest.id); }, `${manifest.name} est désactivé.`)}><Power size={16} />Désactiver</button>}
      {state.installed && <button type="button" className="text-button danger-action" disabled={!native || busy} onClick={() => { setError(''); uninstallDialog.current?.showModal(); }}><Trash2 size={15} />Désinstaller…</button>}
    </div>
    {state.installed && !allRequired && <p className="plugin-hint">Accorde les permissions requises avant l’activation.</p>}
    {error && <p className="plugin-error" role="alert">{error}</p>}{message && <p className="plugin-success" role="status"><Check size={14} />{message}</p>}

    <dialog className="plugin-dialog" ref={installDialog} aria-labelledby={`install-title-${manifest.id}`}><form method="dialog" onSubmit={event => event.preventDefault()}><h2 id={`install-title-${manifest.id}`}>Installer {manifest.name}</h2><p>Confirme les autorisations requises. Les permissions facultatives resteront désactivées.</p>{manifest.demandedPermissions.filter(item => item.required).map(permission => <label className="plugin-permission" key={permission.id}><span><b>{permission.label}</b><small>{permission.description}</small></span><input type="checkbox" checked={checkedRequired.includes(permission.id)} onChange={event => setCheckedRequired(items => event.target.checked ? [...items, permission.id] : items.filter(id => id !== permission.id))} /></label>)}<div className="dialog-actions"><button className="secondary-button" type="button" onClick={() => installDialog.current?.close()}>Annuler</button><button className="primary-button" type="button" disabled={busy || !native || !requiredPermissions.every(id => checkedRequired.includes(id))} onClick={() => void install()}>Confirmer l’installation</button></div>{error && <p className="plugin-error" role="alert">{error}</p>}</form></dialog>
    <dialog className="plugin-dialog" ref={uninstallDialog} aria-labelledby={`uninstall-title-${manifest.id}`}><form method="dialog" onSubmit={event => event.preventDefault()}><h2 id={`uninstall-title-${manifest.id}`}>Désinstaller {manifest.name}</h2><p>Choisis si ORBIT doit aussi effacer les données locales de ce plugin. ORBIT supprime d’abord ses identifiants du coffre Windows. Si Windows refuse une suppression, le plugin et ses données restent en place; tu pourras corriger le problème puis réessayer.</p><div className="dialog-actions uninstall-actions"><button className="secondary-button" type="button" onClick={() => uninstallDialog.current?.close()}>Annuler</button><button className="secondary-button" type="button" disabled={busy} onClick={() => void perform(async () => { await orbitServices.plugins.uninstall(manifest.id, { deleteData: false }); uninstallDialog.current?.close(); }, 'Plugin désinstallé; données locales conservées.')}>Désinstaller et conserver les données</button><button className="danger-button" type="button" disabled={busy} onClick={() => void perform(async () => { await orbitServices.plugins.uninstall(manifest.id, { deleteData: true }); uninstallDialog.current?.close(); }, 'Plugin désinstallé et données locales supprimées.')}>Désinstaller et effacer les données</button></div>{error && <p className="plugin-error" role="alert">{error}</p>}</form></dialog>
  </article>;
}
