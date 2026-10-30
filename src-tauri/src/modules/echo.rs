use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::Duration,
};

use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

const CHANGE_EVENT: &str = "echo://changed";
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_ENTRIES_IN_SNAPSHOT: usize = 200;
const MAX_COLLECTIONS_IN_SNAPSHOT: usize = 100;
const MAX_COLLECTION_LINKS_IN_SEARCH: usize = 32;
const MAX_ENTRY_PREVIEW_CHARS: i64 = 220;
const MAX_ENTRY_DETAIL_BYTES: usize = 5_000_000;
const IMAGE_DATA_URL_PREFIX: &str = "data:image/png;base64,";
const MAX_SNAPSHOT_IMAGE_RAW_BYTES: usize =
    (MAX_SNAPSHOT_IMAGE_DATA_URL_BYTES - IMAGE_DATA_URL_PREFIX.len()) * 3 / 4;
// Echo entries may contain private clipboard data. Bound the serialized image previews
// independently from the per-entry storage limit to keep IPC responses predictable.
const MAX_SNAPSHOT_IMAGE_DATA_URL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoPreferences {
    pub monitoring_enabled: bool,
    pub capture_images: bool,
    pub retention_days: u32,
    pub max_entry_bytes: u64,
    pub excluded_apps: Vec<String>,
}

