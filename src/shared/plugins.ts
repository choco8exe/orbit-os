import { invoke, isTauri } from '@tauri-apps/api/core';
import type { PluginManifest, PluginPermissionId, PluginSecretId, PluginService, PluginState } from './contracts';

const projectShelfManifest: PluginManifest = {
  id: 'com.orbitos.fl-studio',
  version: '1.1.0',
  apiVersion: 1,
  name: 'Project Shelf',
  publisher: 'ORBIT OS',
  trust: 'first-party-prepackaged',
  description: 'Une bibliothèque locale de projets FL Studio. Elle conserve seulement les chemins et métadonnées des fichiers .flp sélectionnés; leur contenu n’est pas lu.',
  capabilities: ['local-project-library', 'studio-project-notes'],
  demandedPermissions: [
    { id: 'project-shelf.local-library', label: 'Bibliothèque locale', description: 'Enregistrer, afficher, épingler et retirer les références de projets .flp sur cet appareil.', required: true, scope: 'local' },
    { id: 'project-shelf.select-project-files', label: 'Sélectionner des projets FL Studio', description: 'Ouvrir le sélecteur Windows pour choisir un ou plusieurs fichiers .flp. ORBIT ne lit pas leur contenu.', required: true, scope: 'local' },
    { id: 'project-shelf.open-project', label: 'Ouvrir un projet sélectionné', description: 'Ouvrir un fichier .flp déjà inscrit dans la bibliothèque avec son application Windows associée.', required: false, scope: 'local' },
    { id: 'fl-studio.session-notes', label: 'Notes de projets FL Studio', description: 'Enregistrer des notes locales de style, BPM, tonalité et idées pour chaque projet Project Shelf.', required: false, scope: 'local' },
  ],
  contributions: {
    pages: [{ routeId: 'project-shelf', label: 'Project Shelf', description: 'Bibliothèque locale de projets FL Studio', icon: 'folder-kanban' }],
  },
  connection: { mode: 'local', summary: 'Fonctionne sur cet appareil et ne demande aucun compte externe.', secretSlots: [] },
};

const activityPulseManifest: PluginManifest = {
  id: 'com.orbitos.activity-pulse',
  version: '1.0.0',
  apiVersion: 1,
  name: 'ORBIT Pulse',
  publisher: 'ORBIT OS',
  trust: 'first-party-prepackaged',
  description: 'Mesure le temps des applications Windows au premier plan et, en option, estime celui des fenêtres visibles en arrière-plan. Cette estimation ne détecte ni lecture audio/vidéo ni sites web. Suivi local désactivé par défaut.',
  capabilities: ['local-activity-history'],
  demandedPermissions: [
    { id: 'activity-pulse.track-active-apps', label: 'Suivre les applications actives', description: 'Enregistrer localement les applications au premier plan et, si l’utilisateur l’active, estimer la durée des fenêtres visibles en arrière-plan. Ne détecte pas les vidéos ou les sites web. Surveillance désactivée par défaut.', required: false, scope: 'local' },
  ],
  contributions: { pages: [{ routeId: 'usage-analytics', label: 'Pulse', description: 'Temps passé dans les applications et jeux', icon: 'chart-no-axes-column-increasing' }] },
  connection: { mode: 'local', summary: 'Aucun compte requis. Les données d’activité restent sur cet appareil.', secretSlots: [] },
};

const creatorSuiteManifest: PluginManifest = {
  id: 'com.orbitos.creator-suite',
  version: '1.1.0',
  apiVersion: 1,
  name: 'Stream Deck',
  publisher: 'ORBIT OS',
  trust: 'first-party-prepackaged',
  description: 'Compagnon local pour piloter ORBIT depuis Elgato Stream Deck.',
  capabilities: ['streamdeck-control'],
  demandedPermissions: [
    { id: 'creator.streamdeck.bridge', label: 'Appairer un Stream Deck local', description: 'Autoriser le compagnon ORBIT installé dans Stream Deck à appeler uniquement les actions locales affichées dans son menu. Jeton révocable conservé dans le coffre Windows.', required: false, scope: 'local' },
    { id: 'pulsar.streamdeck.control', label: 'Contrôler Pulsar depuis Stream Deck', description: 'Autoriser les touches Stream Deck à suspendre/reprendre les automatisations ou lancer un scénario activé après confirmation Windows.', required: false, scope: 'local' },
  ],
  contributions: { pages: [{ routeId: 'stream-deck', label: 'Stream Deck', description: 'Compagnon ORBIT et appairage local', icon: 'keyboard' }] },
  connection: {
    mode: 'companion',
    summary: 'Appairage local avec un code temporaire; aucune intégration OBS ni compte externe.',
    secretSlots: [
      { id: 'creator.streamdeck.pair-token', label: 'Jeton Stream Deck local', required: false, entryMode: 'provider-managed' },
    ],
  },
};

