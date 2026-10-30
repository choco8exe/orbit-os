import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Search, X } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import './quick-actions-palette.css';

export interface QuickActionCommand {
  id: string;
  label: string;
  description: string;
  keywords?: string[];
  section: string;
  icon?: LucideIcon;
  closeOnRun?: boolean;
  focusRouteAfterRun?: boolean;
  run: () => void | Promise<void>;
}

export function QuickActionsPalette({ open, onClose, onRouteClose, commands, reduceMotion }: {
  open: boolean;
  onClose: () => void;
  onRouteClose?: () => void;
  commands: QuickActionCommand[];
  reduceMotion: boolean;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const routeCloseRef = useRef(false);
  const commandRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const [busyCommand, setBusyCommand] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase('fr-FR');
    if (!needle) return commands;
    return commands.filter(command => [command.label, command.description, command.section, ...(command.keywords ?? [])]
      .join(' ').toLocaleLowerCase('fr-FR').includes(needle));
  }, [commands, query]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      triggerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      setQuery(''); setError(''); setNotice(''); setActiveIndex(0);
      dialog.showModal();
      window.requestAnimationFrame(() => searchRef.current?.focus());
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  useEffect(() => {
    setActiveIndex(index => Math.max(0, Math.min(index, filtered.length - 1)));
  }, [filtered.length]);

  const close = () => {
    if (dialogRef.current?.open) dialogRef.current.close();
    onClose();
  };

  const restoreFocus = () => {
    const previous = triggerRef.current;
    onClose();
    if (routeCloseRef.current) {
      routeCloseRef.current = false;
      onRouteClose?.();
      return;
    }
    window.requestAnimationFrame(() => {
      if (previous?.isConnected) previous.focus();
    });
  };

  const run = async (command: QuickActionCommand) => {
    if (busyCommand) return;
    setBusyCommand(command.id); setError(''); setNotice('');
    try {
      await command.run();
      if (command.closeOnRun) {
        routeCloseRef.current = Boolean(command.focusRouteAfterRun);
        close();
      }
      else {
        setNotice(`${command.label} : terminé.`);
        setQuery(''); setActiveIndex(0);
        window.requestAnimationFrame(() => searchRef.current?.focus());
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Cette action n’a pas pu être exécutée.');
    } finally { setBusyCommand(null); }
  };

  const focusCommand = (index: number) => {
    if (!filtered.length) return;
    const next = (index + filtered.length) % filtered.length;
    setActiveIndex(next);
    commandRefs.current[next]?.focus();
  };

  const handleSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown' && filtered.length) { event.preventDefault(); focusCommand(activeIndex); }
    else if (event.key === 'ArrowUp' && filtered.length) { event.preventDefault(); focusCommand(activeIndex === 0 ? filtered.length - 1 : activeIndex - 1); }
    else if (event.key === 'Enter' && filtered[activeIndex]) { event.preventDefault(); void run(filtered[activeIndex]); }
    else if (event.key === 'Escape') { event.preventDefault(); close(); }
  };

  const handleCommandKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (event.key === 'ArrowDown') { event.preventDefault(); focusCommand(index + 1); }
    else if (event.key === 'ArrowUp') { event.preventDefault(); if (index === 0) searchRef.current?.focus(); else focusCommand(index - 1); }
    else if (event.key === 'Home') { event.preventDefault(); focusCommand(0); }
    else if (event.key === 'End') { event.preventDefault(); focusCommand(filtered.length - 1); }
    else if (event.key === 'Escape') { event.preventDefault(); close(); }
  };

  let lastSection = '';
  return <dialog ref={dialogRef} className={`quick-actions-dialog${reduceMotion ? ' reduce-motion' : ''}`} aria-labelledby="quick-actions-title" aria-describedby="quick-actions-description"
    onCancel={event => { event.preventDefault(); close(); }}
    onClose={restoreFocus}
    onClick={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="quick-actions-shell">
      <header className="quick-actions-heading">
        <div><p className="eyebrow">ORBIT · ACCÈS DIRECT</p><h2 id="quick-actions-title">Actions rapides</h2><p id="quick-actions-description">Cherche un module ou lance une commande locale.</p></div>
        <button className="icon-button quick-actions-close" type="button" aria-label="Fermer les actions rapides" onClick={close}><X size={17} /></button>
      </header>
      <label className="quick-actions-search" htmlFor="quick-actions-query">
        <Search size={17} aria-hidden="true" />
        <span className="sr-only">Rechercher une action</span>
        <input id="quick-actions-query" ref={searchRef} type="search" value={query} onChange={event => { setQuery(event.target.value); setActiveIndex(0); setError(''); setNotice(''); }} onKeyDown={handleSearchKeyDown} placeholder="Ex. Echo, Pulse, paramètres…" autoComplete="off" />
        <kbd>Esc</kbd>
      </label>
      <div className="quick-actions-results" role="list" aria-label="Commandes disponibles">
        {filtered.map((command, index) => {
          const showSection = command.section !== lastSection;
          const Icon = command.icon ?? Search;
          lastSection = command.section;
          return <div className="quick-actions-row" role="listitem" key={command.id}>
            {showSection && <p className="quick-actions-section">{command.section}</p>}
            <button ref={element => { commandRefs.current[index] = element; }} className={`quick-actions-item${index === activeIndex ? ' active' : ''}`} type="button" tabIndex={index === activeIndex ? 0 : -1} disabled={busyCommand !== null} onFocus={() => setActiveIndex(index)} onMouseMove={() => setActiveIndex(index)} onClick={() => void run(command)} onKeyDown={event => handleCommandKeyDown(event, index)}>
              <span className="quick-actions-item-icon" aria-hidden="true"><Icon size={16} /></span>
              <span className="quick-actions-item-copy"><strong>{command.label}</strong><small>{command.description}</small></span>
              <span className="quick-actions-item-enter" aria-hidden="true">↵</span>
            </button>
          </div>;
        })}
        {filtered.length === 0 && <p className="quick-actions-empty">Aucune commande ne correspond à « {query.trim()} ».</p>}
      </div>
      <div className="quick-actions-feedback" aria-live="polite">{notice}</div>
      <p className="quick-actions-error" role="alert">{error}</p>
      <footer className="quick-actions-footer"><span><kbd>↑</kbd><kbd>↓</kbd> parcourir</span><span><kbd>Entrée</kbd> exécuter</span><span>Commandes locales · aucune donnée envoyée au cloud</span></footer>
    </div>
  </dialog>;
}
