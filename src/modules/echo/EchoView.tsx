import { useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import type { EchoDuplicateSet, EchoEntry, EchoEntryDetail, EchoEntryKind, EchoPort, EchoSearchHit, EchoSnapshot } from "./types";
import "./echo.css";

type Filter = "all" | EchoEntryKind | "favorites" | "pinned";
const MAX_EXCLUDED_APPS = 100;

function normalizeExcludedApp(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > 128 || /[\\/:\u0000-\u001f<>"|?*]/u.test(trimmed)) return null;
  let normalized = trimmed.toLowerCase();
  while (normalized.endsWith(".exe")) normalized = normalized.slice(0, -4);
  if (!normalized || normalized === "." || normalized === ".." || normalized.length > 128) return null;
  return normalized;
}

const timeLabel = (value: string) => new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium", timeStyle: "short",
}).format(new Date(value));

function kindLabel(kind: EchoEntryKind) {
  return kind === "url" ? "Lien" : kind === "image" ? "Image" : "Texte";
}

function EchoHeading({ actions }: { actions?: ReactNode }) {
  return <header className="orbit-module-heading">
    <div>
      <p className="eyebrow">PROJECT NEBULA <span>·</span> PRESSE-PAPIERS</p>
      <h1 id="echo-title">Echo</h1>
      <p className="page-subtitle">Retrouvez vos éléments copiés sur cet appareil.</p>
    </div>
    {actions && <div className="echo-header__actions">{actions}</div>}
  </header>;
}

function EntryRow({ entry, port, collections, onError, selected, onSelect, onDetails }: {
  entry: EchoEntry;
  port: EchoPort;
  collections: EchoSnapshot["collections"];
  onError: (message: string) => void;
  selected: boolean;
  onSelect: (id: string, selected: boolean) => void;
  onDetails: (id: string) => void;
}) {
  const run = async (action: () => Promise<void>) => {
    try { await action(); } catch (error) { onError(error instanceof Error ? error.message : "Action impossible."); }
  };
  return <article className="echo-entry">
    <input className="echo-entry__select" type="checkbox" checked={selected} onChange={event => onSelect(entry.id, event.target.checked)} aria-label="Sélectionner cette entrée" />
    <div className="echo-entry__kind" aria-hidden="true">{entry.kind === "image" ? "▧" : entry.kind === "url" ? "↗" : "T"}</div>
    <div className="echo-entry__body">
      <div className="echo-entry__meta">
        <span className="echo-kind">{kindLabel(entry.kind)}</span>
        <time dateTime={entry.createdAt}>{timeLabel(entry.createdAt)}</time>
        {entry.sourceApp && <span className="echo-source">{entry.sourceApp}</span>}
        <span className="echo-size">{entry.byteLength < 1024 ? `${entry.byteLength} o` : `${(entry.byteLength / 1024).toFixed(1)} Ko`}</span>
      </div>
      {entry.kind === "image" && entry.imageDataUrl
        ? <div className="echo-image-preview"><img src={entry.imageDataUrl} alt={entry.preview || "Image copiée"} /><span>{entry.preview || "Image copiée"}</span></div>
        : <p className={`echo-entry__preview${entry.kind === "text" ? " echo-entry__preview--code" : ""}`}>{entry.preview}</p>}
      {entry.collectionIds.length > 0 && <div className="echo-entry__collections">{entry.collectionIds.map(id => collections.find(c => c.id === id)?.name).filter(Boolean).map(name => <span key={name}>{name}</span>)}</div>}
    </div>
    <div className="echo-entry__actions" aria-label="Actions de l’entrée">
      <button type="button" onClick={() => onDetails(entry.id)} aria-label="Afficher le détail de cette entrée">Détail</button>
      <button type="button" onClick={() => void run(() => port.copyEntry(entry.id))} aria-label="Recopier cette entrée" title="Recopier">Copier</button>
      <button type="button" className={entry.favorite ? "is-active" : ""} onClick={() => void run(() => port.setFavorite(entry.id, !entry.favorite))} aria-label={entry.favorite ? "Retirer des favoris" : "Ajouter aux favoris"} aria-pressed={entry.favorite} title={entry.favorite ? "Retirer des favoris" : "Ajouter aux favoris"}>★</button>
      <button type="button" className={entry.pinned ? "is-active" : ""} onClick={() => void run(() => port.setPinned(entry.id, !entry.pinned))} aria-label={entry.pinned ? "Désépingler" : "Épingler"} aria-pressed={entry.pinned} title={entry.pinned ? "Désépingler" : "Épingler"}>⌖</button>
      <select aria-label="Ajouter à une collection" value="" onChange={event => event.target.value && void run(() => port.assignCollection(entry.id, event.target.value))}>
        <option value="">Collection…</option>
        {collections.map(collection => <option key={collection.id} value={collection.id}>{collection.name}</option>)}
      </select>
      <button type="button" className="echo-delete" onClick={() => void run(() => port.deleteEntry(entry.id))} aria-label="Supprimer cette entrée" title="Supprimer">×</button>
    </div>
  </article>;
}

