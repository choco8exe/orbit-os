import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { Activity, BookOpen, ChevronRight, CircleHelp, Command, Database, FolderOpen, Gauge, HardDrive, History, Maximize2, Minimize2, Orbit, Settings, ShieldCheck, Sparkles, X, Zap } from 'lucide-react';
import { ORBIT_ACCENT_PRESETS, type CoreHistoryPoint, type ModuleId, type OrbitEventBus, type OrbitThemeId, type OrbitalVisualization, type OrbitSettings, type PluginState, type SystemSnapshot, type TimelineRecord } from '../shared/contracts';
import { isAccessibleAccentColor, isAccessibleSurfaceColor, settingsService } from '../shared/settings';
import { coreService } from '../modules/core/service';
import { OrbitServicesProvider } from '../shared/services-context';
import { orbitServices } from '../shared/services';
import { echoContribution } from '../modules/echo/module';
import { getEchoPort } from '../modules/echo/tauri';
import { pulsarContribution, connectPulsarEvents } from '../modules/pulsar/module';
import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from '@tauri-apps/plugin-autostart';
import { OrbitalThreeScene } from './OrbitalThreeScene';
import { connectEchoEvents } from '../shared/echo-event-bridge';
import { PluginCenterPage } from './PluginCenterPage';
import { pluginPageProviders } from './plugin-providers';
import { QuickActionsPalette, type QuickActionCommand } from './QuickActionsPalette';
import { MaintenancePage } from '../modules/maintenance/MaintenancePage';
import { clearRetiredAtlasDatabaseOnce } from '../shared/retired-atlas-cleanup';

const events: OrbitEventBus = orbitServices.events;
type NavItem = { id: ModuleId; label: string; icon: typeof Orbit };
const navigation: NavItem[] = [
  { id: 'mission-control', label: 'Mission Control', icon: Orbit },
  { id: 'echo', label: 'Echo', icon: Database },
  { id: 'pulsar', label: 'Pulsar', icon: Zap },
  { id: 'core', label: 'Core', icon: Activity },
  { id: 'timeline', label: 'Timeline', icon: History },
  { id: 'plugins', label: 'Plugins', icon: Command },
  { id: 'project-shelf', label: 'Project Shelf', icon: FolderOpen },
  { id: 'settings', label: 'Paramètres', icon: Settings },
];
const titles: Record<ModuleId, string> = {
  'mission-control': 'Mission Control', echo: 'Echo', pulsar: 'Pulsar', core: 'Core', timeline: 'Timeline', settings: 'Paramètres', plugins: 'Plugins', 'project-shelf': 'Project Shelf',
  'usage-analytics': 'Pulse', 'stream-deck': 'Stream Deck',
  workspaces: 'Espaces de travail', 'game-sessions': 'Game Sessions', 'downloads-organizer': 'Downloads Organizer',
};
export default function App() {
  return <OrbitServicesProvider services={orbitServices}><AppShell /></OrbitServicesProvider>;
}

function AppShell() {
  const [active, setActive] = useState<ModuleId>('mission-control');
  const [settings, setSettings] = useState<OrbitSettings>();
  const [snapshot, setSnapshot] = useState<SystemSnapshot>();
  const [timeline, setTimeline] = useState<TimelineRecord[]>([]);
  const [native, setNative] = useState(() => isTauri());
  const [restoreError, setRestoreError] = useState('');
  const [clientReady, setClientReady] = useState(false);
  const [pluginStates, setPluginStates] = useState<PluginState[]>([]);
  const [quickActionsOpen, setQuickActionsOpen] = useState(false);
  const briefingRef = useRef<HTMLDialogElement>(null);
  const visiblePluginProviders = pluginPageProviders.filter(provider => pluginStates.some(state => state.pluginId === provider.pluginId && state.installed && state.enabled));
  const activePluginProvider = visiblePluginProviders.find(provider => provider.routeId === active);
  const permanentNavigation = navigation.filter(item => item.id !== 'project-shelf' && item.id !== 'settings');
  const settingsNavigation = navigation.find(item => item.id === 'settings');
  const visibleNavigation = [...permanentNavigation, ...visiblePluginProviders.map(provider => ({ id: provider.routeId, label: provider.label, icon: provider.icon })), ...(settingsNavigation ? [settingsNavigation] : [])];
  const immersive = settings?.immersive ?? false;
  const quickActions = useMemo<QuickActionCommand[]>(() => {
    const commands: QuickActionCommand[] = visibleNavigation.map(item => ({
      id: `navigate-${item.id}`,
      section: 'Navigation',
      icon: item.icon,
      label: `Ouvrir ${item.label}`,
      description: `Afficher ${item.label} dans ORBIT.`,
      keywords: [item.id.replace(/-/g, ' ')],
      closeOnRun: true,
      focusRouteAfterRun: true,
      run: () => setActive(item.id),
    }));
    if (native && orbitServices.echo) commands.push({
      id: 'echo-copy-previous',
      section: 'Actions',
      icon: Database,
      label: 'Copier l’entrée Echo précédente',
      description: 'Recopier dans le presse-papiers l’avant-dernière entrée conservée par date.',
      keywords: ['clipboard historique presse papier copier précédent dernier'],
      run: async () => {
        const entries = await orbitServices.echo!.search('', { limit: 200 });
        const chronological = [...entries].sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));
        const previous = chronological[1];
        if (!previous) throw new Error('Echo doit contenir au moins deux entrées pour recopier la précédente.');
        await orbitServices.echo!.copy(previous.id);
      },
    });
    commands.push({
      id: 'toggle-immersive',
      section: 'Actions',
      icon: immersive ? Minimize2 : Maximize2,
      label: immersive ? 'Quitter le mode immersif' : 'Activer le mode immersif',
      description: immersive ? 'Réafficher les contrôles et l’en-tête ORBIT.' : 'Masquer les éléments secondaires pour Mission Control.',
      keywords: ['plein écran présentation'],
      closeOnRun: true,
      run: async () => { setSettings(await settingsService.update({ immersive: !immersive })); },
    });
    return commands;
  }, [visibleNavigation, native, immersive]);
  useEffect(() => {
    let mounted = true;
    const initializeClient = async () => {
      void clearRetiredAtlasDatabaseOnce().catch(() => undefined);
      const nativeSession = isTauri();
      if (mounted) setNative(nativeSession);
      try {
        if (nativeSession) {
          const restoreStatus = await invoke<{ pending: boolean; quarantined: boolean; error: string | null }>('maintenance_pending_restore_status');
          if (restoreStatus.error && mounted) setRestoreError(`${restoreStatus.error}${restoreStatus.quarantined ? ' Les données de restauration ont été mises en quarantaine.' : ''}`);
          const restored = await invoke<{ settings?: unknown | null; timeline?: unknown | null } | null>('maintenance_take_restored_client_data');
          if (restored) {
            if (restored.settings != null) localStorage.setItem('orbit.settings.v1', JSON.stringify(restored.settings));
            if (restored.timeline != null) {
              if (!Array.isArray(restored.timeline)) throw new Error('La Timeline restaurée ne contient pas une liste valide.');
              const current = JSON.parse(localStorage.getItem('orbit.timeline.v1') ?? '[]') as unknown;
              if (!Array.isArray(current)) throw new Error('La Timeline locale actuelle est invalide.');
              const records = new Map<string, Record<string, unknown>>();
              for (const row of [...current, ...restored.timeline]) {
                if (!row || typeof row !== 'object') throw new Error('Une entrée Timeline locale est invalide; restauration conservée.');
                const item = row as Record<string, unknown>;
                if (typeof item.id !== 'string' || typeof item.category !== 'string' || typeof item.at !== 'string' || typeof item.title !== 'string') throw new Error('Une entrée Timeline ne possède pas les champs requis; restauration conservée.');
                records.set(`${item.category}:${item.id}`, item);
              }
              localStorage.setItem('orbit.timeline.v1', JSON.stringify([...records.values()].sort((a, b) => String(b.at).localeCompare(String(a.at))).slice(0, 10000)));
            }
            await invoke('maintenance_acknowledge_restored_client_data');
          }
        }
      } catch (cause) {
        if (mounted) setRestoreError(cause instanceof Error ? cause.message : String(cause));
      }
      try {
        const value = await settingsService.read();
        if (mounted) setSettings(value);
      } catch (cause) {
        if (mounted) setRestoreError(current => current || (cause instanceof Error ? cause.message : String(cause)));
      } finally {
        if (mounted) setClientReady(true);
      }
    };
    void initializeClient();
    void orbitServices.plugins.listStates().then(value => { if (mounted) setPluginStates(value); }).catch(() => { if (mounted) setPluginStates([]); });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === 'k' && !event.repeat) {
        event.preventDefault();
        setQuickActionsOpen(true);
      }
    };
    window.addEventListener('keydown', handleShortcut);
    return () => window.removeEventListener('keydown', handleShortcut);
  }, []);
  useEffect(() => {
    if (!native) return;
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void listen<string>('orbit://navigate', event => {
      const target = event.payload;
      if (['mission-control', 'echo', 'pulsar', 'core', 'timeline', 'settings', 'plugins', 'project-shelf', 'usage-analytics', 'stream-deck', 'workspaces', 'game-sessions', 'downloads-organizer'].includes(target)) setActive(target as ModuleId);
    }).then(stop => { if (mounted) unlisten = stop; else stop(); });
    return () => { mounted = false; unlisten?.(); };
  }, [native]);
  useEffect(() => {
    let mounted = true;
    const unsubscribe = coreService.subscribe(value => { if (mounted) setSnapshot(value); events.emit('core.snapshot', value); }, (settings?.coreRefreshSeconds ?? 2) * 1000);
    return () => { mounted = false; unsubscribe(); };
  }, [settings?.coreRefreshSeconds]);
  useEffect(() => orbitServices.timeline.subscribe(setTimeline), []);
  useEffect(() => {
    events.emit('timeline.record', { id: crypto.randomUUID(), category: 'system', title: 'Session ORBIT démarrée', at: new Date().toISOString() });
  }, []);
  useEffect(() => native ? connectPulsarEvents(events) : undefined, [native]);
  useEffect(() => native ? connectEchoEvents(getEchoPort(), events) : undefined, [native]);

  const update = async (patch: Partial<OrbitSettings>) => setSettings(await settingsService.update(patch));
  const showBriefing = () => { if (briefingRef.current && !briefingRef.current.open) briefingRef.current.showModal(); };
  const finishOnboarding = async (destination?: ModuleId) => {
    const next = await settingsService.update({ onboardingComplete: true });
    setSettings(next);
    if (destination) setActive(destination);
  };
  return <div className={`orbit-app${immersive ? ' immersive' : ''}${settings?.reduceMotion ? ' reduce-motion' : ''}`} data-orbit-theme={settings?.theme ?? 'sidereal'} style={{ '--user-accent': settings?.accentColor || undefined, '--user-panel': settings?.panelColor || undefined, '--user-raised': settings?.raisedColor || undefined } as CSSProperties}>
    <main className="main-shell">
      <header className="orbit-header">
        <div className="orbit-identity"><span className="orbit-logo-orbit" aria-hidden="true"><img src="/orbit-mark.svg" alt="" /></span><div><strong>ORBIT OS</strong><span>Your Digital Universe</span></div></div>
        <nav className="command-nav" aria-label="Navigation principale">
          {visibleNavigation.map(({ id, label, icon: Icon }) => <button key={id} className={`nav-item${active === id ? ' active' : ''}`} onClick={() => setActive(id)} aria-label={label} aria-current={active === id ? 'page' : undefined}>
            <Icon size={16} strokeWidth={1.8} aria-hidden="true" /><span>{label}</span>
          </button>)}
        </nav>
        <div className="orbit-header-tools">
          <span className={`connection-state${native ? ' native' : ''}`}><span className="status-dot" />{native ? 'Session locale' : 'Mode aperçu'}</span>
          <button type="button" className="quick-actions-trigger" aria-label="Ouvrir les actions rapides" aria-haspopup="dialog" aria-expanded={quickActionsOpen} aria-keyshortcuts="Control+K" title="Actions rapides (Ctrl+K)" onClick={() => setQuickActionsOpen(true)}><Command size={15} /><span>Actions</span><kbd aria-hidden="true">Ctrl K</kbd></button>
          <button className="header-help" onClick={showBriefing} aria-label="Ouvrir le Briefing ORBIT"><CircleHelp size={16} /><span>Briefing ORBIT</span></button>
          <button className="icon-button" onClick={() => void update({ immersive: !immersive })} aria-label={immersive ? 'Quitter le mode immersif' : 'Activer le mode immersif'} title={immersive ? 'Quitter le mode immersif' : 'Mode immersif'}>
            {immersive ? <Minimize2 size={17} /> : <Maximize2 size={17} />}
          </button>
        </div>
      </header>
      <div className="content-scroll">
        {!clientReady ? <div className="panel startup-status" role="status" aria-live="polite">Préparation des données locales…</div> : <>
        {restoreError && <div className="maintenance-error" role="alert">Restauration ORBIT : {restoreError} Consulte Paramètres → Sauvegarde et diagnostic.</div>}
        {active === 'mission-control' && <MissionControl snapshot={snapshot} native={native} navigate={setActive} timeline={timeline} settings={settings} widgetOrder={settings?.widgetOrder ?? ['system','orbit','modules','activity']} hiddenWidgets={settings?.hiddenWidgets ?? []} openBriefing={showBriefing} />}
        {active === 'echo' && native && <echoContribution.Page navigate={setActive} />}
        {active === 'pulsar' && native && <pulsarContribution.Page navigate={setActive} />}
        {active === 'core' && <CorePage snapshot={snapshot} native={native} />}
        {active === 'settings' && <SettingsPage settings={settings ?? undefined} update={update} />}
        {active === 'timeline' && <TimelinePage records={timeline} />}
        {active === 'plugins' && <PluginCenterPage native={native} states={pluginStates} onStatesChanged={setPluginStates} navigate={setActive} />}
        {activePluginProvider && <activePluginProvider.Page navigate={setActive} />}
        {(['project-shelf', 'usage-analytics', 'stream-deck', 'workspaces', 'game-sessions', 'downloads-organizer'] as ModuleId[]).includes(active) && !activePluginProvider && <PluginCenterPage native={native} states={pluginStates} onStatesChanged={setPluginStates} navigate={setActive} />}
        {active !== 'mission-control' && active !== 'core' && active !== 'settings' && active !== 'timeline' && active !== 'plugins' && active !== 'project-shelf' && active !== 'usage-analytics' && active !== 'stream-deck' && active !== 'workspaces' && active !== 'game-sessions' && active !== 'downloads-organizer' && active !== 'echo' && active !== 'pulsar' && <ModuleUnavailable id={active} navigate={setActive} />}
        {active === 'echo' && !native && <ModuleUnavailable id="echo" navigate={setActive} />}
        {active === 'pulsar' && !native && <ModuleUnavailable id="pulsar" navigate={setActive} />}
        </>}
      </div>
    </main>
    <QuickActionsPalette open={quickActionsOpen} onClose={() => setQuickActionsOpen(false)} onRouteClose={() => {
      window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
        const heading = document.querySelector('.content-scroll h1');
        if (heading instanceof HTMLElement) {
          if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
          heading.focus();
        }
      }));
    }} commands={quickActions} reduceMotion={settings?.reduceMotion ?? false} />
    <BriefingDialog native={native} snapshot={snapshot} dialogRef={briefingRef} />
    {immersive && <button className="immersive-exit" onClick={() => void update({ immersive: false })} aria-label="Quitter le mode immersif"><Minimize2 size={16} />Quitter le mode immersif</button>}
    {settings && !settings.onboardingComplete && <FirstRunDialog native={native} onFinish={finishOnboarding} />}
  </div>;
}
declare global { interface Window { __TAURI_INTERNALS__?: unknown } }

