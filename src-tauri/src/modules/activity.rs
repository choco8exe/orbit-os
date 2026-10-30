use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{Datelike, Local, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};

const PLUGIN_ID: &str = "com.orbitos.activity-pulse";
const TRACK_PERMISSION: &str = "activity-pulse.track-active-apps";
const CHANGE_EVENT: &str = "activity-pulse://changed";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_CREDITED_GAP: Duration = Duration::from_secs(6);
const MAX_EXCLUDED_APPS: usize = 500;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ActivityCategory {
    Game,
    Development,
    Creative,
    Browser,
    Other,
}

impl ActivityCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Development => "development",
            Self::Creative => "creative",
            Self::Browser => "browser",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPreferences {
    pub monitoring_enabled: bool,
    pub count_background_apps: bool,
    pub idle_threshold_seconds: u32,
    pub retention_days: u32,
    pub excluded_apps: Vec<String>,
}

impl Default for ActivityPreferences {
    fn default() -> Self {
        Self {
            monitoring_enabled: false,
            count_background_apps: false,
            idle_threshold_seconds: 300,
            retention_days: 30,
            excluded_apps: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPreferencesUpdate {
    pub count_background_apps: bool,
    pub idle_threshold_seconds: u32,
    pub retention_days: u32,
    pub excluded_apps: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityAppUsage {
    pub app_id: String,
    pub name: String,
    pub category: ActivityCategory,
    pub total_seconds: u64,
    pub background_seconds: u64,
    pub session_count: u64,
    pub daily_seconds: BTreeMap<String, u64>,
    pub daily_background_seconds: BTreeMap<String, u64>,
    pub daily_target_minutes: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDailyGoal {
    pub app_id: String,
    pub app_name: String,
    pub daily_target_minutes: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDayUsage {
    pub date: String,
    pub total_seconds: u64,
    pub background_seconds: u64,
    pub categories: BTreeMap<String, u64>,
    pub applications: BTreeMap<String, u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPulseSnapshot {
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub captured_at: String,
    pub range: String,
    pub total_seconds: u64,
    pub background_seconds: u64,
    pub apps: Vec<ActivityAppUsage>,
    pub daily_goals: Vec<ActivityDailyGoal>,
    pub days: Vec<ActivityDayUsage>,
    pub preferences: ActivityPreferences,
}

struct ActivityInner {
    db: Mutex<Connection>,
    preferences: RwLock<ActivityPreferences>,
    monitoring: AtomicBool,
}

pub struct ActivityPulseState(Arc<ActivityInner>);

impl ActivityInner {
    fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Dossier ORBIT Pulse inaccessible : {e}"))?;
        }
        let db =
            Connection::open(path).map_err(|e| format!("Base ORBIT Pulse inaccessible : {e}"))?;
        db.busy_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS preferences (
               id INTEGER PRIMARY KEY CHECK(id=1), monitoring_enabled INTEGER NOT NULL,
               count_background_apps INTEGER NOT NULL DEFAULT 0,
               idle_threshold_seconds INTEGER NOT NULL, retention_days INTEGER NOT NULL,
               excluded_apps TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS app_categories (
               app_id TEXT PRIMARY KEY, category TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS app_goals (
               app_id TEXT PRIMARY KEY, app_name TEXT NOT NULL DEFAULT '', daily_target_minutes INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
               id INTEGER PRIMARY KEY AUTOINCREMENT, app_id TEXT NOT NULL,
               name TEXT NOT NULL, day TEXT NOT NULL, started_at TEXT NOT NULL,
               ended_at TEXT, active_milliseconds INTEGER NOT NULL DEFAULT 0,
               tracking_mode TEXT NOT NULL DEFAULT 'foreground'
             );
             CREATE INDEX IF NOT EXISTS activity_sessions_day_idx ON sessions(day);
             CREATE INDEX IF NOT EXISTS activity_sessions_app_day_idx ON sessions(app_id,day);
             "
        ).map_err(|e| format!("Impossible d’initialiser ORBIT Pulse : {e}"))?;
        ensure_column(
            &db,
            "preferences",
            "count_background_apps",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &db,
            "sessions",
            "tracking_mode",
            "TEXT NOT NULL DEFAULT 'foreground'",
        )?;
        ensure_column(&db, "app_goals", "app_name", "TEXT NOT NULL DEFAULT ''")?;
        db.execute("INSERT OR IGNORE INTO preferences(id,monitoring_enabled,count_background_apps,idle_threshold_seconds,retention_days,excluded_apps) VALUES(1,0,0,300,30,'[]')", []).map_err(|e| format!("Impossible d’initialiser les réglages Pulse : {e}"))?;
        let preferences = read_preferences(&db)?;
        Ok(Self {
            monitoring: AtomicBool::new(preferences.monitoring_enabled),
            preferences: RwLock::new(preferences),
            db: Mutex::new(db),
        })
    }
}

/// Starts the local app sampler during application setup (not when Pulse's route opens). Tracking is
/// opt-in and the worker rechecks the plugin grant on every sample. It never reads window titles,
/// URLs, input text, screenshots or files.
pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données ORBIT indisponible : {e}"))?;
    path.push("activity-pulse.sqlite3");
    let inner = Arc::new(ActivityInner::open(path)?);
    let worker_inner = inner.clone();
    let worker_app = app.clone();
    app.manage(ActivityPulseState(inner));
    thread::Builder::new()
        .name("orbit-activity-pulse".into())
        .spawn(move || activity_worker(worker_app, worker_inner))
        .map_err(|e| format!("Impossible de démarrer ORBIT Pulse : {e}"))?;
    Ok(())
}

pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    let state = app
        .try_state::<ActivityPulseState>()
        .ok_or_else(|| "ORBIT Pulse n’est pas initialisé.".to_string())?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    db.execute("DELETE FROM sessions", [])
        .map_err(|e| format!("Impossible d’effacer l’historique Pulse : {e}"))?;
    db.execute("DELETE FROM app_categories", [])
        .map_err(|e| format!("Impossible d’effacer les catégories Pulse : {e}"))?;
    db.execute("DELETE FROM app_goals", [])
        .map_err(|e| format!("Impossible d’effacer les objectifs Pulse : {e}"))?;
    db.execute("UPDATE preferences SET monitoring_enabled=0,count_background_apps=0,idle_threshold_seconds=300,retention_days=30,excluded_apps='[]' WHERE id=1", []).map_err(|e| format!("Impossible de réinitialiser les réglages Pulse : {e}"))?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| format!("Impossible de finaliser l’effacement Pulse : {e}"))?;
    drop(db);
    state.0.monitoring.store(false, Ordering::Release);
    if let Ok(mut preferences) = state.0.preferences.write() {
        *preferences = ActivityPreferences::default();
    }
    Ok(())
}

#[tauri::command]
pub fn activity_pulse_snapshot(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    range: String,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    make_snapshot(&state.0, &range)
}

#[tauri::command]
pub fn activity_pulse_set_monitoring(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    enabled: bool,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    if enabled {
        crate::plugins::require_permission(&app, PLUGIN_ID, TRACK_PERMISSION)?;
    }
    let mut preferences = state
        .0
        .preferences
        .write()
        .map_err(|_| "Réglages ORBIT Pulse verrouillés.".to_string())?;
    preferences.monitoring_enabled = enabled;
    let serialized =
        serde_json::to_string(&preferences.excluded_apps).map_err(|e| e.to_string())?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    db.execute(
        "UPDATE preferences SET monitoring_enabled=?1,excluded_apps=?2 WHERE id=1",
        params![enabled as i64, serialized],
    )
    .map_err(|e| format!("Impossible d’enregistrer le suivi : {e}"))?;
    state.0.monitoring.store(enabled, Ordering::Release);
    drop(db);
    drop(preferences);
    make_snapshot(&state.0, "week")
}

#[tauri::command]
pub fn activity_pulse_set_preferences(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    preferences: ActivityPreferencesUpdate,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    if !(30..=300).contains(&preferences.idle_threshold_seconds) {
        return Err("Le seuil d’inactivité doit être compris entre 30 et 300 secondes.".into());
    }
    if ![7, 30, 90, 365].contains(&preferences.retention_days) {
        return Err("Choisis une durée de conservation proposée par ORBIT Pulse.".into());
    }
    let excluded = normalize_exclusions(preferences.excluded_apps);
    let mut current = state
        .0
        .preferences
        .write()
        .map_err(|_| "Réglages ORBIT Pulse verrouillés.".to_string())?;
    current.idle_threshold_seconds = preferences.idle_threshold_seconds;
    current.retention_days = preferences.retention_days;
    current.excluded_apps = excluded;
    current.count_background_apps = preferences.count_background_apps;
    let serialized = serde_json::to_string(&current.excluded_apps).map_err(|e| e.to_string())?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    db.execute("UPDATE preferences SET count_background_apps=?1,idle_threshold_seconds=?2,retention_days=?3,excluded_apps=?4 WHERE id=1", params![current.count_background_apps as i64,current.idle_threshold_seconds, current.retention_days, serialized]).map_err(|e| format!("Impossible d’enregistrer les réglages : {e}"))?;
    cleanup_expired(&db, current.retention_days)?;
    drop(db);
    drop(current);
    make_snapshot(&state.0, "week")
}

#[tauri::command]
pub fn activity_pulse_set_category(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    app_id: String,
    category: ActivityCategory,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    if app_id.len() > 160 || app_id.contains(['/', '\\', ':']) {
        return Err("Identifiant d’application invalide.".into());
    }
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    db.execute("INSERT INTO app_categories(app_id,category) VALUES(?1,?2) ON CONFLICT(app_id) DO UPDATE SET category=excluded.category", params![app_id.to_ascii_lowercase(), category.as_str()]).map_err(|e| format!("Impossible d’enregistrer la catégorie : {e}"))?;
    drop(db);
    make_snapshot(&state.0, "week")
}

#[tauri::command]
pub fn activity_pulse_set_app_goal(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    app_id: String,
    target_minutes: Option<u32>,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    let app_id = normalize_app_id(&app_id)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    store_daily_target(&db, &app_id, target_minutes)?;
    drop(db);
    make_snapshot(&state.0, "week")
}

#[tauri::command]
pub fn activity_pulse_clear_history(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
    before: Option<String>,
) -> Result<ActivityPulseSnapshot, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    match before {
        Some(date) => {
            let day = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .map_err(|_| "Date d’effacement invalide : utilise AAAA-MM-JJ.".to_string())?;
            db.execute("DELETE FROM sessions WHERE day < ?1", [day.to_string()])
                .map_err(|e| format!("Impossible d’effacer l’historique : {e}"))?;
        }
        None => {
            db.execute("DELETE FROM sessions", [])
                .map_err(|e| format!("Impossible d’effacer l’historique : {e}"))?;
        }
    }
    drop(db);
    make_snapshot(&state.0, "week")
}

#[tauri::command]
pub fn activity_pulse_export_csv(
    app: AppHandle,
    state: State<'_, ActivityPulseState>,
) -> Result<String, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    let mut statement = db.prepare(
        "SELECT s.day,s.name,s.app_id,coalesce(c.category,'other'),
         sum(CASE WHEN s.tracking_mode='foreground' THEN s.active_milliseconds ELSE 0 END)/1000,
         sum(CASE WHEN s.tracking_mode='background' THEN s.active_milliseconds ELSE 0 END)/1000
         FROM sessions s LEFT JOIN app_categories c ON c.app_id=s.app_id
         GROUP BY s.day,s.app_id,s.name,c.category ORDER BY s.day DESC,sum(s.active_milliseconds) DESC"
    ).map_err(|e| format!("Impossible de préparer l’export : {e}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, u64>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut csv = String::from(
        "date,application,identifiant,categorie,secondes_premier_plan,secondes_arriere_plan\r\n",
    );
    for row in rows {
        let (day, name, app_id, category, foreground, background) =
            row.map_err(|e| e.to_string())?;
        csv.push_str(&format!(
            "{},{},{},{},{},{}\r\n",
            csv_cell(&day),
            csv_cell(&name),
            csv_cell(&app_id),
            csv_cell(&category),
            foreground,
            background
        ));
    }
    Ok(csv)
}

fn read_preferences(db: &Connection) -> Result<ActivityPreferences, String> {
    let (enabled,background,idle,retention,excluded):(i64,i64,u32,u32,String) = db.query_row("SELECT monitoring_enabled,count_background_apps,idle_threshold_seconds,retention_days,excluded_apps FROM preferences WHERE id=1", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).map_err(|e| format!("Impossible de lire les réglages ORBIT Pulse : {e}"))?;
    let excluded_apps: Vec<String> = serde_json::from_str(&excluded).unwrap_or_default();
    Ok(ActivityPreferences {
        monitoring_enabled: enabled != 0,
        count_background_apps: background != 0,
        idle_threshold_seconds: idle.clamp(30, 300),
        retention_days: if [7, 30, 90, 365].contains(&retention) {
            retention
        } else {
            30
        },
        excluded_apps,
    })
}

fn ensure_column(
    db: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut statement = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    for existing in columns {
        if existing.map_err(|e| e.to_string())? == column {
            return Ok(());
        }
    }
    db.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition}"
    ))
    .map_err(|e| format!("Impossible de mettre à jour le stockage Pulse : {e}"))
}

