//! User-initiated local backup, validation, diagnostics and public release checks.
//! This module deliberately has no access to keyring contents and never exports database rows.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager};

const FORMAT: &str = "ORBIT_OS_BACKUP";
const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SETTINGS_BYTES: usize = 1024 * 1024;
const DATABASES: &[&str] = &[
    "activity-pulse.sqlite3",
    "creator-studio.sqlite3",
    "downloads-organizer.sqlite3",
    "fl-studio-project-shelf.sqlite3",
    "game-sessions.sqlite3",
    "pulsar.sqlite3",
    "workspaces.sqlite3",
];
const ECHO_DATABASE: &str = "echo.sqlite3";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupBundle {
    format: String,
    format_version: u8,
    app_version: String,
    created_at: String,
    settings: Value,
    #[serde(default)]
    timeline: Option<Value>,
    databases: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedRestoreManifest {
    format: String,
    format_version: u8,
    databases: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    path: String,
    bytes: u64,
    databases: Vec<String>,
    includes_echo: bool,
    created_at: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    app_version: String,
    created_at: String,
    databases: Vec<String>,
    includes_echo: bool,
    settings_included: bool,
    timeline_entries: usize,
    bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredClientData {
    settings: Option<Value>,
    timeline: Option<Value>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseHealth {
    name: String,
    present: bool,
    readable: bool,
    integrity: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    generated_at: String,
    app_version: String,
    operating_system: String,
    architecture: String,
    databases: Vec<DatabaseHealth>,
    privacy: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCheck {
    latest_version: String,
    current_version: String,
    update_available: bool,
    release_url: String,
    published_at: Option<String>,
    prerelease: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestoreStatus {
    pub pending: bool,
    pub quarantined: bool,
    pub error: Option<String>,
}

fn app_data(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données ORBIT indisponible : {e}"))
}

fn validate_new_output(path: &str) -> Result<PathBuf, String> {
    let output = PathBuf::from(path);
    if !output.is_absolute() || output.file_name().is_none() {
        return Err("Choisis un emplacement de fichier valide dans la fenêtre Windows.".into());
    }
    if !output
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("orbitbackup"))
    {
        return Err("Le fichier de sauvegarde doit porter l’extension .orbitbackup.".into());
    }
    Ok(output)
}

fn sqlite_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    // VACUUM INTO reads a transactionally consistent view, including committed WAL pages.
    // Quote the destination as a SQLite string literal; it cannot be supplied as a SQL value.
    let source_conn = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| {
            format!(
                "Impossible d’ouvrir {} : {e}",
                source.file_name().unwrap_or_default().to_string_lossy()
            )
        })?;
    let destination_text = destination.to_string_lossy().replace('\'', "''");
    source_conn
        .execute_batch(&format!("VACUUM INTO '{destination_text}'"))
        .map_err(|e| {
            format!(
                "Impossible de créer une copie cohérente de {} : {e}",
                source.file_name().unwrap_or_default().to_string_lossy()
            )
        })?;
    let check = Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("La copie SQLite n’a pas pu être vérifiée : {e}"))?;
    let integrity: String = check
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("Vérification de la copie SQLite impossible : {e}"))?;
    if integrity != "ok" {
        return Err(format!(
            "La base {} a échoué au contrôle d’intégrité.",
            source.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    Ok(())
}

fn validate_database_blob(name: &str, bytes: &[u8]) -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("orbit-backup-check-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).map_err(|e| format!("Préparation du contrôle impossible : {e}"))?;
    let result = (|| {
        let path = root.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Copie temporaire de {name} impossible : {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("Lecture de sauvegarde de {name} interrompue : {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Vérification de {name} impossible : {e}"))?;
        let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("La base {name} n’est pas un SQLite valide : {e}"))?;
        let integrity: String = db
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("Vérification de {name} impossible : {e}"))?;
        if integrity != "ok" {
            return Err(format!("La base {name} a échoué au contrôle d’intégrité."));
        }
        validate_database_schema(name, &db)?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(root);
    result
}

fn expected_schema(
    name: &str,
) -> Option<(
    &'static [(&'static str, &'static str)],
    &'static [(&'static str, &'static str)],
)> {
    Some(match name {
        "activity-pulse.sqlite3" => (&[
            ("preferences", "id:INTEGER,monitoring_enabled:INTEGER,count_background_apps:INTEGER,idle_threshold_seconds:INTEGER,retention_days:INTEGER,excluded_apps:TEXT"),
            ("app_categories", "app_id:TEXT,category:TEXT"), ("app_goals", "app_id:TEXT,app_name:TEXT,daily_target_minutes:INTEGER"),
            ("sessions", "id:INTEGER,app_id:TEXT,name:TEXT,day:TEXT,started_at:TEXT,ended_at:TEXT,active_milliseconds:INTEGER,tracking_mode:TEXT"),
        ], &[("activity_sessions_day_idx", "sessions"), ("activity_sessions_app_day_idx", "sessions")]),
        "echo.sqlite3" => (&[
            ("preferences", "id:INTEGER,monitoring_enabled:INTEGER,capture_images:INTEGER,retention_days:INTEGER,max_entry_bytes:INTEGER,excluded_apps:TEXT"),
            ("entries", "id:TEXT,kind:TEXT,created_at:TEXT,text_content:TEXT,image_png:BLOB,preview:TEXT,byte_length:INTEGER,source_app:TEXT,favorite:INTEGER,pinned:INTEGER"),
            ("collections", "id:TEXT,name:TEXT,created_at:TEXT"), ("entry_collections", "entry_id:TEXT,collection_id:TEXT"),
        ], &[("entries_created_idx", "entries")]),
        "creator-studio.sqlite3" => (&[("studio_notes", "project_id:TEXT,style:TEXT,bpm:INTEGER,musical_key:TEXT,notes:TEXT,updated_at:TEXT")], &[]),
        "fl-studio-project-shelf.sqlite3" => (&[
            ("projects", "id:TEXT,path:TEXT,name:TEXT,pinned:INTEGER,added_at:TEXT,modified_at:TEXT,size_bytes:INTEGER"),
            ("project_tags", "project_id:TEXT,tag:TEXT"),
        ], &[("projects_name_idx", "projects")]),
        "game-sessions.sqlite3" => (&[
            ("game_preferences", "id:INTEGER,enabled:INTEGER"), ("tracked_games", "executable:TEXT"),
            ("game_sessions", "id:INTEGER,executable:TEXT,started_at:TEXT,ended_at:TEXT,seconds:INTEGER"),
        ], &[("game_sessions_ended_idx", "game_sessions"), ("game_sessions_one_open_idx", "game_sessions")]),
        "downloads-organizer.sqlite3" => (&[
            ("organizer_settings", "id:INTEGER,root:TEXT,enabled:INTEGER"),
            ("organizer_rules", "id:TEXT,name:TEXT,extensions:TEXT,contains:TEXT,target_folder:TEXT,enabled:INTEGER"),
            ("organizer_runs", "id:TEXT,completed_at:TEXT,skipped:INTEGER"),
            ("organizer_moves", "id:INTEGER,run_id:TEXT,source:TEXT,destination:TEXT,bytes:INTEGER,undone:INTEGER"),
        ], &[("organizer_moves_run_idx", "organizer_moves")]),
        "pulsar.sqlite3" => (&[
            ("scenarios", "id:TEXT,name:TEXT,enabled:INTEGER,json:TEXT,updated_at:TEXT"),
            ("run_logs", "id:TEXT,scenario_id:TEXT,scenario_name:TEXT,status:TEXT,summary:TEXT,at:TEXT,simulation:INTEGER"),
            ("trigger_state", "scenario_id:TEXT,fingerprint:TEXT,updated_at:TEXT"), ("pulsar_meta", "key:TEXT,value:TEXT"),
        ], &[("run_logs_at_idx", "run_logs")]),
        "workspaces.sqlite3" => (&[
            ("workspaces", "id:TEXT,name:TEXT,created_at:TEXT"),
            ("workspace_apps", "id:TEXT,workspace_id:TEXT,name:TEXT,path:TEXT,position:INTEGER"),
        ], &[("workspace_apps_order_idx", "workspace_apps")]),
        _ => return None,
    })
}

fn expected_index_columns(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "activity_sessions_day_idx" => &["day"],
        "activity_sessions_app_day_idx" => &["app_id", "day"],
        "entries_created_idx" => &["created_at"],
        "projects_name_idx" => &["name"],
        "game_sessions_ended_idx" => &["ended_at"],
        "game_sessions_one_open_idx" => &["executable"],
        "organizer_moves_run_idx" => &["run_id"],
        "run_logs_at_idx" => &["at"],
        "workspace_apps_order_idx" => &["workspace_id", "position"],
        _ => return None,
    })
}

fn expected_index_flags(name: &str) -> Option<(bool, bool)> {
    Some(match name {
        "game_sessions_one_open_idx" => (true, true),
        "activity_sessions_day_idx"
        | "activity_sessions_app_day_idx"
        | "entries_created_idx"
        | "projects_name_idx"
        | "game_sessions_ended_idx"
        | "organizer_moves_run_idx"
        | "run_logs_at_idx"
        | "workspace_apps_order_idx" => (false, false),
        _ => return None,
    })
}

fn is_optional_legacy_column(database: &str, table: &str, column: &str) -> bool {
    database == "activity-pulse.sqlite3"
        && matches!(
            (table, column),
            ("preferences", "count_background_apps")
                | ("app_goals", "app_name")
                | ("sessions", "tracking_mode")
        )
}

fn validate_database_schema(name: &str, db: &Connection) -> Result<(), String> {
    let (tables, indexes) =
        expected_schema(name).ok_or_else(|| format!("Nom de base non autorisé : {name}"))?;
    let mut known_tables = std::collections::HashSet::new();
    let mut statement = db
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        )
        .map_err(|e| format!("Schéma de {name} illisible : {e}"))?;
    let objects = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| format!("Schéma de {name} illisible : {e}"))?;
    for object in objects {
        let (kind, object_name, table_name, sql) = object.map_err(|e| e.to_string())?;
        match kind.as_str() {
            "table" => {
                let Some((_, columns)) = tables.iter().find(|(table, _)| *table == object_name)
                else {
                    return Err(format!("La base {name} contient une table inattendue."));
                };
                if sql
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains("create virtual table")
                {
                    return Err(format!(
                        "La base {name} contient une table virtuelle refusée."
                    ));
                }
                known_tables.insert(object_name.clone());
                let expected = columns
                    .split(',')
                    .filter_map(|value| value.split_once(':'))
                    .collect::<Vec<_>>();
                let mut info = db
                    .prepare(&format!(
                        "PRAGMA table_xinfo(\"{}\")",
                        object_name.replace('"', "\"\"")
                    ))
                    .map_err(|e| format!("Colonnes de {object_name} illisibles : {e}"))?;
                let actual = info
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(6)?,
                        ))
                    })
                    .map_err(|e| format!("Colonnes de {object_name} illisibles : {e}"))?;
                let mut actual_names = std::collections::HashSet::new();
                for column in actual {
                    let (column_name, column_type, hidden) = column.map_err(|e| e.to_string())?;
                    actual_names.insert(column_name.clone());
                    if hidden != 0
                        || !expected.iter().any(|(expected_name, expected_type)| {
                            *expected_name == column_name
                                && expected_type.eq_ignore_ascii_case(&column_type)
                        })
                    {
                        return Err(format!(
                            "La base {name} contient une colonne inattendue dans {object_name}."
                        ));
                    }
                }
                for (expected_name, _) in &expected {
                    if !actual_names.contains(*expected_name)
                        && !is_optional_legacy_column(name, &object_name, expected_name)
                    {
                        return Err(format!(
                            "La base {name} ne contient pas la colonne requise {object_name}.{expected_name}."
                        ));
                    }
                }
            }
            "index" => {
                if !indexes.iter().any(|(expected_name, expected_table)| {
                    *expected_name == object_name && *expected_table == table_name
                }) {
                    return Err(format!("La base {name} contient un index inattendu."));
                }
                let expected_columns = expected_index_columns(&object_name)
                    .ok_or_else(|| format!("Index non autorisé dans {name}."))?;
                let mut info = db
                    .prepare(&format!(
                        "PRAGMA index_info(\"{}\")",
                        object_name.replace('"', "\"\"")
                    ))
                    .map_err(|e| format!("Index {object_name} illisible : {e}"))?;
                let actual = info
                    .query_map([], |row| row.get::<_, Option<String>>(2))
                    .map_err(|e| format!("Index {object_name} illisible : {e}"))?;
                let actual = actual
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                if actual.iter().any(Option::is_none)
                    || actual
                        .iter()
                        .filter_map(Option::as_deref)
                        .ne(expected_columns.iter().copied())
                {
                    return Err(format!(
                        "L’index {object_name} utilise des colonnes inattendues."
                    ));
                }
                let expected_flags = expected_index_flags(&object_name)
                    .ok_or_else(|| format!("Options d’index non autorisées dans {name}."))?;
                let mut list = db
                    .prepare(&format!(
                        "PRAGMA index_list(\"{}\")",
                        table_name.replace('"', "\"\"")
                    ))
                    .map_err(|e| format!("Liste d’index de {table_name} illisible : {e}"))?;
                let entries = list
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, bool>(4)?,
                        ))
                    })
                    .map_err(|e| format!("Liste d’index de {table_name} illisible : {e}"))?;
                let flags = entries
                    .filter_map(Result::ok)
                    .find(|(index_name, _, _)| index_name == &object_name)
                    .map(|(_, unique, partial)| (unique, partial));
                if flags != Some(expected_flags) {
                    return Err(format!("L’index {object_name} a des options inattendues."));
                }
            }
            _ => {
                return Err(format!(
                    "La base {name} contient un objet SQL interdit ({kind})."
                ))
            }
        }
    }
    let expected_tables = tables
        .iter()
        .map(|(table, _)| (*table).to_owned())
        .collect::<std::collections::HashSet<_>>();
    if known_tables != expected_tables {
        return Err(format!(
            "La base {name} ne contient pas exactement les tables ORBIT attendues."
        ));
    }
    Ok(())
}

