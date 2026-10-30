//! User-authorized, local Downloads organization. Every operation is confined to the selected root.
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use uuid::Uuid;

const PLUGIN_ID: &str = "com.orbitos.downloads-organizer";
const MANAGE_PERMISSION: &str = "downloads-organizer.manage-authorized-folder";
const MAX_RULES: usize = 100;
const MAX_PREVIEW: usize = 500;
const MAX_PATH: usize = 4096;
pub struct DownloadsOrganizerState(Mutex<Connection>);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerRule {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub contains: Option<String>,
    pub target_folder: String,
    pub enabled: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerMove {
    pub source: String,
    pub destination: String,
    pub bytes: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerPreview {
    pub root: String,
    pub moves: Vec<OrganizerMove>,
    pub skipped: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerRun {
    pub id: String,
    pub moved: usize,
    pub skipped: usize,
    pub completed_at: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerSnapshot {
    pub root: Option<String>,
    pub enabled: bool,
    pub rules: Vec<OrganizerRule>,
    pub latest_run: Option<OrganizerRun>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub id: Option<String>,
    pub name: String,
    pub extensions: Vec<String>,
    pub contains: Option<String>,
    pub target_folder: String,
    pub enabled: bool,
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier ORBIT indisponible : {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    dir.push("downloads-organizer.sqlite3");
    let db = Connection::open(dir)
        .map_err(|e| format!("Stockage Downloads Organizer indisponible : {e}"))?;
    db.busy_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS organizer_settings(id INTEGER PRIMARY KEY CHECK(id=1),root TEXT,enabled INTEGER NOT NULL DEFAULT 0); INSERT OR IGNORE INTO organizer_settings(id,root,enabled) VALUES(1,NULL,0); CREATE TABLE IF NOT EXISTS organizer_rules(id TEXT PRIMARY KEY,name TEXT NOT NULL,extensions TEXT NOT NULL,contains TEXT,target_folder TEXT NOT NULL,enabled INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS organizer_runs(id TEXT PRIMARY KEY,completed_at TEXT NOT NULL,skipped INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS organizer_moves(id INTEGER PRIMARY KEY,run_id TEXT NOT NULL REFERENCES organizer_runs(id) ON DELETE CASCADE,source TEXT NOT NULL,destination TEXT NOT NULL,bytes INTEGER NOT NULL,undone INTEGER NOT NULL DEFAULT 0); CREATE INDEX IF NOT EXISTS organizer_moves_run_idx ON organizer_moves(run_id);").map_err(|e|format!("Impossible d’initialiser Downloads Organizer : {e}"))?;
    app.manage(DownloadsOrganizerState(Mutex::new(db)));
    Ok(())
}
pub fn uninstall_cleanup(app: &AppHandle, delete_data: bool) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    let Some(state) = app.try_state::<DownloadsOrganizerState>() else {
        return Ok(());
    };
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    db.execute_batch("DELETE FROM organizer_moves; DELETE FROM organizer_runs; DELETE FROM organizer_rules; UPDATE organizer_settings SET root=NULL,enabled=0 WHERE id=1; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| format!("Impossible d’effacer les règles et journaux Downloads Organizer : {e}"))?;
    Ok(())
}
fn require(app: &AppHandle) -> Result<(), String> {
    crate::plugins::require_plugin_enabled(app, PLUGIN_ID)
}
fn authorize(app: &AppHandle) -> Result<(), String> {
    require(app)?;
    crate::plugins::require_permission(app, PLUGIN_ID, MANAGE_PERMISSION)
}
fn validate_root(path: &str) -> Result<PathBuf, String> {
    if path.len() > MAX_PATH {
        return Err("Le chemin sélectionné est trop long.".into());
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("Sélectionne un dossier dans le sélecteur de fichiers.".into());
    }
    let c = p
        .canonicalize()
        .map_err(|_| "Le dossier autorisé n’existe plus ou est inaccessible.")?;
    if !c.is_dir() {
        return Err("Sélectionne un dossier, pas un fichier.".into());
    }
    Ok(c)
}
fn root_from(db: &Connection) -> Result<PathBuf, String> {
    let root: Option<String> = db
        .query_row("SELECT root FROM organizer_settings WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    validate_root(
        root.as_deref()
            .ok_or("Choisis d’abord le dossier à organiser.")?,
    )
}
fn safe_subfolder(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 100
        || value.contains(['/', '\\', ':'])
        || value.chars().any(char::is_control)
        || value == "."
        || value == ".."
    {
        return Err(
            "Le dossier cible doit être un nom simple, sans chemin ni caractère de contrôle."
                .into(),
        );
    }
    Ok(value.to_owned())
}
fn clean_rule(input: RuleInput) -> Result<OrganizerRule, String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Donne à la règle un nom de 1 à 64 caractères.".into());
    }
    if input.extensions.len() > 50 {
        return Err("Une règle peut contenir jusqu’à 50 extensions.".into());
    }
    let mut extensions = Vec::new();
    for ext in input.extensions {
        let e = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        if e.is_empty() || e.len() > 16 || !e.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("Les extensions doivent contenir uniquement des lettres et chiffres, par exemple pdf.".into());
        }
        if !extensions.contains(&e) {
            extensions.push(e);
        }
    }
    let contains = input
        .contains
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    if extensions.is_empty() && contains.is_none() {
        return Err("Ajoute au moins une extension ou un mot à rechercher dans le nom.".into());
    }
    if contains
        .as_ref()
        .is_some_and(|s| s.len() > 80 || s.chars().any(char::is_control))
    {
        return Err(
            "Le texte recherché est trop long ou contient des caractères non autorisés.".into(),
        );
    }
    let target_folder = safe_subfolder(&input.target_folder)?;
    Ok(OrganizerRule {
        id: input.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: name.into(),
        extensions,
        contains,
        target_folder,
        enabled: input.enabled,
    })
}
fn read_rules(db: &Connection) -> Result<Vec<OrganizerRule>, String> {
    let mut st=db.prepare("SELECT id,name,extensions,contains,target_folder,enabled FROM organizer_rules ORDER BY name COLLATE NOCASE").map_err(|e|e.to_string())?;
    let rows = st
        .query_map([], |r| {
            let json: String = r.get(2)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                json,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)? != 0,
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.map(|r| {
        let (id, name, json, contains, target_folder, enabled) = r.map_err(|e| e.to_string())?;
        Ok(OrganizerRule {
            id,
            name,
            extensions: serde_json::from_str(&json).map_err(|e| e.to_string())?,
            contains,
            target_folder,
            enabled,
        })
    })
    .collect()
}
fn latest(db: &Connection) -> Result<Option<OrganizerRun>, String> {
    db.query_row("SELECT id,completed_at,(SELECT count(*) FROM organizer_moves WHERE run_id=organizer_runs.id AND undone=0),(SELECT skipped FROM organizer_runs WHERE id=organizer_runs.id) FROM organizer_runs ORDER BY completed_at DESC LIMIT 1",[],|r|Ok(OrganizerRun{id:r.get(0)?,completed_at:r.get(1)?,moved:r.get::<_,i64>(2)? as usize,skipped:r.get::<_,i64>(3)? as usize})).optional().map_err(|e|e.to_string())
}
fn snapshot(db: &Connection) -> Result<OrganizerSnapshot, String> {
    let (root, enabled): (Option<String>, i64) = db
        .query_row(
            "SELECT root,enabled FROM organizer_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    Ok(OrganizerSnapshot {
        root,
        enabled: enabled != 0,
        rules: read_rules(db)?,
        latest_run: latest(db)?,
    })
}
#[tauri::command]
pub fn downloads_organizer_snapshot(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
) -> Result<OrganizerSnapshot, String> {
    require(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    snapshot(&db)
}
#[tauri::command]
pub fn downloads_organizer_set_root(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
    path: String,
) -> Result<OrganizerSnapshot, String> {
    authorize(&app)?;
    let root = validate_root(&path)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    db.execute(
        "UPDATE organizer_settings SET root=?1 WHERE id=1",
        [root.to_string_lossy().as_ref()],
    )
    .map_err(|e| format!("Impossible d’enregistrer le dossier : {e}"))?;
    snapshot(&db)
}
#[tauri::command]
pub fn downloads_organizer_set_enabled(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
    enabled: bool,
) -> Result<OrganizerSnapshot, String> {
    if enabled {
        authorize(&app)?;
    } else {
        require(&app)?;
    }
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    if enabled {
        root_from(&db)?;
    }
    db.execute(
        "UPDATE organizer_settings SET enabled=?1 WHERE id=1",
        [enabled as i64],
    )
    .map_err(|e| e.to_string())?;
    snapshot(&db)
}
#[tauri::command]
pub fn downloads_organizer_save_rule(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
    input: RuleInput,
) -> Result<OrganizerSnapshot, String> {
    authorize(&app)?;
    let rule = clean_rule(input)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    let count: usize = db
        .query_row("SELECT count(*) FROM organizer_rules", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;
    if count >= MAX_RULES && !read_rules(&db)?.iter().any(|r| r.id == rule.id) {
        return Err(format!("Tu peux enregistrer jusqu’à {MAX_RULES} règles."));
    }
    let ext = serde_json::to_string(&rule.extensions).map_err(|e| e.to_string())?;
    db.execute("INSERT INTO organizer_rules(id,name,extensions,contains,target_folder,enabled) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name,extensions=excluded.extensions,contains=excluded.contains,target_folder=excluded.target_folder,enabled=excluded.enabled",params![rule.id,rule.name,ext,rule.contains,rule.target_folder,rule.enabled as i64]).map_err(|e|format!("Impossible d’enregistrer la règle : {e}"))?;
    snapshot(&db)
}
#[tauri::command]
pub fn downloads_organizer_delete_rule(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
    id: String,
) -> Result<OrganizerSnapshot, String> {
    authorize(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    db.execute("DELETE FROM organizer_rules WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    snapshot(&db)
}
fn matches(rule: &OrganizerRule, name: &str) -> bool {
    let lower = name.to_lowercase();
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    (rule.extensions.is_empty() || rule.extensions.iter().any(|e| e == &ext))
        && rule
            .contains
            .as_ref()
            .map_or(true, |needle| lower.contains(needle))
}
fn unique_destination(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let base = Path::new(name);
    let mut candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = base.file_stem().unwrap_or(name).to_string_lossy();
    let ext = base
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy()))
        .unwrap_or_default();
    for n in 2..=9999 {
        candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{}", Uuid::new_v4(), ext))
}
fn target_directory(root: &Path, folder: &str, create: bool) -> Result<PathBuf, String> {
    let target = root.join(safe_subfolder(folder)?);
    if target.exists() {
        let metadata = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(
                "Le dossier cible est un lien ou n’est pas un dossier. Choisis un autre nom."
                    .into(),
            );
        }
        let canonical = target.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(root) {
            return Err(
                "Le dossier cible sort du dossier autorisé; l’opération a été bloquée.".into(),
            );
        }
        return Ok(canonical);
    }
    if create {
        fs::create_dir(&target)
            .map_err(|e| format!("Impossible de créer le dossier cible : {e}"))?;
        let canonical = target.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(root) {
            return Err(
                "Le dossier cible sort du dossier autorisé; l’opération a été bloquée.".into(),
            );
        }
        Ok(canonical)
    } else {
        Ok(target)
    }
}
fn preview(db: &Connection) -> Result<OrganizerPreview, String> {
    let root = root_from(db)?;
    let rules = read_rules(db)?
        .into_iter()
        .filter(|r| r.enabled)
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return Err("Active au moins une règle avant de lancer l’aperçu.".into());
    }
    let mut moves = Vec::new();
    let mut skipped = 0;
    for entry in
        fs::read_dir(&root).map_err(|e| format!("Impossible de lire le dossier autorisé : {e}"))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_type = entry.file_type().map_err(|e| e.to_string())?;
        if !entry_type.is_file() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Impossible de lire les métadonnées du fichier : {e}"))?;
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        if let Some(rule) = rules.iter().find(|r| matches(r, &name_s)) {
            let target = target_directory(&root, &rule.target_folder, false)?;
            let destination = unique_destination(&target, &name);
            if moves.len() < MAX_PREVIEW {
                moves.push(OrganizerMove {
                    source: entry.path().to_string_lossy().into_owned(),
                    destination: destination.to_string_lossy().into_owned(),
                    bytes: metadata.len(),
                });
            } else {
                skipped += 1;
            }
        }
    }
    Ok(OrganizerPreview {
        root: root.to_string_lossy().into_owned(),
        moves,
        skipped,
    })
}
#[tauri::command]
pub fn downloads_organizer_preview(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
) -> Result<OrganizerPreview, String> {
    require(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    preview(&db)
}
async fn confirm_run(app: &AppHandle, files: usize) -> Result<bool, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!("ORBIT déplacera {files} fichier(s) dans les dossiers montrés par la simulation. Aucun fichier ne sera supprimé. Tu pourras annuler l’opération ensuite."))
        .title("Confirmer le classement")
        .buttons(MessageDialogButtons::OkCancel)
        .kind(MessageDialogKind::Warning)
        .show(move |accepted| { let _ = sender.send(accepted); });
    receiver.await.map_err(|_| {
        "La confirmation système n’a pas répondu. Aucun fichier n’a été déplacé.".to_owned()
    })
}
fn check_confined(root: &Path, path: &Path) -> Result<(), String> {
    if !path.starts_with(root) || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Une opération sortirait du dossier autorisé; elle a été bloquée.".into());
    }
    Ok(())
}
fn prevalidate_moves(
    root: &Path,
    moves: &[OrganizerMove],
) -> Result<Vec<(PathBuf, PathBuf, u64, String)>, String> {
    moves.iter().map(|item| {
        let src = PathBuf::from(&item.source);
        let dst = PathBuf::from(&item.destination);
        check_confined(root, &src)?;
        check_confined(root, &dst)?;
        let source_type = fs::symlink_metadata(&src).map_err(|e| format!("Un fichier de l’aperçu a changé : {e}"))?;
        if !source_type.is_file() || source_type.file_type().is_symlink() {
            return Err("Un élément de l’aperçu n’est plus un fichier local ordinaire.".into());
        }
        let canonical_source = src.canonicalize().map_err(|e| format!("Un fichier de l’aperçu est inaccessible : {e}"))?;
        if !canonical_source.starts_with(root) { return Err("Un fichier de l’aperçu sort du dossier autorisé.".into()); }
        if dst.exists() { return Err("Le dossier a changé depuis l’aperçu. Actualise la simulation avant de continuer.".into()); }
        let folder = dst.parent().and_then(Path::file_name).and_then(|n| n.to_str()).ok_or("Dossier cible invalide.")?.to_owned();
        let target = target_directory(root, &folder, false)?;
        if dst.parent() != Some(target.as_path()) { return Err("La destination de l’aperçu n’est plus valide.".into()); }
        Ok((canonical_source, dst, source_type.len(), folder))
    }).collect()
}
fn recovery_journal(
    db: &mut Connection,
    run: &str,
    moves: &[(PathBuf, PathBuf, u64)],
    skipped: usize,
) -> Result<usize, String> {
    let stranded = moves
        .iter()
        .filter(|(_, dst, _)| dst.is_file())
        .collect::<Vec<_>>();
    if stranded.is_empty() {
        return Ok(0);
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO organizer_runs(id,completed_at,skipped) VALUES(?1,?2,?3)",
        params![run, Utc::now().to_rfc3339(), skipped],
    )
    .map_err(|e| e.to_string())?;
    for (src, dst, bytes) in &stranded {
        tx.execute(
            "INSERT INTO organizer_moves(run_id,source,destination,bytes) VALUES(?1,?2,?3,?4)",
            params![
                run,
                src.to_string_lossy(),
                dst.to_string_lossy(),
                *bytes as i64
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    let count = stranded.len();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}
fn rollback_moves(
    db: &mut Connection,
    run: &str,
    moves: &[(PathBuf, PathBuf, u64)],
    skipped: usize,
    cause: &str,
) -> String {
    let mut failures = Vec::new();
    for (from, to, _) in moves.iter().rev() {
        if to.exists() && from.exists() {
            failures.push(format!(
                "{} (l’emplacement d’origine est déjà occupé)",
                to.display()
            ));
        } else if to.exists() {
            if let Err(error) = fs::rename(to, from) {
                failures.push(format!("{} ({error})", to.display()));
            }
        }
    }
    if failures.is_empty() {
        return format!(
            "{cause} Tous les fichiers déjà déplacés ont été remis à leur emplacement initial."
        );
    }
    match recovery_journal(db, run, moves, skipped) {
        Ok(count) => format!("{cause} La remise en place a échoué pour : {}. {count} fichier(s) restant(s) ont été inscrits au journal de récupération (opération {run}) pour permettre une annulation.", failures.join(", ")),
        Err(journal_error) => format!("{cause} La remise en place a échoué pour : {}. Le journal de récupération n’a pas pu être conservé : {journal_error}. Opération {run}.", failures.join(", ")),
    }
}
fn rollback_undo(restored: &[(PathBuf, PathBuf)], cause: &str) -> String {
    let mut failures = Vec::new();
    for (source, destination) in restored.iter().rev() {
        if source.exists() && destination.exists() {
            failures.push(format!(
                "{} (la destination organisée est déjà occupée)",
                destination.display()
            ));
        } else if source.exists() {
            if let Err(error) = fs::rename(source, destination) {
                failures.push(format!("{} ({error})", source.display()));
            }
        }
    }
    if failures.is_empty() {
        format!("{cause} Les fichiers ont été replacés dans le dossier organisé et le journal reste réversible.")
    } else {
        format!("{cause} L’annulation partielle n’a pas pu être remise dans son état précédent pour : {}. Le journal n’a pas été marqué comme annulé; vérifie ces fichiers avant une nouvelle tentative.", failures.join(", "))
    }
}
fn finish_undo(
    db: &Connection,
    run_id: &str,
    restored: &[(PathBuf, PathBuf)],
) -> Result<(), String> {
    db.execute(
        "UPDATE organizer_moves SET undone=1 WHERE run_id=?1",
        [run_id],
    )
    .map(|_| ())
    .map_err(|error| {
        rollback_undo(
            restored,
            &format!("Le journal n’a pas pu être marqué comme annulé : {error}."),
        )
    })
}
#[tauri::command]
pub async fn downloads_organizer_run(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
) -> Result<Option<OrganizerRun>, String> {
    authorize(&app)?;
    let initial_plan = {
        let db = state
            .0
            .lock()
            .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
        let active: bool = db
            .query_row(
                "SELECT enabled FROM organizer_settings WHERE id=1",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            != 0;
        if !active {
            return Err("L’organisateur est en pause. Active-le avant l’exécution.".into());
        }
        preview(&db)?
    };
    if initial_plan.moves.is_empty() {
        return Err("La simulation ne trouve aucun fichier à déplacer.".into());
    }
    if !confirm_run(&app, initial_plan.moves.len()).await? {
        return Ok(None);
    }
    authorize(&app)?;
    let mut db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    let active: bool = db
        .query_row(
            "SELECT enabled FROM organizer_settings WHERE id=1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;
    if !active {
        return Err("L’organisateur est en pause. Active-le avant l’exécution.".into());
    }
    let plan = preview(&db)?;
    let root = root_from(&db)?;
    let run = Uuid::new_v4().to_string();
    // Validate the complete plan before the first mutation. This catches stale files, invalid
    // destinations and links without leaving a half-completed organization behind.
    let prepared = prevalidate_moves(&root, &plan.moves)?;
    let mut applied: Vec<(PathBuf, PathBuf, u64)> = Vec::new();
    for (src, dst, bytes, folder) in prepared {
        let pre_move_check = (|| -> Result<(), String> {
            check_confined(&root, &src)?;
            check_confined(&root, &dst)?;
            let metadata = fs::symlink_metadata(&src)
                .map_err(|e| format!("Un fichier de l’aperçu a changé : {e}"))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || dst.exists() {
                return Err("Le dossier a changé depuis l’aperçu. Actualise la simulation avant de continuer.".into());
            }
            let parent = target_directory(&root, &folder, true)?;
            check_confined(&root, &parent)?;
            if dst.parent() != Some(parent.as_path()) {
                return Err("La destination a changé depuis l’aperçu.".into());
            }
            Ok(())
        })();
        if let Err(error) = pre_move_check {
            return Err(rollback_moves(
                &mut db,
                &run,
                &applied,
                plan.skipped,
                &error,
            ));
        }
        if let Err(e) = fs::rename(&src, &dst) {
            return Err(rollback_moves(
                &mut db,
                &run,
                &applied,
                plan.skipped,
                &format!("Déplacement interrompu : {e}."),
            ));
        }
        applied.push((src, dst, bytes));
    }
    let now = Utc::now().to_rfc3339();
    let journal_result =
        (|| -> Result<(), String> {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO organizer_runs(id,completed_at,skipped) VALUES(?1,?2,?3)",
                params![run, now, plan.skipped],
            )
            .map_err(|e| e.to_string())?;
            for (src, dst, bytes) in &applied {
                tx.execute(
                "INSERT INTO organizer_moves(run_id,source,destination,bytes) VALUES(?1,?2,?3,?4)",
                params![run, src.to_string_lossy(), dst.to_string_lossy(), *bytes as i64],
            ).map_err(|e| e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())
        })();
    if let Err(error) = journal_result {
        return Err(rollback_moves(
            &mut db,
            &run,
            &applied,
            plan.skipped,
            &format!("Le journal du classement n’a pas pu être enregistré : {error}."),
        ));
    }
    Ok(Some(OrganizerRun {
        id: run,
        completed_at: now,
        moved: applied.len(),
        skipped: plan.skipped,
    }))
}
#[tauri::command(rename_all = "camelCase")]
pub fn downloads_organizer_undo(
    app: AppHandle,
    state: State<'_, DownloadsOrganizerState>,
    run_id: String,
) -> Result<OrganizerSnapshot, String> {
    authorize(&app)?;
    let db = state
        .0
        .lock()
        .map_err(|_| "Stockage Organizer verrouillé.".to_string())?;
    let root = root_from(&db)?;
    let mut st=db.prepare("SELECT source,destination FROM organizer_moves WHERE run_id=?1 AND undone=0 ORDER BY id DESC").map_err(|e|e.to_string())?;
    let rows = st
        .query_map([&run_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let moves = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(st);
    if moves.is_empty() {
        return Err("Aucun déplacement réversible ne reste pour cette opération.".into());
    }
    for (src, dst) in &moves {
        let s = PathBuf::from(src);
        let d = PathBuf::from(dst);
        check_confined(&root, &s)?;
        check_confined(&root, &d)?;
        let destination_meta = fs::symlink_metadata(&d)
            .map_err(|_| "Un fichier classé a changé ou n’est plus accessible.")?;
        let folder = d
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            .ok_or("Dossier classé invalide.")?;
        let safe_parent = target_directory(&root, folder, false)?;
        if d.parent() != Some(safe_parent.as_path())
            || !destination_meta.is_file()
            || destination_meta.file_type().is_symlink()
            || s.exists()
            || s.parent() != Some(root.as_path())
        {
            return Err(
                "L’annulation est bloquée car un fichier a changé ou occupe déjà la destination."
                    .into(),
            );
        }
    }
    let mut restored = Vec::new();
    for (src, dst) in &moves {
        let s = PathBuf::from(src);
        let d = PathBuf::from(dst);
        if let Err(e) = fs::rename(&d, &s) {
            return Err(rollback_undo(
                &restored,
                &format!("Annulation interrompue : {e}."),
            ));
        }
        restored.push((s, d));
    }
    finish_undo(&db, &run_id, &restored)?;
    snapshot(&db)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_require_safe_target_and_matching_criteria() {
        let r = clean_rule(RuleInput {
            id: None,
            name: "Images".into(),
            extensions: vec![".PNG".into()],
            contains: None,
            target_folder: "Images".into(),
            enabled: true,
        })
        .unwrap();
        assert!(matches(&r, "photo.png"));
        assert!(!matches(&r, "photo.jpg"));
        assert!(clean_rule(RuleInput {
            id: None,
            name: "bad".into(),
            extensions: vec!["..".into()],
            contains: None,
            target_folder: "Images".into(),
            enabled: true
        })
        .is_err());
        assert!(safe_subfolder("../secret").is_err());
    }
    #[test]
    fn move_stays_inside_authorized_root() {
        let root = Path::new("C:/Users/test/Downloads");
        assert!(check_confined(root, &root.join("Images/a.png")).is_ok());
        assert!(check_confined(root, Path::new("C:/Users/test/Documents/a.png")).is_err());
    }
    #[test]
    fn collisions_get_a_new_name() {
        let d = std::env::temp_dir().join(format!("orbit-organizer-{}", Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("a.pdf"), b"x").unwrap();
        let path = unique_destination(&d, std::ffi::OsStr::new("a.pdf"));
        assert_eq!(path.file_name().unwrap(), "a (2).pdf");
        fs::remove_dir_all(d).unwrap();
    }

    fn journal_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE organizer_runs(id TEXT PRIMARY KEY,completed_at TEXT NOT NULL,skipped INTEGER NOT NULL DEFAULT 0); CREATE TABLE organizer_moves(id INTEGER PRIMARY KEY,run_id TEXT NOT NULL,source TEXT NOT NULL,destination TEXT NOT NULL,bytes INTEGER NOT NULL,undone INTEGER NOT NULL DEFAULT 0);").unwrap();
        db
    }

    #[test]
    fn prevalidation_failure_on_second_item_moves_nothing() {
        let root =
            std::env::temp_dir().join(format!("orbit-organizer-preflight-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.pdf");
        fs::write(&first, b"first").unwrap();
        let preview = OrganizerPreview {
            root: root.to_string_lossy().into_owned(),
            moves: vec![
                OrganizerMove {
                    source: first.to_string_lossy().into_owned(),
                    destination: root
                        .join("Documents/first.pdf")
                        .to_string_lossy()
                        .into_owned(),
                    bytes: 5,
                },
                OrganizerMove {
                    source: root.join("missing.pdf").to_string_lossy().into_owned(),
                    destination: root
                        .join("Documents/missing.pdf")
                        .to_string_lossy()
                        .into_owned(),
                    bytes: 1,
                },
            ],
            skipped: 0,
        };
        assert!(prevalidate_moves(&root, &preview.moves).is_err());
        assert!(
            first.is_file(),
            "prevalidation must finish before moving the first item"
        );
        assert!(!root.join("Documents").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mid_batch_failure_restores_files_already_moved() {
        let root =
            std::env::temp_dir().join(format!("orbit-organizer-rollback-{}", Uuid::new_v4()));
        let target = root.join("Documents");
        fs::create_dir_all(&target).unwrap();
        let source = root.join("first.pdf");
        let destination = target.join("first.pdf");
        fs::write(&source, b"first").unwrap();
        fs::rename(&source, &destination).unwrap();
        // Force the second move in a batch to fail after the first succeeded.
        assert!(fs::rename(root.join("missing.pdf"), target.join("missing.pdf")).is_err());
        let mut db = journal_db();
        let error = rollback_moves(
            &mut db,
            "run-test",
            &[(source.clone(), destination.clone(), 5)],
            0,
            "Déplacement interrompu.",
        );
        assert!(source.is_file());
        assert!(!destination.exists());
        assert!(error.contains("remis à leur emplacement initial"));
        let journals: i64 = db
            .query_row("SELECT count(*) FROM organizer_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            journals, 0,
            "a fully restored batch needs no recovery journal"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_restore_keeps_recovery_entry_and_reports_it() {
        let root =
            std::env::temp_dir().join(format!("orbit-organizer-recovery-{}", Uuid::new_v4()));
        let target = root.join("Documents");
        fs::create_dir_all(&target).unwrap();
        let source = root.join("first.pdf");
        let destination = target.join("first.pdf");
        fs::write(&source, b"new original").unwrap();
        fs::write(&destination, b"moved file").unwrap();
        let mut db = journal_db();
        let error = rollback_moves(
            &mut db,
            "run-recovery",
            &[(source.clone(), destination.clone(), 10)],
            0,
            "Déplacement interrompu.",
        );
        assert!(error.contains("journal de récupération"));
        let (runs, moves): (i64, i64) = db.query_row("SELECT (SELECT count(*) FROM organizer_runs),(SELECT count(*) FROM organizer_moves)", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((runs, moves), (1, 1));
        assert!(
            destination.is_file(),
            "the journal must preserve the stranded destination for manual undo"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_update_failure_reorganizes_restored_files_to_match_journal() {
        let root =
            std::env::temp_dir().join(format!("orbit-organizer-undo-journal-{}", Uuid::new_v4()));
        let target = root.join("Documents");
        fs::create_dir_all(&target).unwrap();
        let source = root.join("first.pdf");
        let destination = target.join("first.pdf");
        fs::write(&source, b"file").unwrap();
        let db = journal_db();
        db.execute_batch("INSERT INTO organizer_runs(id,completed_at,skipped) VALUES('run','now',0); INSERT INTO organizer_moves(run_id,source,destination,bytes) VALUES('run','source','destination',4); CREATE TRIGGER fail_undo_update BEFORE UPDATE ON organizer_moves BEGIN SELECT RAISE(ABORT,'forced journal failure'); END;").unwrap();
        let restored = vec![(source.clone(), destination.clone())];
        assert!(finish_undo(&db, "run", &restored).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"file");
        assert!(!source.exists());
        let undone: i64 = db
            .query_row(
                "SELECT undone FROM organizer_moves WHERE run_id='run'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            undone, 0,
            "file placement must match the unchanged journal after an update failure"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