fn normalize_exclusions(apps: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for app in apps.into_iter().take(MAX_EXCLUDED_APPS) {
        let value = app.trim().to_ascii_lowercase();
        if !value.is_empty()
            && value.len() <= 160
            && !value.contains(['/', '\\', ':'])
            && !normalized.contains(&value)
        {
            normalized.push(value);
        }
    }
    normalized
}

fn normalize_app_id(app_id: &str) -> Result<String, String> {
    let value = app_id.trim();
    if value.is_empty()
        || value.len() > 160
        || value.contains(['/', '\\', ':'])
        || value.chars().any(char::is_control)
    {
        return Err("Identifiant d’application invalide.".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn validate_daily_target(minutes: u32) -> Result<(), String> {
    if !(5..=1440).contains(&minutes) {
        return Err("Un objectif quotidien doit être compris entre 5 minutes et 24 heures.".into());
    }
    Ok(())
}

fn store_daily_target(
    db: &Connection,
    app_id: &str,
    target_minutes: Option<u32>,
) -> Result<(), String> {
    match target_minutes {
        Some(minutes) => {
            validate_daily_target(minutes)?;
            let app_name = db
                .query_row(
                    "SELECT name FROM sessions WHERE app_id=?1 ORDER BY id DESC LIMIT 1",
                    [app_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| format!("Impossible de retrouver le nom de l’application : {e}"))?
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| app_display_name(app_id));
            db.execute("INSERT INTO app_goals(app_id,app_name,daily_target_minutes) VALUES(?1,?2,?3) ON CONFLICT(app_id) DO UPDATE SET app_name=excluded.app_name,daily_target_minutes=excluded.daily_target_minutes", params![app_id, app_name, minutes])
                .map_err(|e| format!("Impossible d’enregistrer l’objectif quotidien : {e}"))?;
        }
        None => {
            db.execute("DELETE FROM app_goals WHERE app_id=?1", [app_id])
                .map_err(|e| format!("Impossible de supprimer l’objectif quotidien : {e}"))?;
        }
    }
    Ok(())
}

fn app_display_name(app_id: &str) -> String {
    app_id.strip_suffix(".exe").unwrap_or(app_id).to_string()
}

fn read_daily_goals(db: &Connection) -> Result<Vec<ActivityDailyGoal>, String> {
    let mut statement = db.prepare("SELECT app_id,app_name,daily_target_minutes FROM app_goals ORDER BY app_name COLLATE NOCASE,app_id")
        .map_err(|e| format!("Impossible de lire les objectifs Pulse : {e}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })
        .map_err(|e| format!("Impossible de lire les objectifs Pulse : {e}"))?;
    let mut goals = Vec::new();
    for row in rows {
        let (app_id, app_name, daily_target_minutes) = row.map_err(|e| e.to_string())?;
        let app_name = if app_name.trim().is_empty() {
            app_display_name(&app_id)
        } else {
            app_name
        };
        goals.push(ActivityDailyGoal {
            app_id,
            app_name,
            daily_target_minutes,
        });
    }
    Ok(goals)
}

fn make_snapshot(inner: &ActivityInner, range: &str) -> Result<ActivityPulseSnapshot, String> {
    if !["day", "week", "month"].contains(&range) {
        return Err("Période ORBIT Pulse inconnue.".into());
    }
    let db = inner
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.")?;
    let daily_goals = read_daily_goals(&db)?;
    let daily_targets: BTreeMap<String, u32> = daily_goals
        .iter()
        .map(|goal| (goal.app_id.clone(), goal.daily_target_minutes))
        .collect();
    // Read back the durable values so another app instance can't leave a stale dashboard.
    let persisted = read_preferences(&db)?;
    drop(db);
    let preferences = ActivityPreferences {
        monitoring_enabled: inner.monitoring.load(Ordering::Acquire)
            && persisted.monitoring_enabled,
        ..persisted
    };
    let now = Local::now();
    let today = now.date_naive();
    let start_day = match range {
        "day" => today,
        "week" => today - chrono::Days::new(6),
        _ => NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
            .ok_or_else(|| "Date locale invalide.".to_string())?,
    };
    let start_day_text = start_day.to_string();
    let now_utc = Utc::now().to_rfc3339();
    let db = inner
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.".to_string())?;
    let mut app_statement = db.prepare(
        "SELECT s.app_id,s.name,coalesce(c.category,'other'),
         sum(CASE WHEN s.tracking_mode='foreground' THEN s.active_milliseconds ELSE 0 END),
         sum(CASE WHEN s.tracking_mode='background' THEN s.active_milliseconds ELSE 0 END),count(*)
         FROM sessions s LEFT JOIN app_categories c ON c.app_id=s.app_id
         WHERE s.day>=?1 GROUP BY s.app_id,s.name,c.category ORDER BY sum(CASE WHEN s.tracking_mode='foreground' THEN s.active_milliseconds ELSE 0 END) DESC"
    ).map_err(|e| e.to_string())?;
    let app_rows = app_statement
        .query_map([&start_day_text], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, u64>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut apps = Vec::new();
    let mut per_app_days = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut per_app_background_days = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut detail_statement = db.prepare(
        "SELECT app_id,day,tracking_mode,sum(active_milliseconds) FROM sessions WHERE day>=?1 GROUP BY app_id,day,tracking_mode"
    ).map_err(|e| e.to_string())?;
    let detail_rows = detail_statement
        .query_map([&start_day_text], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in detail_rows {
        let (app_id, day, mode, milliseconds) = row.map_err(|e| e.to_string())?;
        let target = if mode == "background" {
            &mut per_app_background_days
        } else {
            &mut per_app_days
        };
        target
            .entry(app_id)
            .or_default()
            .insert(day, milliseconds / 1000);
    }
    for row in app_rows {
        let (app_id, name, category, seconds, background_seconds, count) =
            row.map_err(|e| e.to_string())?;
        let daily_seconds = per_app_days.remove(&app_id).unwrap_or_default();
        let daily_background_seconds = per_app_background_days.remove(&app_id).unwrap_or_default();
        let daily_target_minutes = daily_targets.get(&app_id).copied();
        apps.push(ActivityAppUsage {
            app_id,
            name,
            category: parse_category(&category),
            total_seconds: seconds / 1000,
            background_seconds: background_seconds / 1000,
            session_count: count,
            daily_seconds,
            daily_background_seconds,
            daily_target_minutes,
        });
    }
    let mut daily_statement = db.prepare(
        "SELECT s.day,coalesce(c.category,'other'),sum(CASE WHEN s.tracking_mode='foreground' THEN s.active_milliseconds ELSE 0 END),sum(CASE WHEN s.tracking_mode='background' THEN s.active_milliseconds ELSE 0 END)
         FROM sessions s LEFT JOIN app_categories c ON c.app_id=s.app_id
         WHERE s.day>=?1 GROUP BY s.day,c.category"
    ).map_err(|e| e.to_string())?;
    let daily_rows = daily_statement
        .query_map([&start_day_text], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut daily = BTreeMap::<String, (BTreeMap<String, u64>, u64)>::new();
    for row in daily_rows {
        let (day, category, foreground, background) = row.map_err(|e| e.to_string())?;
        let entry = daily.entry(day).or_default();
        *entry.0.entry(category).or_default() += foreground;
        entry.1 += background;
    }
    let mut application_day_statement = db.prepare("SELECT day,app_id,sum(active_milliseconds) FROM sessions WHERE day>=?1 AND tracking_mode='foreground' GROUP BY day,app_id").map_err(|e| e.to_string())?;
    let application_day_rows = application_day_statement
        .query_map([&start_day_text], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut applications_by_day = BTreeMap::<String, BTreeMap<String, u64>>::new();
    for row in application_day_rows {
        let (day, app_id, milliseconds) = row.map_err(|e| e.to_string())?;
        applications_by_day
            .entry(day)
            .or_default()
            .insert(app_id, milliseconds / 1000);
    }
    let last_day = today;
    let day_count = (last_day - start_day).num_days().max(0) as usize + 1;
    let mut days = Vec::with_capacity(day_count);
    let categories = ["game", "development", "creative", "browser", "other"];
    for offset in 0..day_count {
        let day = (start_day + chrono::Days::new(offset as u64)).to_string();
        let values = daily.get(&day);
        let mut category_values = BTreeMap::new();
        let mut total = 0;
        for category in categories {
            let milliseconds = values
                .and_then(|map| map.0.get(category))
                .copied()
                .unwrap_or(0);
            let seconds = milliseconds / 1000;
            total += seconds;
            category_values.insert(category.to_string(), seconds);
        }
        let background = values.map(|value| value.1 / 1000).unwrap_or(0);
        let applications = applications_by_day.remove(&day).unwrap_or_default();
        days.push(ActivityDayUsage {
            date: day,
            total_seconds: total,
            background_seconds: background,
            categories: category_values,
            applications,
        });
    }
    let total_seconds = apps.iter().map(|app| app.total_seconds).sum();
    let background_seconds = apps.iter().map(|app| app.background_seconds).sum();
    Ok(ActivityPulseSnapshot {
        available: cfg!(windows),
        unavailable_reason: if cfg!(windows) {
            None
        } else {
            Some("La mesure des applications est disponible uniquement sous Windows.".into())
        },
        captured_at: now_utc,
        range: range.into(),
        total_seconds,
        background_seconds,
        apps,
        daily_goals,
        days,
        preferences,
    })
}

fn parse_category(value: &str) -> ActivityCategory {
    match value {
        "game" => ActivityCategory::Game,
        "development" => ActivityCategory::Development,
        "creative" => ActivityCategory::Creative,
        "browser" => ActivityCategory::Browser,
        _ => ActivityCategory::Other,
    }
}

fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn cleanup_expired(db: &Connection, retention_days: u32) -> Result<(), String> {
    let cutoff = Local::now().date_naive() - chrono::Days::new(retention_days as u64);
    db.execute("DELETE FROM sessions WHERE day < ?1", [cutoff.to_string()])
        .map_err(|e| format!("Impossible d’appliquer la conservation ORBIT Pulse : {e}"))?;
    Ok(())
}

struct ActiveSession {
    id: i64,
    app_id: String,
    day: String,
    last_sample: Instant,
}

fn activity_worker(app: AppHandle, inner: Arc<ActivityInner>) {
    let mut system = System::new();
    let mut current: Option<ActiveSession> = None;
    let mut background: BTreeMap<String, ActiveSession> = BTreeMap::new();
    let mut last_cleanup_day: Option<NaiveDate> = None;
    loop {
        thread::sleep(POLL_INTERVAL);
        let now_instant = Instant::now();
        let enabled = inner.monitoring.load(Ordering::Acquire);
        let granted = enabled
            && crate::plugins::has_permission(&app, PLUGIN_ID, TRACK_PERMISSION).unwrap_or(false);
        let preferences = match inner.preferences.read() {
            Ok(value) => value.clone(),
            Err(_) => continue,
        };
        let local_day = Local::now().date_naive();
        if last_cleanup_day != Some(local_day) {
            if let Ok(db) = inner.db.lock() {
                let _ = cleanup_expired(&db, preferences.retention_days);
            }
            last_cleanup_day = Some(local_day);
        }
        if enabled && !granted {
            pause_tracking(&inner);
            if let Some(session) = current.take() {
                let _ = finish_session(&inner, session.id);
                let _ = app.emit(CHANGE_EVENT, ());
            }
            finish_background_sessions(&inner, &mut background);
            continue;
        }
        if !granted
            || !preferences.monitoring_enabled
            || user_is_idle(preferences.idle_threshold_seconds)
        {
            if let Some(session) = current.take() {
                let _ = finish_session(&inner, session.id);
                let _ = app.emit(CHANGE_EVENT, ());
            }
            finish_background_sessions(&inner, &mut background);
            continue;
        }
        let foreground = foreground_app(&mut system);
        let app_info = match foreground {
            Some(info)
                if !preferences
                    .excluded_apps
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&info.app_id)) =>
            {
                Some(info)
            }
            _ => None,
        };
        let info = match app_info {
            Some(info) => info,
            None => {
                if let Some(session) = current.take() {
                    let _ = finish_session(&inner, session.id);
                    let _ = app.emit(CHANGE_EVENT, ());
                }
                finish_background_sessions(&inner, &mut background);
                continue;
            }
        };
        let today = Local::now().date_naive().to_string();
        let same = current
            .as_ref()
            .is_some_and(|session| session.app_id == info.app_id && session.day == today);
        if same {
            if let Some(session) = current.as_mut() {
                let elapsed = now_instant.saturating_duration_since(session.last_sample);
                session.last_sample = now_instant;
                if elapsed <= MAX_CREDITED_GAP {
                    let credited = credited_milliseconds(elapsed, MAX_CREDITED_GAP);
                    if credited > 0 {
                        let _ = add_session_milliseconds(&inner, session.id, credited);
                        let _ = app.emit(CHANGE_EVENT, ());
                    }
                } else {
                    let _ = finish_session(&inner, session.id);
                    if let Ok(id) = start_session(&inner, &info, &today, "foreground") {
                        *session = ActiveSession {
                            id,
                            app_id: info.app_id.clone(),
                            day: today.clone(),
                            last_sample: now_instant,
                        };
                    } else {
                        current = None;
                    }
                }
            }
        } else {
            if let Some(previous) = current.take() {
                let _ = finish_session(&inner, previous.id);
            }
            if let Ok(id) = start_session(&inner, &info, &today, "foreground") {
                current = Some(ActiveSession {
                    id,
                    app_id: info.app_id.clone(),
                    day: today.clone(),
                    last_sample: now_instant,
                });
            }
            let _ = app.emit(CHANGE_EVENT, ());
        }
        if preferences.count_background_apps {
            let candidates = visible_window_apps(&mut system)
                .into_iter()
                .filter(|(app_id, _)| app_id != &info.app_id)
                .filter(|(app_id, _)| {
                    !preferences
                        .excluded_apps
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(app_id))
                })
                .collect::<BTreeMap<_, _>>();
            update_background_sessions(&inner, &mut background, candidates, now_instant, &today);
        } else {
            finish_background_sessions(&inner, &mut background);
        }
    }
}

struct ForegroundApp {
    app_id: String,
    name: String,
}

#[cfg(windows)]
fn foreground_app(system: &mut System) -> Option<ForegroundApp> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }
    let pid = Pid::from_u32(process_id);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
    let process = system.process(pid)?;
    let file = process.exe()?.file_name()?.to_string_lossy().into_owned();
    let app_id = file.to_ascii_lowercase();
    if file.len() > 160 || is_shell_or_orbit_process(&app_id) {
        return None;
    }
    Some(ForegroundApp {
        app_id,
        name: file
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_string(),
    })
}

#[cfg(windows)]
fn visible_window_apps(system: &mut System) -> BTreeMap<String, ForegroundApp> {
    use windows::core::BOOL;
    use windows::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
    };
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
            }
            if pid != 0 {
                let pids = unsafe { &mut *(data.0 as *mut Vec<u32>) };
                pids.push(pid);
            }
        }
        BOOL(1)
    }
    let mut pids = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut pids as *mut Vec<u32> as isize));
    }
    pids.sort_unstable();
    pids.dedup();
    let ids = pids
        .iter()
        .map(|pid| Pid::from_u32(*pid))
        .collect::<Vec<_>>();
    system.refresh_processes(ProcessesToUpdate::Some(&ids), false);
    let mut apps = BTreeMap::new();
    for pid in ids {
        let Some(process) = system.process(pid) else {
            continue;
        };
        let Some(exe) = process.exe().and_then(|path| path.file_name()) else {
            continue;
        };
        let file = exe.to_string_lossy().into_owned();
        let app_id = file.to_ascii_lowercase();
        if file.len() > 160 || is_shell_or_orbit_process(&app_id) {
            continue;
        }
        apps.entry(app_id.clone()).or_insert_with(|| ForegroundApp {
            app_id,
            name: file
                .trim_end_matches(".exe")
                .trim_end_matches(".EXE")
                .to_string(),
        });
    }
    apps
}

