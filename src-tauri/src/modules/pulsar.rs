use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use chrono::{DateTime, Local, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_ACTIONS: usize = 12;
const MAX_CONDITIONS: usize = 12;
const MAX_TRIGGERS: usize = 12;
const MAX_LOGS: i64 = 500;
const MAX_COPY_BYTES: u64 = 2_147_483_648;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Trigger {
    Manual,
    Startup,
    Interval { every_seconds: u64 },
    AtTime { at: String },
    FileCreated { directory: String },
    FileModified { path: String },
    Event { name: String },
    ProcessStarted { executable: String },
}

#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DraftTrigger {
    Manual,
    Startup,
    Interval { every_seconds: u64 },
    AtTime { at: String },
    AfterDelay { seconds: u64 },
    FileCreated { directory: String },
    FileModified { path: String },
    Event { name: String },
    ProcessStarted { executable: String },
}

impl DraftTrigger {
    fn into_trigger(self, now: DateTime<Utc>) -> Result<(Trigger, Option<u64>), String> {
        match self {
            Self::AfterDelay { seconds } => {
                if !(1..=86_400).contains(&seconds) {
                    return Err("Le délai doit être compris entre 1 seconde et 24 heures.".into());
                }
                Ok((
                    Trigger::AtTime {
                        at: (now + chrono::Duration::seconds(seconds as i64)).to_rfc3339(),
                    },
                    Some(seconds),
                ))
            }
            Self::Manual => Ok((Trigger::Manual, None)),
            Self::Startup => Ok((Trigger::Startup, None)),
            Self::Interval { every_seconds } => Ok((Trigger::Interval { every_seconds }, None)),
            Self::AtTime { at } => Ok((Trigger::AtTime { at }, None)),
            Self::FileCreated { directory } => Ok((Trigger::FileCreated { directory }, None)),
            Self::FileModified { path } => Ok((Trigger::FileModified { path }, None)),
            Self::Event { name } => Ok((Trigger::Event { name }, None)),
            Self::ProcessStarted { executable } => {
                Ok((Trigger::ProcessStarted { executable }, None))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSpec {
    pub name: String,
    pub description: String,
    pub triggers: Vec<DraftTrigger>,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub allow_scheduled_file_changes: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePreview {
    pub delay_seconds: u64,
    pub fires_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDraftResult {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_preview: Option<SchedulePreview>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Condition {
    Extension { value: String },
    NameContains { value: String },
    SizeAtLeast { bytes: u64 },
    PathExists { path: String },
    TimeWindow { from: String, to: String },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConditionMode {
    #[default]
    All,
    Any,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    Notify {
        title: String,
        body: String,
    },
    OpenFolder {
        path: String,
    },
    OpenApp {
        app: String,
    },
    CopyFile {
        source: String,
        destination: String,
    },
    MoveFile {
        source: String,
        destination: String,
    },
    RenameFile {
        path: String,
        #[serde(rename = "newName")]
        new_name: String,
    },
    Log {
        message: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub condition_mode: ConditionMode,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub allow_scheduled_file_changes: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_run_at: Option<String>,
    pub last_run_status: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLog {
    pub id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub status: String,
    pub summary: String,
    pub at: String,
    pub simulation: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionResult {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResult {
    pub eligible: bool,
    pub trigger: String,
    pub conditions: Vec<ConditionResult>,
    pub condition_mode: ConditionMode,
    pub actions: Vec<String>,
    pub message: String,
}

struct PulsarInner {
    db: Mutex<Connection>,
    app: AppHandle,
}

pub struct PulsarState(Arc<PulsarInner>);

impl PulsarState {
    pub fn global_pause_status(&self) -> Result<bool, String> {
        let db = self
            .0
            .db
            .lock()
            .map_err(|_| "La base Pulsar est indisponible.")?;
        read_global_pause(&db)
    }

    pub fn set_global_pause(&self, paused: bool) -> Result<bool, String> {
        let db = self
            .0
            .db
            .lock()
            .map_err(|_| "La base Pulsar est indisponible.")?;
        write_global_pause(&db, paused)?;
        drop(db);
        let _ = self.0.app.emit("pulsar.global-pause", paused);
        Ok(paused)
    }
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier ORBIT indisponible : {e}"))?;
    fs::create_dir_all(&path).map_err(|e| format!("Impossible de créer le dossier local : {e}"))?;
    path.push("pulsar.sqlite3");
    let db = Connection::open(path).map_err(|e| format!("Base Pulsar inaccessible : {e}"))?;
    db.busy_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS scenarios(id TEXT PRIMARY KEY, name TEXT NOT NULL, enabled INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS run_logs(id TEXT PRIMARY KEY, scenario_id TEXT NOT NULL, scenario_name TEXT NOT NULL, status TEXT NOT NULL, summary TEXT NOT NULL, at TEXT NOT NULL, simulation INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX IF NOT EXISTS run_logs_at_idx ON run_logs(at DESC);
         CREATE TABLE IF NOT EXISTS trigger_state(scenario_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, updated_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS pulsar_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    ).map_err(|e| format!("Impossible d’initialiser Pulsar : {e}"))?;
    db.execute(
        "INSERT OR IGNORE INTO pulsar_meta(key,value) VALUES('global_pause_v1','false')",
        [],
    )
    .map_err(|e| format!("Impossible d’initialiser l’état de suspension Pulsar : {e}"))?;
    revoke_untrusted_saved_file_grants(&db)?;
    let inner = Arc::new(PulsarInner {
        db: Mutex::new(db),
        app: app.clone(),
    });
    let worker = inner.clone();
    app.manage(PulsarState(inner));
    thread::Builder::new()
        .name("orbit-pulsar-scheduler".into())
        .spawn(move || scheduler(worker))
        .map_err(|e| format!("Impossible de démarrer le planificateur Pulsar : {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn pulsar_list(state: State<'_, PulsarState>) -> Result<Vec<Scenario>, String> {
    read_scenarios(&state.0)
}

#[tauri::command]
pub fn pulsar_global_pause_status(state: State<'_, PulsarState>) -> Result<bool, String> {
    state.global_pause_status()
}

#[tauri::command]
pub fn pulsar_global_pause_set(
    state: State<'_, PulsarState>,
    paused: bool,
) -> Result<bool, String> {
    state.set_global_pause(paused)
}

#[tauri::command]
pub fn pulsar_create_draft(
    state: State<'_, PulsarState>,
    draft: DraftSpec,
) -> Result<CreateDraftResult, String> {
    let now = Utc::now();
    let (mut scenario, schedule_preview) = scenario_from_draft(draft, now)?;
    normalize_scenario(&mut scenario)?;
    canonicalize_executable_triggers(&mut scenario)?;
    validate_scenario(&scenario)?;
    persist_scenario(&state.0, &scenario)
        .map_err(|error| format!("Impossible d’enregistrer le brouillon : {error}"))?;
    Ok(CreateDraftResult {
        id: scenario.id,
        schedule_preview,
    })
}

fn scenario_from_draft(
    draft: DraftSpec,
    now: DateTime<Utc>,
) -> Result<(Scenario, Option<SchedulePreview>), String> {
    let _requested_file_permission = draft.allow_scheduled_file_changes;
    let mut schedule_preview = None;
    let mut triggers = Vec::with_capacity(draft.triggers.len());
    for source in draft.triggers {
        let (trigger, delay) = source.into_trigger(now)?;
        if let Some(delay_seconds) = delay {
            if schedule_preview.is_some() {
                return Err("Un seul délai ponctuel est autorisé par brouillon.".into());
            }
            let fires_at = match &trigger {
                Trigger::AtTime { at } => at.clone(),
                _ => unreachable!(),
            };
            schedule_preview = Some(SchedulePreview {
                delay_seconds,
                fires_at,
            });
        }
        triggers.push(trigger);
    }
    let timestamp = now.to_rfc3339();
    let scenario = Scenario {
        id: Uuid::new_v4().to_string(),
        name: draft.name,
        description: draft.description,
        enabled: false,
        trigger: triggers.first().cloned(),
        triggers,
        conditions: draft.conditions,
        condition_mode: ConditionMode::All,
        actions: draft.actions,
        allow_scheduled_file_changes: false,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        last_run_at: None,
        last_run_status: None,
    };
    Ok((scenario, schedule_preview))
}

#[tauri::command]
pub fn pulsar_save_scenario(
    state: State<'_, PulsarState>,
    mut scenario: Scenario,
) -> Result<(), String> {
    // Renderer-provided state is never authority to activate an automation or grant
    // its persistent file-write capability. Both are issued only by pulsar_set_enabled.
    apply_untrusted_save_policy(&mut scenario);
    normalize_scenario(&mut scenario)?;
    canonicalize_executable_triggers(&mut scenario)?;
    validate_scenario(&scenario)?;
    let json = serde_json::to_string(&scenario).map_err(|e| e.to_string())?;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    db.execute(
        "INSERT INTO scenarios(id,name,enabled,json,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name, enabled=excluded.enabled, json=excluded.json, updated_at=excluded.updated_at",
        params![scenario.id, scenario.name, scenario.enabled as i64, json, scenario.updated_at],
    ).map_err(|e| format!("Impossible d’enregistrer le scénario : {e}"))?;
    drop(db);
    if scenario.enabled {
        set_initial_file_fingerprint(&state.0, &scenario)?;
    }
    Ok(())
}

#[tauri::command]
pub fn pulsar_delete_scenario(state: State<'_, PulsarState>, id: String) -> Result<(), String> {
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    db.execute("DELETE FROM scenarios WHERE id=?1", [id.as_str()])
        .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM run_logs WHERE scenario_id=?1", [id.as_str()])
        .map_err(|e| e.to_string())?;
    db.execute(
        "DELETE FROM trigger_state WHERE scenario_id=?1 OR scenario_id LIKE ?2",
        params![id.as_str(), format!("{id}::%")],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn pulsar_set_enabled(
    state: State<'_, PulsarState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut scenario =
        find_scenario(&state.0, &id)?.ok_or_else(|| "Ce scénario n’existe plus.".to_string())?;
    if enabled {
        normalize_scenario(&mut scenario)?;
        validate_scenario(&scenario)?;
        if has_file_write_action(&scenario) {
            let name = scenario
                .name
                .chars()
                .filter(|character| !character.is_control())
                .take(80)
                .collect::<String>();
            let message = format!("Autoriser durablement le scénario « {name} » à copier, déplacer ou renommer des fichiers lorsqu’il se déclenche ? ORBIT vérifiera les chemins et ne remplacera pas les destinations existantes.");
            require_native_approval(
                native_confirmation("Autorisation Pulsar", &message),
                "Activation annulée : l’autorisation native des actions de fichiers a été refusée.",
            )?;
            scenario.allow_scheduled_file_changes = true;
        } else {
            scenario.allow_scheduled_file_changes = false;
        }
        set_initial_file_fingerprint(&state.0, &scenario)?;
    } else {
        scenario.allow_scheduled_file_changes = false;
        let db = state
            .0
            .db
            .lock()
            .map_err(|_| "La base Pulsar est indisponible.")?;
        db.execute(
            "DELETE FROM trigger_state WHERE scenario_id=?1",
            [&scenario.id],
        )
        .map_err(|e| e.to_string())?;
    }
    scenario.enabled = enabled;
    scenario.updated_at = Utc::now().to_rfc3339();
    persist_scenario(&state.0, &scenario)
}

#[tauri::command]
pub fn pulsar_simulate(
    state: State<'_, PulsarState>,
    id: String,
) -> Result<SimulationResult, String> {
    let scenario =
        find_scenario(&state.0, &id)?.ok_or_else(|| "Ce scénario n’existe plus.".to_string())?;
    validate_scenario(&scenario)?;
    let conditions = evaluate_conditions(&scenario, None);
    let eligible = conditions_match(&conditions, scenario.condition_mode);
    let result = SimulationResult {
        eligible,
        trigger: scenario
            .triggers
            .iter()
            .map(trigger_label)
            .collect::<Vec<_>>()
            .join(" OU "),
        conditions,
        condition_mode: scenario.condition_mode,
        actions: scenario.actions.iter().map(action_label).collect(),
        message: if eligible {
            "Les conditions sont remplies. Les actions affichées ne seront pas exécutées pendant la simulation.".into()
        } else {
            "Au moins une condition n’est pas satisfaite dans le contexte actuel.".into()
        },
    };
    append_log(
        &state.0,
        &scenario,
        "simulated",
        "Simulation sans effet de bord.",
        true,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn pulsar_run(
    state: State<'_, PulsarState>,
    id: String,
    _confirmed: Option<bool>,
) -> Result<(), String> {
    // `_confirmed` is a legacy renderer argument retained for compatibility only;
    // it is deliberately ignored. The OS dialog below is the authority.
    if state.global_pause_status()? {
        return Err(
            "Pulsar est suspendu. Reprenez les automatisations avant de lancer un scénario.".into(),
        );
    }
    let scenario =
        find_scenario(&state.0, &id)?.ok_or_else(|| "Ce scénario n’existe plus.".to_string())?;
    let name = scenario
        .name
        .chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect::<String>();
    let message = format!("Exécuter maintenant le scénario « {name} » ? Les actions configurées seront lancées sur cet ordinateur.");
    require_native_approval(
        native_confirmation("Confirmation d’exécution Pulsar", &message),
        "Exécution annulée par l’utilisateur.",
    )?;
    execute_scenario(&state.0, &scenario, true, None)
}

fn apply_untrusted_save_policy(scenario: &mut Scenario) {
    scenario.enabled = false;
    scenario.allow_scheduled_file_changes = false;
}

fn require_native_approval(approved: bool, error: &str) -> Result<(), String> {
    if approved {
        Ok(())
    } else {
        Err(error.into())
    }
}

#[tauri::command]
pub fn pulsar_logs(
    state: State<'_, PulsarState>,
    limit: Option<u32>,
) -> Result<Vec<RunLog>, String> {
    let limit = limit.unwrap_or(40).clamp(1, 200) as i64;
    let db = state
        .0
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    let mut stmt = db.prepare("SELECT id,scenario_id,scenario_name,status,summary,at,simulation FROM run_logs ORDER BY at DESC LIMIT ?1").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(RunLog {
                id: row.get(0)?,
                scenario_id: row.get(1)?,
                scenario_name: row.get(2)?,
                status: row.get(3)?,
                summary: row.get(4)?,
                at: row.get(5)?,
                simulation: row.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pulsar_fire_event(state: State<'_, PulsarState>, name: String) -> Result<(), String> {
    if !matches!(
        name.as_str(),
        "core.alert" | "echo.entry-added"
    ) {
        return Err("Cet événement ne peut pas déclencher Pulsar.".into());
    }
    if state.global_pause_status()? {
        return Ok(());
    }
    let scenarios = read_scenarios(&state.0)?;
    for scenario in scenarios {
        if scenario.enabled && has_event_trigger(&scenario.triggers, &name) {
            let _ = execute_scenario(&state.0, &scenario, false, None);
        }
    }
    Ok(())
}

fn revoke_untrusted_saved_file_grants(db: &Connection) -> Result<(), String> {
    let migrated: Option<String> = db
        .query_row(
            "SELECT value FROM pulsar_meta WHERE key='native_file_grant_v1'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if migrated.as_deref() == Some("done") {
        return Ok(());
    }

    // Older builds trusted `enabled` and `allowScheduledFileChanges` supplied by
    // the webview. Revoke those persisted grants once, before the scheduler starts.
    let records = {
        let mut statement = db
            .prepare("SELECT id,json FROM scenarios")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for (id, json) in records {
        let Ok(mut scenario) = serde_json::from_str::<Scenario>(&json) else {
            continue;
        };
        if has_file_write_action(&scenario)
            && (scenario.enabled || scenario.allow_scheduled_file_changes)
        {
            scenario.enabled = false;
            scenario.allow_scheduled_file_changes = false;
            scenario.updated_at = Utc::now().to_rfc3339();
            let normalized = serde_json::to_string(&scenario).map_err(|e| e.to_string())?;
            db.execute(
                "UPDATE scenarios SET enabled=0,json=?1,updated_at=?2 WHERE id=?3",
                params![normalized, scenario.updated_at, id],
            )
            .map_err(|e| e.to_string())?;
            db.execute(
                "DELETE FROM trigger_state WHERE scenario_id=?1 OR scenario_id LIKE ?2",
                params![id, format!("{id}::%")],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    db.execute(
        "INSERT OR REPLACE INTO pulsar_meta(key,value) VALUES('native_file_grant_v1','done')",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn native_confirmation(title: &str, message: &str) -> bool {
    use windows::{
        core::PCWSTR,
        Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_YESNO,
        },
    };
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
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
fn native_confirmation(_title: &str, _message: &str) -> bool {
    false
}

fn scheduler(inner: Arc<PulsarInner>) {
    let mut startup_pending = true;
    let mut file_fingerprints: HashMap<String, String> =
        read_trigger_states(&inner).unwrap_or_default();
    let mut process_presence: HashMap<String, bool> = HashMap::new();
    let mut process_system = System::new();
    let scheduler_started_at = Utc::now();
    loop {
        thread::sleep(POLL_INTERVAL);
        process_system.refresh_processes(ProcessesToUpdate::All, true);
        let running_executables = process_system
            .processes()
            .values()
            .filter_map(|process| process.exe().map(normalized_windows_path))
            .collect::<std::collections::HashSet<_>>();
        let scenarios = match read_scenarios(&inner) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let globally_paused = match global_pause_status_inner(&inner) {
            Ok(value) => value,
            Err(_) => continue,
        };
        for scenario in scenarios.into_iter().filter(|item| item.enabled) {
            let mut due = false;
            let mut changed_path = None;
            let mut one_shots_to_consume = Vec::new();
            let mut expired_one_shots = Vec::new();
            let now = Utc::now();
            for trigger in &scenario.triggers {
                match trigger {
                    Trigger::Startup if startup_pending => due = true,
                    Trigger::Interval { every_seconds } => {
                        if let Ok(interval) = i64::try_from(*every_seconds) {
                            let last = scenario.last_run_at.as_deref().and_then(parse_time);
                            due |= last
                                .map(|at| now.signed_duration_since(at).num_seconds() >= interval)
                                .unwrap_or(true);
                        }
                    }
                    Trigger::AtTime { at } => {
                        let target = parse_time(at);
                        let already_fired = scenario
                            .last_run_at
                            .as_deref()
                            .and_then(parse_time)
                            .zip(target)
                            .map(|(last, target)| last >= target)
                            .unwrap_or(false);
                        if let Some(target) = target {
                            if target <= scheduler_started_at && !already_fired {
                                expired_one_shots.push(trigger.clone());
                            } else if now >= target && !already_fired {
                                due = true;
                                one_shots_to_consume.push(trigger.clone());
                            }
                        }
                    }
                    Trigger::FileCreated { directory } => {
                        let key = trigger_state_key(&scenario.id, trigger);
                        if let Some(fingerprint) = directory_fingerprint(Path::new(directory)) {
                            let legacy = (scenario.triggers.len() == 1)
                                .then(|| file_fingerprints.get(&scenario.id).cloned())
                                .flatten();
                            let previous = file_fingerprints
                                .entry(key.clone())
                                .or_insert_with(|| legacy.unwrap_or_else(|| fingerprint.clone()))
                                .clone();
                            if previous != fingerprint {
                                let added = newest_added_file(Path::new(directory), &previous);
                                if added.is_some() {
                                    due = true;
                                    changed_path = added;
                                }
                                file_fingerprints.insert(key.clone(), fingerprint.clone());
                                if persist_trigger_fingerprint(&inner, &key, &fingerprint).is_err()
                                {
                                    emit_trigger_state_error(&inner, &scenario);
                                }
                            }
                        }
                    }
                    Trigger::FileModified { path } => {
                        let key = trigger_state_key(&scenario.id, trigger);
                        if let Some(fingerprint) = path_fingerprint(Path::new(path)) {
                            let legacy = (scenario.triggers.len() == 1)
                                .then(|| file_fingerprints.get(&scenario.id).cloned())
                                .flatten();
                            let previous = file_fingerprints
                                .entry(key.clone())
                                .or_insert_with(|| legacy.unwrap_or_else(|| fingerprint.clone()))
                                .clone();
                            if previous != fingerprint {
                                due = true;
                                changed_path = Some(PathBuf::from(path));
                                file_fingerprints.insert(key.clone(), fingerprint.clone());
                                if persist_trigger_fingerprint(&inner, &key, &fingerprint).is_err()
                                {
                                    emit_trigger_state_error(&inner, &scenario);
                                }
                            }
                        }
                    }
                    Trigger::ProcessStarted { executable } => {
                        let key = trigger_state_key(&scenario.id, trigger);
                        let target = normalized_windows_path(Path::new(executable));
                        let is_running = running_executables
                            .iter()
                            .any(|path| process_matches_path(Path::new(path), Path::new(&target)));
                        let was_running =
                            *process_presence.entry(key.clone()).or_insert(is_running);
                        if process_started(was_running, is_running) {
                            due = true;
                        }
                        process_presence.insert(key, is_running);
                    }
                    Trigger::Manual | Trigger::Event { .. } | Trigger::Startup => {}
                }
            }
            for trigger in expired_one_shots {
                let summary = if globally_paused {
                    "Rappel ignoré : les automatisations Pulsar étaient suspendues."
                } else {
                    "Rappel manqué : ORBIT était fermé à l’heure prévue."
                };
                let _ = append_log(&inner, &scenario, "skipped", summary, false);
                consume_one_shot_trigger(&inner, &scenario, &trigger);
            }
            if due {
                if globally_paused {
                    // File fingerprints and process transitions above are still refreshed while
                    // paused, so those events are intentionally not replayed on resume. A due
                    // one-shot reminder is also consumed and explained in the local history.
                    let _ = update_status(&inner, &scenario, "paused");
                    if !one_shots_to_consume.is_empty() {
                        let _ = append_log(
                            &inner,
                            &scenario,
                            "skipped",
                            "Rappel ignoré : les automatisations Pulsar étaient suspendues.",
                            false,
                        );
                        for trigger in &one_shots_to_consume {
                            consume_one_shot_trigger(&inner, &scenario, trigger);
                        }
                    }
                    continue;
                }
                let permit =
                    !has_file_write_action(&scenario) || scenario.allow_scheduled_file_changes;
                if permit {
                    let _ = execute_scenario(&inner, &scenario, false, changed_path.as_deref());
                } else {
                    let _ = append_log(
                        &inner,
                        &scenario,
                        "skipped",
                        "Action fichier ignorée : autorisation d’arrière-plan absente.",
                        false,
                    );
                }
                for trigger in one_shots_to_consume {
                    consume_one_shot_trigger(&inner, &scenario, &trigger);
                }
            }
        }
        startup_pending = false;
    }
}

fn consume_one_shot_trigger(inner: &PulsarInner, scenario: &Scenario, consumed: &Trigger) {
    let Ok(Some(mut current)) = find_scenario(inner, &scenario.id) else {
        return;
    };
    remove_consumed_triggers(&mut current, std::slice::from_ref(consumed));
    current.updated_at = Utc::now().to_rfc3339();
    let _ = persist_scenario(inner, &current);
}

fn remove_consumed_triggers(scenario: &mut Scenario, consumed: &[Trigger]) {
    scenario
        .triggers
        .retain(|trigger| !consumed.contains(trigger));
    scenario.trigger = scenario.triggers.first().cloned();
    if scenario.triggers.is_empty() {
        scenario.enabled = false;
    }
}

fn execute_scenario(
    inner: &PulsarInner,
    scenario: &Scenario,
    confirmed: bool,
    context_path: Option<&Path>,
) -> Result<(), String> {
    validate_scenario(scenario)?;
    if global_pause_status_inner(inner)? {
        return Err(
            "Pulsar est suspendu. Reprenez les automatisations avant de lancer une action.".into(),
        );
    }
    if is_rate_limited(inner, scenario)? {
        append_log(
            inner,
            scenario,
            "skipped",
            "Limite d’exécution atteinte; réessayez dans une minute.",
            false,
        )?;
        update_status(inner, scenario, "skipped")?;
        return Ok(());
    }
    let conditions = evaluate_conditions(scenario, context_path);
    if !conditions_match(&conditions, scenario.condition_mode) {
        append_log(
            inner,
            scenario,
            "skipped",
            "Les conditions du scénario ne sont pas réunies.",
            false,
        )?;
        update_status(inner, scenario, "skipped")?;
        return Ok(());
    }
    if has_file_write_action(scenario) && !confirmed && !scenario.allow_scheduled_file_changes {
        append_log(
            inner,
            scenario,
            "skipped",
            "Action fichier ignorée : confirmation absente.",
            false,
        )?;
        update_status(inner, scenario, "skipped")?;
        return Ok(());
    }
    update_status(inner, scenario, "started")?;
    emit_run(inner, scenario, "started", "Exécution démarrée.");
    for (index, action) in scenario.actions.iter().enumerate() {
        if global_pause_status_inner(inner)? {
            let summary = format!(
                "Scénario suspendu avant l’étape {} sur {}. Les étapes précédentes sont terminées.",
                index + 1,
                scenario.actions.len(),
            );
            append_log(inner, scenario, "paused", &summary, false)?;
            update_status(inner, scenario, "paused")?;
            emit_run(
                inner,
                scenario,
                "cancelled",
                "Le scénario a été suspendu après l’action en cours.",
            );
            return Ok(());
        }
        if let Err(reason) = execute_action(inner, action) {
            let safe = redact_error(&reason);
            let detail = format!(
                "Étape {} ({}) échouée : {}",
                index + 1,
                action_label(action),
                safe
            );
            append_log(inner, scenario, "failed", &detail, false)?;
            update_status(inner, scenario, "failed")?;
            emit_run(inner, scenario, "failed", "Une action n’a pas abouti.");
            return Err(safe);
        }
        append_log(
            inner,
            scenario,
            "action",
            &format!(
                "Étape {} sur {} terminée : {}.",
                index + 1,
                scenario.actions.len(),
                action_label(action)
            ),
            false,
        )?;
    }
    append_log(
        inner,
        scenario,
        "succeeded",
        "Toutes les actions ont abouti.",
        false,
    )?;
    update_status(inner, scenario, "succeeded")?;
    emit_run(
        inner,
        scenario,
        "succeeded",
        "Toutes les actions ont abouti.",
    );
    Ok(())
}

fn execute_action(inner: &PulsarInner, action: &Action) -> Result<(), String> {
    match action {
        Action::Notify { title, body } => {
            let title = safe_text(title, 80)?;
            let body = safe_text(body, 180)?;
            inner
                .app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|e| format!("Notification Windows indisponible : {e}"))
        }
        Action::OpenFolder { path } => {
            let folder = canonical_existing(path, true)?;
            Command::new("explorer.exe")
                .arg(folder)
                .spawn()
                .map_err(|e| format!("Impossible d’ouvrir le dossier : {e}"))?;
            Ok(())
        }
        Action::OpenApp { app } => {
            let (binary, args) = match app.as_str() {
                "file-explorer" => ("explorer.exe", Vec::<String>::new()),
                "notepad" => ("notepad.exe", Vec::<String>::new()),
                "calculator" => ("calc.exe", Vec::<String>::new()),
                _ => return Err("Cette application n’est pas autorisée.".into()),
            };
            Command::new(binary)
                .args(args)
                .spawn()
                .map_err(|e| format!("Impossible d’ouvrir l’application autorisée : {e}"))?;
            Ok(())
        }
        Action::CopyFile {
            source,
            destination,
        } => {
            let source_path = canonical_existing(source, false)?;
            let destination_dir = canonical_existing(destination, true)?;
            let target = target_in_directory(&source_path, &destination_dir)?;
            let source_size = fs::metadata(&source_path)
                .map_err(|_| "Le fichier source est inaccessible.")?
                .len();
            if source_size > MAX_COPY_BYTES {
                return Err("Le fichier dépasse la limite de copie de 2 Go.".into());
            }
            let mut input =
                fs::File::open(&source_path).map_err(|_| "Le fichier source est inaccessible.")?;
            let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .map_err(|_| "Impossible de créer le fichier de destination sans remplacer un fichier existant.".to_string())?;
            if let Err(error) = std::io::copy(&mut input, &mut output) {
                let _ = fs::remove_file(&target);
                return Err(format!("La copie du fichier a échoué : {error}"));
            }
            Ok(())
        }
        Action::MoveFile {
            source,
            destination,
        } => {
            let source_path = canonical_existing(source, false)?;
            let destination_dir = canonical_existing(destination, true)?;
            let target = target_in_directory(&source_path, &destination_dir)?;
            fs::rename(&source_path, &target).map_err(|e| {
                format!("Déplacement impossible; vérifiez les volumes et les permissions : {e}")
            })?;
            Ok(())
        }
        Action::RenameFile { path, new_name } => {
            let source = canonical_existing(path, false)?;
            let name = safe_file_name(new_name)?;
            let target = source
                .parent()
                .ok_or("Dossier du fichier inaccessible.")?
                .join(name);
            if target.exists() {
                return Err("Un fichier portant déjà ce nom existe.".into());
            }
            fs::rename(&source, &target)
                .map_err(|e| format!("Impossible de renommer le fichier : {e}"))?;
            Ok(())
        }
        Action::Log { message } => {
            let _ = safe_text(message, 180)?;
            inner
                .app
                .emit("pulsar.local-log", "Une action locale a été exécutée.")
                .map_err(|e| e.to_string())
        }
    }
}

fn evaluate_conditions(scenario: &Scenario, context_path: Option<&Path>) -> Vec<ConditionResult> {
    scenario
        .conditions
        .iter()
        .map(|condition| match condition {
            Condition::Extension { value } => {
                let actual = context_path
                    .and_then(Path::extension)
                    .and_then(|ext| ext.to_str())
                    .map(|ext| format!(".{ext}"));
                let expected =
                    format!(".{}", value.trim().trim_start_matches('.')).to_ascii_lowercase();
                let passed = actual
                    .as_ref()
                    .map(|ext| ext.to_ascii_lowercase() == expected)
                    .unwrap_or(false);
                ConditionResult {
                    label: format!("Extension {expected}"),
                    passed,
                    detail: actual
                        .map(|ext| format!("Extension trouvée : {ext}"))
                        .unwrap_or_else(|| "Aucun fichier déclencheur n’est disponible.".into()),
                }
            }
            Condition::NameContains { value } => {
                let actual = context_path
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str());
                let passed = actual
                    .map(|name| name.to_lowercase().contains(&value.to_lowercase()))
                    .unwrap_or(false);
                ConditionResult {
                    label: format!("Le nom contient « {} »", truncate(value, 32)),
                    passed,
                    detail: actual
                        .map(|name| format!("Nom du fichier : {}", truncate(name, 80)))
                        .unwrap_or_else(|| {
                            "Aucun nom de fichier déclencheur n’est disponible.".into()
                        }),
                }
            }
            Condition::SizeAtLeast { bytes } => {
                let size = context_path
                    .and_then(|path| fs::metadata(path).ok())
                    .map(|metadata| metadata.len());
                let passed = size.map(|value| value >= *bytes).unwrap_or(false);
                ConditionResult {
                    label: format!("Taille ≥ {} octets", bytes),
                    passed,
                    detail: size
                        .map(|value| format!("Taille détectée : {value} octets"))
                        .unwrap_or_else(|| "Taille de fichier indisponible.".into()),
                }
            }
            Condition::PathExists { path } => {
                let passed = Path::new(path).exists();
                ConditionResult {
                    label: "Le chemin existe".into(),
                    passed,
                    detail: if passed {
                        "Le chemin est présent.".into()
                    } else {
                        "Le chemin est absent ou inaccessible.".into()
                    },
                }
            }
            Condition::TimeWindow { from, to } => {
                let current = Local::now().time();
                let start = NaiveTime::parse_from_str(from, "%H:%M").ok();
                let end = NaiveTime::parse_from_str(to, "%H:%M").ok();
                let passed = match (start, end) {
                    (Some(start), Some(end)) if start <= end => current >= start && current <= end,
                    (Some(start), Some(end)) => current >= start || current <= end,
                    _ => false,
                };
                ConditionResult {
                    label: format!("Plage horaire {from}–{to}"),
                    passed,
                    detail: format!("Heure locale : {}", current.format("%H:%M")),
                }
            }
        })
        .collect()
}

fn conditions_match(results: &[ConditionResult], mode: ConditionMode) -> bool {
    if results.is_empty() {
        return true;
    }
    match mode {
        ConditionMode::All => results.iter().all(|condition| condition.passed),
        ConditionMode::Any => results.iter().any(|condition| condition.passed),
    }
}

fn validate_scenario(scenario: &Scenario) -> Result<(), String> {
    if scenario.id.is_empty()
        || scenario.id.len() > 64
        || !scenario
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Identifiant de scénario invalide.".into());
    }
    if scenario.name.trim().is_empty() || scenario.name.len() > 128 {
        return Err("Donnez un nom de 1 à 128 caractères.".into());
    }
    if scenario.description.len() > 512 {
        return Err("La description est limitée à 512 caractères.".into());
    }
    if scenario.triggers.is_empty() || scenario.triggers.len() > MAX_TRIGGERS {
        return Err(format!("Ajoutez entre 1 et {MAX_TRIGGERS} déclencheurs."));
    }
    let mut trigger_keys = std::collections::HashSet::new();
    for trigger in &scenario.triggers {
        let key = serde_json::to_string(trigger)
            .map_err(|error| error.to_string())?
            .to_lowercase();
        if !trigger_keys.insert(key) {
            return Err("Supprimez les déclencheurs en double.".into());
        }
    }
    if scenario.actions.is_empty() || scenario.actions.len() > MAX_ACTIONS {
        return Err(format!("Ajoutez entre 1 et {MAX_ACTIONS} actions."));
    }
    if scenario.conditions.len() > MAX_CONDITIONS {
        return Err(format!("Ajoutez au plus {MAX_CONDITIONS} conditions."));
    }
    for trigger in &scenario.triggers {
        match trigger {
            Trigger::Interval { every_seconds } if !(30..=86_400).contains(every_seconds) => {
                return Err("L’intervalle doit être compris entre 30 secondes et 24 heures.".into())
            }
            Trigger::AtTime { at } => {
                let target = parse_time(at).ok_or("La date planifiée n’est pas valide.")?;
                if target <= Utc::now() {
                    return Err(
                        "Choisissez une date future. ORBIT doit rester ouvert à l’heure prévue."
                            .into(),
                    );
                }
            }
            Trigger::FileCreated { directory } => {
                canonical_existing(directory, true)
                    .map_err(|_| "Choisissez un dossier existant à surveiller.".to_string())?;
            }
            Trigger::FileModified { path } => {
                canonical_existing(path, false)
                    .map_err(|_| "Choisissez un fichier existant à surveiller.".to_string())?;
            }
            Trigger::Event { name }
                if !matches!(
                    name.as_str(),
                    "core.alert" | "echo.entry-added"
                ) =>
            {
                return Err("Cet événement ne peut pas déclencher Pulsar.".into())
            }
            Trigger::ProcessStarted { executable } => {
                canonical_executable(executable)
                    .map_err(|_| "Choisissez un fichier exécutable .exe existant.".to_string())?;
            }
            _ => {}
        }
    }
    for condition in &scenario.conditions {
        match condition {
            Condition::Extension { value } if value.trim().is_empty() || value.len() > 16 => {
                return Err("Saisissez une extension de fichier valide.".into())
            }
            Condition::NameContains { value } if value.trim().is_empty() || value.len() > 120 => {
                return Err("Saisissez le texte à rechercher dans le nom.".into())
            }
            Condition::PathExists { path } if path.trim().is_empty() => {
                return Err("Choisissez le chemin à vérifier.".into())
            }
            Condition::TimeWindow { from, to }
                if NaiveTime::parse_from_str(from, "%H:%M").is_err()
                    || NaiveTime::parse_from_str(to, "%H:%M").is_err() =>
            {
                return Err("Choisissez une plage horaire valide.".into())
            }
            _ => {}
        }
    }
    for action in &scenario.actions {
        match action {
            Action::Notify { title, body } => {
                safe_text(title, 80)?;
                safe_text(body, 180)?;
            }
            Action::OpenFolder { path } => {
                canonical_existing(path, true)
                    .map_err(|_| "Choisissez un dossier existant à ouvrir.".to_string())?;
            }
            Action::OpenApp { app }
                if !matches!(app.as_str(), "file-explorer" | "notepad" | "calculator") =>
            {
                return Err("Cette application n’est pas autorisée.".into())
            }
            Action::CopyFile {
                source,
                destination,
            }
            | Action::MoveFile {
                source,
                destination,
            } => {
                canonical_existing(source, false)
                    .map_err(|_| "Choisissez un fichier source existant.".to_string())?;
                canonical_existing(destination, true)
                    .map_err(|_| "Choisissez un dossier de destination existant.".to_string())?;
            }
            Action::RenameFile { path, new_name } => {
                canonical_existing(path, false)
                    .map_err(|_| "Choisissez un fichier existant à renommer.".to_string())?;
                safe_file_name(new_name)?;
            }
            Action::Log { message } => {
                safe_text(message, 180)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn normalize_scenario(scenario: &mut Scenario) -> Result<(), String> {
    if scenario.triggers.is_empty() {
        if let Some(legacy) = scenario.trigger.take() {
            scenario.triggers.push(legacy);
        } else {
            scenario.triggers.push(Trigger::Manual);
        }
    }
    scenario.trigger = scenario.triggers.first().cloned();
    Ok(())
}

fn canonicalize_executable_triggers(scenario: &mut Scenario) -> Result<(), String> {
    for trigger in &mut scenario.triggers {
        if let Trigger::ProcessStarted { executable } = trigger {
            *executable = canonical_executable(executable)?
                .to_string_lossy()
                .into_owned();
        }
    }
    scenario.trigger = scenario.triggers.first().cloned();
    Ok(())
}

fn canonical_executable(value: &str) -> Result<PathBuf, String> {
    let canonical = canonical_existing(value, false)?;
    if !canonical
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
    {
        return Err("Le fichier choisi doit être un exécutable .exe.".into());
    }
    Ok(canonical)
}

fn normalized_windows_path(value: &Path) -> String {
    value.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn process_started(previously_running: bool, is_running: bool) -> bool {
    !previously_running && is_running
}

fn has_event_trigger(triggers: &[Trigger], name: &str) -> bool {
    triggers
        .iter()
        .any(|trigger| matches!(trigger, Trigger::Event { name: configured } if configured == name))
}

fn process_matches_path(process_path: &Path, configured_path: &Path) -> bool {
    normalized_windows_path(process_path) == normalized_windows_path(configured_path)
}

fn trigger_state_key(scenario_id: &str, trigger: &Trigger) -> String {
    let signature = serde_json::to_string(trigger)
        .unwrap_or_else(|_| "trigger".into())
        .to_lowercase();
    format!("{scenario_id}::{signature}")
}

fn read_scenarios(inner: &PulsarInner) -> Result<Vec<Scenario>, String> {
    let records = {
        let db = inner
            .db
            .lock()
            .map_err(|_| "La base Pulsar est indisponible.")?;
        let mut stmt = db
            .prepare("SELECT json FROM scenarios ORDER BY updated_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut scenarios = Vec::with_capacity(records.len());
    for json in records {
        let mut scenario: Scenario =
            serde_json::from_str(&json).map_err(|e| format!("Scénario local illisible : {e}"))?;
        normalize_scenario(&mut scenario)?;
        let normalized = serde_json::to_string(&scenario).map_err(|e| e.to_string())?;
        if normalized != json {
            persist_scenario(inner, &scenario)?;
        }
        scenarios.push(scenario);
    }
    Ok(scenarios)
}

fn find_scenario(inner: &PulsarInner, id: &str) -> Result<Option<Scenario>, String> {
    if id.len() > 64 {
        return Err("Identifiant de scénario invalide.".into());
    }
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    let json = db
        .query_row("SELECT json FROM scenarios WHERE id=?1", [id], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    drop(db);
    json.map(|value| {
        let mut scenario: Scenario = serde_json::from_str(&value).map_err(|e| e.to_string())?;
        normalize_scenario(&mut scenario)?;
        let normalized = serde_json::to_string(&scenario).map_err(|e| e.to_string())?;
        if normalized != value {
            persist_scenario(inner, &scenario)?;
        }
        Ok(scenario)
    })
    .transpose()
}

fn persist_scenario(inner: &PulsarInner, scenario: &Scenario) -> Result<(), String> {
    let json = serde_json::to_string(scenario).map_err(|e| e.to_string())?;
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    db.execute("INSERT INTO scenarios(id,name,enabled,json,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,enabled=excluded.enabled,json=excluded.json,updated_at=excluded.updated_at",
        params![scenario.id, scenario.name, scenario.enabled as i64, json, scenario.updated_at]).map_err(|e| e.to_string())?;
    Ok(())
}

fn global_pause_status_inner(inner: &PulsarInner) -> Result<bool, String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    read_global_pause(&db)
}

fn read_global_pause(db: &Connection) -> Result<bool, String> {
    let value: Option<String> = db
        .query_row(
            "SELECT value FROM pulsar_meta WHERE key='global_pause_v1'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Impossible de lire la suspension Pulsar : {error}"))?;
    Ok(value.as_deref() == Some("true"))
}

fn write_global_pause(db: &Connection, paused: bool) -> Result<(), String> {
    db.execute(
        "INSERT INTO pulsar_meta(key,value) VALUES('global_pause_v1',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [if paused { "true" } else { "false" }],
    ).map(|_| ()).map_err(|error| format!("Impossible d’enregistrer la suspension Pulsar : {error}"))
}

fn update_status(inner: &PulsarInner, scenario: &Scenario, status: &str) -> Result<(), String> {
    let mut updated = scenario.clone();
    updated.last_run_at = Some(Utc::now().to_rfc3339());
    updated.last_run_status = Some(status.into());
    updated.updated_at = Utc::now().to_rfc3339();
    persist_scenario(inner, &updated)
}

fn append_log(
    inner: &PulsarInner,
    scenario: &Scenario,
    status: &str,
    summary: &str,
    simulation: bool,
) -> Result<(), String> {
    let log = RunLog {
        id: Uuid::new_v4().to_string(),
        scenario_id: scenario.id.clone(),
        scenario_name: truncate(&scenario.name, 96),
        status: status.into(),
        summary: truncate(summary, 180),
        at: Utc::now().to_rfc3339(),
        simulation,
    };
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    db.execute("INSERT INTO run_logs(id,scenario_id,scenario_name,status,summary,at,simulation) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![log.id, log.scenario_id, log.scenario_name, log.status, log.summary, log.at, simulation as i64]).map_err(|e| e.to_string())?;
    db.execute("DELETE FROM run_logs WHERE id IN (SELECT id FROM run_logs ORDER BY at DESC LIMIT -1 OFFSET ?1)", [MAX_LOGS]).map_err(|e| e.to_string())?;
    drop(db);
    let _ = inner.app.emit("pulsar.local-log", ());
    Ok(())
}

fn emit_run(inner: &PulsarInner, scenario: &Scenario, status: &str, detail: &str) {
    let at = Utc::now().to_rfc3339();
    let _ = inner.app.emit("pulsar.run", serde_json::json!({ "id": Uuid::new_v4().to_string(), "scenarioId": scenario.id, "status": status, "detail": detail, "at": at }));
    let _ = inner.app.emit("timeline.record", serde_json::json!({ "id": Uuid::new_v4().to_string(), "category": "pulsar", "title": format!("Exécution Pulsar : {status}"), "detail": detail, "at": at }));
}

fn emit_trigger_state_error(inner: &PulsarInner, scenario: &Scenario) {
    emit_run(
        inner,
        scenario,
        "failed",
        "Impossible de conserver l’état du déclencheur sur cet appareil.",
    );
}

fn is_rate_limited(inner: &PulsarInner, scenario: &Scenario) -> Result<bool, String> {
    let since = (Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM run_logs WHERE scenario_id=?1 AND at>=?2 AND status='started'",
            params![scenario.id, since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(rate_limit_exceeded(count))
}

fn rate_limit_exceeded(count: i64) -> bool {
    count >= 10
}

fn set_initial_file_fingerprint(inner: &PulsarInner, scenario: &Scenario) -> Result<(), String> {
    for trigger in &scenario.triggers {
        let fingerprint = match trigger {
            Trigger::FileCreated { directory } => Some(
                directory_fingerprint(Path::new(directory))
                    .ok_or("Le dossier surveillé est inaccessible.".to_string())?,
            ),
            Trigger::FileModified { path } => Some(
                path_fingerprint(Path::new(path))
                    .ok_or("Le fichier surveillé est inaccessible.".to_string())?,
            ),
            _ => None,
        };
        if let Some(value) = fingerprint {
            persist_trigger_fingerprint(inner, &trigger_state_key(&scenario.id, trigger), &value)?;
        }
    }
    Ok(())
}

fn read_trigger_states(inner: &PulsarInner) -> Result<HashMap<String, String>, String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    let mut stmt = db
        .prepare("SELECT scenario_id,fingerprint FROM trigger_state")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|e| e.to_string())
}

fn persist_trigger_fingerprint(
    inner: &PulsarInner,
    id: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let db = inner
        .db
        .lock()
        .map_err(|_| "La base Pulsar est indisponible.")?;
    db.execute("INSERT INTO trigger_state(scenario_id,fingerprint,updated_at) VALUES(?1,?2,?3) ON CONFLICT(scenario_id) DO UPDATE SET fingerprint=excluded.fingerprint,updated_at=excluded.updated_at",
        params![id, fingerprint, Utc::now().to_rfc3339()]).map_err(|e| e.to_string())?;
    Ok(())
}

fn directory_fingerprint(path: &Path) -> Option<String> {
    if !path.is_dir() {
        return None;
    }
    let mut items = fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect::<Vec<_>>();
    items.sort();
    Some(items.join("|"))
}

fn path_fingerprint(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}:{modified}", metadata.len()))
}

fn newest_added_file(directory: &Path, previous: &str) -> Option<PathBuf> {
    let old_names = previous
        .split('|')
        .collect::<std::collections::HashSet<_>>();
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name_string = name.to_string_lossy();
            if old_names.contains(name_string.as_ref()) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some((entry.path(), metadata.modified().ok()?))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

fn has_file_write_action(scenario: &Scenario) -> bool {
    scenario.actions.iter().any(|action| {
        matches!(
            action,
            Action::CopyFile { .. } | Action::MoveFile { .. } | Action::RenameFile { .. }
        )
    })
}

fn target_in_directory(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let filename = source.file_name().ok_or("Nom de fichier indisponible.")?;
    let target = destination.join(filename);
    if target.exists() {
        return Err("Un fichier portant déjà ce nom existe dans le dossier de destination.".into());
    }
    Ok(target)
}

fn canonical_existing(value: &str, directory: bool) -> Result<PathBuf, String> {
    let input = Path::new(value.trim());
    let canonical =
        fs::canonicalize(input).map_err(|_| "Le chemin est absent ou inaccessible.".to_string())?;
    let metadata = fs::metadata(&canonical)
        .map_err(|_| "Le chemin est absent ou inaccessible.".to_string())?;
    if directory && !metadata.is_dir() {
        return Err("Le chemin choisi n’est pas un dossier.".into());
    }
    if !directory && !metadata.is_file() {
        return Err("Le chemin choisi n’est pas un fichier.".into());
    }
    Ok(canonical)
}

fn safe_file_name(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 180
        || value == "."
        || value == ".."
        || value
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':'))
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Le nouveau nom doit être un nom de fichier simple, sans chemin.".into());
    }
    Ok(value)
}

fn safe_text(value: &str, max: usize) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max {
        return Err(format!(
            "Le texte doit contenir entre 1 et {max} caractères."
        ));
    }
    Ok(value)
}

fn action_label(action: &Action) -> String {
    match action {
        Action::Notify { .. } => "Envoyer une notification Windows".into(),
        Action::OpenFolder { .. } => "Ouvrir le dossier choisi".into(),
        Action::OpenApp { app } => format!("Ouvrir l’application autorisée ({app})"),
        Action::CopyFile { .. } => "Copier un fichier vers un dossier".into(),
        Action::MoveFile { .. } => "Déplacer un fichier vers un dossier".into(),
        Action::RenameFile { .. } => "Renommer un fichier".into(),
        Action::Log { .. } => "Ajouter une entrée locale au journal".into(),
    }
}

fn trigger_label(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Manual => "Manuel".into(),
        Trigger::Startup => "Démarrage d’ORBIT".into(),
        Trigger::Interval { every_seconds } => format!("Toutes les {every_seconds} secondes"),
        Trigger::AtTime { .. } => "Date et heure choisies".into(),
        Trigger::FileCreated { .. } => "Nouveau fichier dans un dossier choisi".into(),
        Trigger::FileModified { .. } => "Modification d’un fichier choisi".into(),
        Trigger::Event { name } => format!("Événement ORBIT : {name}"),
        Trigger::ProcessStarted { executable } => format!(
            "Lancement de l’application : {}",
            Path::new(executable)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(".exe")
        ),
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
fn redact_error(value: &str) -> String {
    let lower = value.to_lowercase();
    if lower.contains("clipboard")
        || lower.contains("presse-papiers")
        || lower.contains("secret")
        || lower.contains("password")
    {
        "Action échouée; les détails sensibles ont été masqués.".into()
    } else {
        truncate(value, 180)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(trigger: Trigger, actions: Vec<Action>) -> Scenario {
        Scenario {
            id: "unit-scenario-1".into(),
            name: "Scénario de test".into(),
            description: String::new(),
            enabled: false,
            triggers: vec![trigger.clone()],
            trigger: Some(trigger),
            conditions: vec![],
            condition_mode: ConditionMode::All,
            actions,
            allow_scheduled_file_changes: false,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            last_run_at: None,
            last_run_status: None,
        }
    }

    #[test]
    fn rejects_short_intervals_and_arbitrary_event_names() {
        assert!(validate_scenario(&scenario(
            Trigger::Interval { every_seconds: 29 },
            vec![Action::Log {
                message: "ok".into()
            }]
        ))
        .is_err());
        assert!(validate_scenario(&scenario(
            Trigger::Event {
                name: "pulsar.run".into()
            },
            vec![Action::Log {
                message: "ok".into()
            }]
        ))
        .is_err());
    }

    #[test]
    fn scheduled_file_writes_need_explicit_permission() {
        let value = scenario(
            Trigger::Interval { every_seconds: 60 },
            vec![Action::MoveFile {
                source: "C:\\tmp\\a.txt".into(),
                destination: "C:\\tmp\\out".into(),
            }],
        );
        assert!(has_file_write_action(&value));
        assert!(!value.allow_scheduled_file_changes);
    }

    #[test]
    fn filenames_cannot_escape_the_selected_folder() {
        assert!(safe_file_name("..\\secret.txt").is_err());
        assert!(safe_file_name("nested/secret.txt").is_err());
        assert_eq!(safe_file_name("renamed.txt").unwrap(), "renamed.txt");
    }

    #[test]
    fn rate_limit_is_visible_and_bounded() {
        assert_eq!(MAX_LOGS, 500);
        assert_eq!(MAX_ACTIONS, 12);
        assert!(!rate_limit_exceeded(9));
        assert!(rate_limit_exceeded(10));
    }

    #[test]
    fn extension_condition_uses_a_safe_trigger_path_only() {
        let value = scenario(
            Trigger::Manual,
            vec![Action::Log {
                message: "ok".into(),
            }],
        );
        let mut value = value;
        value.conditions = vec![Condition::Extension {
            value: "pdf".into(),
        }];
        let matched = evaluate_conditions(&value, Some(Path::new("report.PDF")));
        let missing = evaluate_conditions(&value, None);
        assert!(matched[0].passed);
        assert!(!missing[0].passed);
    }

    #[test]
    fn condition_mode_supports_all_and_any_and_empty_conditions() {
        let results = [
            ConditionResult {
                label: "A".into(),
                passed: false,
                detail: String::new(),
            },
            ConditionResult {
                label: "B".into(),
                passed: true,
                detail: String::new(),
            },
        ];
        assert!(!conditions_match(&results, ConditionMode::All));
        assert!(conditions_match(&results, ConditionMode::Any));
        assert!(conditions_match(&[], ConditionMode::All));
        assert!(conditions_match(&[], ConditionMode::Any));
    }

    #[test]
    fn global_pause_is_stored_and_defaults_to_resumed() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE pulsar_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);")
            .unwrap();
        assert!(!read_global_pause(&db).unwrap());
        write_global_pause(&db, true).unwrap();
        assert!(read_global_pause(&db).unwrap());
        write_global_pause(&db, false).unwrap();
        assert!(!read_global_pause(&db).unwrap());
    }

    #[test]
    fn action_serialization_uses_frontend_camel_case() {
        let action = Action::RenameFile {
            path: "C:\\tmp\\a.txt".into(),
            new_name: "b.txt".into(),
        };
        let value = serde_json::to_value(action).unwrap();
        assert_eq!(value["kind"], "rename-file");
        assert_eq!(value["newName"], "b.txt");
    }

    #[test]
    fn legacy_single_trigger_is_migrated_to_the_trigger_list() {
        let legacy = serde_json::json!({
            "id":"legacy-1","name":"Ancien scénario","description":"","enabled":false,
            "trigger":{"kind":"startup"},"conditions":[],"actions":[{"kind":"log","message":"Prêt"}],
            "allowScheduledFileChanges":false,"createdAt":"2025-01-01T00:00:00Z","updatedAt":"2025-01-01T00:00:00Z"
        });
        let mut scenario: Scenario = serde_json::from_value(legacy).unwrap();
        assert!(scenario.triggers.is_empty());
        normalize_scenario(&mut scenario).unwrap();
        assert_eq!(scenario.triggers, vec![Trigger::Startup]);
        assert_eq!(scenario.trigger, Some(Trigger::Startup));
        assert_eq!(scenario.condition_mode, ConditionMode::All);
    }

    #[test]
    fn multiple_triggers_are_or_and_one_shots_leave_other_triggers_armed() {
        let one_shot = Trigger::AtTime {
            at: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        };
        let process = Trigger::ProcessStarted {
            executable: "C:\\Tools\\editor.exe".into(),
        };
        let mut value = scenario(
            one_shot.clone(),
            vec![Action::Log {
                message: "ok".into(),
            }],
        );
        value.enabled = true;
        value.triggers.push(process.clone());
        remove_consumed_triggers(&mut value, &[one_shot]);
        assert_eq!(value.triggers, vec![process.clone()]);
        assert_eq!(value.trigger, Some(process));
        assert!(value.enabled); // A remaining OR-trigger keeps the scenario armed.
    }

    #[test]
    fn process_start_requires_a_new_absent_to_present_transition() {
        assert!(!process_started(false, false));
        assert!(process_started(false, true));
        assert!(!process_started(true, true));
        assert!(!process_started(true, false));
        assert!(process_matches_path(
            Path::new("C:/Apps/Editor.EXE"),
            Path::new("c:\\apps\\editor.exe")
        ));
    }

    #[test]
    fn draft_is_always_created_disabled_and_after_delay_has_a_preview() {
        let draft = DraftSpec {
            name: "Rappel".into(),
            description: "Dans une heure".into(),
            triggers: vec![DraftTrigger::AfterDelay { seconds: 3600 }],
            conditions: vec![],
            actions: vec![Action::Notify {
                title: "Rappel".into(),
                body: "C’est l’heure.".into(),
            }],
            allow_scheduled_file_changes: true,
        };
        let now = Utc::now();
        let (scenario, preview) = scenario_from_draft(draft, now).unwrap();
        assert!(!scenario.enabled);
        assert!(!scenario.allow_scheduled_file_changes);
        assert!(matches!(
            scenario.triggers.first(),
            Some(Trigger::AtTime { .. })
        ));
        let preview = preview.unwrap();
        assert_eq!(preview.delay_seconds, 3600);
        assert_eq!(
            parse_time(&preview.fires_at).unwrap(),
            now + chrono::Duration::hours(1)
        );
    }

    #[test]
    fn validation_rejects_duplicate_triggers_and_excessive_loops() {
        let mut duplicate = scenario(
            Trigger::Manual,
            vec![Action::Log {
                message: "ok".into(),
            }],
        );
        duplicate.triggers.push(Trigger::Manual);
        assert!(validate_scenario(&duplicate)
            .unwrap_err()
            .contains("double"));
        let mut excessive = scenario(
            Trigger::Interval { every_seconds: 30 },
            vec![Action::Log {
                message: "ok".into(),
            }],
        );
        excessive.triggers = (30..=42)
            .map(|seconds| Trigger::Interval {
                every_seconds: seconds,
            })
            .collect();
        assert!(validate_scenario(&excessive)
            .unwrap_err()
            .contains("déclencheurs"));
        let events = [
            Trigger::Manual,
            Trigger::Event {
                name: "core.alert".into(),
            },
        ];
        assert!(has_event_trigger(&events, "core.alert"));
        assert_eq!(MAX_TRIGGERS, 12);
        assert!(!rate_limit_exceeded(9));
        assert!(rate_limit_exceeded(10));
    }

    #[test]
    fn executable_trigger_requires_an_existing_exe_file() {
        let path = std::env::temp_dir().join(format!("orbit-pulsar-{}.exe", Uuid::new_v4()));
        fs::write(&path, b"test fixture").unwrap();
        let valid = canonical_executable(path.to_string_lossy().as_ref());
        assert!(valid.is_ok());
        let txt = path.with_extension("txt");
        fs::rename(&path, &txt).unwrap();
        assert!(canonical_executable(txt.to_string_lossy().as_ref()).is_err());
        let _ = fs::remove_file(txt);
    }

    #[test]
    fn after_delay_rejects_zero_or_more_than_a_day() {
        let now = Utc::now();
        assert!(DraftTrigger::AfterDelay { seconds: 0 }
            .into_trigger(now)
            .is_err());
        assert!(DraftTrigger::AfterDelay { seconds: 86_401 }
            .into_trigger(now)
            .is_err());
    }

    #[test]
    fn renderer_cannot_persist_enabled_state_or_file_write_grant() {
        let mut hostile = scenario(
            Trigger::Manual,
            vec![Action::MoveFile {
                source: "C:\\tmp\\in.txt".into(),
                destination: "C:\\tmp\\out".into(),
            }],
        );
        hostile.enabled = true;
        hostile.allow_scheduled_file_changes = true;
        apply_untrusted_save_policy(&mut hostile);
        assert!(!hostile.enabled);
        assert!(!hostile.allow_scheduled_file_changes);
    }

    #[test]
    fn renderer_boolean_cannot_override_a_native_confirmation_decline() {
        assert!(require_native_approval(false, "refused").is_err());
        assert!(require_native_approval(true, "refused").is_ok());
    }

    #[test]
    fn migration_revokes_legacy_renderer_persisted_file_grants() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE scenarios(id TEXT PRIMARY KEY,name TEXT NOT NULL,enabled INTEGER NOT NULL,json TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE trigger_state(scenario_id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE pulsar_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);").unwrap();
        let mut legacy = scenario(
            Trigger::Startup,
            vec![Action::MoveFile {
                source: "C:\\tmp\\a.txt".into(),
                destination: "C:\\tmp\\out".into(),
            }],
        );
        legacy.enabled = true;
        legacy.allow_scheduled_file_changes = true;
        let json = serde_json::to_string(&legacy).unwrap();
        db.execute(
            "INSERT INTO scenarios(id,name,enabled,json,updated_at) VALUES(?1,?2,1,?3,?4)",
            params![legacy.id, legacy.name, json, legacy.updated_at],
        )
        .unwrap();
        db.execute("INSERT INTO trigger_state(scenario_id,fingerprint,updated_at) VALUES(?1,'fingerprint','now')", [&legacy.id]).unwrap();

        revoke_untrusted_saved_file_grants(&db).unwrap();
        let json: String = db
            .query_row(
                "SELECT json FROM scenarios WHERE id=?1",
                [&legacy.id],
                |row| row.get(0),
            )
            .unwrap();
        let migrated: Scenario = serde_json::from_str(&json).unwrap();
        assert!(!migrated.enabled);
        assert!(!migrated.allow_scheduled_file_changes);
        let trigger_count: i64 = db
            .query_row("SELECT count(*) FROM trigger_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(trigger_count, 0);
        revoke_untrusted_saved_file_grants(&db).unwrap();
    }
}
