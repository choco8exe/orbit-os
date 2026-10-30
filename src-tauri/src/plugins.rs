use keyring::Entry;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

const FL_STUDIO_ID: &str = "com.orbitos.fl-studio";
const FL_STUDIO_VERSION: &str = "1.1.0";
const ACTIVITY_PULSE_ID: &str = "com.orbitos.activity-pulse";
const CREATOR_SUITE_ID: &str = "com.orbitos.creator-suite";
const ECHO_PLUGIN_ID: &str = "com.orbitos.echo-streamdeck";
const WORKSPACES_ID: &str = "com.orbitos.workspaces";
const GAME_SESSIONS_ID: &str = "com.orbitos.game-sessions";
const DOWNLOADS_ORGANIZER_ID: &str = "com.orbitos.downloads-organizer";
const SECRET_SERVICE: &str = "com.orbitos.nebula.plugin";
const LOCAL_LIBRARY: &str = "project-shelf.local-library";
const SELECT_PROJECT_FILES: &str = "project-shelf.select-project-files";
const OPEN_PROJECT: &str = "project-shelf.open-project";
const RETIRED_PROJECT_PERMISSION: &str = "project-shelf.atlas-access";
const TRACK_ACTIVE_APPS: &str = "activity-pulse.track-active-apps";
const STREAMDECK_BRIDGE: &str = "creator.streamdeck.bridge";
const PULSAR_STREAMDECK_CONTROL: &str = "pulsar.streamdeck.control";
const ECHO_COPY: &str = "echo.clipboard.copy-from-streamdeck";
const ECHO_CAPTURE: &str = "echo.clipboard.toggle-capture-from-streamdeck";
const WORKSPACES_MANAGE: &str = "workspaces.manage";
const WORKSPACES_LAUNCH: &str = "workspaces.launch";
const STUDIO_NOTES: &str = "fl-studio.session-notes";
const OBS_PASSWORD_SLOT: &str = "creator.obs.password";
const STREAMDECK_TOKEN_SLOT: &str = "creator.streamdeck.pair-token";
const GAME_TRACKING: &str = "game-sessions.track-selected-processes";
const DOWNLOADS_MANAGE: &str = "downloads-organizer.manage-authorized-folder";
const GITHUB_PLUGIN_ID: &str = "com.orbitos.github-control-room";
const GITHUB_TOKEN_SLOT: &str = "github.personal-access-token";
const YOUTUBE_PLUGIN_ID: &str = "com.orbitos.youtube-hub";
const YOUTUBE_REFRESH_SLOT: &str = "youtube.oauth-refresh-token";

fn registry_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Dossier ORBIT indisponible : {error}"))?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Impossible de créer le dossier ORBIT : {error}"))?;
    Ok(directory.join("plugins.sqlite3"))
}