function SearchResultRow({ hit, port, collections, onError, selected, onSelect, onDetails, onChanged }: {
  hit: EchoSearchHit;
  port: EchoPort;
  collections: EchoSnapshot["collections"];
  onError: (message: string) => void;
  selected: boolean;
  onSelect: (id: string, selected: boolean) => void;
  onDetails: (id: string) => void;
  onChanged: () => Promise<void>;
}) {
  const run = async (action: () => Promise<void>) => {
    try { await action(); await onChanged(); }
    catch (error) { onError(error instanceof Error ? error.message : "Action impossible."); }
  };
  return <article className="echo-entry">
    <input className="echo-entry__select" type="checkbox" checked={selected} onChange={event => onSelect(hit.id, event.target.checked)} aria-label="Sélectionner cette entrée" />
    <div className="echo-entry__kind" aria-hidden="true">{hit.kind === "image" ? "▧" : hit.kind === "url" ? "↗" : "T"}</div>
    <div className="echo-entry__body">
      <div className="echo-entry__meta"><span className="echo-kind">{kindLabel(hit.kind)}</span><time dateTime={hit.createdAt}>{timeLabel(hit.createdAt)}</time></div>
      <p className={`echo-entry__preview${hit.kind === "text" ? " echo-entry__preview--code" : ""}`}>{hit.preview}</p>
      {hit.collectionIds.length > 0 && <div className="echo-entry__collections">{hit.collectionIds.map(id => collections.find(collection => collection.id === id)?.name).filter(Boolean).map(name => <span key={name}>{name}</span>)}</div>}
    </div>
    <div className="echo-entry__actions" aria-label="Actions de l’entrée">
      <button type="button" onClick={() => onDetails(hit.id)} aria-label="Afficher le détail de cette entrée">Détail</button>
      <button type="button" onClick={() => void run(() => port.copyEntry(hit.id))} aria-label="Recopier cette entrée">Copier</button>
      <button type="button" className={hit.favorite ? "is-active" : ""} onClick={() => void run(() => port.setFavorite(hit.id, !hit.favorite))} aria-label={hit.favorite ? "Retirer des favoris" : "Ajouter aux favoris"} aria-pressed={hit.favorite}>★</button>
      <button type="button" className={hit.pinned ? "is-active" : ""} onClick={() => void run(() => port.setPinned(hit.id, !hit.pinned))} aria-label={hit.pinned ? "Désépingler" : "Épingler"} aria-pressed={hit.pinned}>⌖</button>
      <select aria-label="Ajouter à une collection" value="" onChange={event => event.target.value && void run(() => port.assignCollection(hit.id, event.target.value))}>
        <option value="">Collection…</option>{collections.map(collection => <option key={collection.id} value={collection.id}>{collection.name}</option>)}
      </select>
    </div>
  </article>;
}

