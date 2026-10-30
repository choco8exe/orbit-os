//! Local notes attached to FL Studio projects in Project Shelf.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, time::Duration};
use tauri::{AppHandle, Manager};

const PROJECT_PLUGIN: &str = "com.orbitos.fl-studio";
const NOTES_PERMISSION: &str = "fl-studio.session-notes";

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let _ = db(app)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioNote {
    pub project_id: String,
    pub project_name: String,
    pub style: String,
    pub bpm: Option<u16>,
    pub musical_key: String,
    pub notes: String,
    pub updated_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioNoteInput {
    pub project_id: String,
    pub style: String,
    pub bpm: Option<u16>,
    pub musical_key: String,
    pub notes: String,
}

fn db(app: &AppHandle) -> Result<Connection, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Dossier local ORBIT indisponible.".to_owned())?;
    fs::create_dir_all(&dir)
        .map_err(|_| "Impossible de préparer le stockage des notes de studio.".to_owned())?;
    let conn = Connection::open(dir.join("creator-studio.sqlite3"))
        .map_err(|_| "Stockage des notes de studio indisponible.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(3))
        .map_err(|_| "Stockage des notes de studio occupé.".to_owned())?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS studio_notes (project_id TEXT PRIMARY KEY, style TEXT NOT NULL, bpm INTEGER, musical_key TEXT NOT NULL, notes TEXT NOT NULL, updated_at TEXT NOT NULL);")
        .map_err(|_| "Impossible d’initialiser le stockage des notes de studio.".to_owned())?;
    Ok(conn)
}

fn validate_project(app: &AppHandle, project_id: &str) -> Result<String, String> {
    crate::plugins::require_permission(app, PROJECT_PLUGIN, "project-shelf.local-library")?;
    if project_id.is_empty() || project_id.len() > 128 || project_id.chars().any(char::is_control) {
        return Err("Identifiant de projet invalide.".into());
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "Dossier Project Shelf indisponible.".to_owned())?
        .join("fl-studio-project-shelf.sqlite3");
    let conn =
        Connection::open(path).map_err(|_| "Project Shelf n’est pas initialisé.".to_owned())?;
    conn.query_row(
        "SELECT name FROM projects WHERE id=?1",
        [project_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|_| "Impossible de vérifier le projet Project Shelf.".to_owned())?
    .ok_or_else(|| "Ce projet n’existe plus.".into())
}

fn validate_note(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.chars().count() > max || value.chars().any(|character| character == '\0') {
        Err(format!("{label} dépasse la limite autorisée."))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn creator_plugins_studio_get_note(
    app: AppHandle,
    project_id: String,
) -> Result<Option<StudioNote>, String> {
    crate::plugins::require_permission(&app, PROJECT_PLUGIN, NOTES_PERMISSION)?;
    let project_name = validate_project(&app, &project_id)?;
    db(&app)?
        .query_row(
            "SELECT style,bpm,musical_key,notes,updated_at FROM studio_notes WHERE project_id=?1",
            [&project_id],
            |row| {
                Ok(StudioNote {
                    project_id: project_id.clone(),
                    project_name,
                    style: row.get(0)?,
                    bpm: row.get(1)?,
                    musical_key: row.get(2)?,
                    notes: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|_| "Impossible de lire les notes de studio.".to_owned())
}

#[tauri::command]
pub fn creator_plugins_studio_save_note(
    app: AppHandle,
    note: StudioNoteInput,
) -> Result<StudioNote, String> {
    crate::plugins::require_permission(&app, PROJECT_PLUGIN, NOTES_PERMISSION)?;
    let project_name = validate_project(&app, &note.project_id)?;
    validate_note(&note.style, 48, "Le style")?;
    validate_note(&note.musical_key, 16, "La tonalité")?;
    validate_note(&note.notes, 4000, "Les notes")?;
    if note.bpm.is_some_and(|bpm| !(20..=400).contains(&bpm)) {
        return Err("Le BPM doit être compris entre 20 et 400.".into());
    }
    let updated = chrono::Utc::now().to_rfc3339();
    db(&app)?.execute("INSERT INTO studio_notes(project_id,style,bpm,musical_key,notes,updated_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(project_id) DO UPDATE SET style=excluded.style,bpm=excluded.bpm,musical_key=excluded.musical_key,notes=excluded.notes,updated_at=excluded.updated_at", params![note.project_id, note.style, note.bpm, note.musical_key, note.notes, updated])
        .map_err(|_| "Impossible d’enregistrer les notes de studio.".to_owned())?;
    Ok(StudioNote {
        project_id: note.project_id,
        project_name,
        style: note.style,
        bpm: note.bpm,
        musical_key: note.musical_key,
        notes: note.notes,
        updated_at: updated,
    })
}

#[tauri::command]
pub fn creator_plugins_studio_delete_note(
    app: AppHandle,
    project_id: String,
) -> Result<(), String> {
    crate::plugins::require_permission(&app, PROJECT_PLUGIN, NOTES_PERMISSION)?;
    let _ = validate_project(&app, &project_id)?;
    db(&app)?
        .execute("DELETE FROM studio_notes WHERE project_id=?1", [project_id])
        .map_err(|_| "Impossible de supprimer les notes de studio.".to_owned())?;
    Ok(())
}

pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    // FL Studio notes are separate from the Stream Deck companion's pairing credentials.
    // Removing Project Shelf data must never revoke or delete the Stream Deck pairing.
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Dossier des notes FL Studio indisponible : {error}"))?
        .join("creator-studio.sqlite3");
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Impossible d’effacer les notes FL Studio : {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_note_lengths_and_bpm_bounds() {
        assert!(validate_note("Ambient", 48, "Le style").is_ok());
        assert!(validate_note(&"x".repeat(49), 48, "Le style").is_err());
        assert!(validate_note("x\0y", 10, "Les notes").is_err());
        assert!((20..=400).contains(&120));
        assert!(!(20..=400).contains(&401));
    }
}
