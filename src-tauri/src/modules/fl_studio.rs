use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

const PLUGIN_ID: &str = "com.orbitos.fl-studio";
const LOCAL_LIBRARY_PERMISSION: &str = "project-shelf.local-library";
const SELECT_FILES_PERMISSION: &str = "project-shelf.select-project-files";
const OPEN_PERMISSION: &str = "project-shelf.open-project";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectShelfEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub pinned: bool,
    pub added_at: String,
    pub modified_at: Option<String>,
    pub size_bytes: Option<u64>,
    pub exists: bool,
    pub tags: Vec<String>,
}

pub struct FlStudioState(Mutex<Connection>);

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut db_path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données ORBIT indisponible: {e}"))?;
    fs::create_dir_all(&db_path)
        .map_err(|e| format!("Impossible de créer le dossier Project Shelf: {e}"))?;
    db_path.push("fl-studio-project-shelf.sqlite3");
    let db =
        Connection::open(db_path).map_err(|e| format!("Base Project Shelf indisponible: {e}"))?;
    db.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY,
           path TEXT NOT NULL UNIQUE,
           name TEXT NOT NULL,
           pinned INTEGER NOT NULL DEFAULT 0,
           added_at TEXT NOT NULL,
           modified_at TEXT,
           size_bytes INTEGER
         );
         CREATE INDEX IF NOT EXISTS projects_name_idx ON projects(name COLLATE NOCASE);
         CREATE TABLE IF NOT EXISTS project_tags (
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           tag TEXT NOT NULL COLLATE NOCASE,
           PRIMARY KEY(project_id, tag)
         );",
    )
    .map_err(|e| format!("Impossible d’initialiser Project Shelf: {e}"))?;
    app.manage(FlStudioState(Mutex::new(db)));
    Ok(())
}

fn ensure_permission(app: &AppHandle, permission: &str) -> Result<(), String> {
    crate::plugins::require_permission(app, PLUGIN_ID, permission)
}

/// Only indexed metadata is returned; the FLP file itself is never opened for reading.
pub fn list_projects_for_plugin(app: &AppHandle) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(app, LOCAL_LIBRARY_PERMISSION)?;
    let state = app
        .try_state::<FlStudioState>()
        .ok_or_else(|| "Project Shelf n’est pas initialisé.".to_string())?;
    let mut db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    list_entries(&mut db)
}

/// Preserve the index if the plugin is reinstalled without asking to delete its data.
pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    let state = app
        .try_state::<FlStudioState>()
        .ok_or_else(|| "Project Shelf n’est pas initialisé.".to_string())?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    db.execute("DELETE FROM projects", [])
        .map_err(|e| format!("Impossible d’effacer les données Project Shelf: {e}"))?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| {
            format!("Impossible de finaliser l’effacement des données Project Shelf: {e}")
        })?;
    Ok(())
}

#[tauri::command]
pub fn fl_studio_list_projects(app: AppHandle) -> Result<Vec<ProjectShelfEntry>, String> {
    list_projects_for_plugin(&app)
}

#[tauri::command]
pub fn fl_studio_add_project(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    path: String,
) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    ensure_permission(&app, SELECT_FILES_PERMISSION)?;
    let canonical = canonical_project_file(Path::new(&path))?;
    let metadata = metadata_for(&canonical)?;
    let name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Nom de projet FL Studio invalide.".to_string())?;
    let path_string = canonical.to_string_lossy().into_owned();
    let db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    db.execute(
        "INSERT INTO projects(id,path,name,pinned,added_at,modified_at,size_bytes) VALUES (?1,?2,?3,0,?4,?5,?6)
         ON CONFLICT(path) DO UPDATE SET name=excluded.name,modified_at=excluded.modified_at,size_bytes=excluded.size_bytes",
        params![Uuid::new_v4().to_string(), path_string, name, Utc::now().to_rfc3339(), metadata.modified_at, metadata.size_bytes],
    ).map_err(|e| format!("Impossible d’ajouter ce projet à la bibliothèque: {e}"))?;
    drop(db);
    list_projects_for_plugin(&app)
}