function MissionControl({ snapshot, native, navigate, timeline, settings, widgetOrder, hiddenWidgets, openBriefing }: { snapshot?: SystemSnapshot; native: boolean; navigate: (id: ModuleId) => void; timeline: TimelineRecord[]; settings?: OrbitSettings; widgetOrder: string[]; hiddenWidgets: string[]; openBriefing: () => void }) {
  const latest = useMemo(() => snapshot?.capturedAt ? new Date(snapshot.capturedAt).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '—', [snapshot]);
  const [coreHistory, setCoreHistory] = useState<CoreHistoryPoint[]>([]);
  const [chartMetric, setChartMetric] = useState<'cpuPercent' | 'memoryPercent'>('cpuPercent');
  const [sceneStatus, setSceneStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const serviceTone = (label: string) => {
    const value = label.toLocaleLowerCase('fr');
    if (value.includes('erreur') || value.includes('error')) return 'state-error';
    if (value.includes('indisponible') || value.includes('unavailable')) return 'state-unavailable';
    if (value.includes('configurée') || value.includes('active') || /^\d+ actif/.test(value)) return 'state-ready';
    return 'state-pending';
  };
  const [moduleStatus, setModuleStatus] = useState({ echo: 'Lecture…', pulsar: 'Service indisponible', echoItems: 0, pulsarCount: 0, pulsarNames: [] as string[] });
  const [echoEntries, setEchoEntries] = useState<Array<{ id: string; kind: 'text'|'url'|'image'; preview: string; createdAt: string }>>([]);
  const [echoMessage, setEchoMessage] = useState('');
  const lastAlertAt = useRef<Record<string, number>>({});
  useEffect(() => {
    let mounted = true;
    const refresh = async () => {
      const [echo, pulsar] = await Promise.allSettled([orbitServices.echo?.status(), orbitServices.pulsar?.list()]);
      if (!mounted) return;
      const echoStatus = echo.status === 'fulfilled' && echo.value
        ? echo.value.enabled ? 'Surveillance active' : 'Désactivé'
        : 'Service indisponible';
      const enabledScenarios = pulsar.status === 'fulfilled' && pulsar.value ? pulsar.value.filter(item => item.enabled).length : 0;
      const activeNames = pulsar.status === 'fulfilled' && pulsar.value ? pulsar.value.filter(item => item.enabled).slice(0, 3).map(item => item.name) : [];
      setModuleStatus({ echo: echoStatus, pulsar: pulsar.status === 'fulfilled' && pulsar.value ? `${enabledScenarios} actif${enabledScenarios === 1 ? '' : 's'}` : 'Service indisponible', echoItems: echo.status === 'fulfilled' && echo.value ? echo.value.itemCount : 0, pulsarCount: enabledScenarios, pulsarNames: activeNames });
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10_000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);
  useEffect(() => {
    if (!snapshot || !settings) return;
    const now = Date.now();
    const candidates: Array<{ metric: 'cpu'|'memory'|'disk'; value: number; threshold: number; message: string }> = [];
    if (available(snapshot.cpu.usagePercent) && snapshot.cpu.usagePercent.value >= settings.alerts.cpuPercent) candidates.push({ metric: 'cpu', value: snapshot.cpu.usagePercent.value, threshold: settings.alerts.cpuPercent, message: 'Charge processeur élevée' });
    if (snapshot.memory.totalBytes.value && snapshot.memory.usedBytes.value !== null) {
      const percent = snapshot.memory.usedBytes.value / snapshot.memory.totalBytes.value * 100;
      if (percent >= settings.alerts.memoryPercent) candidates.push({ metric: 'memory', value: Math.round(percent), threshold: settings.alerts.memoryPercent, message: 'Utilisation mémoire élevée' });
    }
    const disk = snapshot.disks.find(item => available(item.usedPercent));
    if (disk && available(disk.usedPercent)) {
      const free = 100 - disk.usedPercent.value;
      if (free <= settings.alerts.diskFreePercent) candidates.push({ metric: 'disk', value: Math.round(free), threshold: settings.alerts.diskFreePercent, message: 'Espace disque presque plein' });
    }
    for (const alert of candidates) {
      if (now - (lastAlertAt.current[alert.metric] ?? 0) < 300_000) continue;
      lastAlertAt.current[alert.metric] = now;
      events.emit('core.alert', { id: crypto.randomUUID(), ...alert, at: new Date(now).toISOString() });
    }
  }, [snapshot?.capturedAt, settings?.alerts]);
  useEffect(() => {
    let active = true;
    const load = () => { void coreService.history().then(points => { if (active) setCoreHistory(points); }); };
    load();
    const off = events.on('core.snapshot', load);
    return () => { active = false; off(); };
  }, []);
  useEffect(() => {
    if (!native) return;
    let mounted = true;
    const refresh = async () => {
      try {
        const data = await getEchoPort().getSnapshot();
        if (!mounted) return;
        setEchoEntries(data.entries.slice(0, 3).map(({ id, kind, preview, createdAt }) => ({ id, kind, preview, createdAt })));
        setEchoMessage(data.available ? '' : data.unavailableReason ?? 'Echo ne peut pas lire les données locales.');
      } catch (error) { if (mounted) setEchoMessage(`Echo indisponible : ${String(error)}`); }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10_000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [native]);
  const memory = snapshot?.memory;
  const memoryPercent = available(memory?.usedBytes) && available(memory?.totalBytes) && memory?.totalBytes.value ? Math.round((memory.usedBytes.value! / memory.totalBytes.value) * 100) : null;
  const primaryDisk = snapshot?.disks[0];
  const widgetContent: Record<string, ReactNode> = {
    system: <section className="system-panel panel" aria-labelledby="system-title">
      <div className="panel-heading"><div><h2 id="system-title">État du système</h2><p>{native ? 'Mesures depuis le service Windows local' : 'Les mesures matérielles apparaîtront dans la version Windows native.'}</p></div><button className="text-button" onClick={() => navigate('core')}>Détails Core <ChevronRight size={15} /></button></div>
      <div className="metric-grid">
        <MetricBlock icon={<Gauge size={16} />} label="Processeur" value={formatMetric(snapshot?.cpu.usagePercent, '%')} detail={available(snapshot?.cpu.name) ? snapshot!.cpu.name.value! : dashboardUnavailable(snapshot?.cpu.usagePercent, native)} accent="cyan" />
        <MetricBlock icon={<Database size={16} />} label="Mémoire" value={memoryPercent === null ? 'Non disponible' : `${memoryPercent} %`} detail={memoryPercent === null ? dashboardUnavailable(memory?.usedBytes, native) : `${formatBytes(memory?.usedBytes.value)} / ${formatBytes(memory?.totalBytes.value)}`} accent="blue" />
        <MetricBlock icon={<HardDrive size={16} />} label="Stockage" value={available(primaryDisk?.usedPercent) ? `${Math.round(primaryDisk!.usedPercent.value!)} %` : 'Non disponible'} detail={primaryDisk ? `${primaryDisk.mount} · ${formatBytes(primaryDisk.freeBytes.value)} libres` : 'Aucun volume mesuré'} accent="violet" />
        <MetricBlock icon={<Activity size={16} />} label="Activité GPU" value={formatMetric(snapshot?.gpu.usagePercent, '%')} detail={available(snapshot?.gpu.name) ? snapshot!.gpu.name.value! : dashboardUnavailable(snapshot?.gpu.usagePercent, native)} accent="amber" />
        <MetricBlock icon={<Activity size={16} />} label="Réseau entrant" value={formatRate(snapshot?.network.receivedBytesPerSecond.value)} detail={`Sortie · ${formatRate(snapshot?.network.sentBytesPerSecond.value)}`} accent="blue" />
      </div>
      <div className="metric-note"><span>Intervalle · {settings?.coreRefreshSeconds ?? 2} s</span><span>Historique · {settings?.coreHistoryMinutes ?? 15} min</span><button className="text-button" onClick={() => navigate('settings')}>Réglages Core <ChevronRight size={14} /></button></div>
    </section>,
    modules: <section className="modules-panel panel" aria-labelledby="modules-title">
      <div className="panel-heading"><div><h2 id="modules-title">Modules</h2><p>Services de votre espace ORBIT</p></div><span className="subtle-count">Modules intégrés</span></div>
      <div className="module-list">
        <ModuleRow icon={<Database size={18} />} name="Echo" detail={`Presse-papiers · ${moduleStatus.echoItems} entrées`} state={moduleStatus.echo} tone={moduleStatus.echo === 'Surveillance active' ? 'success' : 'muted'} onClick={() => navigate('echo')} />
        <ModuleRow icon={<Zap size={18} />} name="Pulsar" detail={moduleStatus.pulsarNames.length ? moduleStatus.pulsarNames.join(' · ') : 'Aucune automatisation active'} state={moduleStatus.pulsar} tone={moduleStatus.pulsarCount > 0 ? 'success' : 'muted'} onClick={() => navigate('pulsar')} />
        <ModuleRow icon={<Activity size={18} />} name="Core" detail="Surveillance système" state={native ? (snapshot ? 'Actif' : 'Connexion…') : 'Aperçu'} tone={native ? 'success' : 'muted'} onClick={() => navigate('core')} />
      </div>
    </section>,
    activity: <section className="activity-panel panel" aria-label="Activité et presse-papiers récents">
      <div className="recent-panels">
        <div><div className="panel-heading"><div><h2>Dernières entrées Echo</h2><p>Extraits locaux, visibles uniquement sur cet appareil</p></div><button className="text-button" onClick={() => navigate('echo')}>Ouvrir Echo <ChevronRight size={15} /></button></div>
          {echoEntries.length ? <div className="echo-recent-list">{echoEntries.map(entry => <EchoRecentRow key={entry.id} entry={entry} />)}</div> : <div className="compact-empty">{echoMessage || (native ? 'Aucune entrée conservée.' : 'Disponible dans l’application Windows.')}</div>}
        </div>
        <div><div className="panel-heading"><div><h2>Activité récente</h2><p>Événements ORBIT locaux</p></div><button className="text-button" onClick={() => navigate('timeline')}>Timeline <ChevronRight size={15} /></button></div>
          {timeline.length ? <div className="activity-list">{timeline.slice(0, 3).map(item => <div className="activity-row" key={item.id}><span className={`activity-dot ${item.category}`} /><strong>{item.title}</strong><time>{new Date(item.at).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' })}</time></div>)}</div> : <div className="compact-empty">Aucun événement récent.</div>}
        </div>
      </div>
    </section>,
  };
  // Keep every supported widget in the allowed list as well as the fallback order.
  // `orbit` is user-configurable and otherwise gets filtered out before rendering.
  const fallbackOrder = ['system', 'orbit', 'modules', 'activity'];
  const orderedWidgets = [...new Set([...widgetOrder, ...fallbackOrder])].filter(id => fallbackOrder.includes(id) && !hiddenWidgets.includes(id));
  const historyValues = coreHistory.map(point => point[chartMetric]).filter((value): value is number => value !== null && Number.isFinite(value));
  const chartLast = historyValues.at(-1);
  const chartPoints = historyValues.map((value, index) => `${historyValues.length < 2 ? 200 : 5 + index * 390 / (historyValues.length - 1)},${100 - Math.max(0, Math.min(100, value))}`).join(' ');
  const visualization = settings?.orbitalVisualization ?? 'earth';
  const visualizationLabel: Record<OrbitalVisualization, string> = { earth: 'Terre vivante', 'ringed-planet': 'Monde aux anneaux', 'lunar-station': 'Station lunaire', nebula: 'Nébuleuse', core: 'Noyau ORBIT', wireframe: 'Trame orbitale', constellations: 'Constellations' };
  const zoneOrder = Object.fromEntries(orderedWidgets.map((id, index) => [id, index]));
  return <section className="page mission-page" aria-labelledby="page-title">
    <div className="command-masthead">
      <div className="masthead-title"><div><h1 id="page-title">Mission Control</h1><p className="masthead-description">Mesures du PC et activité récente.</p></div></div>
      <div className="masthead-actions"><div className="masthead-readout"><span className="masthead-state"><i className={native && snapshot ? 'ready' : ''} />{native ? snapshot ? 'Système en ligne' : 'Capteurs en cours' : 'Aperçu navigateur'}</span><span>Actualisé <b>{latest}</b></span></div><button className="briefing-button" onClick={openBriefing}><BookOpen size={15} />Briefing ORBIT</button></div>
    </div>
    <div className="console-grid">
      {orderedWidgets.includes('system') && <div className="console-cell console-system" style={{ order: zoneOrder.system }}>{widgetContent.system}</div>}
      {orderedWidgets.includes('orbit') && <section className="console-cell orbital-stage" style={{ order: zoneOrder.orbit }} aria-labelledby="stage-title">
        <div className="stage-topline"><div><p className="eyebrow">MISSION CONTROL</p><h2 id="stage-title">{visualizationLabel[visualization]}</h2></div><span className="stage-coordinate">{snapshot?.cpu.cores.value ?? '—'} cœurs</span></div>
        <div className="planet-scene" role="img" aria-label={`Scène ${visualizationLabel[visualization]}. Les réglages d’animation se trouvent dans Paramètres, Apparence.`}>
          {(settings?.orbitalEnabled === false || sceneStatus === 'error') && <svg aria-hidden="true" className="planet-svg" viewBox="0 0 620 390" preserveAspectRatio="xMidYMid meet">
            <defs><radialGradient id="planetFill" cx="34%" cy="28%" r="78%"><stop offset="0" stopColor="var(--earth-land)"/><stop offset=".48" stopColor="var(--earth-ocean)"/><stop offset=".86" stopColor="var(--earth-ocean-shadow)"/><stop offset="1" stopColor="var(--earth-ocean-night)"/></radialGradient><linearGradient id="planetLight" x1=".08" y1=".08" x2=".92" y2=".9"><stop stopColor="var(--earth-rim)" stopOpacity=".24"/><stop offset=".48" stopColor="var(--earth-rim)" stopOpacity=".05"/><stop offset="1" stopColor="var(--earth-rim)" stopOpacity="0"/></linearGradient><clipPath id="planetClip"><circle cx="310" cy="202" r="132"/></clipPath></defs>
            <g fill="var(--color-text-primary)" opacity=".7"><circle cx="92" cy="78" r="1.2"/><circle cx="167" cy="45" r=".8"/><circle cx="526" cy="80" r="1"/><circle cx="562" cy="280" r=".8"/><circle cx="72" cy="285" r="1"/><circle cx="414" cy="43" r=".7"/><circle cx="457" cy="338" r=".9"/></g><circle cx="310" cy="202" r="172" fill="none" stroke="var(--color-orbit-line)" strokeWidth="1" strokeDasharray="1 7"/><ellipse cx="310" cy="202" rx="237" ry="75" fill="none" stroke="var(--color-data-primary)" strokeWidth="1.2" transform="rotate(-16 310 202)"/><ellipse cx="310" cy="202" rx="185" ry="49" fill="none" stroke="var(--color-data-secondary)" strokeWidth="1" transform="rotate(28 310 202)" opacity=".62"/><circle cx="310" cy="202" r="132" fill="url(#planetFill)" stroke="var(--earth-rim)" strokeOpacity=".72" strokeWidth="1.2"/><g clipPath="url(#planetClip)"><g fill="var(--earth-land)" stroke="var(--earth-land-lit)" strokeOpacity=".48" strokeWidth="1"><path d="M211 132c10-15 27-23 42-20l13 8 4 13-10 8-4 12-12 5-8 16-13 4-6 13-13-2-6-14-12-8 3-14 10-5 3-10 12-6-3-10Z"/><path d="M263 192l15 4 9 11-3 13 10 10-3 19-11 17-4 18-9 19-9-9-3-22-8-16-2-21-8-15 5-15 9-13 12 0Z"/><path d="M310 128l15-11 20 3 9 12 18 2 13 13 22 5 13 17-9 12-21-1-9 12-13-4-8 13-18 0-8 12-15-8-10-15-17-4-5-16 10-11 1-16 10-7-1-8Z"/><path d="M333 183l16 0 13 9 4 17-9 14-3 19-12 18-10-11-2-18-10-15 4-18 9-15Z"/><path d="M387 236l17 2 14 11-6 13-16 6-15-8-5-13 11-11Z"/></g><g fill="none" stroke="var(--earth-cloud)" strokeLinecap="round" strokeWidth="7" opacity=".14"><path d="M191 177c24-14 43-11 61-3s34 7 52-3"/><path d="M295 151c26-9 45-5 65 5s40 10 61 1"/><path d="M288 246c23-11 43-8 62 2s36 11 55 4"/></g><path d="M178 150c37-49 100-78 159-71 42 5 75 22 100 49-38-26-69-33-101-27-57 11-95 11-158 49Z" fill="url(#planetLight)"/></g><circle cx="310" cy="202" r="132" fill="none" stroke="var(--earth-rim)" strokeOpacity=".28" strokeWidth="4"/><circle cx="504" cy="133" r="4" fill="var(--color-action)"/><circle cx="168" cy="271" r="3" fill="var(--color-data-primary)"/>
          </svg>}
          {settings?.orbitalEnabled !== false && <OrbitalThreeScene key={`${visualization}:${settings?.orbitalIntensity}:${settings?.theme}`} visualization={visualization} reduceMotion={settings?.reduceMotion ?? false} paused={false} rotationSpeed={settings?.orbitalRotationSpeed ?? 1} particlesEnabled={settings?.orbitalParticlesEnabled ?? true} intensity={settings?.orbitalIntensity ?? 'balanced'} onStatus={setSceneStatus} />}
          <div className="scene-status" role="status" aria-live="polite">{settings?.orbitalEnabled === false ? 'Scène 3D désactivée · illustration fixe' : sceneStatus === 'loading' ? 'Chargement de la scène…' : sceneStatus === 'error' ? 'WebGL indisponible · illustration fixe affichée' : settings?.reduceMotion ? 'Scène fixe · mouvement réduit' : 'Scène 3D active'}</div>
          <div className="stage-callout callout-bottom"><span>Mesures système</span><b>{native && snapshot ? 'Collecte active' : 'Capteurs Windows requis'}</b></div>
        </div>
        <div className="stage-footer"><span><i className={native ? 'ready' : ''} />{native ? 'Données locales' : 'Mode aperçu'}</span><button className="text-button" onClick={() => navigate('core')}>Ouvrir Core <ChevronRight size={15} /></button></div>
      </section>}
      {orderedWidgets.includes('modules') && <div className="console-cell console-controls" style={{ order: zoneOrder.modules }}>
        <section className="control-board panel" aria-labelledby="control-title"><div className="panel-heading"><div><p className="eyebrow">Tableau de bord</p><h2 id="control-title">Télémétrie en direct</h2></div><span className="control-live"><i className={native ? 'ready' : ''} />{native ? 'Local' : 'Aperçu'}</span></div>
          <div className="segmented-control" role="group" aria-label="Métrique du graphique"><button aria-pressed={chartMetric === 'cpuPercent'} onClick={() => setChartMetric('cpuPercent')}>Processeur</button><button aria-pressed={chartMetric === 'memoryPercent'} onClick={() => setChartMetric('memoryPercent')}>Mémoire</button></div>
          <div className="live-chart-readout"><div><span>{chartMetric === 'cpuPercent' ? 'CHARGE CPU' : 'MÉMOIRE UTILISÉE'}</span><strong>{chartLast === undefined ? '—' : `${Math.round(chartLast)} %`}</strong></div><span>{coreHistory.length} relevés</span></div>
          <svg className={`console-chart ${chartMetric === 'cpuPercent' ? 'series-cpu' : 'series-memory'}`} viewBox="0 0 400 112" role="img" aria-label={chartLast === undefined ? 'Historique de mesures en attente' : `${chartMetric === 'cpuPercent' ? 'Charge CPU' : 'Mémoire'} : ${Math.round(chartLast)} pour cent sur ${historyValues.length} relevés`}><line x1="0" y1="10" x2="400" y2="10"/><line x1="0" y1="55" x2="400" y2="55"/><line x1="0" y1="100" x2="400" y2="100"/>{historyValues.length > 1 ? <polyline points={chartPoints} /> : <text x="200" y="59" textAnchor="middle">En attente de mesures successives</text>}</svg>
          <div className="chart-range"><span>{coreHistory[0] ? new Date(coreHistory[0].at).toLocaleTimeString('fr-FR', {hour:'2-digit',minute:'2-digit'}) : '—'}</span><span>{coreHistory.at(-1) ? new Date(coreHistory.at(-1)!.at).toLocaleTimeString('fr-FR', {hour:'2-digit',minute:'2-digit'}) : '—'}</span></div>
        </section>
        <div className="status-board"><div className="status-board-heading"><h2>État des services</h2><span>Vérification toutes les 10 s</span></div>
          <button className={`service-notice ${serviceTone(moduleStatus.echo)}`} onClick={() => navigate('echo')}><span className="notice-mark"><Database size={16}/></span><span><b>Echo</b><small>{moduleStatus.echo} · {moduleStatus.echoItems} entrées</small></span><ChevronRight size={15}/></button>
          <button className={`service-notice ${serviceTone(moduleStatus.pulsar)}`} onClick={() => navigate('pulsar')}><span className="notice-mark"><Zap size={16}/></span><span><b>Pulsar</b><small>{moduleStatus.pulsarNames.length ? moduleStatus.pulsarNames.join(' · ') : moduleStatus.pulsar}</small></span><ChevronRight size={15}/></button>
        </div>
      </div>}
      {orderedWidgets.includes('activity') && <div className="console-cell console-activity" style={{ order: zoneOrder.activity }}>{widgetContent.activity}</div>}
    </div>
    <footer className="page-footnote"><span><Command size={13} /> Données conservées sur cet appareil</span><span>{native ? 'Natif Windows' : 'Aperçu web · mesures système indisponibles'} <span className="foot-separator">·</span> actualisation {snapshot ? new Date(snapshot.capturedAt).toLocaleTimeString('fr-FR') : '—'}</span></footer>
  </section>;
}

function BriefingDialog({ native, snapshot, dialogRef }: { native: boolean; snapshot?: SystemSnapshot; dialogRef: RefObject<HTMLDialogElement | null> }) {
  const [states, setStates] = useState({ echo: 'Lecture…', pulsar: 'Lecture…' });
  useEffect(() => {
    let mounted = true;
    void Promise.allSettled([orbitServices.echo?.status(), orbitServices.pulsar?.list()]).then(([echo, pulsar]) => {
      if (!mounted) return;
      setStates({
        echo: echo.status === 'fulfilled' && echo.value ? echo.value.enabled ? echo.value.paused ? 'En pause' : 'Surveillance active' : 'Désactivé' : native ? 'Service indisponible' : 'Presse-papiers Windows inaccessible dans l’aperçu',
        pulsar: pulsar.status === 'fulfilled' && pulsar.value ? `${pulsar.value.filter(item => item.enabled).length} scénario(s) activé(s)` : native ? 'Service indisponible' : 'Service natif indisponible dans l’aperçu',
      });
    });
    return () => { mounted = false; };
  }, [native]);
  return <dialog ref={dialogRef} className="briefing-dialog" onMouseDown={event => { if (event.target === event.currentTarget) dialogRef.current?.close(); }} aria-labelledby="briefing-title">
      <header className="briefing-heading"><div className="briefing-orbit-mark"><img src="/orbit-mark.svg" alt="" /></div><div><p className="eyebrow">YOUR DIGITAL UNIVERSE</p><h2 id="briefing-title">Briefing ORBIT</h2><p>Un centre de commandement pour les outils locaux.</p></div><button className="icon-button" onClick={() => dialogRef.current?.close()} aria-label="Fermer le briefing"><X size={17} /></button></header>
      <div className="briefing-intro"><strong>ORBIT OS rassemble les outils locaux dans une seule application Windows.</strong><span>Les états ci-dessous proviennent des services disponibles dans cette session. L’aperçu navigateur n’accède pas aux services natifs.</span></div>
      <div className="briefing-modules">
        <BriefingModule icon={<Database size={17} />} name="Echo" description="Historique local du presse-papiers, recherche, collections et recopie. La surveillance peut être désactivée." state={states.echo} />
        <BriefingModule icon={<Zap size={17} />} name="Pulsar" description="Scénarios locaux avec déclencheurs, conditions, actions et journal. Les opérations sensibles demandent validation." state={states.pulsar} />
        <BriefingModule icon={<Activity size={17} />} name="Core" description="Mesures système réellement accessibles : processeur, mémoire, stockage et réseau. Les capteurs absents restent indiqués comme indisponibles." state={native ? snapshot ? 'Mesures reçues' : 'Connexion aux capteurs…' : 'Mesures Windows indisponibles dans l’aperçu'} />
      </div>
      <footer className="briefing-footer"><span><ShieldCheck size={15} />Données locales par défaut.</span><button className="primary-button" onClick={() => dialogRef.current?.close()}>Reprendre la mission</button></footer>
    </dialog>;
}
function BriefingModule({ icon, name, description, state }: { icon: ReactNode; name: string; description: string; state: string }) {
  return <article className="briefing-module"><span className="briefing-module-icon" aria-hidden="true">{icon}</span><div><h3>{name}</h3><p>{description}</p></div><span className="briefing-module-state">{state}</span></article>;
}

function MetricBlock({ icon, label, value, detail, accent }: { icon: ReactNode; label: string; value: string; detail: string; accent: string }) {
  return <div className="metric-block"><div className="metric-label"><span className={`metric-mark ${accent}`}>{icon}</span><span>{label}</span></div><strong className={value === 'Non disponible' ? 'unavailable-value' : ''}>{value}</strong><span className="metric-detail">{detail}</span></div>;
}
function ModuleRow({ icon, name, detail, state, tone, onClick }: { icon: ReactNode; name: string; detail: string; state: string; tone: string; onClick: () => void }) {
  return <button className="module-row" onClick={onClick}><span className="module-icon">{icon}</span><span className="module-copy"><strong>{name}</strong><span>{detail}</span></span><span className={`module-state ${tone}`}>{state}</span><ChevronRight size={15} className="row-chevron" /></button>;
}
function EchoRecentRow({ entry }: { entry: { id: string; kind: 'text'|'url'|'image'; preview: string; createdAt: string } }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try { await orbitServices.echo?.copy(entry.id); setCopied(true); window.setTimeout(() => setCopied(false), 1500); }
    catch { setCopied(false); }
  };
  return <div className="echo-recent-row"><span className="echo-kind">{entry.kind === 'url' ? 'URL' : entry.kind === 'image' ? 'IMG' : 'TXT'}</span><span className="echo-preview" title={entry.preview}>{entry.preview}</span><button className="echo-copy" onClick={() => void copy()}>{copied ? 'Copié' : 'Copier'}</button></div>;
}
function FirstRunDialog({ native, onFinish }: { native: boolean; onFinish: (destination?: ModuleId) => Promise<void> }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [step, setStep] = useState(0);
  const [enableEcho, setEnableEcho] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => { if (dialog?.open) dialog.close(); };
  }, []);
  const finish = async (destination?: ModuleId) => {
    setBusy(true); setError('');
    try {
      if (enableEcho && native) await getEchoPort().setMonitoring(true);
      await onFinish(destination);
    } catch (reason) { setError(`Impossible de terminer la configuration : ${String(reason)}`); }
    finally { setBusy(false); }
  };
  return <dialog className="setup-dialog" ref={ref} aria-labelledby="setup-title">
    <p className="eyebrow">ORBIT OS <span>·</span> Première utilisation</p>
    {step === 0 && <><h2 id="setup-title">Bienvenue dans votre centre de commande</h2><p>ORBIT rassemble vos mesures système, l’historique local du presse-papiers et les automatisations dans une seule application.</p><p>Les modules fonctionnent localement sans compte.</p><div className="dialog-actions"><button className="primary-button" onClick={() => setStep(1)}>Continuer</button><button className="text-button" disabled={busy} onClick={() => void finish()}>Passer la configuration</button></div></>}
    {step === 1 && <><h2 id="setup-title">Activer Echo ?</h2><p>Echo conserve sur cet appareil les éléments que vous copiez. La surveillance est désactivée par défaut et peut être mise en pause à tout moment.</p><label className="setup-check"><input type="checkbox" checked={enableEcho} disabled={!native} onChange={event => setEnableEcho(event.target.checked)} /><span>Activer la surveillance du presse-papiers</span></label>{!native && <p className="setting-footnote">La capture est disponible uniquement dans l’application Windows.</p>}<div className="dialog-actions"><button className="text-button" onClick={() => setStep(0)}>Retour</button><button className="primary-button" disabled={busy} onClick={() => setStep(2)}>Continuer</button></div></>}
    {step === 2 && <><h2 id="setup-title">Configuration terminée</h2><p>ORBIT est prêt. Vous pourrez modifier vos préférences depuis Paramètres.</p>{error && <p className="setting-error" role="alert">{error}</p>}<div className="dialog-actions"><button className="text-button" onClick={() => setStep(1)}>Retour</button><button className="primary-button" disabled={busy} onClick={() => void finish()}>Terminer</button></div></>}
  </dialog>;
}
function ModuleUnavailable({ id, navigate }: { id: ModuleId; navigate: (id: ModuleId) => void }) {
  const description: Record<string, string> = { echo: 'Le mode aperçu navigateur n’accède pas au presse-papiers Windows. Ouvrez la version de bureau pour utiliser la capture locale.', pulsar: 'Les automatisations nécessitent l’application Windows native.', timeline: 'Le journal affiche les événements réels des modules activés.' };
  return <section className="page module-state-page" aria-labelledby="page-title"><div className="page-heading"><div><p className="eyebrow">Project Nebula <span>·</span> Module</p><h1 id="page-title">{titles[id]}</h1><p className="page-subtitle">{description[id] ?? 'Ce module n’est pas encore disponible.'}</p></div></div><div className="module-empty panel"><div className="module-empty-mark"><Sparkles size={20} /></div><h2>{id === 'echo' || id === 'pulsar' ? 'Version Windows requise' : 'Module en intégration'}</h2><p>{description[id]}</p><button className="primary-button" onClick={() => navigate('settings')}>Ouvrir les paramètres</button></div></section>;
}
function CorePage({ snapshot, native }: { snapshot?: SystemSnapshot; native: boolean }) {
  const [history, setHistory] = useState<CoreHistoryPoint[]>([]);
  useEffect(() => {
    let active = true;
    const load = () => { void coreService.history().then(value => { if (active) setHistory(value); }); };
    load();
    const unsubscribe = events.on('core.snapshot', load);
    return () => { active = false; unsubscribe(); };
  }, []);
  return <section className="page orbit-module-page" aria-labelledby="page-title"><div className="page-heading orbit-module-heading"><div><p className="eyebrow">SYSTÈME</p><h1 id="page-title">Core</h1><p className="page-subtitle">Mesures système fournies par les capteurs accessibles à ORBIT.</p></div><span className={`tag${native ? ' tag-on' : ''}`}><span className="status-dot" />{native ? 'Collecte locale' : 'Mode aperçu'}</span></div>
    <div className="core-sections orbit-module-content"><section className="panel detail-panel"><div className="panel-heading"><div><h2>Processeur</h2><p>Charge et caractéristiques</p></div><Gauge size={17} className="muted-icon" /></div><DataRow label="Modèle" value={snapshot?.cpu.name.value ?? 'Non disponible'} /><DataRow label="Utilisation" value={formatMetric(snapshot?.cpu.usagePercent, '%')} /><DataRow label="Cœurs / threads" value={`${formatMetric(snapshot?.cpu.cores)} / ${formatMetric(snapshot?.cpu.threads)}`} /></section>
    <section className="panel detail-panel"><div className="panel-heading"><div><h2>Mémoire vive</h2><p>Capacité mesurée par le système</p></div><Database size={17} className="muted-icon" /></div><DataRow label="Total" value={formatBytes(snapshot?.memory.totalBytes.value)} /><DataRow label="Utilisée" value={formatBytes(snapshot?.memory.usedBytes.value)} /><DataRow label="Disponible" value={formatBytes(snapshot?.memory.availableBytes.value)} /></section>
    <section className="panel detail-panel"><div className="panel-heading"><div><h2>Stockage</h2><p>Volumes locaux disponibles</p></div><HardDrive size={17} className="muted-icon" /></div>{snapshot?.disks.length ? snapshot.disks.map(disk => <DataRow key={disk.mount} label={disk.mount} value={`${formatBytes(disk.freeBytes.value)} libres / ${formatBytes(disk.totalBytes.value)}`} />) : <div className="unavailable-row">{native ? 'Aucun volume accessible.' : 'Les volumes sont disponibles dans la version Windows native.'}</div>}</section>
    <section className="panel detail-panel"><div className="panel-heading"><div><h2>GPU et réseau</h2><p>État des sources de données</p></div><Activity size={17} className="muted-icon" /></div><DataRow label="GPU" value={snapshot?.gpu.name.value ?? 'Non disponible'} /><DataRow label="Activité GPU" value={formatMetric(snapshot?.gpu.usagePercent, '%')} /><DataRow label="Entrant" value={formatRate(snapshot?.network.receivedBytesPerSecond.value)} /><DataRow label="Sortant" value={formatRate(snapshot?.network.sentBytesPerSecond.value)} /></section></div>
    <div className="core-history-grid"><HistoryChart title="Charge processeur" points={history} field="cpuPercent" color="var(--color-data-primary)" /><HistoryChart title="Mémoire utilisée" points={history} field="memoryPercent" color="var(--color-data-secondary)" /></div>
  </section>;
}
function DataRow({ label, value }: { label: string; value: string }) { return <div className="data-row"><span>{label}</span><strong>{value}</strong></div>; }
function HistoryChart({ title, points, field, color }: { title: string; points: CoreHistoryPoint[]; field: 'cpuPercent'|'memoryPercent'; color: string }) {
  const values = points.map(point => point[field]);
  const availableValues = values.filter((value): value is number => value !== null && Number.isFinite(value));
  const last = availableValues.at(-1);
  const coordinates = values.map((value, index) => value === null ? null : `${values.length < 2 ? 210 : 32 + index * 360 / (values.length - 1)},${106 - Math.max(0, Math.min(100, value))}`).filter((value): value is string => value !== null).join(' ');
  const startAt = points[0]?.at ? new Date(points[0].at).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' }) : '';
  const endAt = points.at(-1)?.at ? new Date(points.at(-1)!.at).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' }) : '';
  return <section className="panel history-panel"><div className="panel-heading"><div><h2>{title}</h2><p>{availableValues.length > 1 ? `Dernière valeur · ${Math.round(last!)} % · ${points.length} points réels` : 'En attente de mesures successives'}</p></div>{last !== undefined && <strong className="history-current">{Math.round(last)} %</strong>}</div>
    {availableValues.length > 1 ? <><svg className="history-chart" viewBox="0 0 400 118" role="img" aria-label={`${title}: ${Math.round(last!)} pour cent sur ${points.length} mesures réelles`}><text x="0" y="10">100%</text><text x="0" y="35">75%</text><text x="0" y="60">50%</text><text x="0" y="85">25%</text><text x="4" y="110">0%</text><line x1="32" y1="6" x2="392" y2="6" /><line x1="32" y1="31" x2="392" y2="31" /><line x1="32" y1="56" x2="392" y2="56" /><line x1="32" y1="81" x2="392" y2="81" /><line x1="32" y1="106" x2="392" y2="106" /><polyline points={coordinates} style={{ stroke: color }} /></svg><div className="chart-range"><span>{startAt}</span><span>{endAt}</span></div></> : <div className="chart-empty">La courbe apparaîtra après plusieurs relevés disponibles.</div>}
  </section>;
}
function SettingsPage({ settings, update }: { settings?: OrbitSettings; update: (patch: Partial<OrbitSettings>) => Promise<void> }) {
  const [saved, setSaved] = useState(false);
  const [accentError, setAccentError] = useState('');
  const [surfaceError, setSurfaceError] = useState('');
  const currentVisualization = settings?.orbitalVisualization ?? 'earth';
  const [previewVisualization, setPreviewVisualization] = useState<OrbitalVisualization>(currentVisualization);
  const [previewStatus, setPreviewStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  useEffect(() => setPreviewStatus('loading'), [previewVisualization]);
  const [autoStart, setAutoStart] = useState(false);
  const [autoStartBusy, setAutoStartBusy] = useState(false);
  const [autoStartError, setAutoStartError] = useState('');
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [appVersionState, setAppVersionState] = useState<'loading' | 'ready' | 'preview' | 'error'>(() => window.__TAURI_INTERNALS__ ? 'loading' : 'preview');
  useEffect(() => {
    if (!window.__TAURI_INTERNALS__) return;
    void isAutostartEnabled().then(setAutoStart).catch(error => setAutoStartError(`Impossible de lire le démarrage Windows : ${String(error)}`));
  }, []);
  useEffect(() => {
    if (!window.__TAURI_INTERNALS__) { setAppVersionState('preview'); return; }
    let active = true;
    void getVersion().then(version => { if (active) { setAppVersion(version); setAppVersionState('ready'); } }).catch(() => { if (active) setAppVersionState('error'); });
    return () => { active = false; };
  }, []);
  useEffect(() => setPreviewVisualization(currentVisualization), [currentVisualization]);
  const changeAutoStart = async (enabled: boolean) => {
    setAutoStartBusy(true); setAutoStartError('');
    try { if (enabled) await enableAutostart(); else await disableAutostart(); setAutoStart(await isAutostartEnabled()); }
    catch (error) { setAutoStartError(`Impossible de modifier le démarrage Windows : ${String(error)}`); }
    finally { setAutoStartBusy(false); }
  };
  const widgetLabels: Record<string, string> = { system: 'État du système', modules: 'Modules', orbit: 'Scène orbitale', activity: 'Echo et activité' };
  const orbitalOptions: Array<{ id: OrbitalVisualization; label: string; detail: string }> = [
    { id: 'earth', label: 'Terre', detail: 'Reliefs, continents, nuages et atmosphère' },
    { id: 'ringed-planet', label: 'Monde aux anneaux', detail: 'Planète striée, anneaux et satellite' },
    { id: 'lunar-station', label: 'Lune & station', detail: 'Lune cratérisée et station orbitale' },
    { id: 'nebula', label: 'Nébuleuse', detail: 'Nuage plasma et champ d’étoiles' },
    { id: 'core', label: 'Noyau', detail: 'Noyau facetté et trois anneaux' },
    { id: 'wireframe', label: 'Filaire', detail: 'Méridiens, parallèles et deux orbites' },
    { id: 'constellations', label: 'Constellations', detail: 'Étoiles reliées en profondeur 3D' },
  ];
  const themeOptions: Array<{ id: OrbitThemeId; label: string; detail: string }> = [
    { id: 'sidereal', label: 'Sidéral', detail: 'Noir profond · ivoire chaud' },
    { id: 'nebula', label: 'Nébuleuse', detail: 'Violet et plasma' },
    { id: 'obsidian', label: 'Obsidienne', detail: 'Noir franc · blanc net' },
    { id: 'lunar', label: 'Lunaire', detail: 'Ivoire · cuivre et olive' },
    { id: 'aurora', label: 'Aurore', detail: 'Forêt nocturne · jade' },
    { id: 'solar', label: 'Solaire', detail: 'Ocre profond · ambre' },
    { id: 'abyss', label: 'Abysses', detail: 'Noir océanique · cyan' },
  ];
  const currentOrder = settings?.widgetOrder ?? ['system', 'orbit', 'modules', 'activity'];
  const moveWidget = async (id: string, offset: -1 | 1) => {
    const index = currentOrder.indexOf(id);
    const destination = index + offset;
    if (index < 0 || destination < 0 || destination >= currentOrder.length) return;
    const next = [...currentOrder];
    [next[index], next[destination]] = [next[destination], next[index]];
    await change({ widgetOrder: next });
  };
  const toggleWidget = async (id: string, visible: boolean) => {
    const hidden = new Set(settings?.hiddenWidgets ?? []);
    if (visible) hidden.delete(id); else hidden.add(id);
    await change({ hiddenWidgets: [...hidden] });
  };
  const change = async (patch: Partial<OrbitSettings>) => { await update(patch); setSaved(true); window.setTimeout(() => setSaved(false), 1800); };
  const setCustomAccent = async (accentColor: string) => {
    if (!isAccessibleAccentColor(accentColor, settings?.theme ?? 'sidereal')) { setAccentError(settings?.theme === 'lunar' ? 'Cette couleur manque de contraste sur le fond clair. Choisissez une teinte plus sombre.' : 'Cette couleur manque de contraste avec le thème actif. Choisissez une teinte plus claire.'); return; }
    setAccentError('');
    await change({ accentColor });
  };
  const setSurfaceColor = async (key: 'panelColor' | 'raisedColor', color: string) => {
    if (!isAccessibleSurfaceColor(color, settings?.theme === 'lunar')) { setSurfaceError(settings?.theme === 'lunar' ? 'Cette surface rendrait le texte moins lisible. Choisissez une teinte plus claire.' : 'Cette surface rendrait le texte moins lisible. Choisissez une teinte plus sombre.'); return; }
    setSurfaceError('');
    await change(key === 'panelColor' ? { panelColor: color } : { raisedColor: color });
  };
  const surfaceDefault = (key: 'panelColor' | 'raisedColor') => settings?.[key] || (key === 'panelColor'
    ? settings?.theme === 'nebula' ? '#19172c' : settings?.theme === 'obsidian' ? '#131615' : settings?.theme === 'lunar' ? '#fffdf8' : settings?.theme === 'aurora' ? '#142a25' : settings?.theme === 'solar' ? '#332217' : settings?.theme === 'abyss' ? '#0d222b' : '#161c19'
    : settings?.theme === 'nebula' ? '#24213a' : settings?.theme === 'obsidian' ? '#1c201e' : settings?.theme === 'lunar' ? '#f4efe5' : settings?.theme === 'aurora' ? '#1c3831' : settings?.theme === 'solar' ? '#422c1c' : settings?.theme === 'abyss' ? '#14313b' : '#202723');
  return <section className="page orbit-module-page" aria-labelledby="page-title"><div className="page-heading orbit-module-heading"><div><p className="eyebrow">PRÉFÉRENCES</p><h1 id="page-title">Paramètres</h1><p className="page-subtitle">Réglages de cette application.</p></div>{saved && <span className="saved-state" role="status">Enregistré</span>}</div>
    <div className="settings-sections orbit-module-content"><section className="panel settings-panel"><div className="panel-heading"><div><h2>Apparence</h2><p>Thème, couleurs et mouvement</p></div></div>
      <fieldset className="theme-fieldset"><legend>Thème de l’application</legend><div className="theme-options">{themeOptions.map(option => <label className={`theme-option${(settings?.theme ?? 'sidereal') === option.id ? ' selected' : ''}`} key={option.id}><input type="radio" name="orbit-theme" value={option.id} checked={(settings?.theme ?? 'sidereal') === option.id} onChange={() => void change({ theme: option.id })} /><span className={`theme-swatch ${option.id}`} aria-hidden="true"><i /></span><span><strong>{option.label}</strong><small>{option.detail}</small></span></label>)}</div></fieldset>
      <fieldset className="accent-fieldset"><legend>Couleur d’accent</legend><p id="accent-help">Préréglages et couleurs personnalisées; le contraste minimum est vérifié.</p><div className="accent-options" role="group" aria-label="Couleur d’accent">{ORBIT_ACCENT_PRESETS.map(option => <button type="button" key={option.value || 'theme'} className={`accent-option${(settings?.accentColor ?? '') === option.value ? ' selected' : ''}`} aria-pressed={(settings?.accentColor ?? '') === option.value} onClick={() => { setAccentError(''); void change({ accentColor: option.value }); }}><i className={option.value ? '' : 'theme-accent-swatch'} style={option.value ? { backgroundColor: option.value } : undefined} aria-hidden="true" /><span>{option.label}</span></button>)}<label className={`accent-option accent-custom${settings?.accentColor && !ORBIT_ACCENT_PRESETS.some(option => option.value === settings.accentColor) ? ' selected' : ''}`}><i style={settings?.accentColor && !ORBIT_ACCENT_PRESETS.some(option => option.value === settings.accentColor) ? { backgroundColor: settings.accentColor } : undefined} aria-hidden="true" /><span>Personnalisée</span><input type="color" aria-label="Choisir une couleur d’accent personnalisée" aria-describedby="accent-help accent-error" value={settings?.accentColor || (settings?.theme === 'nebula' ? '#75ddd0' : settings?.theme === 'obsidian' ? '#e8ece6' : settings?.theme === 'lunar' ? '#805129' : settings?.theme === 'aurora' ? '#a8dfad' : settings?.theme === 'solar' ? '#f0bc72' : settings?.theme === 'abyss' ? '#66cbd0' : '#d6b978')} onChange={event => void setCustomAccent(event.target.value)} /></label></div>{accentError && <p id="accent-error" className="accent-error" role="alert">{accentError}</p>}</fieldset>
      <fieldset className="surface-colors-fieldset"><legend>Couleurs des cartes</legend><p id="surface-colors-help">Réglez les surfaces principales et surélevées. Le contraste est vérifié selon le thème actif; Réinitialiser reprend la couleur du thème.</p><div className="surface-picker-grid">
        {([['panelColor', 'Carte principale'], ['raisedColor', 'Carte surélevée']] as const).map(([key, label]) => <div className="surface-picker" key={key}><label htmlFor={`surface-${key}`}>{label}</label><div className="surface-picker-control"><span className="surface-preview" aria-hidden="true" style={{ backgroundColor: surfaceDefault(key) }} /><input id={`surface-${key}`} type="color" value={surfaceDefault(key)} aria-describedby="surface-colors-help surface-colors-error" onChange={event => void setSurfaceColor(key, event.target.value)} /><output htmlFor={`surface-${key}`}>{settings?.[key] || 'Couleur du thème'}</output><button type="button" className="text-button" disabled={!settings?.[key]} onClick={() => { setSurfaceError(''); void change(key === 'panelColor' ? { panelColor: '' } : { raisedColor: '' }); }}>Réinitialiser</button></div></div>)}
      </div>{surfaceError && <p id="surface-colors-error" className="accent-error" role="alert">{surfaceError}</p>}</fieldset>
      <SettingToggle title="Réduire les animations" detail="Suit aussi le réglage de mouvement réduit de Windows." checked={settings?.reduceMotion ?? false} onChange={value => void change({ reduceMotion: value })} /><SettingToggle title="Mode immersif" detail="Masquer la navigation." checked={settings?.immersive ?? false} onChange={value => void change({ immersive: value })} />
      <fieldset className="visualization-fieldset"><legend>Scène animée de Mission Control</legend><p>Choisissez une scène pour en voir l’aperçu. Elle ne remplace la scène actuelle qu’après validation.</p>
        <div className="scene-preference-layout">
          <div className="visualization-options" role="group" aria-label="Scènes à prévisualiser">{orbitalOptions.map(option => <button type="button" className={`visualization-option${previewVisualization === option.id ? ' selected' : ''}`} key={option.id} aria-pressed={previewVisualization === option.id} onClick={() => { setPreviewVisualization(option.id); setPreviewStatus('loading'); }}><span className={`visualization-swatch ${option.id}`} aria-hidden="true"><i /></span><span className="visualization-option-copy"><strong>{option.label}{currentVisualization === option.id && <small className="scene-current-badge">Scène actuelle</small>}</strong><small>{option.detail}</small></span></button>)}</div>
          <section className="scene-preview-card" aria-label={`Aperçu : ${orbitalOptions.find(option => option.id === previewVisualization)?.label ?? 'scène'}`}>
            <div className="scene-preview-stage">
              {(!settings?.orbitalEnabled || previewStatus === 'error') && <span className={`visualization-swatch scene-preview-placeholder ${previewVisualization}`} aria-hidden="true"><i /></span>}
              {settings?.orbitalEnabled !== false && <OrbitalThreeScene key={previewVisualization} visualization={previewVisualization} reduceMotion={settings?.reduceMotion ?? false} paused={false} rotationSpeed={settings?.orbitalRotationSpeed ?? 1} particlesEnabled={settings?.orbitalParticlesEnabled ?? true} intensity={settings?.orbitalIntensity ?? 'balanced'} onStatus={setPreviewStatus} />}
              <p className="scene-preview-status" role="status" aria-live="polite">{settings?.orbitalEnabled === false ? 'Rendu 3D désactivé · aperçu stylisé' : previewStatus === 'loading' ? 'Chargement de l’aperçu…' : previewStatus === 'error' ? '3D indisponible · aperçu stylisé affiché' : settings?.reduceMotion ? 'Aperçu fixe · mouvement réduit' : 'Aperçu animé'}</p>
            </div>
            <div className="scene-preview-actions"><div><strong>{orbitalOptions.find(option => option.id === previewVisualization)?.label}</strong><span>{previewVisualization === currentVisualization ? 'Déjà utilisée dans vos préférences' : 'Votre scène actuelle reste inchangée'}</span></div><button type="button" className="primary-button" disabled={previewVisualization === currentVisualization} onClick={() => void change({ orbitalVisualization: previewVisualization })}>{previewVisualization === currentVisualization ? 'Scène actuelle' : 'Utiliser cette scène'}</button></div>
          </section>
        </div>
        <div className="scene-motion-controls">
          <label className="scene-toggle"><span><strong>Activer la scène 3D</strong><small>La scène choisie s’affiche dans Mission Control et dans cet aperçu.</small></span><input type="checkbox" checked={settings?.orbitalEnabled ?? true} onChange={event => void change({ orbitalEnabled: event.target.checked })} /></label>
          <div className="setting-row"><div><strong>Vitesse de rotation</strong><span>Réglage de l’animation orbitale.</span></div><select aria-label="Vitesse de rotation de la scène" value={settings?.orbitalRotationSpeed ?? 1} onChange={event => void change({ orbitalRotationSpeed: Number(event.target.value) as 0.5 | 1 | 1.5 })}><option value="0.5">Lente</option><option value="1">Normale</option><option value="1.5">Rapide</option></select></div>
          <label className="scene-toggle"><span>Particules d’arrière-plan</span><input type="checkbox" checked={settings?.orbitalParticlesEnabled ?? true} onChange={event => void change({ orbitalParticlesEnabled: event.target.checked })} /></label>
          <div className="setting-row"><div><strong>Intensité visuelle</strong><span>Réduit ou augmente l’éclat et les particules.</span></div><select aria-label="Intensité de la scène 3D" value={settings?.orbitalIntensity ?? 'balanced'} onChange={event => void change({ orbitalIntensity: event.target.value as OrbitSettings['orbitalIntensity'] })}><option value="low">Douce</option><option value="balanced">Équilibrée</option><option value="high">Intense</option></select></div>
        </div>
      </fieldset>
    </section>
    <section className="panel settings-panel"><div className="panel-heading"><div><h2>Général</h2><p>Démarrage de l’application</p></div></div><div className="setting-row toggle-row"><div><strong>Démarrer avec Windows</strong><span>ORBIT démarre à la connexion Windows.</span>{autoStartError && <span className="setting-error" role="alert">{autoStartError}</span>}</div><input type="checkbox" checked={autoStart} disabled={!window.__TAURI_INTERNALS__ || autoStartBusy} onChange={event => void changeAutoStart(event.target.checked)} aria-label="Démarrer ORBIT avec Windows" /></div>{!window.__TAURI_INTERNALS__ && <p className="setting-footnote">Disponible dans la version Windows native; désactivé par défaut.</p>}</section>
    <section className="panel settings-panel"><div className="panel-heading"><div><h2>Core</h2><p>Collecte des métriques système</p></div></div><div className="setting-row"><div><strong>Fréquence d’actualisation</strong><span>Intervalle entre deux mesures système.</span></div><select value={settings?.coreRefreshSeconds ?? 2} onChange={event => void change({ coreRefreshSeconds: Number(event.target.value) as 2 | 5 | 10 })}><option value="2">2 secondes</option><option value="5">5 secondes</option><option value="10">10 secondes</option></select></div><div className="setting-row"><div><strong>Conservation de l’historique</strong><span>Durée de conservation locale des relevés.</span></div><select value={settings?.coreHistoryMinutes ?? 15} onChange={event => void change({ coreHistoryMinutes: Number(event.target.value) as 5 | 15 | 60 })}><option value="5">5 minutes</option><option value="15">15 minutes</option><option value="60">60 minutes</option></select></div><SettingNumber title="Alerte processeur" detail="Charge CPU à partir de ce niveau." value={settings?.alerts.cpuPercent ?? 90} min={10} max={100} unit="%" onChange={value => void change({ alerts: { ...(settings?.alerts ?? { cpuPercent: 90, memoryPercent: 90, diskFreePercent: 10 }), cpuPercent: value } })} /><SettingNumber title="Alerte mémoire" detail="Utilisation RAM à partir de ce niveau." value={settings?.alerts.memoryPercent ?? 90} min={10} max={100} unit="%" onChange={value => void change({ alerts: { ...(settings?.alerts ?? { cpuPercent: 90, memoryPercent: 90, diskFreePercent: 10 }), memoryPercent: value } })} /><SettingNumber title="Alerte espace disque libre" detail="Alerte quand l’espace libre descend sous ce seuil." value={settings?.alerts.diskFreePercent ?? 10} min={1} max={50} unit="%" onChange={value => void change({ alerts: { ...(settings?.alerts ?? { cpuPercent: 90, memoryPercent: 90, diskFreePercent: 10 }), diskFreePercent: value } })} /></section>
    <section className="panel settings-panel"><div className="panel-heading"><div><h2>Timeline</h2><p>Conservation du journal local</p></div><History size={17} className="muted-icon" /></div><div className="setting-row"><div><strong>Durée de conservation</strong><span>Le journal est stocké uniquement sur cet appareil.</span></div><select value={settings?.timelineRetentionDays ?? 30} onChange={event => void change({ timelineRetentionDays: Number(event.target.value) as 7 | 30 | 90 })}><option value="7">7 jours</option><option value="30">30 jours</option><option value="90">90 jours</option></select></div></section>
    <section className="panel settings-panel"><div className="panel-heading"><div><h2>Mission Control</h2><p>Disposition des widgets</p></div><button className="text-button" onClick={() => void change({ widgetOrder: ['system', 'orbit', 'modules', 'activity'], hiddenWidgets: [] })}>Réinitialiser</button></div><div className="widget-settings-list">{currentOrder.map((id, index) => <div className="widget-setting-row" key={id}><label><input type="checkbox" checked={!(settings?.hiddenWidgets ?? []).includes(id)} onChange={event => void toggleWidget(id, event.target.checked)} /><span>{widgetLabels[id] ?? id}</span></label><div><button className="text-button" disabled={index === 0} aria-label={`Monter ${widgetLabels[id]}`} onClick={() => void moveWidget(id, -1)}>Monter</button><button className="text-button" disabled={index === currentOrder.length - 1} aria-label={`Descendre ${widgetLabels[id]}`} onClick={() => void moveWidget(id, 1)}>Descendre</button></div></div>)}</div><p className="setting-footnote">Réorganisez les widgets avec les commandes Monter et Descendre. Les tailles sont fixes dans cette version.</p></section>
    <section className="panel settings-panel"><div className="panel-heading"><div><h2>Confidentialité</h2><p>Politique des données ORBIT</p></div><ShieldCheck size={17} className="muted-icon" /></div><p className="privacy-copy">Les préférences sont conservées dans le stockage local de l’application. Aucun service externe ni télémétrie n’est activé par défaut. Les clés API ne doivent pas être enregistrées dans les préférences ordinaires.</p></section></div><div className="settings-maintenance"><MaintenancePage /></div><footer className="settings-footer"><span className="settings-app-version" aria-live="polite">{appVersionState === 'ready' && appVersion ? `Version ${appVersion}` : appVersionState === 'preview' ? 'Version disponible dans l’application Windows' : appVersionState === 'loading' ? 'Lecture de la version…' : 'Version native indisponible'}</span><span>Développé par Choco8exe</span></footer>
  </section>;
}
function TimelinePage({ records }: { records: TimelineRecord[] }) {
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState<TimelineRecord['category'] | 'all'>('all');
  const [visibleRecords, setVisibleRecords] = useState<TimelineRecord[]>(records);
  useEffect(() => {
    let active = true;
    void orbitServices.timeline.list({ query, category: category === 'all' ? undefined : category }).then(rows => { if (active) setVisibleRecords(rows); });
    return () => { active = false; };
  }, [records, query, category]);
  return <section className="page orbit-module-page" aria-labelledby="page-title"><div className="page-heading orbit-module-heading"><div><p className="eyebrow">JOURNAL LOCAL</p><h1 id="page-title">Timeline</h1><p className="page-subtitle">Activité conservée sur cet appareil.</p></div><button className="text-button" onClick={() => { if (window.confirm('Effacer tout le journal Timeline de cet appareil ?')) void orbitServices.timeline.clear(); }}>Effacer le journal</button></div>
    <section className="panel timeline-panel"><div className="timeline-toolbar"><label className="timeline-search"><span className="sr-only">Rechercher un événement</span><input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Rechercher dans Timeline" /></label><label className="timeline-filter"><span className="sr-only">Filtrer par module</span><select value={category} onChange={event => setCategory(event.target.value as TimelineRecord['category'] | 'all')}><option value="all">Tous les modules</option><option value="echo">Echo</option><option value="pulsar">Pulsar</option><option value="core">Core</option><option value="system">Système</option></select></label><span className="subtle-count">{visibleRecords.length} ÉVÉNEMENT{visibleRecords.length === 1 ? '' : 'S'}</span></div>
    {visibleRecords.length ? <div className="timeline-list">{visibleRecords.map(record => <article className="timeline-record" key={`${record.category}:${record.id}`}><span className={`timeline-marker ${record.category}`} /><div className="timeline-record-content"><strong>{record.title}</strong>{record.detail && <p>{record.detail}</p>}<span>{record.category.toUpperCase()} · {new Date(record.at).toLocaleString('fr-FR')}</span></div></article>)}</div> : <div className="empty-activity"><div className="empty-symbol"><History size={17} /></div><div><strong>{query ? 'Aucun résultat pour cette recherche' : 'Aucun événement enregistré'}</strong><span>{query ? 'Modifiez le texte ou le filtre.' : 'Les événements apparaîtront ici lorsqu’un module produira une activité.'}</span></div></div>}</section>
  </section>;
}
function SettingToggle({ title, detail, checked, onChange }: { title: string; detail: string; checked: boolean; onChange: (value: boolean) => void }) { return <label className="setting-row toggle-row"><span><strong>{title}</strong><span>{detail}</span></span><input type="checkbox" checked={checked} onChange={event => onChange(event.target.checked)} /></label>; }
function SettingNumber({ title, detail, value, min, max, unit, onChange }: { title: string; detail: string; value: number; min: number; max: number; unit: string; onChange: (value: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const parsed = Number(draft);
    if (!Number.isFinite(parsed)) { setDraft(String(value)); return; }
    onChange(Math.max(min, Math.min(max, parsed)));
  };
  return <label className="setting-row number-setting"><span><strong>{title}</strong><span>{detail}</span></span><span className="number-control"><input type="number" min={min} max={max} value={draft} onChange={event => setDraft(event.target.value)} onBlur={commit} /><span>{unit}</span></span></label>;
}
function available<T>(metric?: { value: T | null; availability: string }): metric is { value: T; availability: string } { return Boolean(metric && metric.availability === 'available' && metric.value !== null); }
function dashboardUnavailable(metric?: { detail?: string; availability: string }, native = false) { return native ? availabilityText(metric, true) : 'Windows requis'; }
function formatMetric(metric?: { value: number | null; availability: string }, suffix = '') { return available(metric) ? `${Math.round(metric.value * 10) / 10}${suffix}` : 'Non disponible'; }
function availabilityText(metric?: { detail?: string; availability: string }, native = false) { return metric?.detail ?? (native ? 'Capteur non disponible' : 'Version Windows native requise'); }
function formatRate(bytesPerSecond?: number | null) { return bytesPerSecond === null || bytesPerSecond === undefined ? 'Non disponible' : `${formatBytes(bytesPerSecond)}/s`; }
function formatBytes(bytes?: number | null) {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) return 'Non disponible';
  const units = ['o', 'Ko', 'Mo', 'Go', 'To']; let value = bytes; let index = 0;
  while (value >= 1024 && index < units.length - 1) { value /= 1024; index++; }
  return `${new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 1 }).format(value)} ${units[index]}`;
}