fn sanitize_settings(value: Option<Value>) -> Result<Value, String> {
    let Some(value) = value else {
        return Ok(Value::Null);
    };
    if value.is_null() {
        return Ok(Value::Null);
    }
    let encoded = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err("Les préférences dépassent la taille maximale autorisée.".into());
    }
    let object = value
        .as_object()
        .ok_or("Les préférences locales sont illisibles.")?;
    // Fixed allowlist prevents a future setting from accidentally carrying credentials.
    let allowed = [
        "version",
        "locale",
        "onboardingComplete",
        "sidebarCompact",
        "immersive",
        "reduceMotion",
        "theme",
        "accentColor",
        "panelColor",
        "raisedColor",
        "orbitalVisualization",
        "coreRefreshSeconds",
        "coreHistoryMinutes",
        "timelineRetentionDays",
        "alerts",
        "widgetOrder",
        "hiddenWidgets",
        "themeColorProfiles",
        "orbitalEnabled",
        "orbitalRotationSpeed",
        "orbitalParticlesEnabled",
        "orbitalIntensity",
    ];
    let mut clean = object
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()))
        .filter(|(key, value)| match key.as_str() {
            "version" => value.as_u64() == Some(1),
            "locale" => value.as_str() == Some("fr"),
            "onboardingComplete"
            | "sidebarCompact"
            | "immersive"
            | "reduceMotion"
            | "orbitalEnabled"
            | "orbitalParticlesEnabled" => value.is_boolean(),
            "theme" => value.as_str().is_some_and(|v| {
                [
                    "sidereal", "nebula", "obsidian", "lunar", "aurora", "solar", "abyss",
                ]
                .contains(&v)
            }),
            "orbitalVisualization" => value.as_str().is_some_and(|v| {
                [
                    "earth",
                    "ringed-planet",
                    "lunar-station",
                    "nebula",
                    "core",
                    "wireframe",
                    "constellations",
                ]
                .contains(&v)
            }),
            "orbitalIntensity" => value
                .as_str()
                .is_some_and(|v| ["low", "balanced", "high"].contains(&v)),
            "orbitalRotationSpeed" => value.as_f64().is_some_and(|v| [0.5, 1.0, 1.5].contains(&v)),
            "coreRefreshSeconds" => value.as_u64().is_some_and(|v| [2, 5, 10].contains(&v)),
            "coreHistoryMinutes" => value.as_u64().is_some_and(|v| [5, 15, 60].contains(&v)),
            "timelineRetentionDays" => value.as_u64().is_some_and(|v| [7, 30, 90].contains(&v)),
            "accentColor" | "panelColor" | "raisedColor" => value.as_str().is_some_and(|v| {
                v.is_empty()
                    || (v.len() == 7
                        && v.starts_with('#')
                        && v[1..].chars().all(|c| c.is_ascii_hexdigit()))
            }),
            "themeColorProfiles" => value.is_object(),
            "alerts" => value.is_object(),
            "widgetOrder" | "hiddenWidgets" => value.is_array(),
            _ => false,
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    clean.insert("version".into(), json!(1));
    clean.insert("locale".into(), json!("fr"));
    if let Some(profiles) = object.get("themeColorProfiles") {
        let mut sanitized = serde_json::Map::new();
        if let Some(themes) = profiles.as_object() {
            for theme in [
                "sidereal", "nebula", "obsidian", "lunar", "aurora", "solar", "abyss",
            ] {
                let Some(colors) = themes.get(theme).and_then(Value::as_object) else {
                    continue;
                };
                let mut profile = serde_json::Map::new();
                for color in ["accentColor", "panelColor", "raisedColor"] {
                    if let Some(value) = colors.get(color).and_then(Value::as_str) {
                        if value.is_empty()
                            || (value.len() == 7
                                && value.starts_with('#')
                                && value[1..].chars().all(|c| c.is_ascii_hexdigit()))
                        {
                            profile.insert(color.into(), Value::String(value.to_uppercase()));
                        }
                    }
                }
                if !profile.is_empty() {
                    sanitized.insert(theme.into(), Value::Object(profile));
                }
            }
        }
        clean.insert("themeColorProfiles".into(), Value::Object(sanitized));
    }
    if let Some(alerts) = object.get("alerts").and_then(Value::as_object) {
        let mut sanitized = serde_json::Map::new();
        for (key, min, max) in [
            ("cpuPercent", 10.0, 100.0),
            ("memoryPercent", 10.0, 100.0),
            ("diskFreePercent", 1.0, 50.0),
        ] {
            if let Some(value) = alerts
                .get(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= min && *v <= max)
            {
                sanitized.insert(key.into(), json!(value));
            }
        }
        clean.insert("alerts".into(), Value::Object(sanitized));
    }
    for key in ["widgetOrder", "hiddenWidgets"] {
        if let Some(values) = object.get(key).and_then(Value::as_array) {
            let sanitized = values
                .iter()
                .filter_map(Value::as_str)
                .filter(|v| !v.is_empty() && v.len() <= 128)
                .take(100)
                .map(|v| Value::String(v.into()))
                .collect::<Vec<_>>();
            clean.insert(key.into(), Value::Array(sanitized));
        }
    }
    Ok(Value::Object(clean))
}

fn sanitize_timeline(value: Option<Value>) -> Result<Option<Value>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let rows = value
        .as_array()
        .ok_or("L’historique Timeline est illisible.")?;
    if rows.len() > 500 {
        return Err("Timeline dépasse la limite de 500 événements.".into());
    }
    let mut clean = Vec::with_capacity(rows.len());
    for row in rows {
        let object = row
            .as_object()
            .ok_or("Un événement Timeline est invalide.")?;
        let string = |key: &str, maximum: usize| -> Result<String, String> {
            object
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| text.len() <= maximum)
                .map(str::to_owned)
                .ok_or_else(|| format!("Un champ Timeline ({key}) est invalide."))
        };
        let category = string("category", 16)?;
        if category == "atlas" { continue; }
        if !["echo", "pulsar", "core", "system"].contains(&category.as_str()) {
            return Err("Un événement Timeline a une catégorie inconnue.".into());
        }
        let mut item = serde_json::Map::new();
        item.insert("id".into(), Value::String(string("id", 256)?));
        item.insert("category".into(), Value::String(category));
        item.insert("title".into(), Value::String(string("title", 1024)?));
        item.insert("at".into(), Value::String(string("at", 128)?));
        if object.contains_key("detail") {
            item.insert("detail".into(), Value::String(string("detail", 4096)?));
        }
        clean.push(Value::Object(item));
    }
    Ok(Some(Value::Array(clean)))
}