#[tauri::command]
pub fn fl_studio_set_pinned(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    id: String,
    pinned: bool,
) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    if db
        .execute(
            "UPDATE projects SET pinned=?1 WHERE id=?2",
            params![pinned as i64, id],
        )
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("Ce projet ne figure plus dans la bibliothèque.".into());
    }
    drop(db);
    list_projects_for_plugin(&app)
}

#[tauri::command]
pub fn fl_studio_remove_project(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    id: String,
) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    db.execute("DELETE FROM projects WHERE id=?1", [&id])
        .map_err(|e| format!("Impossible de retirer ce projet: {e}"))?;
    drop(db);
    list_projects_for_plugin(&app)
}

#[tauri::command]
pub fn fl_studio_open_project(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    id: String,
) -> Result<(), String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    ensure_permission(&app, OPEN_PERMISSION)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    let path = indexed_project_path(&db, &id)?;
    drop(db);
    open_with_default_association(&path)
}

#[tauri::command]
pub fn fl_studio_add_tag(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    id: String,
    tag: String,
) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    let mut db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    add_project_tag(&mut db, &id, &tag)?;
    drop(db);
    list_projects_for_plugin(&app)
}

#[tauri::command]
pub fn fl_studio_remove_tag(
    app: AppHandle,
    state: State<'_, FlStudioState>,
    id: String,
    tag: String,
) -> Result<Vec<ProjectShelfEntry>, String> {
    ensure_permission(&app, LOCAL_LIBRARY_PERMISSION)?;
    let mut db = state
        .0
        .lock()
        .map_err(|_| "Base Project Shelf verrouillée.".to_string())?;
    remove_project_tag(&mut db, &id, &tag)?;
    drop(db);
    list_projects_for_plugin(&app)
}

struct FileMetadata {
    modified_at: Option<String>,
    size_bytes: Option<u64>,
}

fn metadata_for(path: &Path) -> Result<FileMetadata, String> {
    let metadata =
        fs::metadata(path).map_err(|_| "Le fichier FLP est absent ou inaccessible.".to_string())?;
    if !metadata.is_file() {
        return Err("Le fichier FLP est absent ou inaccessible.".into());
    }
    let modified_at = metadata.modified().ok().and_then(|modified| {
        let timestamp = modified.duration_since(UNIX_EPOCH).ok()?;
        let datetime: DateTime<Utc> =
            DateTime::from_timestamp(timestamp.as_secs() as i64, timestamp.subsec_nanos())?;
        Some(datetime.to_rfc3339())
    });
    Ok(FileMetadata {
        modified_at,
        size_bytes: Some(metadata.len()),
    })
}

/// Accept only explicitly selected, absolute `.flp` paths. Reject parent traversal before canonicalizing.
fn canonical_project_file(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("Sélectionnez un chemin absolu sans remontée de dossier.".into());
    }
    if !is_flp_path(path) {
        return Err("Seuls les fichiers .flp peuvent être ajoutés.".into());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|_| "Le fichier FLP est absent ou inaccessible.".to_string())?;
    if !canonical.is_file() || !is_flp_path(&canonical) {
        return Err("Le chemin choisi n’est pas un fichier .flp.".into());
    }
    Ok(canonical)
}

fn is_flp_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("flp"))
}