impl Default for EchoPreferences {
    fn default() -> Self {
        Self {
            monitoring_enabled: false,
            capture_images: false,
            retention_days: 30,
            max_entry_bytes: 1_000_000,
            excluded_apps: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoCollection {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoEntry {
    pub id: String,
    pub kind: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub image_data_url: Option<String>,
    pub preview: String,
    pub byte_length: u64,
    pub source_app: Option<String>,
    pub favorite: bool,
    pub pinned: bool,
    pub collection_ids: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoEntryDetail {
    pub id: String,
    pub kind: String,
    pub text: Option<String>,
    pub image_data_url: Option<String>,
    pub byte_length: u64,
    pub truncated: bool,
}

struct EchoEntryMetadata {
    kind: String,
    preview: String,
    byte_length: u64,
    text_bytes: usize,
    image_bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoSnapshot {
    pub entries: Vec<EchoEntry>,
    pub collections: Vec<EchoCollection>,
    pub preferences: EchoPreferences,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoSearchHit {
    pub id: String,
    pub kind: String,
    pub preview: String,
    pub created_at: String,
    pub favorite: bool,
    pub pinned: bool,
    pub collection_ids: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoDuplicateSet {
    pub ids: Vec<String>,
    pub kind: String,
    pub preview: String,
    pub count: usize,
}

struct EchoInner {
    db: Mutex<Connection>,
    preferences: RwLock<EchoPreferences>,
    monitoring: AtomicBool,
    suppressed_clipboard_sequence: AtomicU32,
    clipboard_sequence_lock: Mutex<()>,
    detail_prompt_pending: AtomicBool,
    database_available: bool,
    unavailable_reason: Option<String>,
}

pub struct EchoState(Arc<EchoInner>);

impl EchoInner {
    fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Dossier Echo inaccessible: {e}"))?;
        }
        let connection =
            Connection::open(path).map_err(|e| format!("Base Echo inaccessible: {e}"))?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA secure_delete=ON;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS preferences (
               id INTEGER PRIMARY KEY CHECK(id = 1), monitoring_enabled INTEGER NOT NULL,
               capture_images INTEGER NOT NULL, retention_days INTEGER NOT NULL,
               max_entry_bytes INTEGER NOT NULL, excluded_apps TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS entries (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, created_at TEXT NOT NULL,
               text_content TEXT, image_png BLOB, preview TEXT NOT NULL, byte_length INTEGER NOT NULL,
               source_app TEXT, favorite INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS entries_created_idx ON entries(created_at DESC);
             CREATE TABLE IF NOT EXISTS collections (
               id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS entry_collections (
               entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
               collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
               PRIMARY KEY(entry_id, collection_id)
             );
             INSERT OR IGNORE INTO preferences
               (id, monitoring_enabled, capture_images, retention_days, max_entry_bytes, excluded_apps)
               VALUES (1, 0, 0, 30, 1000000, '[]');",
        ).map_err(|e| format!("Impossible d’initialiser Echo: {e}"))?;
        let preferences = read_preferences(&connection).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(connection),
            monitoring: AtomicBool::new(preferences.monitoring_enabled),
            suppressed_clipboard_sequence: AtomicU32::new(0),
            clipboard_sequence_lock: Mutex::new(()),
            detail_prompt_pending: AtomicBool::new(false),
            preferences: RwLock::new(preferences),
            database_available: true,
            unavailable_reason: None,
        })
    }
}

/// Called from Tauri setup. Storage failure is reported to the host; CORE can keep the app alive.
pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données ORBIT indisponible: {e}"))?;
    path.push("echo.sqlite3");
    let inner = Arc::new(EchoInner::open(path)?);
    let worker_inner = inner.clone();
    let worker_app = app.clone();
    app.manage(EchoState(inner));
    thread::Builder::new()
        .name("orbit-echo-clipboard".into())
        .spawn(move || clipboard_worker(worker_app, worker_inner))
        .map_err(|e| format!("Impossible de démarrer Echo: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn clipboard_sequence() -> u32 {
    0
}

fn clipboard_worker(app: AppHandle, inner: Arc<EchoInner>) {
    let mut last_sequence = clipboard_sequence();
    loop {
        thread::sleep(POLL_INTERVAL);
        if !inner.monitoring.load(Ordering::Relaxed) {
            last_sequence = clipboard_sequence();
            continue;
        }
        let Ok(_clipboard_guard) = inner.clipboard_sequence_lock.lock() else {
            continue;
        };
        let sequence = clipboard_sequence();
        if !should_capture_clipboard(
            sequence,
            &mut last_sequence,
            &inner.suppressed_clipboard_sequence,
        ) {
            continue;
        }
        if let Err(error) = capture_current(&inner) {
            // Do not include clipboard contents in diagnostics.
            eprintln!("Echo capture failed: {error}");
        } else {
            let _ = app.emit(CHANGE_EVENT, ());
        }
    }
}

fn should_capture_clipboard(
    sequence: u32,
    last_sequence: &mut u32,
    suppressed_sequence: &AtomicU32,
) -> bool {
    if sequence == 0 || sequence == *last_sequence {
        return false;
    }
    *last_sequence = sequence;
    let suppressed = suppressed_sequence.load(Ordering::Acquire);
    if suppressed == sequence {
        let _ = suppressed_sequence.compare_exchange(
            suppressed,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        return false;
    }
    if suppressed != 0 {
        let _ = suppressed_sequence.compare_exchange(
            suppressed,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    true
}

fn capture_current(inner: &EchoInner) -> Result<(), String> {
    let preferences = inner
        .preferences
        .read()
        .map_err(|_| "Echo preferences lock failed")?
        .clone();
    let source_app = clipboard_owner_app_name();
    if is_excluded_app(source_app.as_deref(), &preferences.excluded_apps) {
        return Ok(());
    }
    let mut clipboard = Clipboard::new().map_err(|_| "Windows clipboard is unavailable")?;
    if let Ok(text) = clipboard.get_text() {
        if text.is_empty() || text.len() > preferences.max_entry_bytes as usize {
            return Ok(());
        }
        let kind = if text.trim_start().starts_with("https://")
            || text.trim_start().starts_with("http://")
        {
            "url"
        } else {
            "text"
        };
        insert_text(inner, kind, &text, source_app.as_deref())?;
        return Ok(());
    }
    if !preferences.capture_images {
        return Ok(());
    }
    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(_) => return Ok(()),
    };
    let raw = image.bytes.as_ref();
    if raw.len() > preferences.max_entry_bytes as usize || image.width == 0 || image.height == 0 {
        return Ok(());
    }
    let png = encode_png(image.width as u32, image.height as u32, raw)?;
    insert_image(
        inner,
        image.width,
        image.height,
        png,
        raw.len() as u64,
        source_app.as_deref(),
    )?;
    Ok(())
}

/// Returns only the executable basename of the process that owns the clipboard.
/// If Windows cannot identify that process, the caller fails closed whenever any
/// application exclusions are configured.
#[cfg(windows)]
fn clipboard_owner_app_name() -> Option<String> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    use windows::Win32::{
        System::DataExchange::GetClipboardOwner, UI::WindowsAndMessaging::GetWindowThreadProcessId,
    };

    let owner = unsafe { GetClipboardOwner() }.ok()?;
    if owner.0.is_null() {
        return None;
    }
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(owner, Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }

    let process_id = Pid::from_u32(process_id);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    system.process(process_id).and_then(|process| {
        process
            .exe()
            .and_then(|path| path.file_stem())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    })
}

#[cfg(not(windows))]
fn clipboard_owner_app_name() -> Option<String> {
    None
}

fn normalize_app_name(name: &str) -> Option<String> {
    let basename = name.trim().rsplit(['\\', '/']).next()?.trim();
    if basename.is_empty() || basename.len() > 128 {
        return None;
    }
    let normalized = basename.to_lowercase();
    Some(
        normalized
            .strip_suffix(".exe")
            .unwrap_or(&normalized)
            .to_owned(),
    )
}

fn is_excluded_app(source_app: Option<&str>, excluded_apps: &[String]) -> bool {
    if excluded_apps.is_empty() {
        return false;
    }
    let Some(source_app) = source_app.and_then(normalize_app_name) else {
        return true;
    };
    excluded_apps
        .iter()
        .filter_map(|app| normalize_app_name(app))
        .any(|app| app == source_app)
}

fn normalize_exclusions(apps: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for app in apps.into_iter().take(100) {
        if let Some(app) = normalize_app_name(&app) {
            if !normalized.contains(&app) {
                normalized.push(app);
            }
        }
    }
    normalized
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("Image clipboard invalide: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("Impossible d’encoder l’image: {e}"))?;
    }
    Ok(output)
}

fn bounded_image_data_url(bytes: Vec<u8>, remaining_budget: &mut usize) -> Option<String> {
    let prefix = IMAGE_DATA_URL_PREFIX;
    if bytes.len() > MAX_SNAPSHOT_IMAGE_RAW_BYTES {
        return None;
    }
    let encoded_len = bytes
        .len()
        .saturating_add(2)
        .checked_div(3)?
        .saturating_mul(4);
    let output_len = prefix.len().saturating_add(encoded_len);
    if output_len > *remaining_budget {
        return None;
    }
    *remaining_budget -= output_len;
    Some(format!("{prefix}{}", BASE64.encode(bytes)))
}

fn insert_text(
    inner: &EchoInner,
    kind: &str,
    text: &str,
    source_app: Option<&str>,
) -> Result<(), String> {
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    if newest_text_is(&db, text)? {
        return Ok(());
    }
    let created_at = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let preview = text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(220)
        .collect::<String>();
    db.execute("INSERT INTO entries(id, kind, created_at, text_content, preview, byte_length, source_app) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, kind, created_at, text, preview, text.len() as u64, source_app]).map_err(|e| format!("Impossible d’enregistrer l’entrée: {e}"))?;
    prune(
        &db,
        &*inner
            .preferences
            .read()
            .map_err(|_| "Echo preferences lock failed")?,
    )?;
    Ok(())
}

fn newest_text_is(db: &Connection, text: &str) -> Result<bool, String> {
    let previous: Option<Option<String>> = db
        .query_row(
            "SELECT text_content FROM entries ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(previous.flatten().as_deref() == Some(text))
}

fn insert_image(
    inner: &EchoInner,
    width: usize,
    height: usize,
    png: Vec<u8>,
    source_bytes: u64,
    source_app: Option<&str>,
) -> Result<(), String> {
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    let created_at = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let preview = format!("Image de {width} × {height} pixels");
    db.execute("INSERT INTO entries(id, kind, created_at, image_png, preview, byte_length, source_app) VALUES (?1, 'image', ?2, ?3, ?4, ?5, ?6)",
        params![id, created_at, png, preview, source_bytes, source_app]).map_err(|e| format!("Impossible d’enregistrer l’image: {e}"))?;
    prune(
        &db,
        &*inner
            .preferences
            .read()
            .map_err(|_| "Echo preferences lock failed")?,
    )?;
    Ok(())
}

fn prune(db: &Connection, preferences: &EchoPreferences) -> Result<(), String> {
    if preferences.retention_days == 0 {
        return Ok(());
    }
    let cutoff =
        (Utc::now() - chrono::Duration::days(preferences.retention_days as i64)).to_rfc3339();
    let removed = db
        .execute(
            "DELETE FROM entries WHERE pinned = 0 AND created_at < ?1",
            [cutoff],
        )
        .map_err(|e| e.to_string())?;
    if removed > 0 {
        checkpoint_after_erasure(db)?;
    }
    Ok(())
}

/// Best-effort cleanup for deleted clipboard content. secure_delete overwrites SQLite
/// cells and this truncates the WAL after a successful checkpoint. Neither operation
/// guarantees forensic erasure from SSD remapping, filesystem snapshots or backups.
fn checkpoint_after_erasure(db: &Connection) -> Result<(), String> {
    let busy: i64 = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|error| format!("Entrées supprimées, mais le journal temporaire Echo n’a pas pu être nettoyé : {error}"))?;
    if busy != 0 {
        return Err("Entrées supprimées, mais le journal temporaire Echo est encore utilisé; le nettoyage reste incomplet.".into());
    }
    Ok(())
}

fn read_preferences(db: &Connection) -> rusqlite::Result<EchoPreferences> {
    db.query_row("SELECT monitoring_enabled, capture_images, retention_days, max_entry_bytes, excluded_apps FROM preferences WHERE id=1", [], |row| {
        let excluded: String = row.get(4)?;
        Ok(EchoPreferences {
            monitoring_enabled: row.get::<_, i64>(0)? != 0,
            capture_images: row.get::<_, i64>(1)? != 0,
            retention_days: row.get::<_, u32>(2)?,
            max_entry_bytes: row.get::<_, u64>(3)?,
            excluded_apps: serde_json::from_str(&excluded).unwrap_or_default(),
        })
    })
}

fn snapshot(inner: &EchoInner) -> Result<EchoSnapshot, String> {
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    let preferences = inner
        .preferences
        .read()
        .map_err(|_| "Echo preferences lock failed")?
        .clone();
    let mut collections = Vec::new();
    {
        let mut stmt = db.prepare("SELECT id, name, created_at FROM collections ORDER BY name COLLATE NOCASE LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([MAX_COLLECTIONS_IN_SNAPSHOT as i64], |row| {
                Ok(EchoCollection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            collections.push(row.map_err(|e| e.to_string())?);
        }
    }
    let mut entries = Vec::new();
    let mut remaining_image_budget = MAX_SNAPSHOT_IMAGE_DATA_URL_BYTES;
    {
        let mut stmt = db.prepare("SELECT id, kind, created_at, CASE WHEN length(image_png)<=?2 THEN substr(image_png,1,?2) ELSE NULL END, substr(preview,1,?3), byte_length, source_app, favorite, pinned FROM entries ORDER BY pinned DESC, created_at DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![
                    MAX_ENTRIES_IN_SNAPSHOT as i64,
                    MAX_SNAPSHOT_IMAGE_RAW_BYTES as i64,
                    MAX_ENTRY_PREVIEW_CHARS
                ],
                |row| {
                    let id: String = row.get(0)?;
                    let kind: String = row.get(1)?;
                    let image: Option<Vec<u8>> = row.get(3)?;
                    let image_data_url = image.and_then(|bytes| {
                        bounded_image_data_url(bytes, &mut remaining_image_budget)
                    });
                    Ok(EchoEntry {
                        id,
                        kind,
                        created_at: row.get(2)?,
                        text: None,
                        image_data_url,
                        preview: row.get(4)?,
                        byte_length: row.get(5)?,
                        source_app: row.get(6)?,
                        favorite: row.get::<_, i64>(7)? != 0,
                        pinned: row.get::<_, i64>(8)? != 0,
                        collection_ids: Vec::new(),
                    })
                },
            )
            .map_err(|e| e.to_string())?;
        for row in rows {
            entries.push(row.map_err(|e| e.to_string())?);
        }
    }
    for entry in &mut entries {
        let mut stmt = db
            .prepare("SELECT collection_id FROM entry_collections WHERE entry_id=?1 LIMIT ?2")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![&entry.id, MAX_COLLECTIONS_IN_SNAPSHOT as i64],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        for row in rows {
            entry.collection_ids.push(row.map_err(|e| e.to_string())?);
        }
    }
    let available = inner.database_available && cfg!(windows);
    let unavailable_reason = if available {
        None
    } else if !cfg!(windows) {
        Some("La capture du presse-papiers est prise en charge uniquement sous Windows.".into())
    } else {
        inner.unavailable_reason.clone()
    };
    Ok(EchoSnapshot {
        entries,
        collections,
        preferences,
        available,
        unavailable_reason,
    })
}

#[tauri::command]
pub fn echo_search(
    state: State<'_, EchoState>,
    query: String,
    since: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<EchoSearchHit>, String> {
    if query.len() > 512 {
        return Err("La recherche Echo est limitée à 512 octets.".into());
    }
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let limit = limit.unwrap_or(50).clamp(1, 200) as i64;
    let needle = query.trim().to_lowercase();
    let mut stmt = db.prepare(ECHO_SEARCH_SQL).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![needle, since, limit, MAX_ENTRY_PREVIEW_CHARS],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)? != 0,
                    row.get::<_, i64>(5)? != 0,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for row in rows {
        let (id, kind, preview, created_at, favorite, pinned) = row.map_err(|e| e.to_string())?;
        let mut collections = db
            .prepare("SELECT collection_id FROM entry_collections WHERE entry_id=?1 LIMIT ?2")
            .map_err(|e| e.to_string())?;
        let values = collections
            .query_map(params![id, MAX_COLLECTION_LINKS_IN_SEARCH as i64], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?;
        let collection_ids = values
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| e.to_string())?;
        hits.push(EchoSearchHit {
            id,
            kind,
            preview,
            created_at,
            favorite,
            pinned,
            collection_ids,
        });
    }
    Ok(hits)
}

const ECHO_SEARCH_SQL: &str = "SELECT id, kind, substr(preview,1,?4), created_at, favorite, pinned FROM entries WHERE (?1 = '' OR instr(lower(coalesce(text_content, preview)), ?1) > 0) AND (?2 IS NULL OR created_at >= ?2) ORDER BY pinned DESC, created_at DESC LIMIT ?3";

fn validate_entry_id(id: &str) -> Result<(), String> {
    if id.len() != 36 || Uuid::parse_str(id).is_err() {
        return Err("Identifiant d’entrée Echo invalide.".into());
    }
    Ok(())
}

fn read_entry_detail(
    db: &Connection,
    id: &str,
    metadata: &EchoEntryMetadata,
) -> Result<EchoEntryDetail, String> {
    validate_entry_id(id)?;
    let record: Option<(String, Option<Vec<u8>>, Option<Vec<u8>>, u64)> = db
        .query_row(
            "SELECT kind,
                CAST(substr(CAST(text_content AS BLOB),1,?2) AS BLOB),
                CASE WHEN length(image_png)<=?2 THEN substr(image_png,1,?2) ELSE NULL END,
                byte_length
         FROM entries WHERE id=?1",
            params![id, MAX_ENTRY_DETAIL_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (kind, text_bytes, image, byte_length) =
        record.ok_or_else(|| "Cette entrée Echo n’existe plus.".to_owned())?;
    let text = text_bytes.map(|bytes| utf8_prefix(&bytes));
    let image_data_url =
        image.map(|bytes| format!("{IMAGE_DATA_URL_PREFIX}{}", BASE64.encode(bytes)));
    let truncated = metadata.text_bytes > MAX_ENTRY_DETAIL_BYTES
        || metadata.image_bytes > MAX_ENTRY_DETAIL_BYTES
        || (kind == "image" && image_data_url.is_none());
    Ok(EchoEntryDetail {
        id: id.to_owned(),
        kind,
        text,
        image_data_url,
        byte_length,
        truncated,
    })
}

fn utf8_prefix(bytes: &[u8]) -> String {
    let mut end = bytes.len().min(MAX_ENTRY_DETAIL_BYTES);
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    String::from_utf8(bytes[..end].to_vec()).unwrap_or_default()
}

fn read_entry_metadata(db: &Connection, id: &str) -> Result<EchoEntryMetadata, String> {
    validate_entry_id(id)?;
    let record: Option<(String, String, u64, Option<i64>, Option<i64>)> = db.query_row(
        "SELECT kind,substr(preview,1,120),byte_length,length(CAST(text_content AS BLOB)),length(image_png) FROM entries WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).optional().map_err(|e| e.to_string())?;
    let (kind, preview, byte_length, text_bytes, image_bytes) =
        record.ok_or_else(|| "Cette entrée Echo n’existe plus.".to_owned())?;
    Ok(EchoEntryMetadata {
        kind,
        preview,
        byte_length,
        text_bytes: text_bytes.unwrap_or(0).max(0) as usize,
        image_bytes: image_bytes.unwrap_or(0).max(0) as usize,
    })
}

fn detail_consent_message(metadata: &EchoEntryMetadata) -> String {
    let preview = metadata
        .preview
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(120)
        .collect::<String>();
    let truncation = if metadata.text_bytes > MAX_ENTRY_DETAIL_BYTES
        || metadata.image_bytes > MAX_ENTRY_DETAIL_BYTES
    {
        "\nLe détail affiché sera tronqué à 5 Mo."
    } else {
        ""
    };
    format!(
        "Type : {}\nTaille : {} octets\nAperçu : {}{}\n\nAfficher le détail de cette entrée Echo dans ORBIT ? Le contenu restera local.",
        metadata.kind, metadata.byte_length, preview, truncation
    )
}

#[cfg(windows)]
fn native_detail_consent(metadata: &EchoEntryMetadata) -> bool {
    use windows::{
        core::PCWSTR,
        Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_YESNO,
        },
    };
    let title: Vec<u16> = "Confirmer l’affichage du détail Echo"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let message: Vec<u16> = detail_consent_message(metadata)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
        ) == IDYES
    }
}

#[cfg(not(windows))]
fn native_detail_consent(_metadata: &EchoEntryMetadata) -> bool {
    false
}

fn require_detail_consent(approved: bool) -> Result<(), String> {
    if approved {
        Ok(())
    } else {
        Err("Affichage du détail Echo annulé. Le texte ou l’image détaillé(e) n’a pas été lu(e) ni renvoyé(e); seules les métadonnées de confirmation ont été consultées.".into())
    }
}

struct DetailPromptGuard<'a>(&'a AtomicBool);

impl Drop for DetailPromptGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn acquire_detail_prompt(flag: &AtomicBool) -> Result<DetailPromptGuard<'_>, String> {
    flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| DetailPromptGuard(flag))
        .map_err(|_| {
            "Une confirmation Echo est déjà en attente. Réessayez après sa fermeture.".into()
        })
}

/// Reads at most one selected entry. Raw clipboard values are never included in the list snapshot.
#[tauri::command]
pub fn echo_get_entry(state: State<'_, EchoState>, id: String) -> Result<EchoEntryDetail, String> {
    let _prompt_guard = acquire_detail_prompt(&state.0.detail_prompt_pending)?;
    let metadata = {
        let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
        read_entry_metadata(&db, &id)?
    };
    require_detail_consent(native_detail_consent(&metadata))?;
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    read_entry_detail(&db, &id, &metadata)
}

fn duplicate_groups(db: &Connection) -> Result<Vec<EchoDuplicateSet>, String> {
    use std::collections::HashMap;
    let mut stmt = db
        .prepare(
            "SELECT id,kind,text_content,image_png,preview FROM entries ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut groups: HashMap<String, EchoDuplicateSet> = HashMap::new();
    for row in rows {
        let (id, kind, text, image, preview) = row.map_err(|e| e.to_string())?;
        let payload = text.map(String::into_bytes).or(image).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        hasher.update([0]);
        hasher.update(payload);
        let key = format!("{kind}:{:x}", hasher.finalize());
        groups
            .entry(key)
            .and_modify(|group| {
                group.ids.push(id.clone());
                group.count += 1;
            })
            .or_insert_with(|| EchoDuplicateSet {
                ids: vec![id],
                kind,
                preview,
                count: 1,
            });
    }
    let mut values: Vec<_> = groups
        .into_values()
        .filter(|group| group.count > 1)
        .collect();
    values.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.preview.cmp(&b.preview))
    });
    Ok(values)
}

#[tauri::command]
pub fn echo_find_duplicates(state: State<'_, EchoState>) -> Result<Vec<EchoDuplicateSet>, String> {
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    duplicate_groups(&db)
}

fn remove_duplicate_sets(
    db: &mut Connection,
    groups: &[EchoDuplicateSet],
) -> Result<usize, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut removed = 0;
    for group in groups {
        let Some(keeper) = group.ids.first() else {
            continue;
        };
        let mut favorite = false;
        let mut pinned = false;
        for id in &group.ids {
            let flags: Option<(i64, i64)> = tx
                .query_row(
                    "SELECT favorite,pinned FROM entries WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if let Some((f, p)) = flags {
                favorite |= f != 0;
                pinned |= p != 0;
            }
            tx.execute("INSERT OR IGNORE INTO entry_collections(entry_id,collection_id) SELECT ?1,collection_id FROM entry_collections WHERE entry_id=?2", params![keeper, id]).map_err(|e| e.to_string())?;
            if id != keeper {
                removed += tx
                    .execute("DELETE FROM entries WHERE id=?1", [id])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.execute(
            "UPDATE entries SET favorite=?1,pinned=?2 WHERE id=?3",
            params![favorite as i64, pinned as i64, keeper],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(removed)
}

#[tauri::command]
pub fn echo_remove_duplicates(
    app: AppHandle,
    state: State<'_, EchoState>,
) -> Result<usize, String> {
    let mut db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let groups = duplicate_groups(&db)?;
    let removed = remove_duplicate_sets(&mut db, &groups)?;
    if removed > 0 {
        checkpoint_after_erasure(&db)?;
    }
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(removed)
}

#[tauri::command]
pub fn echo_delete_entries(
    app: AppHandle,
    state: State<'_, EchoState>,
    ids: Vec<String>,
) -> Result<(), String> {
    if ids.len() > MAX_ENTRIES_IN_SNAPSHOT {
        return Err("Sélection trop importante. Supprimez au plus 200 entrées à la fois.".into());
    }
    let mut db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut removed = 0;
    for id in ids {
        removed += tx
            .execute("DELETE FROM entries WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if removed > 0 {
        checkpoint_after_erasure(&db)?;
    }
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_snapshot(state: State<'_, EchoState>) -> Result<EchoSnapshot, String> {
    snapshot(&state.0)
}

#[tauri::command]
pub fn echo_set_monitoring(
    app: AppHandle,
    state: State<'_, EchoState>,
    enabled: bool,
) -> Result<(), String> {
    if enabled && !cfg!(windows) {
        return Err("La capture Echo est disponible uniquement sous Windows.".into());
    }
    let inner = &state.0;
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    db.execute(
        "UPDATE preferences SET monitoring_enabled=?1 WHERE id=1",
        [enabled as i64],
    )
    .map_err(|e| e.to_string())?;
    inner
        .preferences
        .write()
        .map_err(|_| "Echo preferences lock failed")?
        .monitoring_enabled = enabled;
    inner.monitoring.store(enabled, Ordering::Relaxed);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoPreferencesPatch {
    monitoring_enabled: Option<bool>,
    capture_images: Option<bool>,
    retention_days: Option<u32>,
    max_entry_bytes: Option<u64>,
    excluded_apps: Option<Vec<String>>,
}

#[tauri::command]
pub fn echo_set_preferences(
    app: AppHandle,
    state: State<'_, EchoState>,
    patch: EchoPreferencesPatch,
) -> Result<(), String> {
    let inner = &state.0;
    let mut preferences = inner
        .preferences
        .write()
        .map_err(|_| "Echo preferences lock failed")?;
    if let Some(v) = patch.capture_images {
        preferences.capture_images = v;
    }
    if let Some(v) = patch.retention_days {
        if ![0, 1, 7, 30, 90].contains(&v) {
            return Err("Durée de conservation invalide.".into());
        }
        preferences.retention_days = v;
    }
    if let Some(v) = patch.max_entry_bytes {
        if ![65_536, 262_144, 1_000_000, 5_000_000].contains(&v) {
            return Err("Limite de taille invalide.".into());
        }
        preferences.max_entry_bytes = v;
    }
    if let Some(v) = patch.excluded_apps {
        preferences.excluded_apps = normalize_exclusions(v);
    }
    if let Some(v) = patch.monitoring_enabled {
        preferences.monitoring_enabled = v;
        inner.monitoring.store(v, Ordering::Relaxed);
    }
    let serialized =
        serde_json::to_string(&preferences.excluded_apps).map_err(|e| e.to_string())?;
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    db.execute("UPDATE preferences SET monitoring_enabled=?1,capture_images=?2,retention_days=?3,max_entry_bytes=?4,excluded_apps=?5 WHERE id=1",
        params![preferences.monitoring_enabled as i64, preferences.capture_images as i64, preferences.retention_days, preferences.max_entry_bytes, serialized]).map_err(|e| e.to_string())?;
    prune(&db, &preferences)?;
    drop(db);
    drop(preferences);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_copy_entry(state: State<'_, EchoState>, id: String) -> Result<(), String> {
    echo_copy_entry_inner(&state.0, &id, false)
}

pub fn echo_copy_entry_from_streamdeck(
    state: State<'_, EchoState>,
    id: String,
) -> Result<(), String> {
    echo_copy_entry_inner(&state.0, &id, true)
}

fn echo_copy_entry_inner(
    inner: &EchoInner,
    id: &str,
    suppress_capture: bool,
) -> Result<(), String> {
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    let row: Option<(Option<String>, Option<Vec<u8>>)> = db
        .query_row(
            "SELECT text_content,image_png FROM entries WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (text, image) = row.ok_or_else(|| "Cette entrée Echo n’existe plus.".to_string())?;
    drop(db);
    let clipboard_guard = inner
        .clipboard_sequence_lock
        .lock()
        .map_err(|_| "Echo clipboard lock failed")?;
    let result = (|| {
        let mut clipboard = Clipboard::new()
            .map_err(|_| "Le presse-papiers Windows est inaccessible.".to_string())?;
        if let Some(text) = text {
            clipboard
                .set_text(text)
                .map_err(|_| "Impossible de copier cette entrée.".to_string())?;
        } else if let Some(bytes) = image {
            let decoder = png::Decoder::new(bytes.as_slice());
            let mut reader = decoder
                .read_info()
                .map_err(|_| "Image Echo illisible.".to_string())?;
            let mut buffer = vec![0; reader.output_buffer_size()];
            let info = reader
                .next_frame(&mut buffer)
                .map_err(|_| "Image Echo illisible.".to_string())?;
            let mut clipboard_image = Clipboard::new()
                .map_err(|_| "Le presse-papiers Windows est inaccessible.".to_string())?;
            clipboard_image
                .set_image(arboard::ImageData {
                    width: info.width as usize,
                    height: info.height as usize,
                    bytes: std::borrow::Cow::Owned(buffer[..info.buffer_size()].to_vec()),
                })
                .map_err(|_| "Impossible de recopier cette image.".to_string())?;
        } else {
            return Err("Cette entrée Echo ne contient plus de données copiables.".into());
        }
        Ok(())
    })();
    if result.is_ok() && suppress_capture && inner.monitoring.load(Ordering::Relaxed) {
        inner
            .suppressed_clipboard_sequence
            .store(clipboard_sequence(), Ordering::Release);
    }
    drop(clipboard_guard);
    result
}

#[tauri::command]
pub fn echo_set_favorite(
    app: AppHandle,
    state: State<'_, EchoState>,
    id: String,
    favorite: bool,
) -> Result<(), String> {
    update_entry_flag(&state.0, &id, "favorite", favorite)?;
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_set_pinned(
    app: AppHandle,
    state: State<'_, EchoState>,
    id: String,
    pinned: bool,
) -> Result<(), String> {
    update_entry_flag(&state.0, &id, "pinned", pinned)?;
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

fn update_entry_flag(inner: &EchoInner, id: &str, column: &str, value: bool) -> Result<(), String> {
    let sql = match column {
        "favorite" => "UPDATE entries SET favorite=?1 WHERE id=?2",
        "pinned" => "UPDATE entries SET pinned=?1 WHERE id=?2",
        _ => return Err("Champ Echo invalide.".into()),
    };
    let db = inner.db.lock().map_err(|_| "Echo database lock failed")?;
    if db
        .execute(sql, params![value as i64, id])
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("Cette entrée Echo n’existe plus.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn echo_assign_collection(
    app: AppHandle,
    state: State<'_, EchoState>,
    id: String,
    collection_id: Option<String>,
) -> Result<(), String> {
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    if let Some(collection_id) = collection_id {
        let current: i64 = db
            .query_row(
                "SELECT count(*) FROM entry_collections WHERE entry_id=?1",
                [&id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current >= MAX_COLLECTIONS_IN_SNAPSHOT as i64 {
            return Err("Une entrée Echo ne peut pas être liée à plus de 100 collections.".into());
        }
        db.execute(
            "INSERT OR IGNORE INTO entry_collections(entry_id, collection_id) VALUES (?1,?2)",
            params![id, collection_id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        db.execute("DELETE FROM entry_collections WHERE entry_id=?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_create_collection(
    app: AppHandle,
    state: State<'_, EchoState>,
    name: String,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 {
        return Err("Le nom doit contenir entre 1 et 48 caractères.".into());
    }
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let existing: i64 = db
        .query_row("SELECT count(*) FROM collections", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if existing >= MAX_COLLECTIONS_IN_SNAPSHOT as i64 {
        return Err("Echo accepte au maximum 100 collections.".into());
    }
    db.execute(
        "INSERT INTO collections(id,name,created_at) VALUES (?1,?2,?3)",
        params![Uuid::new_v4().to_string(), name, Utc::now().to_rfc3339()],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            "Une collection porte déjà ce nom.".to_string()
        } else {
            e.to_string()
        }
    })?;
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_delete_entry(
    app: AppHandle,
    state: State<'_, EchoState>,
    id: String,
) -> Result<(), String> {
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let removed = db
        .execute("DELETE FROM entries WHERE id=?1", [&id])
        .map_err(|e| e.to_string())?;
    if removed > 0 {
        checkpoint_after_erasure(&db)?;
    }
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn echo_clear_history(app: AppHandle, state: State<'_, EchoState>) -> Result<(), String> {
    let db = state.0.db.lock().map_err(|_| "Echo database lock failed")?;
    let removed = db
        .execute("DELETE FROM entries", [])
        .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM entry_collections", [])
        .map_err(|e| e.to_string())?;
    if removed > 0 {
        checkpoint_after_erasure(&db)?;
    }
    drop(db);
    let _ = app.emit(CHANGE_EVENT, ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let db = Connection::open_in_memory().expect("in-memory Echo database");
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA secure_delete=ON;
             CREATE TABLE entries (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, created_at TEXT NOT NULL,
               text_content TEXT, image_png BLOB, preview TEXT NOT NULL,
               byte_length INTEGER NOT NULL, source_app TEXT,
               favorite INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE collections (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL);
             CREATE TABLE entry_collections (
               entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
               collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
               PRIMARY KEY(entry_id, collection_id)
             );
             CREATE TABLE preferences (
               id INTEGER PRIMARY KEY, monitoring_enabled INTEGER NOT NULL, capture_images INTEGER NOT NULL,
               retention_days INTEGER NOT NULL, max_entry_bytes INTEGER NOT NULL, excluded_apps TEXT NOT NULL
             );
             INSERT INTO preferences VALUES (1,0,0,30,1000000,'[]');",
        ).expect("Echo schema");
        db
    }

    fn insert_entry(
        db: &Connection,
        id: &str,
        kind: &str,
        at: &str,
        text: Option<&str>,
        image: Option<&[u8]>,
        preview: &str,
        favorite: bool,
        pinned: bool,
    ) {
        db.execute(
            "INSERT INTO entries(id,kind,created_at,text_content,image_png,preview,byte_length,favorite,pinned) VALUES (?1,?2,?3,?4,?5,?6,0,?7,?8)",
            params![id, kind, at, text, image, preview, favorite as i64, pinned as i64],
        ).expect("insert test clipboard item");
    }

    #[test]
    fn duplicate_detection_matches_exact_content_and_kind() {
        let db = test_db();
        insert_entry(
            &db,
            "text-old",
            "text",
            "2026-01-01T10:00:00Z",
            Some("same"),
            None,
            "same",
            false,
            false,
        );
        insert_entry(
            &db,
            "text-new",
            "text",
            "2026-01-01T10:01:00Z",
            Some("same"),
            None,
            "same",
            false,
            false,
        );
        insert_entry(
            &db,
            "text-different",
            "text",
            "2026-01-01T10:02:00Z",
            Some("same "),
            None,
            "same",
            false,
            false,
        );
        insert_entry(
            &db,
            "url-same",
            "url",
            "2026-01-01T10:03:00Z",
            Some("same"),
            None,
            "same",
            false,
            false,
        );
        insert_entry(
            &db,
            "image-old",
            "image",
            "2026-01-01T10:04:00Z",
            None,
            Some(&[1, 2, 3]),
            "Image",
            false,
            false,
        );
        insert_entry(
            &db,
            "image-new",
            "image",
            "2026-01-01T10:05:00Z",
            None,
            Some(&[1, 2, 3]),
            "Image",
            false,
            false,
        );

        let groups = duplicate_groups(&db).expect("duplicate scan");
        assert_eq!(groups.len(), 2);
        assert!(groups
            .iter()
            .any(|g| g.kind == "text" && g.count == 2 && g.ids.contains(&"text-old".to_string())));
        assert!(groups.iter().any(|g| g.kind == "image"
            && g.count == 2
            && g.ids.contains(&"image-old".to_string())));
    }

    #[test]
    fn duplicate_removal_keeps_newest_and_merges_user_metadata() {
        let mut db = test_db();
        db.execute("INSERT INTO collections(id,name,created_at) VALUES ('c1','Older collection','t'),('c2','Newer collection','t')", []).expect("collections");
        insert_entry(
            &db,
            "old",
            "text",
            "2026-01-01T10:00:00Z",
            Some("repeat"),
            None,
            "repeat",
            true,
            true,
        );
        insert_entry(
            &db,
            "new",
            "text",
            "2026-01-01T10:01:00Z",
            Some("repeat"),
            None,
            "repeat",
            false,
            false,
        );
        db.execute(
            "INSERT INTO entry_collections VALUES ('old','c1'),('new','c2')",
            [],
        )
        .expect("collection links");

        let groups = duplicate_groups(&db).expect("duplicate scan");
        assert_eq!(groups[0].ids, vec!["new", "old"]);
        assert_eq!(
            remove_duplicate_sets(&mut db, &groups).expect("remove duplicate"),
            1
        );

        let row: (i64, i64) = db
            .query_row(
                "SELECT favorite,pinned FROM entries WHERE id='new'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("newest retained");
        assert_eq!(row, (1, 1));
        let links: i64 = db
            .query_row(
                "SELECT count(*) FROM entry_collections WHERE entry_id='new'",
                [],
                |r| r.get(0),
            )
            .expect("merged collection links");
        assert_eq!(links, 2);
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .expect("remaining rows");
        assert_eq!(remaining, 1);
    }

    #[test]
    fn clipboard_capture_defaults_to_opt_in() {
        let preferences = EchoPreferences::default();
        assert!(!preferences.monitoring_enabled);
        assert!(!preferences.capture_images);
        assert_eq!(preferences.retention_days, 30);

        let stored = read_preferences(&test_db()).expect("stored defaults");
        assert!(!stored.monitoring_enabled);
        assert!(!stored.capture_images);
    }

    #[test]
    fn clipboard_restored_from_streamdeck_is_suppressed_once_without_content_tracking() {
        let suppressed_sequence = AtomicU32::new(41);
        let mut last = 40;
        assert!(!should_capture_clipboard(
            41,
            &mut last,
            &suppressed_sequence
        ));
        assert_eq!(suppressed_sequence.load(Ordering::Relaxed), 0);
        suppressed_sequence.store(42, Ordering::Release);
        assert!(!should_capture_clipboard(
            42,
            &mut last,
            &suppressed_sequence
        ));
        assert!(!should_capture_clipboard(
            42,
            &mut last,
            &suppressed_sequence
        ));
        suppressed_sequence.store(43, Ordering::Release);
        // A user's new clipboard write wins over the older ORBIT suppression marker.
        assert!(should_capture_clipboard(
            44,
            &mut last,
            &suppressed_sequence
        ));
        assert_eq!(suppressed_sequence.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn echo_database_uses_sqlite_secure_delete_mode() {
        let db = test_db();
        let enabled: i64 = db
            .query_row("PRAGMA secure_delete", [], |row| row.get(0))
            .unwrap();
        assert_eq!(enabled, 1);
    }

    #[test]
    fn excluded_clipboard_owner_is_matched_by_executable_basename() {
        let excluded = vec!["chrome".to_string(), "keepass.exe".to_string()];
        assert!(is_excluded_app(
            Some("C:\\Program Files\\Browser\\Chrome.exe"),
            &excluded
        ));
        assert!(is_excluded_app(Some("KeePass.exe"), &excluded));
        assert!(!is_excluded_app(Some("notepad.exe"), &excluded));
    }

    #[test]
    fn excluded_apps_fail_closed_if_windows_cannot_identify_clipboard_owner() {
        assert!(is_excluded_app(None, &["password-manager".into()]));
        assert!(!is_excluded_app(None, &[]));
    }

    #[test]
    fn exclusions_are_normalized_bounded_and_deduplicated() {
        assert_eq!(
            normalize_exclusions(vec![
                " C:\\Apps\\Chrome.exe ".into(),
                "chrome".into(),
                "".into(),
                "x".repeat(129),
            ]),
            vec!["chrome".to_string()]
        );
    }

    #[test]
    fn image_snapshot_payload_stays_within_a_single_response_budget() {
        let mut budget = 128;
        let data_url = bounded_image_data_url(vec![7; 64], &mut budget).expect("small image fits");
        assert!(data_url.len() <= 128);
        assert!(bounded_image_data_url(vec![7; 100], &mut budget).is_none());
        assert_eq!(budget, 128 - data_url.len());
    }

    #[test]
    fn list_snapshot_omits_raw_clipboard_text() {
        let db = test_db();
        let secret_text = format!("private-body-{}", "x".repeat(MAX_ENTRY_DETAIL_BYTES));
        insert_entry(
            &db,
            "snapshot-entry",
            "text",
            "2026-01-01T10:00:00Z",
            Some(&secret_text),
            None,
            "private preview",
            false,
            false,
        );
        let inner = EchoInner {
            db: Mutex::new(db),
            preferences: RwLock::new(EchoPreferences::default()),
            monitoring: AtomicBool::new(false),
            suppressed_clipboard_sequence: AtomicU32::new(0),
            clipboard_sequence_lock: Mutex::new(()),
            detail_prompt_pending: AtomicBool::new(false),
            database_available: true,
            unavailable_reason: None,
        };
        let value = snapshot(&inner).unwrap();
        assert_eq!(value.entries.len(), 1);
        assert!(value.entries[0].text.is_none());
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(!serialized.contains("private-body"));
        assert!(serialized.len() < MAX_SNAPSHOT_IMAGE_DATA_URL_BYTES + 1_000_000);
    }

    #[test]
    fn on_demand_detail_validates_id_and_truncates_large_content() {
        let db = test_db();
        let id = Uuid::new_v4().to_string();
        let text = "é".repeat(MAX_ENTRY_DETAIL_BYTES / 2 + 10);
        insert_entry(
            &db,
            &id,
            "text",
            "2026-01-01T10:00:00Z",
            Some(&text),
            None,
            "preview",
            false,
            false,
        );
        assert!(read_entry_metadata(&db, "not-an-id").is_err());
        let metadata = read_entry_metadata(&db, &id).unwrap();
        let detail = read_entry_detail(&db, &id, &metadata).unwrap();
        assert!(detail.truncated);
        let returned = detail.text.unwrap();
        assert!(returned.len() <= MAX_ENTRY_DETAIL_BYTES);
        assert!(returned.is_char_boundary(returned.len()));
    }

    #[test]
    fn on_demand_detail_fails_when_the_entry_was_deleted() {
        let db = test_db();
        assert!(read_entry_metadata(&db, &Uuid::new_v4().to_string()).is_err());
    }

    #[test]
    fn declining_detail_consent_is_reported_as_cancellation() {
        let error = require_detail_consent(false).unwrap_err();
        assert!(error.contains("annulé"));
        assert!(error.contains("métadonnées de confirmation"));
    }

    #[test]
    fn only_one_detail_confirmation_can_be_pending() {
        let pending = AtomicBool::new(false);
        let first = acquire_detail_prompt(&pending).unwrap();
        assert!(acquire_detail_prompt(&pending).is_err());
        drop(first);
        assert!(acquire_detail_prompt(&pending).is_ok());
    }

    #[test]
    fn search_sql_bounds_historical_preview_length() {
        let db = test_db();
        let id = Uuid::new_v4().to_string();
        let preview = "p".repeat(5_000);
        insert_entry(
            &db,
            &id,
            "text",
            "2026-01-01T10:00:00Z",
            None,
            None,
            &preview,
            false,
            false,
        );
        let returned: String = db
            .query_row(
                ECHO_SEARCH_SQL,
                params!["", Option::<String>::None, 50_i64, MAX_ENTRY_PREVIEW_CHARS],
                |row| row.get(2),
            )
            .unwrap();
        assert_eq!(returned.chars().count(), MAX_ENTRY_PREVIEW_CHARS as usize);
    }

    #[test]
    fn detail_consent_shows_kind_size_and_sanitized_short_preview() {
        let metadata = EchoEntryMetadata {
            kind: "text".into(),
            preview: "hello\nworld".into(),
            byte_length: 42,
            text_bytes: 42,
            image_bytes: 0,
        };
        let message = detail_consent_message(&metadata);
        assert!(message.contains("Type : text"));
        assert!(message.contains("Taille : 42 octets"));
        assert!(message.contains("hello world"));
    }

    #[test]
    fn oversized_image_is_not_returned_by_bounded_detail_query() {
        let db = test_db();
        let id = Uuid::new_v4().to_string();
        let image = vec![9u8; MAX_ENTRY_DETAIL_BYTES + 1];
        db.execute(
            "INSERT INTO entries(id,kind,created_at,image_png,preview,byte_length) VALUES(?1,'image','2026-01-01T10:00:00Z',?2,'large image',?3)",
            params![id, image, (MAX_ENTRY_DETAIL_BYTES + 1) as u64],
        ).unwrap();
        let metadata = read_entry_metadata(&db, &id).unwrap();
        let detail = read_entry_detail(&db, &id, &metadata).unwrap();
        assert!(detail.truncated);
        assert!(detail.image_data_url.is_none());
    }

    #[test]
    fn oversized_image_is_not_returned_by_bounded_snapshot_query() {
        let db = test_db();
        let id = Uuid::new_v4().to_string();
        let image = vec![9u8; MAX_SNAPSHOT_IMAGE_RAW_BYTES + 1];
        db.execute(
            "INSERT INTO entries(id,kind,created_at,image_png,preview,byte_length) VALUES(?1,'image','2026-01-01T10:00:00Z',?2,'large image',?3)",
            params![id, image, (MAX_SNAPSHOT_IMAGE_RAW_BYTES + 1) as u64],
        ).unwrap();
        let inner = EchoInner {
            db: Mutex::new(db),
            preferences: RwLock::new(EchoPreferences::default()),
            monitoring: AtomicBool::new(false),
            suppressed_clipboard_sequence: AtomicU32::new(0),
            clipboard_sequence_lock: Mutex::new(()),
            detail_prompt_pending: AtomicBool::new(false),
            database_available: true,
            unavailable_reason: None,
        };
        let value = snapshot(&inner).unwrap();
        assert_eq!(value.entries.len(), 1);
        assert!(value.entries[0].image_data_url.is_none());
    }
}
