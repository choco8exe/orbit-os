//! Local, user-selected executable groups for launching common work sessions.
//!
//! Paths are chosen through the native file picker, validated again here, and passed directly to
//! `Command::new`. This module never accepts command-line arguments or invokes a shell.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::Duration,
};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

const PLUGIN_ID: &str = "com.orbitos.workspaces";
const MANAGE_PERMISSION: &str = "workspaces.manage";
const LAUNCH_PERMISSION: &str = "workspaces.launch";
const MAX_WORKSPACES: u64 = 40;
const MAX_APPS_PER_WORKSPACE: u64 = 20;
const MAX_NAME_CHARS: usize = 48;
const MAX_PATH_BYTES: usize = 2048;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceApplication {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub applications: Vec<WorkspaceApplication>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLaunchResult {
    pub workspace_name: String,
    pub launched_count: usize,
}

pub struct WorkspacesState(Mutex<Connection>);

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données ORBIT indisponible : {e}"))?;
    fs::create_dir_all(&path)
        .map_err(|e| format!("Impossible de créer le dossier des espaces de travail : {e}"))?;
    path.push("workspaces.sqlite3");
    let db = Connection::open(path)
        .map_err(|e| format!("Base des espaces de travail indisponible : {e}"))?;
    db.busy_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS workspaces (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL COLLATE NOCASE UNIQUE,
           created_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS workspace_apps (
           id TEXT PRIMARY KEY,
           workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
           name TEXT NOT NULL,
           path TEXT NOT NULL,
           position INTEGER NOT NULL,
           UNIQUE(workspace_id, path)
         );
         CREATE INDEX IF NOT EXISTS workspace_apps_order_idx ON workspace_apps(workspace_id, position);",
    ).map_err(|e| format!("Impossible d’initialiser les espaces de travail : {e}"))?;
    app.manage(WorkspacesState(Mutex::new(db)));
    Ok(())
}

fn require_manage(app: &AppHandle) -> Result<(), String> {
    crate::plugins::require_permission(app, PLUGIN_ID, MANAGE_PERMISSION)
}

fn validate_workspace_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(format!("Le nom doit contenir de 1 à {MAX_NAME_CHARS} caractères et ne pas inclure de caractères de contrôle."));
    }
    Ok(name.to_owned())
}

/// Canonicalizes only existing absolute executables. The selected path is never interpreted as a command string.
fn validate_executable(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Choisis une application avec le sélecteur Windows.".into());
    }
    if path.as_os_str().len() > MAX_PATH_BYTES {
        return Err("Le chemin de l’application est trop long.".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Le fichier .exe sélectionné est introuvable ou inaccessible.".to_owned())?;
    let metadata = fs::metadata(&canonical)
        .map_err(|_| "Le fichier .exe sélectionné est introuvable ou inaccessible.".to_owned())?;
    if !metadata.is_file()
        || !canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err("Choisis un fichier d’application Windows portant l’extension .exe.".into());
    }
    Ok(canonical)
}

fn app_name(path: &Path) -> Result<String, String> {
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Nom de l’application invalide.".to_owned())?
        .trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("Nom de l’application invalide.".into());
    }
    Ok(name.chars().take(MAX_NAME_CHARS).collect())
}

fn list_workspaces(db: &Connection) -> Result<Vec<Workspace>, String> {
    let mut statement = db
        .prepare("SELECT id,name,created_at FROM workspaces ORDER BY name COLLATE NOCASE,id")
        .map_err(|e| format!("Impossible de lire les espaces de travail : {e}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Impossible de lire les espaces de travail : {e}"))?;
    let mut workspaces = Vec::new();
    for row in rows {
        let (id, name, created_at) = row.map_err(|e| e.to_string())?;
        let mut app_statement = db.prepare("SELECT id,name,path FROM workspace_apps WHERE workspace_id=?1 ORDER BY position,id")
            .map_err(|e| format!("Impossible de lire les applications de l’espace : {e}"))?;
        let app_rows = app_statement
            .query_map([&id], |row| {
                Ok(WorkspaceApplication {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                })
            })
            .map_err(|e| format!("Impossible de lire les applications de l’espace : {e}"))?;
        let mut applications = Vec::new();
        for app in app_rows {
            applications.push(app.map_err(|e| e.to_string())?);
        }
        workspaces.push(Workspace {
            id,
            name,
            created_at,
            applications,
        });
    }
    Ok(workspaces)
}

pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    let state = app
        .try_state::<WorkspacesState>()
        .ok_or_else(|| "Les espaces de travail ne sont pas initialisés.".to_owned())?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    db.execute("DELETE FROM workspaces", [])
        .map_err(|e| format!("Impossible d’effacer les espaces de travail : {e}"))?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| {
            format!("Impossible de finaliser l’effacement des espaces de travail : {e}")
        })?;
    Ok(())
}

#[tauri::command]
pub fn workspaces_list(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
) -> Result<Vec<Workspace>, String> {
    crate::plugins::require_plugin_enabled(&app, PLUGIN_ID)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    list_workspaces(&db)
}

#[tauri::command]
pub fn workspaces_create(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    name: String,
) -> Result<Vec<Workspace>, String> {
    require_manage(&app)?;
    let name = validate_workspace_name(&name)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    let count: u64 = db
        .query_row("SELECT count(*) FROM workspaces", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if count >= MAX_WORKSPACES {
        return Err(format!(
            "Tu peux créer jusqu’à {MAX_WORKSPACES} espaces de travail."
        ));
    }
    let id = Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO workspaces(id,name,created_at) VALUES(?1,?2,?3)",
        params![id, name, Utc::now().to_rfc3339()],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            "Un espace de travail porte déjà ce nom.".to_owned()
        } else {
            format!("Impossible de créer l’espace de travail : {e}")
        }
    })?;
    list_workspaces(&db)
}

#[tauri::command]
pub fn workspaces_rename(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    id: String,
    name: String,
) -> Result<Vec<Workspace>, String> {
    require_manage(&app)?;
    let name = validate_workspace_name(&name)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    let changed = db
        .execute(
            "UPDATE workspaces SET name=?1 WHERE id=?2",
            params![name, id],
        )
        .map_err(|e| format!("Impossible de renommer l’espace : {e}"))?;
    if changed == 0 {
        return Err("Cet espace de travail n’existe plus.".into());
    }
    list_workspaces(&db)
}

#[tauri::command]
pub fn workspaces_delete(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    id: String,
) -> Result<Vec<Workspace>, String> {
    require_manage(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    db.execute("DELETE FROM workspaces WHERE id=?1", [&id])
        .map_err(|e| format!("Impossible de supprimer l’espace de travail : {e}"))?;
    list_workspaces(&db)
}

#[tauri::command(rename_all = "camelCase")]
pub fn workspaces_add_app(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    workspace_id: String,
    path: String,
) -> Result<Vec<Workspace>, String> {
    require_manage(&app)?;
    let canonical = validate_executable(Path::new(&path))?;
    let name = app_name(&canonical)?;
    let canonical_path = canonical.to_string_lossy().into_owned();
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM workspace_apps WHERE workspace_id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .map_err(|_| "Cet espace de travail n’existe plus.".to_owned())?;
    if count >= MAX_APPS_PER_WORKSPACE {
        return Err(format!(
            "Un espace peut contenir jusqu’à {MAX_APPS_PER_WORKSPACE} applications."
        ));
    }
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM workspaces WHERE id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if id.is_none() {
        return Err("Cet espace de travail n’existe plus.".into());
    }
    let position: u64 = count;
    db.execute(
        "INSERT INTO workspace_apps(id,workspace_id,name,path,position) VALUES(?1,?2,?3,?4,?5)",
        params![
            Uuid::new_v4().to_string(),
            workspace_id,
            name,
            canonical_path,
            position
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            "Cette application est déjà dans cet espace de travail.".to_owned()
        } else {
            format!("Impossible d’ajouter l’application : {e}")
        }
    })?;
    list_workspaces(&db)
}