#[cfg(not(windows))]
fn visible_window_apps(_system: &mut System) -> BTreeMap<String, ForegroundApp> {
    BTreeMap::new()
}

#[cfg(not(windows))]
fn foreground_app(_system: &mut System) -> Option<ForegroundApp> {
    None
}

fn is_shell_or_orbit_process(app_id: &str) -> bool {
    [
        "orbit-os.exe",
        "lockapp.exe",
        "logonui.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "searchhost.exe",
        "searchapp.exe",
        "textinputhost.exe",
    ]
    .contains(&app_id)
}

#[cfg(windows)]
fn user_is_idle(threshold_seconds: u32) -> bool {
    use std::mem::size_of;
    use windows::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
    if !ok {
        return true;
    }
    let now = unsafe { GetTickCount64() } as u32;
    idle_elapsed_ms(now, info.dwTime) >= (threshold_seconds as u64 * 1000)
}

#[cfg(not(windows))]
fn user_is_idle(_threshold_seconds: u32) -> bool {
    true
}

fn idle_elapsed_ms(now_tick: u32, last_input_tick: u32) -> u64 {
    now_tick.wrapping_sub(last_input_tick) as u64
}

fn credited_milliseconds(elapsed: Duration, max_gap: Duration) -> u64 {
    if elapsed > max_gap {
        0
    } else {
        elapsed.as_millis().min(max_gap.as_millis()) as u64
    }
}