const echoStreamDeckManifest: PluginManifest = {
  id: 'com.orbitos.echo-streamdeck',
  version: '1.1.0',
  apiVersion: 1,
  name: 'Echo Stream Deck Access',
  publisher: 'ORBIT OS',
  trust: 'first-party-prepackaged',
  description: 'Autorise explicitement le Stream Deck appairé à recopier une entrée Echo choisie et, avec une permission distincte, à mettre la capture en pause ou à la reprendre. Le contenu reste local et n’apparaît jamais sur les touches ou dans les journaux.',
  capabilities: ['echo-copy-via-streamdeck', 'echo-streamdeck-capture-control'],
  demandedPermissions: [
    { id: 'echo.clipboard.copy-from-streamdeck', label: 'Copier des entrées Echo depuis Stream Deck', description: 'Autoriser le compagnon ORBIT appairé à recopier l’entrée Echo la plus récente ou la précédente. Les données ne sont jamais affichées dans Stream Deck.', required: false, scope: 'local' },
    { id: 'echo.clipboard.toggle-capture-from-streamdeck', label: 'Mettre la capture Echo en pause / reprise depuis Stream Deck', description: 'Autoriser le compagnon appairé à arrêter ou reprendre la surveillance du presse-papiers. Autorisation distincte de la copie; le contenu ne quitte pas cet appareil.', required: false, scope: 'local' },
  ],
  contributions: { pages: [] },
  connection: { mode: 'local', summary: 'Utilise l’appairage local ORBIT Stream Deck; aucune donnée Echo n’est envoyée sur le réseau ou affichée sur les touches.', secretSlots: [] },
};

const workspacesManifest: PluginManifest = {
  id: 'com.orbitos.workspaces',
  version: '1.0.0',
  apiVersion: 1,
  name: 'Workspaces',
  publisher: 'ORBIT OS',
  trust: 'first-party-prepackaged',
  description: 'Crée des espaces locaux et lance ensemble les applications Windows que tu as sélectionnées.',
  capabilities: ['local-workspace-launcher'],
  demandedPermissions: [
    { id: 'workspaces.manage', label: 'Gérer les espaces', description: 'Créer des groupes locaux et enregistrer les exécutables .exe choisis depuis le sélecteur Windows.', required: true, scope: 'local' },
    { id: 'workspaces.launch', label: 'Lancer les applications des espaces', description: 'Autoriser ORBIT à démarrer les exécutables sélectionnés, sans arguments ni commande shell.', required: false, scope: 'local' },
  ],
  contributions: { pages: [{ routeId: 'workspaces', label: 'Espaces de travail', description: 'Lance tes applications par session', icon: 'panels-top-left' }] },
  connection: { mode: 'local', summary: 'Les listes restent sur cet appareil; aucun compte ni service externe requis.', secretSlots: [] },
};

const gameSessionsManifest: PluginManifest = {
  id: 'com.orbitos.game-sessions', version: '1.0.0', apiVersion: 1, name: 'Game Sessions', publisher: 'ORBIT OS', trust: 'first-party-prepackaged',
  description: 'Mesure localement la durée des processus .exe choisis. Aucun suivi web, titre de fenêtre ou contenu n’est collecté.',
  capabilities: ['local-game-session-history'],
  demandedPermissions: [{ id: 'game-sessions.track-selected-processes', label: 'Suivre les jeux choisis', description: 'Mesurer le temps pendant lequel les noms de processus ajoutés sont en cours. Désactivé par défaut et suspendable.', required: false, scope: 'local' }],
  contributions: { pages: [{ routeId: 'game-sessions', label: 'Game Sessions', description: 'Historique local du temps de jeu', icon: 'gamepad-2' }] },
  connection: { mode: 'local', summary: 'Les durées restent sur cet appareil. Aucun compte requis.', secretSlots: [] },
};