#[tauri::command(rename_all = "camelCase")]
pub fn workspaces_remove_app(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    workspace_id: String,
    app_id: String,
) -> Result<Vec<Workspace>, String> {
    require_manage(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
    db.execute(
        "DELETE FROM workspace_apps WHERE id=?1 AND workspace_id=?2",
        params![app_id, workspace_id],
    )
    .map_err(|e| format!("Impossible de retirer l’application : {e}"))?;
    list_workspaces(&db)
}

#[tauri::command(rename_all = "camelCase")]
pub fn workspaces_launch(
    app: AppHandle,
    state: State<'_, WorkspacesState>,
    workspace_id: String,
) -> Result<WorkspaceLaunchResult, String> {
    crate::plugins::require_permission(&app, PLUGIN_ID, LAUNCH_PERMISSION)?;
    let (workspace_name, apps) = {
        let db = state
            .0
            .lock()
            .map_err(|_| "Base des espaces de travail verrouillée.".to_owned())?;
        let workspace_name: String = db
            .query_row(
                "SELECT name FROM workspaces WHERE id=?1",
                [&workspace_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Cet espace de travail n’existe plus.".to_owned())?;
        let mut statement = db
            .prepare(
                "SELECT path,name FROM workspace_apps WHERE workspace_id=?1 ORDER BY position,id",
            )
            .map_err(|e| format!("Impossible de lire les applications de l’espace : {e}"))?;
        let rows = statement
            .query_map([&workspace_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Impossible de lire les applications de l’espace : {e}"))?;
        let mut apps = Vec::new();
        for row in rows {
            apps.push(row.map_err(|e| e.to_string())?);
        }
        (workspace_name, apps)
    };
    if apps.is_empty() {
        return Err("Ajoute au moins une application avant de lancer cet espace.".into());
    }

    // Validate every executable before starting any application, so missing files fail closed.
    let validated = apps
        .iter()
        .map(|(path, name)| validate_executable(Path::new(path)).map(|path| (path, name.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    let total = validated.len();
    let mut launched = 0;
    for (path, name) in validated {
        if let Err(error) = Command::new(&path).spawn() {
            return Err(if launched == 0 {
                format!("Impossible d’ouvrir {name} depuis l’espace « {workspace_name} » : {error}")
            } else {
                format!("{launched} application(s) ouverte(s) sur {total}; impossible d’ouvrir {name} : {error}")
            });
        }
        launched += 1;
    }
    Ok(WorkspaceLaunchResult {
        workspace_name,
        launched_count: launched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_names_are_trimmed_bounded_and_control_free() {
        assert_eq!(validate_workspace_name("  Studio  ").unwrap(), "Studio");
        assert!(validate_workspace_name("").is_err());
        assert!(validate_workspace_name(&"x".repeat(MAX_NAME_CHARS + 1)).is_err());
        assert!(validate_workspace_name("bad\nname").is_err());
    }

    #[test]
    fn selected_paths_must_be_existing_absolute_exes() {
        let dir = std::env::temp_dir().join(format!("orbit-workspace-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Sample.EXE");
        fs::write(&exe, b"not executed in this test").unwrap();
        assert!(validate_executable(&exe).is_ok());
        assert!(validate_executable(Path::new("relative.exe")).is_err());
        assert!(validate_executable(&dir.join("missing.exe")).is_err());
        let text_file = dir.join("notes.txt");
        fs::write(&text_file, b"not an executable").unwrap();
        assert!(validate_executable(&text_file).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removed_executable_is_reported_as_unavailable_when_launch_rechecks_it() {
        let dir = std::env::temp_dir().join(format!("orbit-workspace-stale-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Selected.exe");
        fs::write(&exe, b"placeholder").unwrap();
        assert!(validate_executable(&exe).is_ok());
        fs::remove_file(&exe).unwrap();
        let error = validate_executable(&exe).unwrap_err();
        assert!(error.contains("introuvable ou inaccessible"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn database_round_trips_groups_apps_and_cascade_deletes() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE workspaces(id TEXT PRIMARY KEY,name TEXT NOT NULL COLLATE NOCASE UNIQUE,created_at TEXT NOT NULL); CREATE TABLE workspace_apps(id TEXT PRIMARY KEY,workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,name TEXT NOT NULL,path TEXT NOT NULL,position INTEGER NOT NULL,UNIQUE(workspace_id,path));").unwrap();
        db.execute(
            "INSERT INTO workspaces(id,name,created_at) VALUES('one','Studio','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO workspace_apps(id,workspace_id,name,path,position) VALUES('app-one','one','OBS','C:/obs.exe',0)", []).unwrap();
        let groups = list_workspaces(&db).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].applications[0].name, "OBS");
        db.execute("DELETE FROM workspaces WHERE id='one'", [])
            .unwrap();
        let apps_left: u64 = db
            .query_row("SELECT count(*) FROM workspace_apps", [], |row| row.get(0))
            .unwrap();
        assert_eq!(apps_left, 0);
    }
}