fn pause_tracking(inner: &ActivityInner) {
    inner.monitoring.store(false, Ordering::Release);
    if let Ok(mut preferences) = inner.preferences.write() {
        preferences.monitoring_enabled = false;
        if let (Ok(serialized), Ok(db)) = (
            serde_json::to_string(&preferences.excluded_apps),
            inner.db.lock(),
        ) {
            let _ = db.execute(
                "UPDATE preferences SET monitoring_enabled=0,excluded_apps=?1 WHERE id=1",
                [serialized],
            );
        }
    }
}

fn start_session(
    inner: &ActivityInner,
    app: &ForegroundApp,
    day: &str,
    mode: &str,
) -> Result<i64, String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.")?;
    db.execute("INSERT INTO sessions(app_id,name,day,started_at,ended_at,active_milliseconds,tracking_mode) VALUES(?1,?2,?3,?4,NULL,0,?5)",params![app.app_id,app.name,day,Utc::now().to_rfc3339(),mode]).map_err(|e| format!("Impossible de commencer la session : {e}"))?;
    Ok(db.last_insert_rowid())
}

fn update_background_sessions(
    inner: &ActivityInner,
    active: &mut BTreeMap<String, ActiveSession>,
    candidates: BTreeMap<String, ForegroundApp>,
    now: Instant,
    today: &str,
) {
    let stale = active
        .keys()
        .filter(|id| !candidates.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    for id in stale {
        if let Some(session) = active.remove(&id) {
            let _ = finish_session(inner, session.id);
        }
    }
    for (app_id, info) in candidates {
        if let Some(session) = active.get_mut(&app_id) {
            if session.day == today {
                let elapsed = now.saturating_duration_since(session.last_sample);
                session.last_sample = now;
                if elapsed <= MAX_CREDITED_GAP {
                    let _ = add_session_milliseconds(
                        inner,
                        session.id,
                        credited_milliseconds(elapsed, MAX_CREDITED_GAP),
                    );
                    continue;
                }
            }
            let _ = finish_session(inner, session.id);
        }
        if let Ok(id) = start_session(inner, &info, today, "background") {
            active.insert(
                app_id.clone(),
                ActiveSession {
                    id,
                    app_id,
                    day: today.to_string(),
                    last_sample: now,
                },
            );
        }
    }
}

fn finish_background_sessions(inner: &ActivityInner, active: &mut BTreeMap<String, ActiveSession>) {
    for (_, session) in std::mem::take(active) {
        let _ = finish_session(inner, session.id);
    }
}

fn add_session_milliseconds(
    inner: &ActivityInner,
    id: i64,
    milliseconds: u64,
) -> Result<(), String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.")?;
    db.execute(
        "UPDATE sessions SET active_milliseconds=active_milliseconds+?1 WHERE id=?2",
        params![milliseconds as i64, id],
    )
    .map_err(|e| format!("Impossible d’enregistrer la durée active : {e}"))?;
    Ok(())
}