fn list_entries(db: &mut Connection) -> Result<Vec<ProjectShelfEntry>, String> {
    let rows = {
        let mut statement = db.prepare("SELECT id,path,name,pinned,added_at FROM projects ORDER BY pinned DESC,name COLLATE NOCASE,added_at DESC").map_err(|e| e.to_string())?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        mapped
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
    };
    let mut tags_by_project = std::collections::HashMap::<String, Vec<String>>::new();
    {
        let mut statement = db
            .prepare("SELECT project_id,tag FROM project_tags ORDER BY tag COLLATE NOCASE")
            .map_err(|e| e.to_string())?;
        let tags = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for tag in tags {
            let (project_id, tag) = tag.map_err(|e| e.to_string())?;
            tags_by_project.entry(project_id).or_default().push(tag);
        }
    }
    let mut projects = Vec::with_capacity(rows.len());
    for (id, path, name, pinned, added_at) in rows {
        let canonical = PathBuf::from(&path);
        let file = metadata_for(&canonical).ok();
        let exists = file.is_some();
        let (modified_at, size_bytes) = file
            .map(|metadata| (metadata.modified_at, metadata.size_bytes))
            .unwrap_or((None, None));
        if exists {
            db.execute(
                "UPDATE projects SET name=?1,modified_at=?2,size_bytes=?3 WHERE id=?4",
                params![
                    canonical
                        .file_name()
                        .and_then(|file| file.to_str())
                        .unwrap_or(&name),
                    modified_at,
                    size_bytes,
                    id
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        let tags = tags_by_project.remove(&id).unwrap_or_default();
        projects.push(ProjectShelfEntry {
            id,
            name: canonical
                .file_name()
                .and_then(|file| file.to_str())
                .unwrap_or(&name)
                .to_string(),
            path,
            pinned,
            added_at,
            modified_at,
            size_bytes,
            exists,
            tags,
        });
    }
    Ok(projects)
}

fn normalize_tag(tag: &str) -> Result<String, String> {
    let normalized = tag
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty()
        || normalized.chars().count() > 32
        || normalized
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err("Un tag doit contenir de 1 à 32 caractères, sans séparateur de chemin.".into());
    }
    Ok(normalized)
}

fn project_exists(db: &Connection, id: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
        [id],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

fn add_project_tag(db: &mut Connection, id: &str, value: &str) -> Result<(), String> {
    let tag = normalize_tag(value)?;
    if !project_exists(db, id)? {
        return Err("Ce projet ne figure plus dans la bibliothèque.".into());
    }
    let count: usize = db
        .query_row(
            "SELECT count(*) FROM project_tags WHERE project_id=?1",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM project_tags WHERE project_id=?1 AND tag=?2)",
            params![id, tag],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(());
    }
    if count >= 12 {
        return Err("Un projet peut avoir au maximum 12 tags.".into());
    }
    db.execute(
        "INSERT INTO project_tags(project_id,tag) VALUES (?1,?2)",
        params![id, tag],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn remove_project_tag(db: &mut Connection, id: &str, value: &str) -> Result<(), String> {
    let tag = normalize_tag(value)?;
    if !project_exists(db, id)? {
        return Err("Ce projet ne figure plus dans la bibliothèque.".into());
    }
    db.execute(
        "DELETE FROM project_tags WHERE project_id=?1 AND tag=?2",
        params![id, tag],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn indexed_project_path(db: &Connection, id: &str) -> Result<PathBuf, String> {
    let stored: Option<String> = db
        .query_row("SELECT path FROM projects WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let stored =
        stored.ok_or_else(|| "Ce projet n’est pas indexé dans Project Shelf.".to_string())?;
    let path = canonical_project_file(Path::new(&stored))?;
    if path.to_string_lossy() != stored {
        return Err("Le chemin du projet a changé depuis son indexation.".into());
    }
    Ok(path)
}

#[cfg(windows)]
fn open_with_default_association(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HWND,
            UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        },
    };

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            Some(HWND(std::ptr::null_mut())),
            PCWSTR(verb.as_ptr()),
            PCWSTR(wide_path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err("Windows n’a pas pu ouvrir ce projet avec l’application associée.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_with_default_association(_path: &Path) -> Result<(), String> {
    Err("L’ouverture des projets FL Studio est disponible uniquement sous Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("orbit-fl-studio-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("test temp directory");
        path
    }

    fn test_db() -> Connection {
        let db = Connection::open_in_memory().expect("in-memory shelf database");
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE projects(id TEXT PRIMARY KEY,path TEXT NOT NULL UNIQUE,name TEXT NOT NULL,pinned INTEGER NOT NULL,added_at TEXT NOT NULL,modified_at TEXT,size_bytes INTEGER); CREATE TABLE project_tags(project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,tag TEXT NOT NULL COLLATE NOCASE,PRIMARY KEY(project_id,tag));").expect("project table");
        db
    }

    fn write_file(path: &Path, content: &[u8]) {
        fs::write(path, content).expect("write test project");
        let _ = fs::File::open(path).and_then(|file| file.sync_all());
    }

    #[test]
    fn accepts_flp_case_insensitively_and_reads_metadata_only() {
        let folder = temp_dir("extension");
        let path = folder.join("Beat.FLp");
        write_file(&path, b"binary-not-parsed\0\xff");
        let canonical = canonical_project_file(&path).expect("selected .flp file");
        let metadata = metadata_for(&canonical).expect("filesystem metadata");
        assert_eq!(canonical.file_name().unwrap(), "Beat.FLp");
        assert_eq!(metadata.size_bytes, Some(19));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn rejects_missing_files_non_flp_and_parent_traversal() {
        let folder = temp_dir("validation");
        let valid = folder.join("project.flp");
        let other = folder.join("audio.wav");
        write_file(&valid, b"FLP");
        write_file(&other, b"audio");
        assert!(canonical_project_file(&folder.join("missing.flp")).is_err());
        assert!(canonical_project_file(&other).is_err());
        let traversal = folder.join("..").join("escape.flp");
        assert!(canonical_project_file(&traversal).is_err());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn open_lookup_only_returns_a_current_indexed_project() {
        let folder = temp_dir("indexed");
        let path = folder.join("indexed.flp");
        write_file(&path, b"data");
        let canonical = canonical_project_file(&path).expect("valid project");
        let db = test_db();
        db.execute("INSERT INTO projects VALUES ('project-1',?1,'indexed.flp',0,'2026-01-01T00:00:00Z','',0)", [canonical.to_string_lossy()]).expect("index project");
        assert_eq!(
            indexed_project_path(&db, "project-1").expect("indexed ID"),
            canonical
        );
        assert!(indexed_project_path(&db, "unindexed").is_err());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn manual_tags_are_normalized_unique_removable_and_bounded() {
        let mut db = test_db();
        db.execute("INSERT INTO projects VALUES ('project-1','C:/Beat.flp','Beat.flp',0,'2026-01-01T00:00:00Z',NULL,NULL)", []).expect("indexed project");

        add_project_tag(&mut db, "project-1", "  House   Lofi ").expect("manual tag");
        add_project_tag(&mut db, "project-1", "HOUSE LOFI").expect("duplicate tag is idempotent");
        let tags: i64 = db
            .query_row(
                "SELECT count(*) FROM project_tags WHERE project_id='project-1'",
                [],
                |row| row.get(0),
            )
            .expect("tag count");
        assert_eq!(tags, 1);

        for index in 1..12 {
            add_project_tag(&mut db, "project-1", &format!("style-{index}"))
                .expect("up to twelve tags");
        }
        assert!(add_project_tag(&mut db, "project-1", "thirteenth").is_err());
        assert!(add_project_tag(&mut db, "project-1", "a".repeat(33).as_str()).is_err());
        assert!(add_project_tag(&mut db, "project-1", "../outside").is_err());
        remove_project_tag(&mut db, "project-1", "house lofi").expect("remove normalized tag");
        let remaining: i64 = db
            .query_row(
                "SELECT count(*) FROM project_tags WHERE project_id='project-1'",
                [],
                |row| row.get(0),
            )
            .expect("remaining tags");
        assert_eq!(remaining, 11);
        db.execute("DELETE FROM projects WHERE id='project-1'", [])
            .expect("remove project");
        let orphan_tags: i64 = db
            .query_row("SELECT count(*) FROM project_tags", [], |row| row.get(0))
            .expect("cascaded tag cleanup");
        assert_eq!(orphan_tags, 0);
    }

    #[test]
    fn missing_indexed_file_reports_no_current_metadata() {
        let folder = temp_dir("missing");
        let path = folder.join("gone.flp");
        write_file(&path, b"project");
        let canonical = canonical_project_file(&path).expect("selected project");
        let mut db = test_db();
        db.execute("INSERT INTO projects VALUES ('project-1',?1,'gone.flp',0,'2026-01-01T00:00:00Z',NULL,NULL)", [canonical.to_string_lossy()]).expect("indexed project");
        fs::remove_file(&path).expect("remove test project");

        let entry = list_entries(&mut db).expect("list shelf").remove(0);
        assert!(!entry.exists);
        assert_eq!(entry.modified_at, None);
        assert_eq!(entry.size_bytes, None);
        assert!(indexed_project_path(&db, "project-1").is_err());
        let _ = fs::remove_dir_all(folder);
    }
}