export function EchoView({ port }: { port: EchoPort }) {
  const [snapshot, setSnapshot] = useState<EchoSnapshot | null>(null);
  const [detailEntryId, setDetailEntryId] = useState<string | null>(null);
  const [detail, setDetail] = useState<null | { status: "loading" } | { status: "loaded"; entry: EchoEntryDetail } | { status: "error"; message: string }>(null);
  const detailDialog = useRef<HTMLDialogElement>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [searchHits, setSearchHits] = useState<EchoSearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [selectedCollection, setSelectedCollection] = useState<string>("");
  const [newCollection, setNewCollection] = useState("");
  const [showSettings, setShowSettings] = useState(false);
  const [excludedAppDraft, setExcludedAppDraft] = useState("");
  const [excludedAppError, setExcludedAppError] = useState("");
  const [excludedAppStatus, setExcludedAppStatus] = useState("");
  const [savingExcludedApps, setSavingExcludedApps] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [duplicates, setDuplicates] = useState<EchoDuplicateSet[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(true);
  const normalizedQuery = query.trim();
  const queryRef = useRef(normalizedQuery);
  queryRef.current = normalizedQuery;

  useEffect(() => {
    let mounted = true;
    const refresh = (next: EchoSnapshot) => {
      if (mounted) { setSnapshot(next); setBusy(false); }
      const activeQuery = queryRef.current;
      if (mounted && activeQuery) {
        void port.search(activeQuery, { limit: 200 }).then(hits => {
          if (mounted && queryRef.current === activeQuery) setSearchHits(hits);
        }).catch(reason => {
          if (mounted && queryRef.current === activeQuery) setSearchError(reason instanceof Error ? reason.message : "La recherche Echo a échoué.");
        });
      }
    };
    void port.getSnapshot().then(refresh).catch(reason => { if (mounted) { setError(reason instanceof Error ? reason.message : "Impossible de charger Echo."); setBusy(false); } });
    const unsubscribe = port.subscribe(refresh);
    return () => { mounted = false; unsubscribe(); };
  }, [port]);

  useEffect(() => {
    const dialog = detailDialog.current;
    if (!detailEntryId || !dialog || dialog.open) return;
    dialog.showModal();
  }, [detailEntryId]);

  useEffect(() => {
    if (!detailEntryId) { setDetail(null); return; }
    let active = true;
    const requestedId = detailEntryId;
    setDetail({ status: "loading" });
    void port.getEntry(requestedId).then(entry => {
      if (!active) return;
      if (entry.id !== requestedId) throw new Error("Echo a renvoyé une entrée différente de celle demandée.");
      setDetail({ status: "loaded", entry });
    }).catch(reason => {
      if (active) setDetail({ status: "error", message: reason instanceof Error ? reason.message : "Impossible de charger le détail." });
    });
    return () => { active = false; };
  }, [port, detailEntryId]);

  useEffect(() => {
    setSelectedIds([]);
    if (!normalizedQuery) {
      setSearchHits([]);
      setSearchError("");
      setSearching(false);
      return;
    }
    const queryBytes = new TextEncoder().encode(normalizedQuery).length;
    if (queryBytes > 512) {
      setSearchHits([]);
      setSearching(false);
      setSearchError("La recherche est limitée à 512 octets.");
      return;
    }
    let active = true;
    setSearching(true);
    setSearchHits([]);
    setSearchError("");
    const timer = window.setTimeout(() => {
      void port.search(normalizedQuery, { limit: 200 }).then(hits => {
        if (active) setSearchHits(hits);
      }).catch(reason => {
        if (active) setSearchError(reason instanceof Error ? reason.message : "La recherche Echo a échoué.");
      }).finally(() => {
        if (active) setSearching(false);
      });
    }, 180);
    return () => { active = false; window.clearTimeout(timer); };
  }, [normalizedQuery, port]);

  const entries = useMemo(() => {
    return (snapshot?.entries ?? []).filter(entry => {
      const filterMatches = filter === "all" || (filter === "favorites" ? entry.favorite : filter === "pinned" ? entry.pinned : entry.kind === filter);
      const collectionMatches = !selectedCollection || entry.collectionIds.includes(selectedCollection);
      return filterMatches && collectionMatches;
    });
  }, [filter, selectedCollection, snapshot]);

  const visibleSearchHits = useMemo(() => searchHits.filter(hit => {
    const filterMatches = filter === "all" || (filter === "favorites" ? hit.favorite : filter === "pinned" ? hit.pinned : hit.kind === filter);
    const collectionMatches = !selectedCollection || hit.collectionIds.includes(selectedCollection);
    return filterMatches && collectionMatches;
  }), [filter, searchHits, selectedCollection]);

  const invoke = async (action: () => Promise<void>) => {
    setError("");
    try { await action(); } catch (reason) { setError(reason instanceof Error ? reason.message : "Action impossible."); }
  };
  const createCollection = async () => {
    const name = newCollection.trim();
    if (!name) return;
    await invoke(async () => { await port.createCollection(name); setNewCollection(""); });
  };
  const refreshSearchResults = async () => {
    const currentQuery = queryRef.current;
    if (!currentQuery) return;
    setSearching(true);
    setSearchError("");
    try {
      const [hits, nextSnapshot] = await Promise.all([
        port.search(currentQuery, { limit: 200 }),
        port.getSnapshot(),
      ]);
      setSnapshot(nextSnapshot);
      if (queryRef.current === currentQuery) setSearchHits(hits);
    } catch (reason) {
      if (queryRef.current === currentQuery) setSearchError(reason instanceof Error ? reason.message : "Impossible d’actualiser les résultats Echo.");
    } finally {
      if (queryRef.current === currentQuery) setSearching(false);
    }
  };
  const saveExcludedApps = async (next: string[]) => {
    setSavingExcludedApps(true);
    setExcludedAppError("");
    setExcludedAppStatus("Enregistrement des exclusions…");
    try {
      await port.setPreferences({ excludedApps: next });
      // The command resolves only after SQLite is updated. Reflect it locally only
      // after that succeeds; the native change event will refresh the full snapshot.
      setSnapshot(current => current ? {
        ...current,
        preferences: { ...current.preferences, excludedApps: next },
      } : current);
      setExcludedAppStatus("Liste enregistrée sur cet appareil.");
      return true;
    } catch (reason) {
      setExcludedAppError(reason instanceof Error ? reason.message : "Impossible d’enregistrer les exclusions.");
      return false;
    } finally {
      setSavingExcludedApps(false);
    }
  };
  const addExcludedApp = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!snapshot) return;
    const normalized = normalizeExcludedApp(excludedAppDraft);
    if (!normalized) {
      setExcludedAppStatus("");
      setExcludedAppError("Saisissez uniquement un nom de processus, par exemple chrome.exe, sans chemin ni caractère spécial.");
      return;
    }
    const current = snapshot.preferences.excludedApps;
    if (current.some(item => normalizeExcludedApp(item) === normalized)) {
      setExcludedAppError("");
      setExcludedAppStatus("Cette application est déjà exclue.");
      return;
    }
    if (current.length >= MAX_EXCLUDED_APPS) {
      setExcludedAppStatus("");
      setExcludedAppError(`La liste est limitée à ${MAX_EXCLUDED_APPS} applications. Retirez-en une avant d’en ajouter une autre.`);
      return;
    }
    if (await saveExcludedApps([...current, normalized])) setExcludedAppDraft("");
  };
  const removeExcludedApp = (name: string) => {
    if (!snapshot) return Promise.resolve(false);
    const current = snapshot.preferences.excludedApps;
    return saveExcludedApps(current.filter(item => normalizeExcludedApp(item) !== name));
  };
  const clearHistory = async () => {
    if (window.confirm("Supprimer définitivement tout l’historique Echo ? Cette action est irréversible.")) await invoke(() => port.clearHistory());
  };
  const toggleSelected = (id: string, checked: boolean) => setSelectedIds(current => checked ? [...new Set([...current, id])] : current.filter(item => item !== id));
  const deleteSelected = async () => {
    const count = selectedIds.length;
    if (!count || !window.confirm(`Supprimer ${count} entrée${count > 1 ? "s" : ""} sélectionnée${count > 1 ? "s" : ""} ? Cette action est irréversible.`)) return;
    await invoke(async () => { await port.deleteEntries(selectedIds); setSelectedIds([]); });
  };
  const detectDuplicates = async () => await invoke(async () => setDuplicates(await port.findDuplicates()));
  const removeDuplicates = async () => {
    const count = duplicates?.reduce((sum, group) => sum + Math.max(0, group.count - 1), 0) ?? 0;
    if (!count || !window.confirm(`Supprimer ${count} copie${count > 1 ? "s" : ""} en double ? L’entrée la plus récente de chaque groupe sera conservée.`)) return;
    await invoke(async () => { await port.removeDuplicates(); setDuplicates([]); });
  };
  const captureActive = Boolean(snapshot?.available && snapshot.preferences.monitoringEnabled);
  const activeDetail = detail?.status === "loaded" && detail.entry.id !== detailEntryId ? null : detail;

  if (busy && !snapshot) return <section className="orbit-module-page echo-page" aria-labelledby="echo-title"><EchoHeading /><div className="orbit-module-content echo-module-content"><p className="echo-message" role="status">Chargement de l’historique…</p></div></section>;
  if (!snapshot) return <section className="orbit-module-page echo-page" aria-labelledby="echo-title"><EchoHeading /><div className="orbit-module-content echo-module-content"><div className="echo-error" role="alert"><strong>Echo n’a pas pu démarrer.</strong><span>{error || "Le service local est indisponible. Vérifiez que l’application ORBIT est à jour, puis réessayez."}</span></div></div></section>;

  return <section className="orbit-module-page echo-page" aria-labelledby="echo-title">
    <EchoHeading actions={<>
      <button type="button" className="echo-secondary" onClick={() => setShowSettings(value => !value)} aria-expanded={showSettings}>Conservation</button>
      <button type="button" className="echo-secondary" onClick={() => void detectDuplicates()}>Vérifier les doublons</button>
      <button type="button" className="echo-secondary echo-clear" onClick={() => void clearHistory()} disabled={!snapshot.entries.length}>Effacer l’historique</button>
    </>} />
    <div className="orbit-module-content echo-module-content">

    {!snapshot.available && <div className="echo-unavailable" role="status"><strong>Capture indisponible</strong><span>{snapshot.unavailableReason || "Le service Windows n’est pas connecté."}</span></div>}
    {error && <div className="echo-error" role="alert"><span>{error}</span><button type="button" onClick={() => setError("")} aria-label="Fermer le message">Fermer</button></div>}

    <section className="echo-capture" aria-label="État de la capture">
      <div><span className={`echo-state-dot${captureActive ? " is-on" : ""}`} aria-hidden="true" />
        <div><strong>{captureActive ? "Capture active" : snapshot.available ? "Capture en pause" : "Capture indisponible"}</strong><p>{captureActive ? "Les éléments compatibles sont conservés sur cet appareil." : snapshot.available ? "Echo ne capture rien tant que vous ne l’activez pas." : "Les éléments déjà enregistrés restent accessibles; aucune nouvelle copie n’est capturée."}</p></div>
      </div>
      <button type="button" className={captureActive ? "echo-secondary" : "echo-primary"} disabled={!snapshot.available} onClick={() => void invoke(() => port.setMonitoring(!snapshot.preferences.monitoringEnabled))}>{!snapshot.available ? "Capture indisponible" : captureActive ? "Mettre en pause" : "Activer la capture"}</button>
    </section>

    {showSettings && <section className="echo-settings" aria-labelledby="echo-settings-title">
      <div className="echo-settings__title"><h2 id="echo-settings-title">Confidentialité et conservation</h2><p>Les réglages sont enregistrés localement.</p></div>
      <label className="echo-setting-toggle"><input type="checkbox" checked={snapshot.preferences.captureImages} onChange={event => void invoke(() => port.setPreferences({ captureImages: event.target.checked }))} /> Capturer les images du presse-papiers</label>
      <label>Conserver l’historique
        <select value={snapshot.preferences.retentionDays} onChange={event => void invoke(() => port.setPreferences({ retentionDays: Number(event.target.value) }))}>
          <option value={1}>1 jour</option><option value={7}>7 jours</option><option value={30}>30 jours</option><option value={90}>90 jours</option><option value={0}>Sans limite</option>
        </select>
      </label>
      <label>Taille maximale par entrée
        <select value={snapshot.preferences.maxEntryBytes} onChange={event => void invoke(() => port.setPreferences({ maxEntryBytes: Number(event.target.value) }))}>
          <option value={65536}>64 Ko</option><option value={262144}>256 Ko</option><option value={1000000}>1 Mo</option><option value={5000000}>5 Mo</option>
        </select>
      </label>
      <section className="echo-exclusions" aria-labelledby="echo-exclusions-title">
        <div className="echo-exclusions__heading">
          <div><h3 id="echo-exclusions-title">Applications exclues</h3><p>Echo ignore les copies provenant des processus sélectionnés.</p></div>
          <span>{snapshot.preferences.excludedApps.length} / {MAX_EXCLUDED_APPS}</span>
        </div>
        <p id="echo-exclusions-help" className="echo-exclusions__help">Saisissez le nom de l’exécutable, par exemple <code>chrome.exe</code>, sans chemin. Echo compare le propriétaire Windows du presse-papiers. Si Windows ne peut pas l’identifier et que cette liste n’est pas vide, la capture est ignorée par précaution.</p>
        <form className="echo-exclusions__form" onSubmit={event => void addExcludedApp(event)}>
          <label htmlFor="echo-excluded-app">Nom du processus</label>
          <div className="echo-exclusions__input-row">
            <input id="echo-excluded-app" type="text" name="excluded-app" autoComplete="off" spellCheck={false} maxLength={128} value={excludedAppDraft} onChange={event => { setExcludedAppDraft(event.target.value); setExcludedAppError(""); setExcludedAppStatus(""); }} aria-invalid={Boolean(excludedAppError)} aria-describedby={`echo-exclusions-help${excludedAppError ? " echo-exclusions-error" : ""}`} placeholder="chrome.exe" />
            <button type="submit" className="echo-secondary" disabled={savingExcludedApps || !snapshot.available}>Ajouter</button>
          </div>
        </form>
        {excludedAppError && <p id="echo-exclusions-error" className="echo-exclusions__error" role="alert">{excludedAppError}</p>}
        <p className="echo-exclusions__status" role="status" aria-live="polite">{excludedAppStatus}</p>
        {snapshot.preferences.excludedApps.length > 0
          ? <ul className="echo-exclusions__list">{snapshot.preferences.excludedApps.map(stored => {
            const normalized = normalizeExcludedApp(stored) ?? stored;
            return <li key={normalized}><code>{normalized}.exe</code><button type="button" className="echo-secondary" disabled={savingExcludedApps || !snapshot.available} onClick={() => void removeExcludedApp(normalized)} aria-label={`Retirer ${normalized}.exe des applications exclues`}>Retirer</button></li>;
          })}</ul>
          : <p className="echo-exclusions__empty">Aucune application exclue.</p>}
      </section>
    </section>}

    <div className="echo-toolbar">
      <label className="echo-search"><span aria-hidden="true">⌕</span><input type="search" maxLength={512} value={query} onChange={event => setQuery(event.target.value)} placeholder="Rechercher dans l’historique" aria-label="Rechercher dans l’historique" /></label>
      <div className="echo-filters" role="group" aria-label="Filtrer les entrées">
        {(["all", "text", "url", "image", "favorites", "pinned"] as Filter[]).map(value => <button key={value} type="button" aria-pressed={filter === value} className={filter === value ? "selected" : ""} onClick={() => setFilter(value)}>{({ all: "Tout", text: "Texte", url: "Liens", image: "Images", favorites: "Favoris", pinned: "Épinglés" })[value]}</button>)}
      </div>
      <label className="echo-collection-filter"><span className="sr-only">Collection</span><select value={selectedCollection} onChange={event => setSelectedCollection(event.target.value)}><option value="">Toutes les collections</option>{snapshot.collections.map(collection => <option key={collection.id} value={collection.id}>{collection.name}</option>)}</select></label>
    </div>

    <section className="echo-list" aria-label="Entrées du presse-papiers" aria-live="polite">
      <div className="echo-list__heading"><h2>Entrées</h2><span>{normalizedQuery ? visibleSearchHits.length : entries.length} résultat{(normalizedQuery ? visibleSearchHits.length : entries.length) > 1 ? "s" : ""}</span></div>
      {selectedIds.length > 0 && <div className="echo-bulkbar"><span>{selectedIds.length} sélectionnée{selectedIds.length > 1 ? "s" : ""}</span><button type="button" className="echo-secondary" onClick={() => setSelectedIds([])}>Annuler la sélection</button><button type="button" className="echo-secondary echo-clear" onClick={() => void deleteSelected()}>Supprimer la sélection</button></div>}
      {duplicates && <div className="echo-duplicates" role="status"><div><strong>{duplicates.length ? `${duplicates.length} groupe${duplicates.length > 1 ? "s" : ""} en double` : "Aucun doublon détecté"}</strong><p>{duplicates.length ? `Jusqu’à ${duplicates.reduce((sum, group) => sum + group.count - 1, 0)} copie(s) peuvent être supprimées. L’entrée la plus récente sera conservée.` : "L’historique ne contient pas de contenus identiques."}</p>{duplicates.slice(0, 4).map((group, index) => <small key={`${group.ids[0]}-${index}`}>{group.count} × {group.kind === "image" ? "Image" : group.preview || "Texte vide"}</small>)}</div>{duplicates.length > 0 && <button type="button" className="echo-secondary echo-clear" onClick={() => void removeDuplicates()}>Supprimer les doublons</button>}<button type="button" className="echo-icon-close" onClick={() => setDuplicates(null)} aria-label="Fermer le résultat">×</button></div>}
      {searchError && normalizedQuery && <div className="echo-error" role="alert"><span>{searchError}</span></div>}
      {searching && normalizedQuery && <p className="echo-message" role="status">Recherche dans le contenu local…</p>}
      {normalizedQuery
        ? visibleSearchHits.length ? visibleSearchHits.map(hit => <SearchResultRow key={hit.id} hit={hit} port={port} collections={snapshot.collections} onError={setError} selected={selectedIds.includes(hit.id)} onSelect={toggleSelected} onDetails={setDetailEntryId} onChanged={refreshSearchResults} />)
          : !searching && !searchError && <div className="echo-empty"><strong>Aucun résultat</strong><p>Aucune entrée ne correspond à « {normalizedQuery} » avec les filtres actuels.</p><button type="button" className="echo-secondary" onClick={() => setQuery("")}>Effacer la recherche</button></div>
        : entries.length ? entries.map(entry => <EntryRow key={entry.id} entry={entry} port={port} collections={snapshot.collections} onError={setError} selected={selectedIds.includes(entry.id)} onSelect={toggleSelected} onDetails={setDetailEntryId} />)
          : <div className="echo-empty"><strong>{snapshot.available ? "Votre historique est vide" : "Aucune entrée enregistrée"}</strong><p>{snapshot.available ? "Activez la capture pour conserver localement les éléments copiés." : "Le service de capture est indisponible. Vos éléments copiés ne sont pas enregistrés."}</p></div>}
    </section>

    <dialog ref={detailDialog} className="echo-detail-dialog" aria-labelledby="echo-detail-title" onClose={() => setDetailEntryId(null)}>
      <div className="echo-detail-dialog__heading"><h2 id="echo-detail-title">Détail de l’entrée</h2><button type="button" className="echo-secondary" onClick={() => detailDialog.current?.close()}>Fermer</button></div>
      {!activeDetail || activeDetail.status === "loading" ? <p role="status">Chargement du détail…</p>
        : activeDetail.status === "error" ? <p className="echo-detail-dialog__error" role="alert">{activeDetail.message}</p>
          : <>
            <div className="echo-detail-dialog__meta"><span>{kindLabel(activeDetail.entry.kind)}</span><span>{activeDetail.entry.byteLength < 1024 ? `${activeDetail.entry.byteLength} o` : `${(activeDetail.entry.byteLength / 1024).toFixed(1)} Ko`}</span></div>
            {activeDetail.entry.truncated && <p className="echo-detail-dialog__notice">L’aperçu est tronqué pour limiter les données chargées. Utilisez « Copier » dans la liste pour recopier l’entrée conservée localement.</p>}
            {activeDetail.entry.kind === "image"
              ? activeDetail.entry.imageDataUrl ? <img className="echo-detail-dialog__image" src={activeDetail.entry.imageDataUrl} alt="Aperçu de l’image copiée" /> : <p className="echo-detail-dialog__notice">Aucun aperçu image disponible pour cette entrée.</p>
              : <pre className="echo-detail-dialog__text">{activeDetail.entry.text ?? "Aucun texte disponible pour cette entrée."}</pre>}
          </>}
    </dialog>

    <section className="echo-collections" aria-labelledby="echo-collections-title">
      <div><h2 id="echo-collections-title">Collections</h2><p>Regroupez les éléments que vous voulez retrouver.</p></div>
      <div className="echo-collection-create"><label className="sr-only" htmlFor="echo-new-collection">Nom de la collection</label><input id="echo-new-collection" value={newCollection} onChange={event => setNewCollection(event.target.value)} onKeyDown={event => { if (event.key === "Enter") void createCollection(); }} placeholder="Nouvelle collection" maxLength={48} /><button type="button" className="echo-secondary" onClick={() => void createCollection()} disabled={!newCollection.trim()}>Créer</button></div>
      <div className="echo-collection-chips">{snapshot.collections.map(collection => <button key={collection.id} type="button" aria-pressed={selectedCollection === collection.id} className={selectedCollection === collection.id ? "selected" : ""} onClick={() => setSelectedCollection(selectedCollection === collection.id ? "" : collection.id)}>{collection.name}</button>)}</div>
    </section>
    </div>
  </section>;
}