const downloadsOrganizerManifest: PluginManifest = {
  id: 'com.orbitos.downloads-organizer', version: '1.0.0', apiVersion: 1, name: 'Downloads Organizer', publisher: 'ORBIT OS', trust: 'first-party-prepackaged',
  description: 'Classe les fichiers directement présents dans un dossier autorisé, après simulation et confirmation explicite.',
  capabilities: ['local-download-organization'],
  demandedPermissions: [{ id: 'downloads-organizer.manage-authorized-folder', label: 'Organiser un dossier choisi', description: 'Autoriser ORBIT à simuler, déplacer et annuler des fichiers dans le dossier local que tu sélectionnes.', required: false, scope: 'local' }],
  contributions: { pages: [{ routeId: 'downloads-organizer', label: 'Downloads Organizer', description: 'Règles de classement avec aperçu', icon: 'folder-sync' }] },
  connection: { mode: 'local', summary: 'Données et opérations restent sur cet appareil. Aucun déplacement automatique en arrière-plan.', secretSlots: [] },
};

/** Closed, bundled catalog. New entries require compiled UI and native allowlist support. */
export const pluginCatalog: PluginManifest[] = [projectShelfManifest, activityPulseManifest, creatorSuiteManifest, echoStreamDeckManifest, workspacesManifest, gameSessionsManifest, downloadsOrganizerManifest];

const localPreviewState = (): PluginState[] => pluginCatalog.map(manifest => ({
  pluginId: manifest.id,
  installed: false,
  enabled: false,
  grantedPermissions: [],
  installedVersion: null,
}));

export const pluginService: PluginService = {
  async listCatalog() {
    if (!isTauri()) return pluginCatalog;
    return invoke<PluginManifest[]>('plugin_list_catalog');
  },
  async listStates() {
    if (!isTauri()) return localPreviewState();
    return invoke<PluginState[]>('plugin_list_states');
  },
  async install(pluginId) {
    if (!isTauri()) throw new Error('L’installation locale est disponible dans l’application Windows. Aucun téléchargement n’est proposé dans l’aperçu web.');
    return invoke<PluginState>('plugin_install', { pluginId });
  },
  async enable(pluginId) {
    if (!isTauri()) throw new Error('L’activation des plugins nécessite l’application Windows.');
    return invoke<PluginState>('plugin_enable', { pluginId });
  },
  async disable(pluginId) {
    if (!isTauri()) throw new Error('La désactivation des plugins nécessite l’application Windows.');
    return invoke<PluginState>('plugin_disable', { pluginId });
  },
  async uninstall(pluginId, options) {
    if (!isTauri()) throw new Error('La désinstallation des plugins nécessite l’application Windows.');
    await invoke<void>('plugin_uninstall', { pluginId, deleteData: options.deleteData });
  },
  async setPermissions(pluginId, grantedPermissions: PluginPermissionId[]) {
    if (!isTauri()) throw new Error('Les autorisations des plugins se règlent dans l’application Windows.');
    return invoke<PluginState>('plugin_set_permissions', { pluginId, grantedPermissions });
  },
  async secretStatus(pluginId, secretId: PluginSecretId) {
    if (!isTauri()) return false;
    return invoke<boolean>('plugin_secret_status', { pluginId, secretId });
  },
  async setSecret(pluginId, secretId: PluginSecretId, value: string) {
    if (!isTauri()) throw new Error('Les identifiants sécurisés se configurent dans ORBIT sous Windows.');
    await invoke<void>('plugin_set_secret', { pluginId, secretId, value });
  },
  async removeSecret(pluginId, secretId: PluginSecretId) {
    if (!isTauri()) throw new Error('Les identifiants sécurisés se gèrent dans ORBIT sous Windows.');
    await invoke<void>('plugin_remove_secret', { pluginId, secretId });
  },
  async retiredCredentialStatus() {
    if (!isTauri()) return { obs: false, github: false, youtube: false };
    return invoke<{ obs: boolean; github: boolean; youtube: boolean }>('plugin_retired_credential_status');
  },
  async cleanupRetiredCredentials() {
    if (!isTauri()) throw new Error('Le nettoyage des anciens identifiants exige l’application Windows.');
    await invoke<void>('plugin_cleanup_retired_credentials');
  },
};
