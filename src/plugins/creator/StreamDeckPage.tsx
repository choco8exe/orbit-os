import { useEffect, useState } from 'react';
import { Check, CircleHelp, Download, Orbit, ShieldCheck, X } from 'lucide-react';
import { orbitServices } from '../../shared/services';
import { streamDeckService, type StreamDeckPairingCode, type StreamDeckStatus } from './service';
import './creator.css';

const BRIDGE_PERMISSION = 'creator.streamdeck.bridge';
const PULSAR_PERMISSION = 'pulsar.streamdeck.control';
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export function StreamDeckPage({ navigate }: { navigate: (id: 'plugins') => void }) {
  const [status, setStatus] = useState<StreamDeckStatus | null>(null);
  const [pairingCode, setPairingCode] = useState<(StreamDeckPairingCode & { expiresAt: number }) | null>(null);
  const [secondsLeft, setSecondsLeft] = useState(0);
  const [authorized, setAuthorized] = useState(false);
  const [pulsarAuthorized, setPulsarAuthorized] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  useEffect(() => {
    let active = true;
    void orbitServices.plugins.listStates().then(async states => {
      if (!active) return;
      const plugin = states.find(item => item.pluginId === 'com.orbitos.creator-suite');
      const allowed = Boolean(plugin?.installed && plugin.enabled && plugin.grantedPermissions.includes(BRIDGE_PERMISSION));
      setPulsarAuthorized(Boolean(plugin?.installed && plugin.enabled && plugin.grantedPermissions.includes(PULSAR_PERMISSION)));
      setAuthorized(allowed);
      if (allowed) {
        try { const result = await streamDeckService.status(); if (active) setStatus(result); }
        catch (cause) { if (active) setError(errorText(cause)); }
      }
    }).catch(cause => { if (active) setError(errorText(cause)); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!pairingCode) { setSecondsLeft(0); return; }
    const update = () => {
      const left = Math.max(0, Math.ceil((pairingCode.expiresAt - Date.now()) / 1000));
      setSecondsLeft(left);
      if (!left) setPairingCode(null);
    };
    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [pairingCode]);

  const run = async (action: () => Promise<void>, success?: string) => {
    setBusy(true); setError(''); setNotice('');
    try { await action(); if (success) setNotice(success); }
    catch (cause) { setError(errorText(cause)); }
    finally { setBusy(false); }
  };
  const install = () => void run(() => streamDeckService.installCompanion(), 'Le paquet du compagnon a été ouvert par Windows. Termine son installation dans Stream Deck.');
  const pair = () => void run(async () => {
    const value = await streamDeckService.startPairing();
    setPairingCode({ ...value, expiresAt: Date.now() + value.expiresInSeconds * 1000 });
  }, 'Code temporaire créé. Saisis-le dans les réglages de l’action ORBIT Control dans Stream Deck.');
  const refresh = () => void run(async () => setStatus(await streamDeckService.status()));
  const revoke = () => void run(async () => {
    await streamDeckService.revokePairing();
    setStatus(current => current ? { ...current, paired: false } : current);
    setPairingCode(null);
  }, 'Jeton ORBIT révoqué. Dissocie aussi Stream Deck depuis le panneau de l’action pour supprimer sa copie locale.');

  return <section className="page orbit-module-page creator-studio-page" aria-labelledby="stream-deck-title">
    <header className="page-heading orbit-module-heading">
      <div><p className="eyebrow">COMPAGNON LOCAL · ELGATO</p><h1 id="stream-deck-title">Stream Deck</h1><p className="page-subtitle">Pilote les modules ORBIT depuis tes touches.</p></div>
    </header>
    {!streamDeckService.available && <div className="plugin-native-note" role="status"><CircleHelp size={17} /><span>L’installation et l’appairage fonctionnent dans ORBIT sous Windows.</span></div>}
    {error && <p className="plugin-error" role="alert">{error}</p>}
    {notice && <p className="plugin-success" role="status"><Check size={14} />{notice}</p>}

    <section className="creator-panel panel" aria-labelledby="streamdeck-guide-title">
      <div className="creator-panel-heading"><div><p className="eyebrow">GUIDE DE PREMIÈRE CONNEXION</p><h2 id="streamdeck-guide-title">Installer et relier Stream Deck</h2><p>Le compagnon livré avec ORBIT ajoute ses actions au catalogue Elgato. L’appairage reste local et révocable.</p></div></div>
      <ol className="creator-streamdeck-guide creator-streamdeck-full-guide">
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">1</span><div><strong>Préparer Stream Deck</strong><p>Installe l’application Stream Deck d’Elgato à jour sur ce PC et branche ton appareil Stream Deck.</p></div></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">2</span><div><strong>Activer l’intégration ORBIT</strong><p>Dans le Plugin Center d’ORBIT, active Stream Deck et accorde « Appairer un Stream Deck local ».</p></div><button type="button" className="text-button" onClick={() => navigate('plugins')}>Ouvrir Plugin Center</button></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">3</span><div><strong>Importer le compagnon fourni</strong><p>Utilise le bouton Installer dans le panneau de connexion ci-dessous. Windows ouvre le fichier compagnon ORBIT; confirme son installation dans Stream Deck.</p></div></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">4</span><div><strong>Ajouter l’action ORBIT Control</strong><p>Dans Stream Deck, glisse l’action ORBIT Control sur une touche puis ouvre ses réglages.</p></div></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">5</span><div><strong>Appairer avec un code temporaire</strong><p>Dans le panneau de connexion ORBIT, crée un code. Saisis-le dans les réglages de l’action ORBIT Control et valide l’appairage. Le code expire rapidement.</p></div></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">6</span><div><strong>Accorder seulement les accès utiles puis tester</strong><p>Dans Plugin Center, active au besoin les permissions distinctes Echo (copier la dernière ou l’avant-dernière entrée, pause de capture) et Pulsar. Pour un premier essai, choisis « Ouvrir ORBIT » ou « Copier Echo précédent » dans les réglages de la touche.</p></div></li>
      </ol>
    </section>

    <section className="creator-panel panel" aria-labelledby="companion-title">
      <div className="creator-panel-heading">
        <div><p className="eyebrow">PAIRING LOCAL · CODE TEMPORAIRE</p><h2 id="companion-title">Connecter le compagnon ORBIT</h2><p>Le pont n’écoute que sur cet ordinateur et demande une autorisation explicite.</p></div>
        <span className={`creator-state${status?.paired ? ' is-ready' : ''}`}><i aria-hidden="true" />{status?.paired ? 'Appairé' : status?.available === false ? 'Pont indisponible' : 'Non appairé'}</span>
      </div>
      <ol className="creator-streamdeck-guide">
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">1</span><div><strong>Installer le compagnon</strong><p>Elgato Stream Deck 7.1 ou ultérieur est requis sur ce PC.</p></div><button type="button" className="secondary-button" disabled={!streamDeckService.available || busy} onClick={install}><Download size={15} />Installer</button></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">2</span><div><strong>Autoriser l’appairage</strong><p>Dans Plugin Center, active Stream Deck et accorde « Appairer un Stream Deck local ».</p></div><button type="button" className="text-button" onClick={() => navigate('plugins')}>Ouvrir Plugin Center</button></li>
        <li><span className="creator-streamdeck-step-number" aria-hidden="true">3</span><div><strong>Créer un code temporaire</strong><p>Ajoute l’action « ORBIT Control », puis saisis le code dans son panneau.</p></div><button type="button" className="primary-button" disabled={!streamDeckService.available || busy || !authorized || status?.available === false} onClick={pair}><Orbit size={15} />Créer un code</button></li>
      </ol>
      {!authorized && <p className="creator-permission-help">Active le plugin Stream Deck et accorde son autorisation d’appairage dans le Plugin Center.</p>}
      {pairingCode && <div className="creator-streamdeck-code" aria-live="polite"><span>Code d’appairage · expire dans {secondsLeft}s</span><strong aria-label="Code temporaire">{pairingCode.code}</strong></div>}
      {status?.error && <p className="creator-permission-help" role="status">{status.error}</p>}
      <div className="creator-output-actions"><button type="button" className="text-button" disabled={busy || !authorized} onClick={refresh}>Actualiser l’état</button><button type="button" className="text-button danger-action" disabled={busy || !authorized || !status?.paired} onClick={revoke}><X size={15} />Révoquer l’appairage</button></div>
    </section>

    <section className="creator-panel panel" aria-labelledby="streamdeck-actions-title">
      <div className="creator-panel-heading"><div><p className="eyebrow">ACTIONS AUTORISÉES</p><h2 id="streamdeck-actions-title">Utiliser ORBIT depuis une touche</h2><p>Choisis une action dans les réglages de la touche ORBIT Control.</p></div></div>
      <div className="creator-streamdeck-actions"><ShieldCheck size={16} /><p>Ouvrir ORBIT, Mission Control, Echo, Pulsar, Core, Timeline, Paramètres ou Espaces. Copier l’entrée Echo la plus récente ou précédente et suspendre sa capture. Suspendre/reprendre Pulsar, lancer un scénario activé après confirmation Windows, ou contrôler Pulse et afficher son résumé hebdomadaire.</p></div>
      <p className="creator-security-note">Le contrôle Pulsar est <strong>{pulsarAuthorized ? 'autorisé' : 'désactivé'}</strong>. Pour autoriser ses touches, ouvre Plugin Center et active « Contrôler Pulsar depuis Stream Deck » dans les permissions facultatives de Stream Deck. Les scénarios Pulsar demandent toujours une confirmation système à chaque lancement.</p>
      <p className="creator-security-note">Les contrôles Echo et Pulse ont leurs propres permissions dans Plugin Center. Le contenu Echo ne s’affiche pas sur les touches et ne quitte pas ce PC. Tu peux révoquer l’appairage ici; les touches existantes restent enregistrées dans Stream Deck.</p>
      {!pulsarAuthorized && <button type="button" className="text-button" onClick={() => navigate('plugins')}>Configurer les permissions Stream Deck</button>}
    </section>
  </section>;
}