#[tauri::command]
pub fn maintenance_backup_create(
    app: AppHandle,
    output_path: String,
    include_echo: bool,
    settings: Option<Value>,
    include_timeline: bool,
    timeline: Option<Value>,
) -> Result<BackupResult, String> {
    let output = validate_new_output(&output_path)?;
    let parent = output.parent().ok_or("Dossier de destination invalide.")?;
    if !parent.is_dir() {
        return Err("Le dossier de destination n’existe pas.".into());
    }
    if output.exists() {
        return Err(
            "Ce fichier existe déjà. Choisis un nouveau nom pour éviter de l’écraser.".into(),
        );
    }
    let data = app_data(&app)?;
    let temp = std::env::temp_dir().join(format!("orbit-backup-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&temp)
        .map_err(|e| format!("Impossible de préparer la sauvegarde temporaire : {e}"))?;
    let result = (|| {
        let mut databases = BTreeMap::new();
        let names = DATABASES
            .iter()
            .copied()
            .chain(include_echo.then_some(ECHO_DATABASE));
        let mut total_snapshot_bytes = 0_u64;
        for name in names {
            let source = data.join(name);
            if !source.is_file() {
                continue;
            }
            let snapshot = temp.join(name);
            sqlite_snapshot(&source, &snapshot)?;
            let bytes =
                fs::read(&snapshot).map_err(|e| format!("Lecture de la copie impossible : {e}"))?;
            total_snapshot_bytes = total_snapshot_bytes.saturating_add(bytes.len() as u64);
            if total_snapshot_bytes > MAX_BACKUP_BYTES / 2 {
                return Err(
                    "Les données sélectionnées dépassent la limite de sauvegarde de 256 Mo.".into(),
                );
            }
            databases.insert(name.to_string(), STANDARD.encode(bytes));
        }
        let created_at = Utc::now().to_rfc3339();
        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: 1,
            app_version: env!("CARGO_PKG_VERSION").into(),
            created_at: created_at.clone(),
            settings: sanitize_settings(settings)?,
            timeline: if include_timeline {
                sanitize_timeline(timeline)?
            } else {
                None
            },
            databases,
        };
        let encoded = serde_json::to_vec(&bundle)
            .map_err(|e| format!("Encodage de sauvegarde impossible : {e}"))?;
        if encoded.len() as u64 > MAX_BACKUP_BYTES {
            return Err("La sauvegarde dépasse la limite de 512 Mo.".into());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|e| format!("Impossible de créer le fichier de sauvegarde : {e}"))?;
        file.write_all(&encoded).map_err(|e| {
            let _ = fs::remove_file(&output);
            format!("Écriture de la sauvegarde interrompue : {e}")
        })?;
        file.sync_all().map_err(|e| {
            let _ = fs::remove_file(&output);
            format!("Finalisation de la sauvegarde interrompue : {e}")
        })?;
        let names = bundle.databases.keys().cloned().collect();
        Ok(BackupResult {
            path: output.display().to_string(),
            bytes: encoded.len() as u64,
            databases: names,
            includes_echo: include_echo && bundle.databases.contains_key(ECHO_DATABASE),
            created_at,
        })
    })();
    let _ = fs::remove_dir_all(temp);
    result
}

fn read_bundle(path: &str) -> Result<(BackupBundle, u64), String> {
    let source = PathBuf::from(path);
    if !source.is_absolute() || !source.is_file() {
        return Err("Choisis un fichier de sauvegarde ORBIT existant.".into());
    }
    let metadata = fs::metadata(&source).map_err(|e| format!("Fichier inaccessible : {e}"))?;
    if metadata.len() > MAX_BACKUP_BYTES {
        return Err("Le fichier dépasse la limite de 512 Mo.".into());
    }
    let bytes = fs::read(source).map_err(|e| format!("Impossible de lire la sauvegarde : {e}"))?;
    let mut bundle: BackupBundle = serde_json::from_slice(&bytes)
        .map_err(|_| "Ce fichier n’est pas une sauvegarde ORBIT valide.".to_owned())?;
    if bundle.format != FORMAT || bundle.format_version != 1 {
        return Err("Cette version de sauvegarde n’est pas prise en charge.".into());
    }
    for (name, encoded) in &bundle.databases {
        if !DATABASES.contains(&name.as_str()) && name != ECHO_DATABASE {
            return Err("La sauvegarde contient un nom de base non autorisé.".into());
        }
        let decoded = STANDARD
            .decode(encoded)
            .map_err(|_| format!("La copie de {name} est endommagée."))?;
        if decoded.len() as u64 > MAX_BACKUP_BYTES / 2 {
            return Err(format!("La copie de {name} dépasse la limite."));
        }
        validate_database_blob(name, &decoded)?;
    }
    bundle.settings = sanitize_settings(Some(bundle.settings))?;
    bundle.timeline = sanitize_timeline(bundle.timeline)?;
    Ok((bundle, metadata.len()))
}

#[tauri::command]
pub fn maintenance_backup_preview(backup_path: String) -> Result<BackupPreview, String> {
    let (bundle, bytes) = read_bundle(&backup_path)?;
    Ok(BackupPreview {
        app_version: bundle.app_version,
        created_at: bundle.created_at,
        databases: bundle.databases.keys().cloned().collect(),
        includes_echo: bundle.databases.contains_key(ECHO_DATABASE),
        settings_included: !bundle.settings.is_null(),
        timeline_entries: bundle
            .timeline
            .as_ref()
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        bytes,
    })
}

/// Stages a validated restore. The integration layer must apply the staged package on the next
/// launch before opening module databases; live SQLite connections are never replaced in-place.
#[tauri::command]
pub fn maintenance_backup_stage_restore(
    app: AppHandle,
    backup_path: String,
    confirmed: bool,
) -> Result<BackupPreview, String> {
    if !confirmed {
        return Err("Confirme le remplacement des données ORBIT avant de continuer.".into());
    }
    let (bundle, bytes) = read_bundle(&backup_path)?;
    if bundle.databases.is_empty()
        && bundle.settings.is_null()
        && bundle.timeline.is_none()
    {
        return Err("Cette sauvegarde ne contient aucune donnée ORBIT à restaurer.".into());
    }
    let root = app_data(&app)?.join("pending-restore");
    if root.exists() {
        return Err(
            "Une restauration est déjà en attente. Redémarre ORBIT avant d’en préparer une autre."
                .into(),
        );
    }
    fs::create_dir_all(&root)
        .map_err(|e| format!("Préparation de la restauration impossible : {e}"))?;
    let result = (|| {
        for (name, encoded) in &bundle.databases {
            let decoded = STANDARD
                .decode(encoded)
                .map_err(|_| format!("La copie de {name} est endommagée."))?;
            let destination = root.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|e| format!("Écriture de {name} impossible : {e}"))?;
            file.write_all(&decoded)
                .map_err(|e| format!("Écriture de {name} interrompue : {e}"))?;
            file.sync_all()
                .map_err(|e| format!("Finalisation de {name} interrompue : {e}"))?;
            let check =
                Connection::open_with_flags(root.join(name), OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| format!("La base {name} est invalide : {e}"))?;
            let integrity: String = check
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if integrity != "ok" {
                return Err(format!("La base {name} a échoué au contrôle d’intégrité."));
            }
            validate_database_schema(name, &check)?;
            drop(check);
        }
        if !bundle.settings.is_null() {
            let settings = serde_json::to_vec(&sanitize_settings(Some(bundle.settings.clone()))?)
                .map_err(|e| e.to_string())?;
            fs::write(root.join("settings.json"), settings)
                .map_err(|e| format!("Préférences de restauration illisibles : {e}"))?;
        }
        if let Some(timeline) = &bundle.timeline {
            fs::write(
                root.join("timeline.json"),
                serde_json::to_vec(
                    &sanitize_timeline(Some(timeline.clone()))?.unwrap_or(Value::Null),
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Écriture Timeline impossible : {e}"))?;
        }
        fs::write(root.join("manifest.json"), serde_json::to_vec(&json!({"format":FORMAT,"formatVersion":1,"appVersion":bundle.app_version,"createdAt":bundle.created_at,"databases":bundle.databases.keys().collect::<Vec<_>>()})).map_err(|e| e.to_string())?).map_err(|e| format!("Manifeste de restauration impossible : {e}"))?;
        Ok(BackupPreview {
            app_version: bundle.app_version,
            created_at: bundle.created_at,
            databases: bundle.databases.keys().cloned().collect(),
            includes_echo: bundle.databases.contains_key(ECHO_DATABASE),
            settings_included: !bundle.settings.is_null(),
            timeline_entries: bundle
                .timeline
                .as_ref()
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            bytes,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    } else {
        let _ = fs::remove_file(restore_failure_path(&app_data(&app)?));
    }
    result
}

/// The integration layer calls this from Tauri setup before any module database is opened.
/// Each replacement is staged beside its destination, integrity-checked, then atomically renamed.
/// Existing databases are moved to a rollback folder first; any partial failure is reverted.
/// One-time removal of the retired Atlas Gemini key from this app's Windows Credential Manager.
/// It touches only the exact legacy service/account and writes its marker after successful deletion.
pub fn cleanup_retired_atlas_data(app: &AppHandle) -> Result<(), String> {
    let data = app_data(app)?;
    let marker = data.join("migrations").join("retired-atlas-gemini-key.v1");
    if marker.is_file() { return Ok(()); }
    let entry = keyring::Entry::new("ORBIT OS", "Gemini API key")
        .map_err(|_| "Impossible d’ouvrir l’entrée d’identifiant Atlas retirée.".to_owned())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {},
        Err(_) => return Err("Impossible de supprimer l’ancien identifiant Gemini du coffre Windows.".into()),
    }
    let parent = marker.parent().ok_or("Dossier de migration indisponible.")?;
    fs::create_dir_all(parent).map_err(|_| "Impossible de créer le marqueur de migration Atlas.".to_owned())?;
    fs::write(marker, b"done").map_err(|_| "Impossible d’enregistrer le nettoyage Atlas.".to_owned())
}

pub fn apply_staged_restore(app: &AppHandle) -> Result<bool, String> {
    apply_staged_restore_resilient_at(&app_data(app)?)
}

fn restore_failure_path(data: &Path) -> PathBuf {
    data.join("restore-failure.json")
}

fn read_restore_status_at(data: &Path) -> PendingRestoreStatus {
    let failure: Option<Value> = fs::read(restore_failure_path(data))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let pending = data.join("pending-restore").join("manifest.json").is_file();
    PendingRestoreStatus {
        pending,
        quarantined: failure
            .as_ref()
            .and_then(|value| value.get("quarantined"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        error: failure.and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
    }
}

fn quarantine_failed_restore_at(data: &Path, error: &str) -> Result<(), String> {
    let pending = data.join("pending-restore");
    let quarantine = data.join(format!("restore-quarantine-{}", uuid::Uuid::new_v4()));
    let mut quarantined = false;
    if pending.exists() {
        match fs::rename(&pending, &quarantine) {
            Ok(()) => quarantined = true,
            Err(rename_error) => {
                // Marker prevents the same failing package from aborting or retrying on every launch.
                let safe_error =
                    format!("{error} (mise en quarantaine impossible : {rename_error})");
                let state = json!({"error":safe_error,"quarantined":false});
                fs::write(
                    restore_failure_path(data),
                    serde_json::to_vec(&state).map_err(|e| e.to_string())?,
                )
                .map_err(|e| {
                    format!("État d’échec de restauration impossible à enregistrer : {e}")
                })?;
                return Ok(());
            }
        }
    }
    let state = json!({"error":error,"quarantined":quarantined});
    fs::write(
        restore_failure_path(data),
        serde_json::to_vec(&state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("État d’échec de restauration impossible à enregistrer : {e}"))?;
    Ok(())
}

fn apply_staged_restore_resilient_at(data: &Path) -> Result<bool, String> {
    let status = read_restore_status_at(data);
    if status.error.is_some() {
        // A prior failure is visible in the app UI. Do not retry it during every startup.
        return Ok(false);
    }
    match apply_staged_restore_at(data) {
        Ok(applied) => Ok(applied),
        Err(error) => {
            let _ = quarantine_failed_restore_at(data, &error);
            Err(error)
        }
    }
}

fn apply_staged_restore_at(data: &Path) -> Result<bool, String> {
    let root = data.join("pending-restore");
    let _ = fs::remove_file(data.join("atlas-conversations-restored.json"));
    let manifest_path = root.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let _ = fs::remove_file(root.join("atlas-conversations.json"));
    let manifest: StagedRestoreManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|e| format!("Restauration en attente illisible : {e}"))?,
    )
    .map_err(|_| "Le manifeste de restauration est invalide.".to_owned())?;
    if manifest.format != FORMAT || manifest.format_version != 1 {
        return Err("Le manifeste de restauration ne correspond pas à ORBIT.".into());
    }
    if manifest.databases.is_empty()
        && !root.join("settings.json").is_file()
        && !root.join("timeline.json").is_file()
    {
        let _ = fs::remove_dir_all(&root);
        return Ok(false);
    }
    for name in [
        "pending-restored-settings.json",
        "timeline-restored.json",
    ] {
        if data.join(name).exists() {
            return Err(
                "Des données restaurées précédentes attendent encore leur import dans ORBIT."
                    .into(),
            );
        }
    }
    let mut unique = std::collections::HashSet::new();
    for name in &manifest.databases {
        if (!DATABASES.contains(&name.as_str()) && name != ECHO_DATABASE) || !unique.insert(name) {
            return Err("Le manifeste contient un nom de base non autorisé.".into());
        }
        let staged = root.join(name);
        let metadata = fs::symlink_metadata(&staged)
            .map_err(|e| format!("La copie de {name} manque : {e}"))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "La copie de {name} n’est pas un fichier ordinaire."
            ));
        }
        let check = Connection::open_with_flags(&staged, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("La copie de {name} est invalide : {e}"))?;
        let integrity: String = check
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("Vérification de {name} impossible : {e}"))?;
        if integrity != "ok" {
            return Err(format!(
                "La copie de {name} a échoué au contrôle d’intégrité."
            ));
        }
        validate_database_schema(name, &check)?;
    }
    let rollback = data.join(format!("restore-rollback-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&rollback)
        .map_err(|e| format!("Impossible de créer la restauration de sécurité : {e}"))?;
    let rollback_name = rollback
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let rollback_pointer = data.join("last-restore-rollback.txt");
    let old_pointer_contents = fs::read_to_string(&rollback_pointer).ok();
    let previous_rollback = old_pointer_contents.as_ref().and_then(|name| {
        let path = PathBuf::from(name.as_str());
        let file = path.file_name()?.to_str()?;
        (file == name.as_str() && file.starts_with("restore-rollback-")).then(|| data.join(file))
    });
    let previous_pointer = previous_rollback.as_ref().map(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    fs::write(&rollback_pointer, rollback_name.as_bytes()).map_err(|e| {
        let _ = fs::remove_dir_all(&rollback);
        if let Some(contents) = old_pointer_contents {
            let _ = fs::write(&rollback_pointer, contents);
        }
        format!("Impossible de préparer le retour arrière : {e}")
    })?;
    let mut replaced: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
    let apply = (|| {
        for name in &manifest.databases {
            let source = root.join(name);
            let target = data.join(name);
            let previous = rollback.join(name);
            let temp_target = data.join(format!(".{name}.restore-{}.tmp", uuid::Uuid::new_v4()));
            fs::copy(&source, &temp_target)
                .map_err(|e| format!("Préparation de {name} impossible : {e}"))?;
            let copied =
                Connection::open_with_flags(&temp_target, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| format!("Copie temporaire de {name} invalide : {e}"))?;
            let integrity: String = copied
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if integrity != "ok" {
                drop(copied);
                let _ = fs::remove_file(&temp_target);
                return Err(format!("Copie temporaire de {name} endommagée."));
            }
            if let Err(error) = validate_database_schema(name, &copied) {
                drop(copied);
                let _ = fs::remove_file(&temp_target);
                return Err(error);
            }
            drop(copied);
            let existed = match fs::symlink_metadata(&target) {
                Ok(meta) if !meta.file_type().is_file() => {
                    let _ = fs::remove_file(&temp_target);
                    return Err(format!(
                        "La destination {name} n’est pas un fichier ordinaire."
                    ));
                }
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    let _ = fs::remove_file(&temp_target);
                    return Err(format!(
                        "Impossible de vérifier la destination {name} : {error}"
                    ));
                }
            };
            if existed {
                fs::rename(&target, &previous)
                    .map_err(|e| format!("Impossible de protéger la base actuelle {name} : {e}"))?;
            }
            if let Err(error) = fs::rename(&temp_target, &target) {
                if existed {
                    let _ = fs::rename(&previous, &target);
                }
                let _ = fs::remove_file(&temp_target);
                return Err(format!("Impossible d’appliquer la base {name} : {error}"));
            }
            replaced.push((target, previous, existed));
        }
        let settings = root.join("settings.json");
        if settings.is_file() {
            let staged_settings: Value =
                serde_json::from_slice(&fs::read(&settings).map_err(|e| e.to_string())?)
                    .map_err(|_| "Préférences de restauration invalides.".to_owned())?;
            let clean = sanitize_settings(Some(staged_settings))?;
            let final_settings = data.join("pending-restored-settings.json");
            if final_settings.exists() {
                return Err(
                    "Des préférences restaurées attendent déjà leur import dans l’application."
                        .into(),
                );
            }
            fs::write(
                final_settings,
                serde_json::to_vec(&clean).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Préférences de restauration impossibles à préparer : {e}"))?;
        }
        for (staged_name, final_name) in [
            ("timeline.json", "timeline-restored.json"),
        ] {
            let staged = root.join(staged_name);
            if staged.is_file() {
                let restored = data.join(final_name);
                if restored.exists() {
                    return Err(format!(
                        "Des données restaurées attendent déjà leur import ({staged_name})."
                    ));
                }
                let value: Value =
                    serde_json::from_slice(&fs::read(&staged).map_err(|e| e.to_string())?)
                        .map_err(|_| format!("Donnée restaurée invalide : {staged_name}"))?;
                let clean = sanitize_timeline(Some(value))?.unwrap_or(Value::Null);
                fs::write(
                    &restored,
                    serde_json::to_vec(&clean).map_err(|e| e.to_string())?,
                )
                .map_err(|e| format!("Préparation de {staged_name} impossible : {e}"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = apply {
        for (target, previous, existed) in replaced.iter().rev() {
            let _ = fs::remove_file(target);
            if *existed {
                let _ = fs::rename(previous, target);
            }
        }
        let _ = fs::remove_dir_all(&rollback);
        for name in [
            "pending-restored-settings.json",
            "timeline-restored.json",
            ] {
            let _ = fs::remove_file(data.join(name));
        }
        if let Some(previous) = previous_pointer {
            let _ = fs::write(&rollback_pointer, previous);
        } else {
            let _ = fs::remove_file(&rollback_pointer);
        }
        return Err(error);
    }
    if let Some(previous) = previous_rollback {
        if previous != rollback {
            let _ = fs::remove_dir_all(previous);
        }
    }
    // Remove the manifest first, so a cleanup failure cannot cause the same restore to repeat.
    fs::remove_file(&manifest_path).map_err(|e| {
        format!("Bases appliquées. Le manifeste reste à nettoyer manuellement : {e}")
    })?;
    let _ = fs::remove_dir_all(root);
    Ok(true)
}

/// Called before the app reads settings and loads its client-side modules.
#[tauri::command]
pub fn maintenance_take_restored_client_data(
    app: AppHandle,
) -> Result<Option<RestoredClientData>, String> {
    let data = app_data(&app)?;
    let read = |name: &str| -> Result<Option<Value>, String> {
        let path = data.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        let value = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|_| format!("Donnée restaurée invalide : {name}"))?;
        Ok(Some(value))
    };
    let settings = read("pending-restored-settings.json")?
        .map(|v| sanitize_settings(Some(v)))
        .transpose()?;
    let timeline = read("timeline-restored.json")?
        .map(|v| sanitize_timeline(Some(v)))
        .transpose()?
        .flatten();
    if settings.is_none() && timeline.is_none() {
        return Ok(None);
    }
    Ok(Some(RestoredClientData {
        settings,
        timeline,
    }))
}

#[tauri::command]
pub fn maintenance_acknowledge_restored_client_data(app: AppHandle) -> Result<(), String> {
    let data = app_data(&app)?;
    for name in [
        "pending-restored-settings.json",
        "timeline-restored.json",
    ] {
        let path = data.join(name);
        if path.is_file() {
            fs::remove_file(path)
                .map_err(|e| format!("Impossible de finaliser la restauration ({name}) : {e}"))?;
        }
    }
    Ok(())
}

/// Reports whether a restore has been staged and still needs its next-start application.
#[tauri::command]
pub fn maintenance_pending_restore_status(app: AppHandle) -> Result<PendingRestoreStatus, String> {
    Ok(read_restore_status_at(&app_data(&app)?))
}

fn collect_diagnostics(app: &AppHandle) -> Result<DiagnosticReport, String> {
    let data = app_data(&app)?;
    let mut databases = Vec::new();
    for name in DATABASES.iter().copied().chain([ECHO_DATABASE]) {
        let path = data.join(name);
        if !path.is_file() {
            databases.push(DatabaseHealth {
                name: name.into(),
                present: false,
                readable: false,
                integrity: "absente".into(),
            });
            continue;
        }
        let check = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY);
        let (readable, integrity) = match check {
            Ok(connection) => {
                let value: Result<String, _> =
                    connection.query_row("PRAGMA integrity_check", [], |row| row.get(0));
                match value {
                    Ok(value) if value == "ok" => (true, "ok".into()),
                    Ok(_) => (true, "anomalie détectée".into()),
                    Err(_) => (false, "contrôle impossible".into()),
                }
            }
            Err(_) => (false, "lecture impossible".into()),
        };
        databases.push(DatabaseHealth {
            name: name.into(),
            present: true,
            readable,
            integrity,
        });
    }
    Ok(DiagnosticReport { generated_at: Utc::now().to_rfc3339(), app_version: env!("CARGO_PKG_VERSION").into(), operating_system: std::env::consts::OS.into(), architecture: std::env::consts::ARCH.into(), databases, privacy: "Rapport local : aucun chemin utilisateur, contenu Echo, conversation, secret, jeton ou nom d’application n’est inclus.".into() })
}

#[tauri::command]
pub fn maintenance_diagnostics(app: AppHandle) -> Result<DiagnosticReport, String> {
    collect_diagnostics(&app)
}

#[tauri::command]
pub fn maintenance_diagnostics_export(app: AppHandle, output_path: String) -> Result<(), String> {
    let output = PathBuf::from(output_path);
    if !output.is_absolute() || output.exists() {
        return Err("Choisis un nouveau fichier de destination pour le rapport.".into());
    }
    if output.extension().and_then(|v| v.to_str()) != Some("json") {
        return Err("Le rapport de diagnostic doit être exporté en JSON.".into());
    }
    // Recompute the report in native code. Never accept a frontend-supplied object as an
    // allegedly redacted diagnostic export.
    let report = collect_diagnostics(&app)?;
    let encoded = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    if encoded.len() > 1024 * 1024 {
        return Err("Le rapport de diagnostic dépasse la taille autorisée.".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|e| format!("Création du rapport impossible : {e}"))?;
    file.write_all(&encoded)
        .map_err(|e| format!("Écriture du rapport interrompue : {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Finalisation du rapport interrompue : {e}"))
}

fn parse_version(raw: &str) -> Option<(Vec<u64>, Option<String>)> {
    let raw = raw.trim().trim_start_matches('v').split('+').next()?;
    let (numeric, pre) = raw
        .split_once('-')
        .map_or((raw, None), |(a, b)| (a, Some(b.to_owned())));
    let parts = numeric
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    Some((parts, pre))
}

fn compare_prerelease(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let left_parts = left.split('.').collect::<Vec<_>>();
    let right_parts = right.split('.').collect::<Vec<_>>();
    for (a, b) in left_parts.iter().zip(&right_parts) {
        let comparison = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if comparison != Ordering::Equal {
            return comparison;
        }
    }
    left_parts.len().cmp(&right_parts.len())
}

fn is_newer(latest: &str, current: &str) -> Result<bool, String> {
    let (a, ap) = parse_version(latest)
        .ok_or("La version publiée ne suit pas le format numérique attendu.")?;
    let (b, bp) = parse_version(current)
        .ok_or("La version locale ne suit pas le format numérique attendu.")?;
    for index in 0..a.len().max(b.len()) {
        let left = *a.get(index).unwrap_or(&0);
        let right = *b.get(index).unwrap_or(&0);
        if left != right {
            return Ok(left > right);
        }
    }
    Ok(match (ap.as_deref(), bp.as_deref()) {
        (None, None) => false,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(left), Some(right)) => compare_prerelease(left, right).is_gt(),
    })
}

#[tauri::command]
pub async fn maintenance_check_updates(
    owner: String,
    repository: String,
) -> Result<ReleaseCheck, String> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !valid(&owner) || !valid(&repository) {
        return Err(
            "Saisis le propriétaire et le dépôt publics GitHub au format owner/repo.".into(),
        );
    }
    let url = format!("https://api.github.com/repos/{owner}/{repository}/releases/latest");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("ORBIT-OS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Connexion de mise à jour indisponible.".to_string())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| {
            "Impossible de joindre GitHub. Vérifie ta connexion et le dépôt public.".to_string()
        })?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(
            "Aucune version publiée trouvée. Vérifie le propriétaire et le dépôt public.".into(),
        );
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub a refusé la vérification (HTTP {}). Réessaie plus tard.",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 1024 * 1024)
    {
        return Err("La réponse GitHub dépasse la taille autorisée.".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Réponse GitHub incomplète.".to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("La réponse GitHub dépasse la taille autorisée.".into());
    }
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Réponse GitHub invalide.".to_string())?;
    let latest = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("La publication GitHub ne contient pas de version.")?;
    let release_url = body
        .get("html_url")
        .and_then(Value::as_str)
        .ok_or("La publication GitHub ne contient pas de lien.")?;
    if !release_url.starts_with(&format!(
        "https://github.com/{owner}/{repository}/releases/"
    )) {
        return Err("Lien de publication GitHub inattendu.".into());
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    Ok(ReleaseCheck {
        latest_version: latest.into(),
        current_version: current.clone(),
        update_available: is_newer(latest, &current)?,
        release_url: release_url.into(),
        published_at: body
            .get("published_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        prerelease: body
            .get("prerelease")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("orbit-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn make_db(path: &Path, value: &str) {
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE IF NOT EXISTS studio_notes(project_id TEXT PRIMARY KEY,style TEXT NOT NULL,bpm INTEGER,musical_key TEXT NOT NULL,notes TEXT NOT NULL,updated_at TEXT NOT NULL); DELETE FROM studio_notes;")
            .unwrap();
        db.execute("INSERT INTO studio_notes(project_id,style,musical_key,notes,updated_at) VALUES('test',?1,'C','note','today')", [value])
            .unwrap();
    }
    fn read_db(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT style FROM studio_notes WHERE project_id='test'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn compares_semantic_release_versions_without_string_ordering() {
        assert!(is_newer("v1.10.0", "1.9.9").unwrap());
        assert!(!is_newer("1.2.0", "1.2.0").unwrap());
        assert!(is_newer("1.2.0", "1.2.0-beta.1").unwrap());
        assert!(!is_newer("1.2.0-rc.1", "1.2.0").unwrap());
        assert!(is_newer("1.2.0-beta.2", "1.2.0-beta.1").unwrap());
        assert!(!is_newer("1.2.0-beta.1", "1.2.0-beta.2").unwrap());
        assert!(is_newer("1.2.1", "1.2.0").unwrap());
    }
    #[test]
    fn settings_backup_uses_fixed_non_secret_allowlist() {
        let cleaned = sanitize_settings(Some(json!({
            "theme":"obsidian", "apiKey":"do-not-copy", "secretToken":"do-not-copy",
            "widgetOrder":["cpu", {"secret":"hidden"}],
            "alerts":{"cpuPercent":90,"token":"hidden"},
            "orbitalEnabled":{"token":"hidden"},
            "themeColorProfiles":{"nebula":{"accentColor":"#A982F0","apiKey":"hidden"}}
        })))
        .unwrap();
        assert_eq!(
            cleaned.get("theme").and_then(Value::as_str),
            Some("obsidian")
        );
        assert!(cleaned.get("apiKey").is_none());
        assert!(cleaned.get("secretToken").is_none());
        assert!(cleaned["alerts"].get("token").is_none());
        assert_eq!(cleaned["alerts"]["cpuPercent"].as_f64(), Some(90.0));
        assert!(cleaned.get("orbitalEnabled").is_none());
        assert!(cleaned["themeColorProfiles"]["nebula"]
            .get("apiKey")
            .is_none());
        assert_eq!(
            cleaned["themeColorProfiles"]["nebula"]["accentColor"],
            "#A982F0"
        );
    }
    #[test]
    fn legacy_atlas_timeline_records_are_filtered_without_dropping_other_categories() {
        let rows = sanitize_timeline(Some(json!([
            {"id":"old","category":"atlas","title":"Legacy","at":"a"},
            {"id":"echo","category":"echo","title":"Clip","at":"b"}
        ]))).unwrap().unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["category"], "echo");
    }

    #[test]
    fn legacy_atlas_backup_payload_is_ignored() {
        let bundle: BackupBundle = serde_json::from_value(json!({
            "format": FORMAT, "formatVersion": 1, "appVersion": "1.5.3",
            "createdAt": "2026-10-30T00:00:00Z", "settings": {},
            "timeline": null, "atlasConversations": {"malformed": true}, "databases": {}
        })).expect("legacy Atlas field is ignored");
        assert!(bundle.timeline.is_none());
        assert!(bundle.databases.is_empty());
    }

    #[test]
    fn bundle_preview_validates_database_and_sanitizes_optional_client_data() {
        let root = temp_dir("bundle-preview");
        let database = root.join("creator-studio.sqlite3");
        make_db(&database, "valid");
        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: 1,
            app_version: "1.5.0".into(),
            created_at: "2026-10-30T00:00:00Z".into(),
            settings: json!({"theme":"obsidian", "geminiKey":"private"}),
            timeline: Some(
                json!([{"id":"1","category":"system","title":"Build","at":"today","secret":"private"}]),
            ),
            databases: BTreeMap::from([(
                "creator-studio.sqlite3".into(),
                STANDARD.encode(fs::read(&database).unwrap()),
            )]),
        };
        let file = root.join("valid.orbitbackup");
        fs::write(&file, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let (checked, bytes) = read_bundle(file.to_str().unwrap()).unwrap();
        assert_eq!(checked.databases.len(), 1);
        assert_eq!(checked.timeline.as_ref().unwrap()[0].get("secret"), None);
        assert_eq!(checked.settings.get("geminiKey"), None);
        assert!(bytes > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restore_stage_prevalidation_rejects_unexpected_schema_and_preserves_active_database() {
        let root = temp_dir("stage-schema-rejected");
        let active = root.join("active.sqlite3");
        make_db(&active, "keep-this");
        let external = root.join("external.sqlite3");
        make_db(&external, "unexpected");
        let connection = Connection::open(&external).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER unexpected_trigger AFTER INSERT ON studio_notes BEGIN SELECT 1; END;",
            )
            .unwrap();
        drop(connection);
        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: 1,
            app_version: "1.5.0".into(),
            created_at: "2026-10-30T00:00:00Z".into(),
            settings: Value::Null,
            timeline: None,
            databases: BTreeMap::from([(
                "creator-studio.sqlite3".into(),
                STANDARD.encode(fs::read(&external).unwrap()),
            )]),
        };
        let backup = root.join("unexpected.orbitbackup");
        fs::write(&backup, serde_json::to_vec(&bundle).unwrap()).unwrap();

        assert!(read_bundle(backup.to_str().unwrap()).is_err());
        assert_eq!(read_db(&active), "keep-this");
        assert!(!root.join("pending-restore").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_restore_is_quarantined_and_reported_without_repeating_on_startup() {
        let data = temp_dir("restore-quarantine");
        make_db(&data.join("creator-studio.sqlite3"), "keep-this");
        let stage = data.join("pending-restore");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("creator-studio.sqlite3"), b"not sqlite").unwrap();
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec(
                &json!({"format":FORMAT,"formatVersion":1,"databases":["creator-studio.sqlite3"]}),
            )
            .unwrap(),
        )
        .unwrap();

        assert!(apply_staged_restore_resilient_at(&data).is_err());
        let status = read_restore_status_at(&data);
        assert!(!status.pending);
        assert!(status.quarantined);
        assert!(status
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty()));
        assert_eq!(read_db(&data.join("creator-studio.sqlite3")), "keep-this");
        let quarantines = fs::read_dir(&data)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("restore-quarantine-")
            })
            .count();
        assert_eq!(quarantines, 1);
        assert!(!apply_staged_restore_resilient_at(&data).unwrap());
        assert_eq!(
            quarantines,
            fs::read_dir(&data)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("restore-quarantine-"))
                .count()
        );
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn staged_restore_replaces_only_allowlisted_present_databases_and_keeps_rollback() {
        let data = temp_dir("restore");
        make_db(&data.join("creator-studio.sqlite3"), "newer-local-state");
        make_db(&data.join("pulsar.sqlite3"), "not-in-backup");
        let stage = data.join("pending-restore");
        fs::create_dir(&stage).unwrap();
        make_db(&stage.join("creator-studio.sqlite3"), "restored-state");
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec(
                &json!({"format":FORMAT,"formatVersion":1,"databases":["creator-studio.sqlite3"]}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(apply_staged_restore_at(&data).unwrap());
        assert_eq!(
            read_db(&data.join("creator-studio.sqlite3")),
            "restored-state"
        );
        assert_eq!(read_db(&data.join("pulsar.sqlite3")), "not-in-backup");
        let rollback_name = fs::read_to_string(data.join("last-restore-rollback.txt")).unwrap();
        assert_eq!(
            read_db(&data.join(rollback_name).join("creator-studio.sqlite3")),
            "newer-local-state"
        );
        assert!(!stage.exists());
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn invalid_staged_database_does_not_replace_current_data() {
        let data = temp_dir("restore-invalid");
        make_db(&data.join("creator-studio.sqlite3"), "keep-this");
        let stage = data.join("pending-restore");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("creator-studio.sqlite3"), b"not sqlite").unwrap();
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec(
                &json!({"format":FORMAT,"formatVersion":1,"databases":["creator-studio.sqlite3"]}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(apply_staged_restore_at(&data).is_err());
        assert_eq!(read_db(&data.join("creator-studio.sqlite3")), "keep-this");
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn staged_database_with_trigger_is_rejected_and_active_database_is_preserved() {
        let data = temp_dir("restore-trigger");
        make_db(&data.join("creator-studio.sqlite3"), "keep-this");
        let stage = data.join("pending-restore");
        fs::create_dir(&stage).unwrap();
        let staged = stage.join("creator-studio.sqlite3");
        make_db(&staged, "triggered");
        let db = Connection::open(&staged).unwrap();
        db.execute_batch(
            "CREATE TRIGGER unexpected_trigger AFTER INSERT ON studio_notes BEGIN SELECT 1; END;",
        )
        .unwrap();
        drop(db);
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec(
                &json!({"format":FORMAT,"formatVersion":1,"databases":["creator-studio.sqlite3"]}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(apply_staged_restore_at(&data).is_err());
        assert_eq!(read_db(&data.join("creator-studio.sqlite3")), "keep-this");
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn sqlite_snapshot_is_integrity_checked_and_includes_committed_data() {
        let dir =
            std::env::temp_dir().join(format!("orbit-snapshot-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let source = dir.join("source.sqlite3");
        let target = dir.join("snapshot.sqlite3");
        let db = Connection::open(&source).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE items(value TEXT); INSERT INTO items VALUES('kept');").unwrap();
        drop(db);
        sqlite_snapshot(&source, &target).unwrap();
        let snapshot = Connection::open(&target).unwrap();
        let value: String = snapshot
            .query_row("SELECT value FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "kept");
        let _ = fs::remove_dir_all(dir);
    }
}