fn finish_session(inner: &ActivityInner, id: i64) -> Result<(), String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "Base ORBIT Pulse verrouillée.")?;
    db.execute(
        "UPDATE sessions SET ended_at=?1 WHERE id=?2 AND ended_at IS NULL",
        params![Utc::now().to_rfc3339(), id],
    )
    .map_err(|e| format!("Impossible de fermer la session : {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_elapsed_handles_tick_counter_wraparound() {
        assert_eq!(idle_elapsed_ms(5, u32::MAX - 4), 10);
    }

    #[test]
    fn sleep_or_large_scheduler_gap_is_never_credited() {
        assert_eq!(
            credited_milliseconds(Duration::from_millis(2350), MAX_CREDITED_GAP),
            2350
        );
        assert_eq!(
            credited_milliseconds(Duration::from_secs(6), MAX_CREDITED_GAP),
            6000
        );
        assert_eq!(
            credited_milliseconds(Duration::from_secs(30), MAX_CREDITED_GAP),
            0
        );
    }

    #[test]
    fn exclusions_are_normalized_bounded_and_do_not_store_paths() {
        let values = normalize_exclusions(vec![
            "  Chrome.EXE ".into(),
            "chrome.exe".into(),
            "C:\\secret\\app.exe".into(),
            "".into(),
        ]);
        assert_eq!(values, vec!["chrome.exe"]);
    }

    #[test]
    fn category_parser_uses_safe_other_fallback() {
        assert_eq!(parse_category("game"), ActivityCategory::Game);
        assert_eq!(parse_category("unexpected"), ActivityCategory::Other);
    }

    #[test]
    fn csv_escapes_quotes_and_never_interpolates_cells() {
        assert_eq!(csv_cell("A \"quoted\" app"), "\"A \"\"quoted\"\" app\"");
    }

    #[test]
    fn local_calendar_periods_are_explicit_and_validated() {
        assert!(NaiveDate::parse_from_str("not-a-date", "%Y-%m-%d").is_err());
    }

    #[test]
    fn daily_goal_validation_bounds_are_explicit() {
        assert!(validate_daily_target(5).is_ok());
        assert!(validate_daily_target(1440).is_ok());
        assert!(validate_daily_target(4).is_err());
        assert!(validate_daily_target(1441).is_err());
    }

    #[test]
    fn app_id_is_trimmed_normalized_and_rejects_paths() {
        assert_eq!(normalize_app_id("  Chrome.EXE ").unwrap(), "chrome.exe");
        assert!(normalize_app_id("C:\\Users\\Me\\app.exe").is_err());
        assert!(normalize_app_id("").is_err());
    }

    #[test]
    fn daily_goal_is_saved_updated_and_removed_locally() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE app_goals(app_id TEXT PRIMARY KEY,app_name TEXT NOT NULL DEFAULT '',daily_target_minutes INTEGER NOT NULL); CREATE TABLE sessions(id INTEGER PRIMARY KEY,app_id TEXT NOT NULL,name TEXT NOT NULL);").unwrap();
        db.execute(
            "INSERT INTO sessions(app_id,name) VALUES('game.exe','Game')",
            [],
        )
        .unwrap();
        store_daily_target(&db, "game.exe", Some(90)).unwrap();
        let stored: u32 = db
            .query_row(
                "SELECT daily_target_minutes FROM app_goals WHERE app_id=?1",
                ["game.exe"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 90);
        store_daily_target(&db, "game.exe", Some(120)).unwrap();
        let updated: u32 = db
            .query_row(
                "SELECT daily_target_minutes FROM app_goals WHERE app_id=?1",
                ["game.exe"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated, 120);
        db.execute("DELETE FROM sessions", []).unwrap();
        let saved = read_daily_goals(&db).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].app_id, "game.exe");
        assert_eq!(saved[0].app_name, "Game");
        assert_eq!(saved[0].daily_target_minutes, 120);
        store_daily_target(&db, "game.exe", None).unwrap();
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM app_goals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn existing_daily_goal_schema_gains_a_persistent_app_name_column() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE app_goals(app_id TEXT PRIMARY KEY,daily_target_minutes INTEGER NOT NULL);").unwrap();
        ensure_column(&db, "app_goals", "app_name", "TEXT NOT NULL DEFAULT ''").unwrap();
        db.execute(
            "INSERT INTO app_goals(app_id,daily_target_minutes) VALUES('old.exe',45)",
            [],
        )
        .unwrap();
        let goals = read_daily_goals(&db).unwrap();
        assert_eq!(goals[0].app_name, "old");
        assert_eq!(goals[0].daily_target_minutes, 45);
    }
}
