import { useEffect, useMemo, useState } from 'react';
import { FileMusic, FolderOpen, Plus, Search, Star, Tag, Trash2 } from 'lucide-react';
import type { ModuleId, ProjectShelfEntry } from '../shared/contracts';
import { orbitServices } from '../shared/services';
import { flStudioProjectProvider, selectFlStudioProjects } from '../plugins/fl-studio/provider';
import { studioNotesService, type StudioNote } from '../plugins/creator/service';

export function ProjectShelfPage({ navigate }: { navigate: (id: ModuleId) => void }) {
  const [entries, setEntries] = useState<ProjectShelfEntry[]>([]);
  const [query, setQuery] = useState('');
  const [tagDraft, setTagDraft] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [canSelect, setCanSelect] = useState(false);
  const [canOpen, setCanOpen] = useState(false);
  const [canManageNotes, setCanManageNotes] = useState(false);
  const [noteDraft, setNoteDraft] = useState<Pick<StudioNote, 'projectId' | 'style' | 'bpm' | 'musicalKey' | 'notes'> | null>(null);
  const [noteSaved, setNoteSaved] = useState(false);
  const load = async () => setEntries(await flStudioProjectProvider.list());
  useEffect(() => {
    let active = true;
    void Promise.all([load(), orbitServices.plugins.listStates()]).then(([, states]) => {
      if (!active) return;
      const plugin = states.find(item => item.pluginId === 'com.orbitos.fl-studio');
      setCanSelect(Boolean(plugin?.enabled && plugin.grantedPermissions.includes('project-shelf.select-project-files')));
      setCanOpen(Boolean(plugin?.enabled && plugin.grantedPermissions.includes('project-shelf.open-project')));
      setCanManageNotes(Boolean(plugin?.enabled && plugin.grantedPermissions.includes('fl-studio.session-notes')));
    }).catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, []);
  const filtered = useMemo(() => {
    const value = query.trim().toLocaleLowerCase('fr');
    return entries.filter(item => !value || `${item.name} ${item.path} ${item.tags.join(' ')}`.toLocaleLowerCase('fr').includes(value));
  }, [entries, query]);
  const mutate = async (operation: () => Promise<ProjectShelfEntry[]>): Promise<boolean> => {
    setBusy(true); setError('');
    try { setEntries(await operation()); return true; } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); return false; }
    finally { setBusy(false); }
  };
  const addProjects = async () => {
    setBusy(true); setError('');
    try {
      const paths = await selectFlStudioProjects();
      for (const path of paths) await flStudioProjectProvider.add(path);
      if (paths.length) await load();
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const addTag = async (entry: ProjectShelfEntry) => {
    const tag = tagDraft[entry.id]?.trim();
    if (!tag) return;
    if (await mutate(() => orbitServices.projectShelf!.addTag(entry.id, tag))) setTagDraft(current => ({ ...current, [entry.id]: '' }));
  };
  const openNotes = async (entry: ProjectShelfEntry) => {
    setBusy(true); setError(''); setNoteSaved(false);
    try {
      const saved = await studioNotesService.get(entry.id);
      setNoteDraft(saved ? { projectId: saved.projectId, style: saved.style, bpm: saved.bpm, musicalKey: saved.musicalKey, notes: saved.notes } : { projectId: entry.id, style: '', bpm: null, musicalKey: '', notes: '' });
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const saveNotes = async () => {
    if (!noteDraft) return;
    setBusy(true); setError(''); setNoteSaved(false);
    try { await studioNotesService.save(noteDraft); setNoteSaved(true); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const deleteNotes = async () => {
    if (!noteDraft) return;
    setBusy(true); setError(''); setNoteSaved(false);
    try { await studioNotesService.remove(noteDraft.projectId); setNoteDraft({ ...noteDraft, style: '', bpm: null, musicalKey: '', notes: '' }); setNoteSaved(true); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  return <section className="page orbit-module-page project-shelf-page" aria-labelledby="page-title">
    <div className="page-heading orbit-module-heading"><div><p className="eyebrow">PLUGIN LOCAL · FL STUDIO</p><h1 id="page-title">Project Shelf</h1><p className="page-subtitle">Vos projets, indexés localement. ORBIT ne lit pas leur contenu.</p></div><button type="button" className="primary-button" onClick={() => void addProjects()} disabled={busy || !canSelect || !flStudioProjectProvider.available} title={!canSelect ? 'Accordez la permission de sélection dans Plugin Center.' : undefined}><Plus size={16} />Ajouter des projets</button></div>
    <div className="project-shelf-toolbar panel"><label className="project-search"><Search size={16} /><span className="sr-only">Rechercher les projets et tags</span><input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Rechercher nom, chemin ou tag" /></label><span>{filtered.length} projet{filtered.length === 1 ? '' : 's'}</span></div>
    {error && <p className="plugin-error" role="alert">{error}</p>}
    {filtered.length ? <div className="project-list">{filtered.map(entry => <article className="project-card panel" key={entry.id}>
      <div className="project-card-heading"><div className="project-file-icon"><FolderOpen size={18} /></div><div className="project-file-title"><h2>{entry.name}</h2><p title={entry.path}>{entry.path}</p></div><button className={`icon-button project-pin${entry.pinned ? ' pinned' : ''}`} type="button" aria-label={entry.pinned ? `Désépingler ${entry.name}` : `Épingler ${entry.name}`} aria-pressed={entry.pinned} disabled={busy} onClick={() => void mutate(() => flStudioProjectProvider.setPinned(entry.id, !entry.pinned))}><Star size={16} /></button></div>
      <div className="project-metadata"><span>{entry.exists ? 'Disponible' : 'Fichier introuvable'}</span><span>{entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleDateString('fr-FR') : 'Date inconnue'}</span><span>{entry.sizeBytes == null ? 'Taille inconnue' : `${(entry.sizeBytes / 1024 / 1024).toLocaleString('fr-FR', { maximumFractionDigits: 1 })} Mo`}</span></div>
      <div className="project-tags"><Tag size={14} />{entry.tags.map(tag => <button key={tag} type="button" className="project-tag" aria-label={`Retirer le tag ${tag}`} disabled={busy} onClick={() => void mutate(() => orbitServices.projectShelf!.removeTag(entry.id, tag))}>{tag}<span aria-hidden="true">×</span></button>)}<form onSubmit={event => { event.preventDefault(); void addTag(entry); }}><label className="sr-only" htmlFor={`tag-${entry.id}`}>Ajouter un tag à {entry.name}</label><input id={`tag-${entry.id}`} value={tagDraft[entry.id] ?? ''} maxLength={32} placeholder="Ajouter un tag" onChange={event => setTagDraft(current => ({ ...current, [entry.id]: event.target.value }))} /><button type="submit" disabled={busy || !tagDraft[entry.id]?.trim()} aria-label={`Ajouter le tag à ${entry.name}`}>Ajouter</button></form></div>
      <div className="project-actions"><button type="button" className="secondary-button" disabled={busy || !entry.exists || !canOpen} title={!entry.exists ? 'Le fichier n’est plus accessible.' : !canOpen ? 'Accordez la permission d’ouverture dans Plugin Center.' : undefined} onClick={() => { setBusy(true); void flStudioProjectProvider.open(entry.id).then(() => setError('')).catch(cause => setError(String(cause))).finally(() => setBusy(false)); }}><FolderOpen size={15} />Ouvrir</button>{canManageNotes && <button type="button" className="secondary-button" disabled={busy} onClick={() => void openNotes(entry)}><FileMusic size={15} />Notes FL Studio</button>}{entry.exists && !canOpen && <span className="project-permission-help">Accordez « Ouvrir un projet sélectionné » dans Plugin Center.</span>}<button type="button" className="text-button danger-action" disabled={busy} onClick={() => void mutate(() => flStudioProjectProvider.remove(entry.id))}><Trash2 size={15} />Retirer de la bibliothèque</button></div>
    </article>)}</div> : <div className="plugin-empty panel"><FolderOpen size={22} /><h2>{query ? 'Aucun projet correspondant' : 'Votre bibliothèque est vide'}</h2><p>{query ? 'Essayez un autre nom ou tag.' : 'Ajoutez des fichiers .flp avec le sélecteur Windows. Le contenu des fichiers reste intact.'}</p>{!query && <><button type="button" className="primary-button" onClick={() => void addProjects()} disabled={busy || !canSelect || !flStudioProjectProvider.available}><Plus size={15} />Sélectionner des projets</button>{!canSelect && <p className="project-permission-help">Autorisez la sélection de fichiers dans Plugin Center pour ajouter un projet.</p>}</>}</div>}
    {noteDraft && canManageNotes && <section className="panel project-notes-panel" aria-labelledby="project-notes-title"><div className="panel-heading"><div><p className="eyebrow">NOTES LOCALES · FL STUDIO</p><h2 id="project-notes-title">Session musicale · {entries.find(entry => entry.id === noteDraft.projectId)?.name ?? 'Projet'}</h2></div><FileMusic size={18} /></div><p>Ces notes restent sur cet appareil.</p><div className="project-note-fields"><label>Style<input value={noteDraft.style} maxLength={48} onChange={event => setNoteDraft({ ...noteDraft, style: event.target.value })} placeholder="Ambient, house…" /></label><label>BPM<input type="number" min={20} max={400} value={noteDraft.bpm ?? ''} onChange={event => setNoteDraft({ ...noteDraft, bpm: event.target.value ? Number(event.target.value) : null })} placeholder="120" /></label><label>Tonalité<input value={noteDraft.musicalKey} maxLength={16} onChange={event => setNoteDraft({ ...noteDraft, musicalKey: event.target.value })} placeholder="La mineur" /></label><label className="project-note-wide">Idées<textarea value={noteDraft.notes} maxLength={4000} rows={4} onChange={event => setNoteDraft({ ...noteDraft, notes: event.target.value })} placeholder="Notes de session…" /></label></div><div className="project-actions"><button type="button" className="primary-button" disabled={busy} onClick={() => void saveNotes()}>Enregistrer les notes</button><button type="button" className="text-button danger-action" disabled={busy} onClick={() => void deleteNotes()}>Effacer</button>{noteSaved && <span role="status">Notes enregistrées localement.</span>}</div></section>}
    <button className="text-button" type="button" onClick={() => navigate('plugins')}>← Retour au Plugin Center</button>
  </section>;
}
