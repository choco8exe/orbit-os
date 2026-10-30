//! Opt-in local game session tracker. It reads only process executable names and never inspects
//! window titles, input, files, browser data or network traffic.
use chrono::{Local, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use sysinfo::{ProcessesToUpdate, System};
use tauri::{AppHandle, Manager, State};

const PLUGIN_ID: &str = "com.orbitos.game-sessions";
const TRACK_PERMISSION: &str = "game-sessions.track-selected-processes";
const POLL_SECONDS: u64 = 15;
const MAX_TRACKED: usize = 100;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProcess {
    pub executable: String,
    pub running: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameUsage {
    pub executable: String,
    pub seconds: u64,
    pub sessions: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDay {
    pub date: String,
    pub seconds: u64,
    pub applications: BTreeMap<String, u64>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub enabled: bool,
    pub tracked: Vec<String>,
    pub running: Vec<GameProcess>,
    pub week: Vec<GameDay>,
    pub totals: Vec<GameUsage>,
}
pub struct GameSessionsState(Arc<Inner>);
struct Inner {
    db: Mutex<Connection>,
    enabled: AtomicBool,
}

impl Inner {
    fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Dossier Game Sessions inaccessible : {e}"))?;
        }
        let db =
            Connection::open(path).map_err(|e| format!("Base Game Sessions inaccessible : {e}"))?;
        db.busy_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS game_preferences(id INTEGER PRIMARY KEY CHECK(id=1),enabled INTEGER NOT NULL DEFAULT 0); INSERT OR IGNORE INTO game_preferences(id,enabled) VALUES(1,0); CREATE TABLE IF NOT EXISTS tracked_games(executable TEXT PRIMARY KEY COLLATE NOCASE); CREATE TABLE IF NOT EXISTS game_sessions(id INTEGER PRIMARY KEY, executable TEXT NOT NULL COLLATE NOCASE, started_at TEXT NOT NULL, ended_at TEXT, seconds INTEGER NOT NULL CHECK(seconds>=0)); CREATE INDEX IF NOT EXISTS game_sessions_ended_idx ON game_sessions(ended_at); CREATE UNIQUE INDEX IF NOT EXISTS game_sessions_one_open_idx ON game_sessions(executable COLLATE NOCASE) WHERE ended_at IS NULL;").map_err(|e|format!("Impossible d’initialiser Game Sessions : {e}"))?;
        let enabled: bool = db
            .query_row("SELECT enabled FROM game_preferences WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0)
            != 0;
        Ok(Self {
            db: Mutex::new(db),
            enabled: AtomicBool::new(enabled),
        })
    }
}
pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier ORBIT indisponible : {e}"))?;
    path.push("game-sessions.sqlite3");
    let inner = Arc::new(Inner::open(path)?);
    let worker = inner.clone();
    let worker_app = app.clone();
    app.manage(GameSessionsState(inner));
    thread::Builder::new()
        .name("orbit-game-sessions".into())
        .spawn(move || {
            let mut system = System::new();
            let mut last = std::time::Instant::now();
            loop {
                thread::sleep(Duration::from_secs(POLL_SECONDS));
                let now = Utc::now().to_rfc3339();
                if !worker.enabled.load(Ordering::Acquire)
                    || crate::plugins::require_plugin_enabled(&worker_app, PLUGIN_ID).is_err()
                    || crate::plugins::require_permission(&worker_app, PLUGIN_ID, TRACK_PERMISSION)
                        .is_err()
                {
                    if let Ok(mut db) = worker.db.lock() {
                        let _ = close_open_sessions(&mut db, &now);
                    }
                    last = std::time::Instant::now();
                    continue;
                }
                system.refresh_processes(ProcessesToUpdate::All, true);
                let elapsed = last.elapsed().as_secs().clamp(1, POLL_SECONDS + 5);
                last = std::time::Instant::now();
                let tracked = match worker.db.lock() {
                    Ok(db) => read_tracked(&db).unwrap_or_default(),
                    Err(_) => continue,
                };
                // Deduplicate process instances: launching a game helper process must never multiply its time.
                let present = system
                    .processes()
                    .values()
                    .filter_map(|process| {
                        let name = process.name().to_string_lossy().to_string();
                        tracked
                            .iter()
                            .any(|item| item.eq_ignore_ascii_case(&name))
                            .then_some((name.to_ascii_lowercase(), name))
                    })
                    .collect::<std::collections::BTreeMap<_, _>>();
                let mut distinct = present.into_values().collect::<Vec<_>>();
                distinct.sort_by_key(|name| name.to_ascii_lowercase());
                if let Ok(mut db) = worker.db.lock() {
                    let _ = record_sample(&mut db, &distinct, elapsed, &now);
                }
            }
        })
        .map_err(|e| format!("Impossible de lancer le suivi Game Sessions : {e}"))?;
    Ok(())
}
pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    let Some(state) = app.try_state::<GameSessionsState>() else {
        return Ok(());
    };
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    db.execute("DELETE FROM game_sessions", [])
        .map_err(|e| format!("Impossible d’effacer l’historique Game Sessions : {e}"))?;
    db.execute("DELETE FROM tracked_games", [])
        .map_err(|e| format!("Impossible d’effacer les jeux autorisés : {e}"))?;
    db.execute("UPDATE game_preferences SET enabled=0 WHERE id=1", [])
        .map_err(|e| format!("Impossible de désactiver le suivi Game Sessions : {e}"))?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| format!("Impossible de finaliser l’effacement Game Sessions : {e}"))?;
    state.0.enabled.store(false, Ordering::Release);
    Ok(())
}
fn close_open_sessions(db: &mut Connection, ended_at: &str) -> Result<(), String> {
    db.execute(
        "UPDATE game_sessions SET ended_at=?1 WHERE ended_at IS NULL",
        [ended_at],
    )
    .map(|_| ())
    .map_err(|e| format!("Impossible de clôturer une session : {e}"))
}
fn record_sample(
    db: &mut Connection,
    present: &[String],
    elapsed: u64,
    now: &str,
) -> Result<(), String> {
    let present = present
        .iter()
        .map(|name| (name.to_ascii_lowercase(), name.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut stmt = tx
        .prepare("SELECT executable FROM game_sessions WHERE ended_at IS NULL")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let open = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for executable in &open {
        if present
            .iter()
            .any(|name| name.eq_ignore_ascii_case(executable))
        {
            tx.execute("UPDATE game_sessions SET seconds=seconds+?1 WHERE executable=?2 COLLATE NOCASE AND ended_at IS NULL", params![elapsed, executable]).map_err(|e| e.to_string())?;
        } else {
            tx.execute("UPDATE game_sessions SET ended_at=?1 WHERE executable=?2 COLLATE NOCASE AND ended_at IS NULL", params![now, executable]).map_err(|e| e.to_string())?;
        }
    }
    for executable in present {
        if !open
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&executable))
        {
            tx.execute("INSERT INTO game_sessions(executable,started_at,ended_at,seconds) VALUES(?1,?2,NULL,?3)", params![executable, now, elapsed]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}
fn read_tracked(db: &Connection) -> Result<Vec<String>, String> {
    let mut s = db
        .prepare("SELECT executable FROM tracked_games ORDER BY executable COLLATE NOCASE")
        .map_err(|e| e.to_string())?;
    let rows = s.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())
}
fn enabled(state: &GameSessionsState) -> bool {
    state.0.enabled.load(Ordering::Acquire)
}
fn snapshot(state: &GameSessionsState) -> Result<GameSnapshot, String> {
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    let tracked = read_tracked(&db)?;
    drop(db);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let running = system
        .processes()
        .values()
        .filter_map(|p| {
            let name = p.name().to_string_lossy().to_string();
            tracked
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&name))
                .then_some(GameProcess {
                    executable: name,
                    running: true,
                })
        })
        .fold(BTreeMap::new(), |mut unique, game| {
            unique
                .entry(game.executable.to_ascii_lowercase())
                .or_insert(game);
            unique
        })
        .into_values()
        .collect();
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    let today = Local::now().date_naive();
    let start = (today - chrono::Days::new(6))
        .format("%Y-%m-%d")
        .to_string();
    let mut stmt=db.prepare("SELECT substr(COALESCE(ended_at,started_at),1,10), executable, SUM(seconds) FROM game_sessions WHERE substr(COALESCE(ended_at,started_at),1,10)>=?1 GROUP BY substr(COALESCE(ended_at,started_at),1,10),executable").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([start], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut days: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for d in 0..7 {
        let date = (today - chrono::Days::new(6 - d))
            .format("%Y-%m-%d")
            .to_string();
        days.insert(date, BTreeMap::new());
    }
    for row in rows {
        let (date, exe, seconds) = row.map_err(|e| e.to_string())?;
        *days
            .entry(date)
            .or_default()
            .entry(exe.clone())
            .or_default() += seconds;
    }
    drop(stmt);
    let week = days
        .into_iter()
        .map(|(date, applications)| GameDay {
            seconds: applications.values().sum(),
            date,
            applications,
        })
        .collect();
    let mut all_time = db.prepare("SELECT executable,SUM(seconds),COUNT(*) FROM game_sessions GROUP BY executable ORDER BY SUM(seconds) DESC, executable COLLATE NOCASE").map_err(|e|e.to_string())?;
    let total_rows = all_time
        .query_map([], |r| {
            Ok(GameUsage {
                executable: r.get(0)?,
                seconds: r.get(1)?,
                sessions: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let totals = total_rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(GameSnapshot {
        enabled: enabled(state),
        tracked,
        running,
        week,
        totals,
    })
}
fn require(app: &AppHandle) -> Result<(), String> {
    crate::plugins::require_plugin_enabled(app, PLUGIN_ID)
}
#[tauri::command]
pub fn game_sessions_snapshot(
    app: AppHandle,
    state: State<'_, GameSessionsState>,
) -> Result<GameSnapshot, String> {
    require(&app)?;
    snapshot(&state)
}
#[tauri::command]
pub fn game_sessions_set_enabled(
    app: AppHandle,
    state: State<'_, GameSessionsState>,
    enabled: bool,
) -> Result<GameSnapshot, String> {
    require(&app)?;
    if enabled {
        crate::plugins::require_permission(&app, PLUGIN_ID, TRACK_PERMISSION)?;
    }
    let mut db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    db.execute(
        "UPDATE game_preferences SET enabled=?1 WHERE id=1",
        [enabled as i64],
    )
    .map_err(|e| format!("Impossible d’enregistrer la préférence : {e}"))?;
    if !enabled {
        close_open_sessions(&mut *db, &Utc::now().to_rfc3339())?;
    }
    state.0.enabled.store(enabled, Ordering::Release);
    drop(db);
    snapshot(&state)
}
fn validate_executable(executable: &str) -> Result<String, String> {
    let name = executable.trim();
    if name.is_empty()
        || name.len() > 180
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        || !name.to_ascii_lowercase().ends_with(".exe")
    {
        return Err("Ajoute un nom de programme .exe, par exemple Minecraft.exe.".into());
    }
    Ok(name.to_owned())
}
#[tauri::command]
pub fn game_sessions_add(
    app: AppHandle,
    state: State<'_, GameSessionsState>,
    executable: String,
) -> Result<GameSnapshot, String> {
    require(&app)?;
    crate::plugins::require_permission(&app, PLUGIN_ID, TRACK_PERMISSION)?;
    let executable = validate_executable(&executable)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    let count: usize = db
        .query_row("SELECT count(*) FROM tracked_games", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;
    if count >= MAX_TRACKED {
        return Err("La liste atteint sa limite de 100 jeux suivis.".into());
    }
    db.execute(
        "INSERT OR IGNORE INTO tracked_games(executable) VALUES(?1)",
        [executable],
    )
    .map_err(|e| format!("Impossible d’ajouter ce programme : {e}"))?;
    drop(db);
    snapshot(&state)
}
#[tauri::command]
pub fn game_sessions_remove(
    app: AppHandle,
    state: State<'_, GameSessionsState>,
    executable: String,
) -> Result<GameSnapshot, String> {
    require(&app)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    db.execute(
        "DELETE FROM tracked_games WHERE executable=?1 COLLATE NOCASE",
        [executable],
    )
    .map_err(|e| e.to_string())?;
    drop(db);
    snapshot(&state)
}
#[tauri::command]
pub fn game_sessions_clear(
    app: AppHandle,
    state: State<'_, GameSessionsState>,
) -> Result<GameSnapshot, String> {
    require(&app)?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "Base Game Sessions verrouillée.".to_string())?;
    db.execute("DELETE FROM game_sessions", [])
        .map_err(|e| format!("Impossible d’effacer l’historique : {e}"))?;
    drop(db);
    snapshot(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE game_sessions(id INTEGER PRIMARY KEY, executable TEXT NOT NULL COLLATE NOCASE, started_at TEXT NOT NULL, ended_at TEXT, seconds INTEGER NOT NULL CHECK(seconds>=0)); CREATE UNIQUE INDEX game_sessions_one_open_idx ON game_sessions(executable COLLATE NOCASE) WHERE ended_at IS NULL;").unwrap();
        db
    }

    #[test]
    fn only_process_basename_is_accepted() {
        assert_eq!(validate_executable("  Game.exe ").unwrap(), "Game.exe");
        for bad in ["", "C:\\game.exe", "../game.exe", "game.txt", "bad\n.exe"] {
            assert!(validate_executable(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn repeated_process_samples_accumulate_one_continuous_session() {
        let mut db = sample_db();
        record_sample(
            &mut db,
            &["Game.exe".into(), "game.exe".into()],
            15,
            "2026-10-30T10:00:00Z",
        )
        .unwrap();
        record_sample(&mut db, &["game.exe".into()], 15, "2026-10-30T10:00:15Z").unwrap();
        let (count, seconds): (i64, i64) = db
            .query_row("SELECT count(*),sum(seconds) FROM game_sessions", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((count, seconds), (1, 30));
        record_sample(&mut db, &[], 15, "2026-10-30T10:00:30Z").unwrap();
        record_sample(&mut db, &["Game.exe".into()], 15, "2026-10-30T10:00:45Z").unwrap();
        let (count, seconds): (i64, i64) = db
            .query_row("SELECT count(*),sum(seconds) FROM game_sessions", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((count, seconds), (2, 45));
    }
    #[test]
    fn session_rows_roll_up_by_day_and_executable() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE game_sessions(id INTEGER PRIMARY KEY,executable TEXT,started_at TEXT,ended_at TEXT,seconds INTEGER);").unwrap();
        db.execute("INSERT INTO game_sessions(executable,started_at,ended_at,seconds) VALUES('game.exe','2026-10-30T10:00:00Z','2026-10-30T10:01:00Z',60)",[]).unwrap();
        let total: u64 = db
            .query_row(
                "SELECT SUM(seconds) FROM game_sessions WHERE executable='game.exe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 60);
    }
}