fn open_db(app: &AppHandle) -> Result<Connection, String> {
    let connection = Connection::open(registry_path(app)?)
        .map_err(|error| format!("Registre des plugins inaccessible : {error}"))?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_state (
            plugin_id TEXT PRIMARY KEY,
            installed INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 0,
            granted_permissions TEXT NOT NULL DEFAULT '[]',
            installed_version TEXT
        );",
        )
        .map_err(|error| format!("Impossible de préparer le registre des plugins : {error}"))?;
    Ok(connection)
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let connection = open_db(app)?;
    connection
        .execute(
            "DELETE FROM plugin_state WHERE plugin_id IN (?1, ?2)",
            params![GITHUB_PLUGIN_ID, YOUTUBE_PLUGIN_ID],
        )
        .map_err(|error| {
            format!("Impossible de retirer les anciennes intégrations du registre : {error}")
        })?;
    // Retire only the obsolete Atlas grant from Project Shelf; preserve every other permission.
    let mut statement = connection
        .prepare("SELECT granted_permissions FROM plugin_state WHERE plugin_id=?1")
        .map_err(|_| "Impossible de vérifier les autorisations Project Shelf retirées.".to_owned())?;
    let rows = statement
        .query_map([FL_STUDIO_ID], |row| row.get::<_, String>(0))
        .map_err(|_| "Impossible de lire les autorisations Project Shelf retirées.".to_owned())?;
    let grants = rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Impossible de lire les autorisations Project Shelf retirées.".to_owned())?;
    drop(statement);
    for raw in grants {
        let mut permissions: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
        let old_len = permissions.len();
        permissions.retain(|permission| permission != RETIRED_PROJECT_PERMISSION);
        if permissions.len() != old_len {
            connection.execute(
                "UPDATE plugin_state SET granted_permissions=?1 WHERE plugin_id=?2",
                params![serde_json::to_string(&permissions).map_err(|_| "Autorisations Project Shelf invalides.".to_owned())?, FL_STUDIO_ID],
            ).map_err(|_| "Impossible de retirer l’ancienne autorisation Project Shelf.".to_owned())?;
        }
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionManifest {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    required: bool,
    scope: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageContribution {
    route_id: &'static str,
    label: &'static str,
    description: &'static str,
    icon: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contributions {
    pages: Vec<PageContribution>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretSlotManifest {
    id: &'static str,
    label: &'static str,
    required: bool,
    entry_mode: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionManifest {
    mode: &'static str,
    summary: &'static str,
    secret_slots: Vec<SecretSlotManifest>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    id: &'static str,
    version: &'static str,
    api_version: u8,
    name: &'static str,
    publisher: &'static str,
    trust: &'static str,
    description: &'static str,
    capabilities: Vec<&'static str>,
    demanded_permissions: Vec<PermissionManifest>,
    contributions: Contributions,
    connection: ConnectionManifest,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginState {
    plugin_id: String,
    installed: bool,
    enabled: bool,
    granted_permissions: Vec<String>,
    installed_version: Option<String>,
}

fn catalog() -> Vec<PluginManifest> {
    vec![PluginManifest {
        id: FL_STUDIO_ID,
        version: FL_STUDIO_VERSION,
        api_version: 1,
        name: "Project Shelf",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Une bibliothèque locale de projets FL Studio. Elle conserve seulement les chemins et métadonnées des fichiers .flp sélectionnés; leur contenu n’est pas lu.",
        capabilities: vec!["local-project-library", "studio-project-notes"],
        demanded_permissions: vec![
            PermissionManifest { id: LOCAL_LIBRARY, label: "Bibliothèque locale", description: "Enregistrer, afficher, épingler et retirer les références de projets .flp sur cet appareil.", required: true, scope: "local" },
            PermissionManifest { id: SELECT_PROJECT_FILES, label: "Sélectionner des projets FL Studio", description: "Ouvrir le sélecteur Windows pour choisir un ou plusieurs fichiers .flp. ORBIT ne lit pas leur contenu.", required: true, scope: "local" },
            PermissionManifest { id: OPEN_PROJECT, label: "Ouvrir un projet sélectionné", description: "Ouvrir un fichier .flp déjà inscrit dans la bibliothèque avec son application Windows associée.", required: false, scope: "local" },
            PermissionManifest { id: STUDIO_NOTES, label: "Notes de projets FL Studio", description: "Enregistrer localement style, BPM, tonalité et idées pour chaque projet Project Shelf.", required: false, scope: "local" },
        ],
        contributions: Contributions {
            pages: vec![PageContribution { route_id: "project-shelf", label: "Project Shelf", description: "Bibliothèque locale de projets FL Studio", icon: "folder-kanban" }],
        },
        connection: ConnectionManifest { mode: "local", summary: "Fonctionne sur cet appareil et ne demande aucun compte externe.", secret_slots: vec![] },
    }, PluginManifest {
        id: ACTIVITY_PULSE_ID,
        version: "1.0.0",
        api_version: 1,
        name: "ORBIT Pulse",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Mesure le temps des applications Windows au premier plan et, en option, estime celui des fenêtres visibles en arrière-plan. Cette estimation ne détecte pas la lecture audio/vidéo ni les sites web. Le suivi reste local et désactivé par défaut.",
        capabilities: vec!["local-activity-history"],
        demanded_permissions: vec![PermissionManifest { id: TRACK_ACTIVE_APPS, label: "Suivre les applications actives", description: "Enregistrer localement les applications au premier plan et, si l’utilisateur l’active, estimer la durée des fenêtres visibles en arrière-plan. Ne détecte pas les vidéos ou les sites web. Surveillance désactivée par défaut.", required: false, scope: "local" }],
        contributions: Contributions {
            pages: vec![PageContribution { route_id: "usage-analytics", label: "Pulse", description: "Temps passé dans les applications et jeux", icon: "chart-no-axes-column-increasing" }],
        },
        connection: ConnectionManifest { mode: "local", summary: "Aucun compte requis. Les données d’activité restent sur cet appareil.", secret_slots: vec![] },
    }, PluginManifest {
        id: CREATOR_SUITE_ID,
        version: "1.1.0",
        api_version: 1,
        name: "Stream Deck",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Compagnon local pour piloter ORBIT depuis Elgato Stream Deck.",
        capabilities: vec!["streamdeck-control"],
        demanded_permissions: vec![
            PermissionManifest { id: STREAMDECK_BRIDGE, label: "Appairer un Stream Deck local", description: "Autoriser le compagnon ORBIT installé dans Stream Deck à appeler uniquement les actions locales affichées dans son menu. Jeton révocable conservé dans le coffre Windows.", required: false, scope: "local" },
            PermissionManifest { id: PULSAR_STREAMDECK_CONTROL, label: "Contrôler Pulsar depuis Stream Deck", description: "Autoriser les touches Stream Deck à suspendre/reprendre les automatisations ou lancer un scénario activé après confirmation Windows.", required: false, scope: "local" },
        ],
        contributions: Contributions {
            pages: vec![PageContribution { route_id: "stream-deck", label: "Stream Deck", description: "Compagnon ORBIT et appairage local", icon: "keyboard" }],
        },
        connection: ConnectionManifest {
            mode: "companion",
            summary: "Appairage local avec un code temporaire; aucune intégration OBS ni compte externe.",
            secret_slots: vec![SecretSlotManifest { id: STREAMDECK_TOKEN_SLOT, label: "Jeton Stream Deck local", required: false, entry_mode: "provider-managed" }],
        },
    }, PluginManifest {
        id: ECHO_PLUGIN_ID,
        version: "1.1.0",
        api_version: 1,
        name: "Echo Stream Deck Access",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Autorise explicitement le Stream Deck appairé à recopier une entrée Echo choisie et, avec une permission distincte, à mettre la capture en pause ou à la reprendre. Le contenu reste local et n’apparaît jamais sur les touches ou dans les journaux.",
        capabilities: vec!["echo-copy-via-streamdeck", "echo-streamdeck-capture-control"],
        demanded_permissions: vec![
            PermissionManifest { id: ECHO_COPY, label: "Copier des entrées Echo depuis Stream Deck", description: "Autoriser le compagnon ORBIT appairé à recopier l’entrée Echo la plus récente ou la précédente. Les données ne sont jamais affichées dans Stream Deck.", required: false, scope: "local" },
            PermissionManifest { id: ECHO_CAPTURE, label: "Mettre la capture Echo en pause / reprise depuis Stream Deck", description: "Autoriser le compagnon appairé à arrêter ou reprendre la surveillance du presse-papiers. Permission distincte de la copie; le contenu ne quitte pas cet appareil.", required: false, scope: "local" },
        ],
        contributions: Contributions { pages: vec![] },
        connection: ConnectionManifest { mode: "local", summary: "Utilise l’appairage local ORBIT Stream Deck; aucune donnée Echo n’est envoyée sur le réseau ou affichée sur les touches.", secret_slots: vec![] },
    }, PluginManifest {
        id: WORKSPACES_ID,
        version: "1.0.0",
        api_version: 1,
        name: "Workspaces",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Crée des espaces locaux et lance ensemble les applications Windows que tu as sélectionnées.",
        capabilities: vec!["local-workspace-launcher"],
        demanded_permissions: vec![
            PermissionManifest { id: WORKSPACES_MANAGE, label: "Gérer les espaces", description: "Créer des groupes locaux et enregistrer les exécutables .exe choisis depuis le sélecteur Windows.", required: true, scope: "local" },
            PermissionManifest { id: WORKSPACES_LAUNCH, label: "Lancer les applications des espaces", description: "Autoriser ORBIT à démarrer les exécutables sélectionnés, sans arguments ni commande shell.", required: false, scope: "local" },
        ],
        contributions: Contributions { pages: vec![PageContribution { route_id: "workspaces", label: "Espaces de travail", description: "Lance tes applications par session", icon: "panels-top-left" }] },
        connection: ConnectionManifest { mode: "local", summary: "Les listes restent sur cet appareil; aucun compte ni service externe requis.", secret_slots: vec![] },
    }, PluginManifest {
        id: GAME_SESSIONS_ID,
        version: "1.0.0",
        api_version: 1,
        name: "Game Sessions",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Mesure localement la durée des processus .exe choisis. Aucun suivi web, titre de fenêtre ou contenu n’est collecté.",
        capabilities: vec!["local-game-session-history"],
        demanded_permissions: vec![PermissionManifest { id: GAME_TRACKING, label: "Suivre les jeux choisis", description: "Mesurer le temps pendant lequel les noms de processus ajoutés sont en cours. Désactivé par défaut et suspendable.", required: false, scope: "local" }],
        contributions: Contributions { pages: vec![PageContribution { route_id: "game-sessions", label: "Game Sessions", description: "Historique local du temps de jeu", icon: "gamepad-2" }] },
        connection: ConnectionManifest { mode: "local", summary: "Les durées restent sur cet appareil. Aucun compte requis.", secret_slots: vec![] },
    }, PluginManifest {
        id: DOWNLOADS_ORGANIZER_ID,
        version: "1.0.0",
        api_version: 1,
        name: "Downloads Organizer",
        publisher: "ORBIT OS",
        trust: "first-party-prepackaged",
        description: "Classe les fichiers directement présents dans un dossier autorisé, après simulation et confirmation explicite.",
        capabilities: vec!["local-download-organization"],
        demanded_permissions: vec![PermissionManifest { id: DOWNLOADS_MANAGE, label: "Organiser un dossier choisi", description: "Autoriser ORBIT à simuler, déplacer et annuler des fichiers dans le dossier local que tu sélectionnes.", required: false, scope: "local" }],
        contributions: Contributions { pages: vec![PageContribution { route_id: "downloads-organizer", label: "Downloads Organizer", description: "Règles de classement avec aperçu", icon: "folder-sync" }] },
        connection: ConnectionManifest { mode: "local", summary: "Données et opérations restent sur cet appareil. Aucun déplacement automatique en arrière-plan.", secret_slots: vec![] },
    }]
}

fn allowed_permissions(plugin_id: &str) -> Option<&'static [&'static str]> {
    match plugin_id {
        FL_STUDIO_ID => Some(&[
            LOCAL_LIBRARY,
            SELECT_PROJECT_FILES,
            OPEN_PROJECT,
            STUDIO_NOTES,
        ]),
        ACTIVITY_PULSE_ID => Some(&[TRACK_ACTIVE_APPS]),
        CREATOR_SUITE_ID => Some(&[STREAMDECK_BRIDGE, PULSAR_STREAMDECK_CONTROL]),
        ECHO_PLUGIN_ID => Some(&[ECHO_COPY, ECHO_CAPTURE]),
        WORKSPACES_ID => Some(&[WORKSPACES_MANAGE, WORKSPACES_LAUNCH]),
        GAME_SESSIONS_ID => Some(&[GAME_TRACKING]),
        DOWNLOADS_ORGANIZER_ID => Some(&[DOWNLOADS_MANAGE]),
        _ => None,
    }
}

fn secret_slot_permission(plugin_id: &str, secret_id: &str) -> Option<&'static str> {
    match (plugin_id, secret_id) {
        (CREATOR_SUITE_ID, STREAMDECK_TOKEN_SLOT) => Some(STREAMDECK_BRIDGE),
        _ => None,
    }
}

/// Retired OBS credentials are addressable only for presence checks and deletion.
fn legacy_cleanup_secret(plugin_id: &str, secret_id: &str) -> bool {
    plugin_id == CREATOR_SUITE_ID && secret_id == OBS_PASSWORD_SLOT
}

/// Credentials belonging to integrations removed from the catalog remain addressable only for
/// presence checks and deletion. They cannot be read, written, authorized, or executed.
fn retired_online_secret(plugin_id: &str, secret_id: &str) -> bool {
    matches!(
        (plugin_id, secret_id),
        (GITHUB_PLUGIN_ID, GITHUB_TOKEN_SLOT) | (YOUTUBE_PLUGIN_ID, YOUTUBE_REFRESH_SLOT)
    )
}

fn secret_slot_known(plugin_id: &str, secret_id: &str) -> bool {
    secret_slot_permission(plugin_id, secret_id).is_some()
        || legacy_cleanup_secret(plugin_id, secret_id)
        || retired_online_secret(plugin_id, secret_id)
}

fn secret_slot_user_writable(_plugin_id: &str, _secret_id: &str) -> bool {
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PluginDataCleanup {
    FlStudio,
    ActivityPulse,
    Workspaces,
    GameSessions,
    DownloadsOrganizer,
}

fn plugin_data_cleanup(plugin_id: &str) -> Option<PluginDataCleanup> {
    match plugin_id {
        FL_STUDIO_ID => Some(PluginDataCleanup::FlStudio),
        ACTIVITY_PULSE_ID => Some(PluginDataCleanup::ActivityPulse),
        WORKSPACES_ID => Some(PluginDataCleanup::Workspaces),
        GAME_SESSIONS_ID => Some(PluginDataCleanup::GameSessions),
        DOWNLOADS_ORGANIZER_ID => Some(PluginDataCleanup::DownloadsOrganizer),
        _ => None,
    }
}

fn keyring_entry(plugin_id: &str, secret_id: &str) -> Result<Entry, String> {
    if !secret_slot_known(plugin_id, secret_id) {
        return Err("Identifiant de secret inconnu pour ce plugin.".into());
    }
    let account = format!("{plugin_id}:{secret_id}");
    Entry::new(SECRET_SERVICE, &account)
        .map_err(|_| "Le coffre sécurisé Windows n’est pas disponible.".to_owned())
}

fn delete_manifest_secret_slots<F>(
    slots: &[SecretSlotManifest],
    mut delete_one: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    for slot in slots {
        delete_one(slot.id).map_err(|error| {
            format!(
                "Impossible d’effacer « {} » du coffre Windows : {error}",
                slot.label
            )
        })?;
    }
    Ok(())
}

fn delete_keyring_secret(plugin_id: &str, secret_id: &str) -> Result<(), String> {
    keyring_delete_result(keyring_entry(plugin_id, secret_id)?.delete_credential())
}

fn keyring_delete_result(result: Result<(), keyring::Error>) -> Result<(), String> {
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Le coffre Windows a refusé la suppression. Réessaie ou supprime l’identifiant depuis le Gestionnaire d’identifiants Windows.".into()),
    }
}

fn validate_secret(
    plugin_id: &str,
    secret_id: &str,
    permission_id: &str,
    value: &str,
) -> Result<(), String> {
    if secret_slot_permission(plugin_id, secret_id) != Some(permission_id) {
        return Err("Ce secret n’est pas autorisé pour cette permission.".into());
    }
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(
            "La valeur du secret est vide, trop longue ou contient des caractères de contrôle."
                .into(),
        );
    }
    Ok(())
}

/// Backend-only access. Secret values are never returned through a Tauri command to JavaScript.
pub fn get_secret_from_backend(
    app: &AppHandle,
    plugin_id: &str,
    secret_id: &str,
    permission_id: &str,
) -> Result<Zeroizing<String>, String> {
    validate_secret(plugin_id, secret_id, permission_id, "placeholder")?;
    require_permission(app, plugin_id, permission_id)?;
    keyring_entry(plugin_id, secret_id)?
        .get_password()
        .map(Zeroizing::new)
        .map_err(|error| match error {
            keyring::Error::NoEntry => "Aucun secret n’est configuré pour ce plugin.".to_owned(),
            _ => "Impossible de lire le secret depuis le coffre sécurisé Windows.".to_owned(),
        })
}

pub fn set_secret_from_backend(
    app: &AppHandle,
    plugin_id: &str,
    secret_id: &str,
    permission_id: &str,
    value: &str,
) -> Result<(), String> {
    validate_secret(plugin_id, secret_id, permission_id, value)?;
    require_permission(app, plugin_id, permission_id)?;
    let secret = Zeroizing::new(value.to_owned());
    keyring_entry(plugin_id, secret_id)?
        .set_password(secret.as_str())
        .map_err(|_| {
            "Impossible d’enregistrer le secret dans le coffre sécurisé Windows.".to_owned()
        })
}

pub fn delete_secret_from_backend(
    app: &AppHandle,
    plugin_id: &str,
    secret_id: &str,
    permission_id: &str,
) -> Result<(), String> {
    if retired_online_secret(plugin_id, secret_id) {
        return delete_keyring_secret(plugin_id, secret_id);
    }
    if legacy_cleanup_secret(plugin_id, secret_id) {
        require_plugin_installed(app, plugin_id)?;
        return delete_keyring_secret(plugin_id, secret_id);
    }
    if secret_slot_permission(plugin_id, secret_id) != Some(permission_id) {
        return Err("Ce secret n’est pas autorisé pour cette permission.".into());
    }
    // Removing a credential must remain possible after its permission is revoked.
    require_plugin_installed(app, plugin_id)?;
    match keyring_entry(plugin_id, secret_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Impossible de supprimer le secret du coffre sécurisé Windows.".into()),
    }
}

pub fn secret_exists(
    app: &AppHandle,
    plugin_id: &str,
    secret_id: &str,
    permission_id: &str,
) -> Result<bool, String> {
    if secret_slot_permission(plugin_id, secret_id) != Some(permission_id) {
        return Err("Ce secret n’est pas autorisé pour cette permission.".into());
    }
    // Expose only presence, and allow cleanup after permission revocation.
    require_plugin_installed(app, plugin_id)?;
    match keyring_entry(plugin_id, secret_id)?.get_password() {
        Ok(secret) => {
            drop(Zeroizing::new(secret));
            Ok(true)
        }
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err("Impossible de vérifier le secret dans le coffre sécurisé Windows.".into()),
    }
}

fn state_for(connection: &Connection, plugin_id: &str) -> Result<PluginState, String> {
    if allowed_permissions(plugin_id).is_none() {
        return Err("Plugin inconnu ou non inclus dans ORBIT.".into());
    }
    let row = connection.query_row(
        "SELECT installed, enabled, granted_permissions, installed_version FROM plugin_state WHERE plugin_id=?1",
        [plugin_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)),
    ).optional().map_err(|error| format!("Impossible de lire le registre des plugins : {error}"))?;
    match row {
        Some((installed, enabled, permissions, installed_version)) => {
            let valid_permissions = allowed_permissions(plugin_id).expect("catalog plugin");
            let granted_permissions: Vec<String> =
                serde_json::from_str::<Vec<String>>(&permissions)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|permission| valid_permissions.contains(&permission.as_str()))
                    .collect();
            Ok(PluginState {
                plugin_id: plugin_id.into(),
                installed: installed != 0,
                enabled: installed != 0 && enabled != 0,
                granted_permissions,
                installed_version,
            })
        }
        None => Ok(PluginState {
            plugin_id: plugin_id.into(),
            installed: false,
            enabled: false,
            granted_permissions: Vec::new(),
            installed_version: None,
        }),
    }
}

fn save_state(connection: &Connection, state: &PluginState) -> Result<(), String> {
    let permissions = serde_json::to_string(&state.granted_permissions)
        .map_err(|error| format!("Permissions invalides : {error}"))?;
    connection.execute(
        "INSERT INTO plugin_state(plugin_id, installed, enabled, granted_permissions, installed_version) VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(plugin_id) DO UPDATE SET installed=excluded.installed, enabled=excluded.enabled, granted_permissions=excluded.granted_permissions, installed_version=excluded.installed_version",
        params![state.plugin_id, i64::from(state.installed), i64::from(state.enabled), permissions, state.installed_version],
    ).map_err(|error| format!("Impossible d’enregistrer l’état du plugin : {error}"))?;
    Ok(())
}

fn ensure_installed(connection: &Connection, plugin_id: &str) -> Result<PluginState, String> {
    let state = state_for(connection, plugin_id)?;
    if !state.installed {
        return Err("Installez ce plugin avant de modifier son état.".into());
    }
    Ok(state)
}

fn state_has(state: &PluginState, permission: &str) -> bool {
    state
        .granted_permissions
        .iter()
        .any(|granted| granted == permission)
}

/// Called by native module commands as the authority for plugin permissions.
pub fn require_permission(
    app: &AppHandle,
    plugin_id: &str,
    permission_id: &str,
) -> Result<(), String> {
    let known_permissions = allowed_permissions(plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    if !known_permissions.contains(&permission_id) {
        return Err("Cette permission n’est pas déclarée par le plugin.".into());
    }
    let connection = open_db(app)?;
    let state = state_for(&connection, plugin_id)?;
    if !state.installed {
        return Err("Installez ce plugin depuis le centre Plugins.".into());
    }
    if !state.enabled {
        return Err("Activez ce plugin avant cette opération.".into());
    }
    if !state_has(&state, permission_id) {
        return Err(
            "Cette action nécessite une autorisation explicite dans le centre Plugins.".into(),
        );
    }
    Ok(())
}

pub fn require_plugin_enabled(app: &AppHandle, plugin_id: &str) -> Result<(), String> {
    if allowed_permissions(plugin_id).is_none() {
        return Err("Plugin inconnu ou non inclus dans ORBIT.".into());
    }
    let connection = open_db(app)?;
    let state = state_for(&connection, plugin_id)?;
    if !state.installed {
        return Err("Installe ce plugin depuis le centre Plugins.".into());
    }
    if !state.enabled {
        return Err("Active ce plugin depuis le centre Plugins.".into());
    }
    Ok(())
}

fn require_plugin_installed(app: &AppHandle, plugin_id: &str) -> Result<(), String> {
    if allowed_permissions(plugin_id).is_none() {
        return Err("Plugin inconnu ou non inclus dans ORBIT.".into());
    }
    let connection = open_db(app)?;
    if !state_for(&connection, plugin_id)?.installed {
        return Err("Installe ce plugin depuis le centre Plugins.".into());
    }
    Ok(())
}

pub fn has_permission(
    app: &AppHandle,
    plugin_id: &str,
    permission_id: &str,
) -> Result<bool, String> {
    let known_permissions = allowed_permissions(plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    if !known_permissions.contains(&permission_id) {
        return Err("Cette permission n’est pas déclarée par le plugin.".into());
    }
    let connection = open_db(app)?;
    let state = state_for(&connection, plugin_id)?;
    Ok(state.installed && state.enabled && state_has(&state, permission_id))
}

#[tauri::command]
pub fn plugin_list_catalog() -> Vec<PluginManifest> {
    catalog()
}

#[tauri::command]
pub fn plugin_list_states(app: AppHandle) -> Result<Vec<PluginState>, String> {
    let connection = open_db(&app)?;
    catalog()
        .iter()
        .map(|manifest| state_for(&connection, manifest.id))
        .collect()
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_install(app: AppHandle, plugin_id: String) -> Result<PluginState, String> {
    let manifest = catalog()
        .into_iter()
        .find(|manifest| manifest.id == plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    let connection = open_db(&app)?;
    let current = state_for(&connection, &plugin_id)?;
    if current.installed {
        return Ok(current);
    }
    let state = PluginState {
        plugin_id,
        installed: true,
        enabled: false,
        granted_permissions: Vec::new(),
        installed_version: Some(manifest.version.into()),
    };
    save_state(&connection, &state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_enable(app: AppHandle, plugin_id: String) -> Result<PluginState, String> {
    let connection = open_db(&app)?;
    let mut state = ensure_installed(&connection, &plugin_id)?;
    let manifest = catalog()
        .into_iter()
        .find(|manifest| manifest.id == plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    let missing: Vec<_> = manifest
        .demanded_permissions
        .iter()
        .filter(|permission| permission.required && !state_has(&state, permission.id))
        .map(|permission| permission.label)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Accordez ces permissions avant l’activation : {}.",
            missing.join(", ")
        ));
    }
    state.enabled = true;
    save_state(&connection, &state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_disable(app: AppHandle, plugin_id: String) -> Result<PluginState, String> {
    let connection = open_db(&app)?;
    let mut state = ensure_installed(&connection, &plugin_id)?;
    state.enabled = false;
    save_state(&connection, &state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_set_permissions(
    app: AppHandle,
    plugin_id: String,
    granted_permissions: Vec<String>,
) -> Result<PluginState, String> {
    let connection = open_db(&app)?;
    let mut state = ensure_installed(&connection, &plugin_id)?;
    let allowed = allowed_permissions(&plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    let mut normalized = Vec::new();
    for permission in granted_permissions {
        if !allowed.contains(&permission.as_str()) {
            return Err(format!("Permission non déclarée : {permission}"));
        }
        if !normalized.contains(&permission) {
            normalized.push(permission);
        }
    }
    state.granted_permissions = normalized;
    let required: Vec<_> = catalog()
        .into_iter()
        .find(|manifest| manifest.id == plugin_id)
        .expect("known plugin")
        .demanded_permissions
        .into_iter()
        .filter(|permission| permission.required)
        .map(|permission| permission.id)
        .collect();
    if required
        .iter()
        .any(|permission| !state_has(&state, permission))
    {
        state.enabled = false;
    }
    save_state(&connection, &state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_set_secret(
    app: AppHandle,
    plugin_id: String,
    secret_id: String,
    value: String,
) -> Result<(), String> {
    if !secret_slot_user_writable(&plugin_id, &secret_id) {
        return Err("Ce secret est réservé au flux de connexion natif du fournisseur.".into());
    }
    let permission = secret_slot_permission(&plugin_id, &secret_id)
        .ok_or_else(|| "Identifiant de secret inconnu pour ce plugin.".to_string())?;
    let value = Zeroizing::new(value);
    set_secret_from_backend(&app, &plugin_id, &secret_id, permission, &value)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_remove_secret(
    app: AppHandle,
    plugin_id: String,
    secret_id: String,
) -> Result<(), String> {
    if legacy_cleanup_secret(&plugin_id, &secret_id)
        || retired_online_secret(&plugin_id, &secret_id)
    {
        return delete_secret_from_backend(&app, &plugin_id, &secret_id, "");
    }
    let permission = secret_slot_permission(&plugin_id, &secret_id)
        .ok_or_else(|| "Identifiant de secret inconnu pour ce plugin.".to_string())?;
    delete_secret_from_backend(&app, &plugin_id, &secret_id, permission)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_secret_status(
    app: AppHandle,
    plugin_id: String,
    secret_id: String,
) -> Result<bool, String> {
    let permission = secret_slot_permission(&plugin_id, &secret_id)
        .ok_or_else(|| "Identifiant de secret inconnu pour ce plugin.".to_string())?;
    secret_exists(&app, &plugin_id, &secret_id, permission)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetiredCredentialStatus {
    pub obs: bool,
    pub github: bool,
    pub youtube: bool,
}

fn keyring_secret_present(plugin_id: &str, secret_id: &str) -> Result<bool, String> {
    match keyring_entry(plugin_id, secret_id)?.get_password() {
        Ok(secret) => {
            drop(Zeroizing::new(secret));
            Ok(true)
        }
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err(
            "Impossible de vérifier la présence d’un ancien identifiant dans le coffre Windows."
                .into(),
        ),
    }
}

#[tauri::command]
pub fn plugin_retired_credential_status() -> Result<RetiredCredentialStatus, String> {
    Ok(RetiredCredentialStatus {
        obs: keyring_secret_present(CREATOR_SUITE_ID, OBS_PASSWORD_SLOT)?,
        github: keyring_secret_present(GITHUB_PLUGIN_ID, GITHUB_TOKEN_SLOT)?,
        youtube: keyring_secret_present(YOUTUBE_PLUGIN_ID, YOUTUBE_REFRESH_SLOT)?,
    })
}

#[tauri::command]
pub fn plugin_cleanup_retired_credentials() -> Result<(), String> {
    delete_keyring_secret(CREATOR_SUITE_ID, OBS_PASSWORD_SLOT)?;
    delete_keyring_secret(GITHUB_PLUGIN_ID, GITHUB_TOKEN_SLOT)?;
    delete_keyring_secret(YOUTUBE_PLUGIN_ID, YOUTUBE_REFRESH_SLOT)
}

#[tauri::command(rename_all = "camelCase")]
pub fn plugin_uninstall(
    app: AppHandle,
    plugin_id: String,
    delete_data: bool,
) -> Result<(), String> {
    if allowed_permissions(&plugin_id).is_none() {
        return Err("Plugin inconnu ou non inclus dans ORBIT.".into());
    }
    let manifest = catalog()
        .into_iter()
        .find(|manifest| manifest.id == plugin_id)
        .ok_or_else(|| "Plugin inconnu ou non inclus dans ORBIT.".to_string())?;
    let connection = open_db(&app)?;
    ensure_installed(&connection, &plugin_id)?;

    // Credential Manager cleanup is the first destructive step. Keep the plugin state and its
    // local data intact if any registered credential cannot be removed, so the user can retry.
    delete_manifest_secret_slots(&manifest.connection.secret_slots, |secret_id| {
        delete_keyring_secret(&plugin_id, secret_id)
    })?;
    if legacy_cleanup_secret(&plugin_id, STREAMDECK_TOKEN_SLOT) {
        keyring_delete_result(
            keyring_entry(&plugin_id, STREAMDECK_TOKEN_SLOT)?.delete_credential(),
        )
        .map_err(|error| {
            format!("Impossible d’effacer l’ancien jeton Stream Deck du coffre Windows : {error}")
        })?;
    }

    if delete_data {
        match plugin_data_cleanup(&plugin_id) {
            Some(PluginDataCleanup::FlStudio) => {
                crate::modules::fl_studio::uninstall_cleanup(&app, true)?;
                crate::modules::creator_plugins::uninstall_cleanup(&app, true)?;
            }
            Some(PluginDataCleanup::ActivityPulse) => {
                crate::modules::activity::uninstall_cleanup(&app, true)?
            }
            Some(PluginDataCleanup::Workspaces) => {
                crate::modules::workspaces::uninstall_cleanup(&app, true)?
            }
            Some(PluginDataCleanup::GameSessions) => {
                crate::modules::game_sessions::uninstall_cleanup(&app, true)?
            }
            Some(PluginDataCleanup::DownloadsOrganizer) => {
                crate::modules::downloads_organizer::uninstall_cleanup(&app, true)?
            }
            None => {}
        }
    }
    connection
        .execute("DELETE FROM plugin_state WHERE plugin_id=?1", [plugin_id])
        .map_err(|error| format!("Impossible de désinstaller le plugin : {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keyring_entry_is_already_cleaned() {
        assert!(keyring_delete_result(Err(keyring::Error::NoEntry)).is_ok());
    }

    #[test]
    fn uninstall_secret_failure_stops_before_later_slots_and_is_visible() {
        let slots = [
            SecretSlotManifest {
                id: "secret.first",
                label: "Premier secret",
                required: false,
                entry_mode: "user-input",
            },
            SecretSlotManifest {
                id: "secret.second",
                label: "Second secret",
                required: false,
                entry_mode: "provider-managed",
            },
            SecretSlotManifest {
                id: "secret.third",
                label: "Troisième secret",
                required: false,
                entry_mode: "provider-managed",
            },
        ];
        let mut visited = Vec::new();
        let result = delete_manifest_secret_slots(&slots, |secret_id| {
            visited.push(secret_id.to_owned());
            if secret_id == "secret.second" {
                Err("coffre verrouillé".to_owned())
            } else {
                Ok(())
            }
        });
        let error = result.expect_err("the second secret deletion should abort uninstall");
        assert!(error.contains("Second secret"));
        assert!(error.contains("coffre verrouillé"));
        assert_eq!(
            visited,
            vec!["secret.first".to_owned(), "secret.second".to_owned()]
        );
    }

    #[test]
    fn catalog_permissions_and_routes_match_the_closed_native_allowlist() {
        let expected = [
            (FL_STUDIO_ID, "project-shelf"),
            (ACTIVITY_PULSE_ID, "usage-analytics"),
            (CREATOR_SUITE_ID, "stream-deck"),
            (GAME_SESSIONS_ID, "game-sessions"),
            (DOWNLOADS_ORGANIZER_ID, "downloads-organizer"),
        ];
        let manifests = catalog();
        assert_eq!(manifests.len(), expected.len() + 2);
        for (plugin_id, route_id) in expected {
            let manifest = manifests
                .iter()
                .find(|manifest| manifest.id == plugin_id)
                .expect("catalog entry");
            assert_eq!(manifest.contributions.pages.len(), 1);
            assert_eq!(manifest.contributions.pages[0].route_id, route_id);
            let allowed = allowed_permissions(plugin_id).expect("native allowlist");
            assert!(manifest
                .demanded_permissions
                .iter()
                .all(|permission| allowed.contains(&permission.id)));
        }
        let echo = manifests
            .iter()
            .find(|manifest| manifest.id == ECHO_PLUGIN_ID)
            .expect("Echo Stream Deck permission manifest");
        let allowed = allowed_permissions(ECHO_PLUGIN_ID).expect("Echo permission allowlist");
        assert!(echo
            .demanded_permissions
            .iter()
            .all(|permission| allowed.contains(&permission.id)));
        assert!(echo.contributions.pages.is_empty());
        let fl_studio = manifests
            .iter()
            .find(|manifest| manifest.id == FL_STUDIO_ID)
            .expect("FL Studio manifest");
        assert!(fl_studio
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == STUDIO_NOTES));
        let workspaces = manifests
            .iter()
            .find(|manifest| manifest.id == WORKSPACES_ID)
            .expect("Workspaces manifest");
        assert_eq!(workspaces.contributions.pages[0].route_id, "workspaces");
        assert!(workspaces
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == WORKSPACES_MANAGE && permission.required));
        assert!(workspaces
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == WORKSPACES_LAUNCH && !permission.required));
        let allowed = allowed_permissions(WORKSPACES_ID).expect("Workspaces permission allowlist");
        assert!(workspaces
            .demanded_permissions
            .iter()
            .all(|permission| allowed.contains(&permission.id)));
    }

    #[test]
    fn streamdeck_pairing_is_explicit_and_credential_is_not_user_writable() {
        let creator = catalog()
            .into_iter()
            .find(|manifest| manifest.id == CREATOR_SUITE_ID)
            .expect("Stream Deck manifest");
        assert!(creator.capabilities.contains(&"streamdeck-control"));
        assert!(!creator
            .capabilities
            .iter()
            .any(|capability| capability.contains("obs")));
        assert!(creator
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == STREAMDECK_BRIDGE));
        assert!(creator
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == PULSAR_STREAMDECK_CONTROL && !permission.required));
        assert!(!creator
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == STUDIO_NOTES));
        assert!(!creator
            .connection
            .secret_slots
            .iter()
            .any(|slot| slot.id == OBS_PASSWORD_SLOT));
        assert!(creator
            .connection
            .secret_slots
            .iter()
            .any(|slot| slot.id == STREAMDECK_TOKEN_SLOT && slot.entry_mode == "provider-managed"));
        assert!(allowed_permissions(CREATOR_SUITE_ID)
            .expect("creator permission allowlist")
            .contains(&&STREAMDECK_BRIDGE));
        assert!(allowed_permissions(CREATOR_SUITE_ID)
            .expect("creator permission allowlist")
            .contains(&&PULSAR_STREAMDECK_CONTROL));
        assert_eq!(
            secret_slot_permission(CREATOR_SUITE_ID, STREAMDECK_TOKEN_SLOT),
            Some(STREAMDECK_BRIDGE)
        );
        assert!(!secret_slot_user_writable(
            CREATOR_SUITE_ID,
            STREAMDECK_TOKEN_SLOT
        ));
        assert!(!legacy_cleanup_secret(
            CREATOR_SUITE_ID,
            STREAMDECK_TOKEN_SLOT
        ));
        assert!(legacy_cleanup_secret(CREATOR_SUITE_ID, OBS_PASSWORD_SLOT));
    }

    #[test]
    fn echo_streamdeck_action_has_its_own_opt_in_permission() {
        let echo = catalog()
            .into_iter()
            .find(|manifest| manifest.id == ECHO_PLUGIN_ID)
            .expect("Echo Stream Deck manifest");
        assert!(echo
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == ECHO_COPY && !permission.required));
        assert!(echo
            .demanded_permissions
            .iter()
            .any(|permission| permission.id == ECHO_CAPTURE && !permission.required));
        assert!(allowed_permissions(ECHO_PLUGIN_ID)
            .expect("Echo permission allowlist")
            .contains(&&ECHO_COPY));
        assert!(allowed_permissions(ECHO_PLUGIN_ID)
            .expect("Echo permission allowlist")
            .contains(&&ECHO_CAPTURE));
    }

    #[test]
    fn only_user_input_secret_slots_accept_frontend_writes() {
        assert!(!secret_slot_user_writable(
            CREATOR_SUITE_ID,
            OBS_PASSWORD_SLOT
        ));
        assert!(!secret_slot_user_writable(
            CREATOR_SUITE_ID,
            STREAMDECK_TOKEN_SLOT
        ));
        assert!(!secret_slot_user_writable(
            GITHUB_PLUGIN_ID,
            GITHUB_TOKEN_SLOT
        ));
        assert!(!secret_slot_user_writable(
            YOUTUBE_PLUGIN_ID,
            YOUTUBE_REFRESH_SLOT
        ));
        assert!(retired_online_secret(GITHUB_PLUGIN_ID, GITHUB_TOKEN_SLOT));
        assert!(retired_online_secret(
            YOUTUBE_PLUGIN_ID,
            YOUTUBE_REFRESH_SLOT
        ));
        assert!(!secret_slot_permission(GITHUB_PLUGIN_ID, GITHUB_TOKEN_SLOT).is_some());
    }


}
