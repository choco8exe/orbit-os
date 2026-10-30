//! Narrow localhost bridge for the ORBIT Stream Deck companion.
//!
//! It exposes only pairing, navigation, and fixed local Echo/Pulse actions. The listener is
//! bound to IPv4 loopback only and every request checks its ORBIT permission again.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use zeroize::Zeroizing;

const CREATOR_PLUGIN: &str = "com.orbitos.creator-suite";
const BRIDGE_PERMISSION: &str = "creator.streamdeck.bridge";
const BRIDGE_SECRET: &str = "creator.streamdeck.pair-token";
const ECHO_PLUGIN: &str = "com.orbitos.echo-streamdeck";
const ECHO_PLUGIN_PERMISSION: &str = "echo.clipboard.copy-from-streamdeck";
const ECHO_CAPTURE_PERMISSION: &str = "echo.clipboard.toggle-capture-from-streamdeck";
const PULSE_PLUGIN: &str = "com.orbitos.activity-pulse";
const PULSE_PERMISSION: &str = "activity-pulse.track-active-apps";
const PULSAR_PERMISSION: &str = "pulsar.streamdeck.control";
const PORT: u16 = 45122;
const PAIRING_LIFETIME: Duration = Duration::from_secs(90);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_REQUEST: usize = 8192;

#[derive(Default)]
struct BridgeState {
    available: bool,
    unavailable_reason: Option<String>,
    challenge: Option<PairingCode>,
}

struct PairingCode {
    value: String,
    expires: Instant,
    attempts: u8,
}
static STATE: OnceLock<Mutex<BridgeState>> = OnceLock::new();
fn state() -> &'static Mutex<BridgeState> {
    STATE.get_or_init(|| Mutex::new(BridgeState::default()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingChallenge {
    pub code: String,
    pub expires_in_seconds: u64,
    pub port: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub available: bool,
    pub paired: bool,
    pub port: u16,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeActionResult {
    pub completed: bool,
    pub summary_seconds: Option<u64>,
    pub monitoring_enabled: Option<bool>,
    pub global_paused: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ActionRequest {
    action: String,
    #[serde(default)]
    scenario_id: Option<String>,
}

/// The unpaired Stream Deck action is deliberately narrower than ActionRequest:
/// no target, scenario, or extension fields can be supplied by the caller.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenOrbitRequest {
    action: String,
}

pub fn initialize(_app: &AppHandle) {
    let listener =
        match std::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                if let Ok(mut s) = state().lock() {
                    s.available = false;
                    s.unavailable_reason = Some(format!(
                        "Le port local {PORT} est déjà utilisé ou indisponible ({error})."
                    ));
                }
                return;
            }
        };
    if let Err(error) = listener.set_nonblocking(true) {
        if let Ok(mut s) = state().lock() {
            s.available = false;
            s.unavailable_reason = Some(format!("Impossible de préparer le pont local ({error})."));
        }
        return;
    }
    let app = (*_app).clone();
    // `TcpListener::from_std` must run while a Tokio reactor is entered. Tauri's
    // setup callback binds the std listener before the async task is polled, so
    // converting it here would panic and terminate the whole application.
    tauri::async_runtime::spawn(async move {
        match TcpListener::from_std(listener) {
            Ok(listener) => {
                if let Ok(mut s) = state().lock() {
                    s.available = true;
                    s.unavailable_reason = None;
                }
                serve(listener, app).await;
            }
            Err(error) => {
                if let Ok(mut s) = state().lock() {
                    s.available = false;
                    s.unavailable_reason =
                        Some(format!("Impossible de démarrer le pont local ({error})."));
                }
            }
        }
    });
}

fn ensure(app: &AppHandle) -> Result<(), String> {
    crate::plugins::require_permission(app, CREATOR_PLUGIN, BRIDGE_PERMISSION)
}

#[tauri::command]
pub fn creator_streamdeck_pairing_start(app: AppHandle) -> Result<PairingChallenge, String> {
    ensure(&app)?;
    let mut s = state()
        .lock()
        .map_err(|_| "État du pont Stream Deck indisponible.".to_owned())?;
    if !s.available {
        return Err(s
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| "Le pont local Stream Deck est indisponible.".into()));
    }
    let uuid = uuid::Uuid::new_v4();
    let n = u128::from_be_bytes(uuid.as_bytes()[..16].try_into().expect("UUID length")) % 1_000_000;
    let code = format!("{n:06}");
    s.challenge = Some(PairingCode {
        value: code.clone(),
        expires: Instant::now() + PAIRING_LIFETIME,
        attempts: 0,
    });
    Ok(PairingChallenge {
        code,
        expires_in_seconds: PAIRING_LIFETIME.as_secs(),
        port: PORT,
    })
}

#[tauri::command]
pub fn creator_streamdeck_pairing_status(app: AppHandle) -> Result<BridgeStatus, String> {
    ensure(&app)?;
    let s = state()
        .lock()
        .map_err(|_| "État du pont Stream Deck indisponible.".to_owned())?;
    let paired =
        crate::plugins::secret_exists(&app, CREATOR_PLUGIN, BRIDGE_SECRET, BRIDGE_PERMISSION)?;
    Ok(BridgeStatus {
        available: s.available,
        paired,
        port: PORT,
        error: s.unavailable_reason.clone(),
    })
}

#[tauri::command]
pub fn creator_streamdeck_revoke(app: AppHandle) -> Result<(), String> {
    // Revocation must work even if the user has already revoked the bridge permission.
    crate::plugins::delete_secret_from_backend(
        &app,
        CREATOR_PLUGIN,
        BRIDGE_SECRET,
        BRIDGE_PERMISSION,
    )?;
    if let Ok(mut s) = state().lock() {
        s.challenge = None;
    }
    Ok(())
}

#[tauri::command]
pub fn creator_streamdeck_install(app: AppHandle) -> Result<(), String> {
    const FILE: &str = "com.orbitos.creator.streamDeckPlugin";
    let bundled = app
        .path()
        .resolve(format!("downloads/{FILE}"), BaseDirectory::Resource)
        .ok();
    let development = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join("public").join("downloads").join(FILE));
    let path = bundled.into_iter().chain(development).find(|path| path.is_file())
        .ok_or_else(|| "Le paquet Stream Deck n’est pas présent. Reconstruis le compagnon avec `npm run package` dans streamdeck-companion.".to_owned())?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|_| "Windows n’a pas pu ouvrir l’installateur Stream Deck. Vérifie que Stream Deck est installé.".to_owned())
}

async fn serve(listener: TcpListener, app: AppHandle) {
    loop {
        let Ok((stream, peer)) = listener.accept().await else {
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let app = app.clone();
        tokio::spawn(async move {
            let _ = timeout(REQUEST_TIMEOUT, handle(stream, app)).await;
        });
    }
}

async fn read_request(
    stream: &mut TcpStream,
) -> Result<(String, String, Option<String>, Vec<u8>), String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    let header_end;
    loop {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| "Lecture HTTP locale impossible.".to_owned())?;
        if n == 0 {
            return Err("Requête locale incomplète.".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > MAX_REQUEST {
            return Err("Requête locale trop volumineuse.".into());
        }
        if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "En-têtes HTTP locaux invalides.".to_owned())?
        .to_owned();
    let mut lines = headers.split("\r\n");
    let request = lines.next().ok_or("Requête locale invalide.")?;
    let mut parts = request.split_ascii_whitespace();
    let method = parts.next().ok_or("Requête locale invalide.")?.to_owned();
    let path = parts.next().ok_or("Requête locale invalide.")?.to_owned();
    if parts.next() != Some("HTTP/1.1") {
        return Err("Version HTTP locale invalide.".into());
    }
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| {
            value
                .trim()
                .parse::<usize>()
                .map_err(|_| "Taille HTTP locale invalide.".to_owned())
        })
        .transpose()?
        .unwrap_or(0);
    if header_end + length > MAX_REQUEST {
        return Err("Requête locale trop volumineuse.".into());
    }
    while bytes.len() < header_end + length {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| "Lecture HTTP locale impossible.".to_owned())?;
        if n == 0 {
            return Err("Corps HTTP local incomplet.".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    let authorization = headers
        .split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.trim().strip_prefix("Bearer ").map(str::to_owned))
        .flatten();
    Ok((
        method,
        path,
        authorization,
        bytes[header_end..header_end + length].to_vec(),
    ))
}

async fn handle(mut stream: TcpStream, app: AppHandle) -> Result<(), String> {
    let response = match read_request(&mut stream).await {
        Ok((method, path, authorization, body)) => {
            route(&app, &method, &path, authorization.as_deref(), &body).await
        }
        Err(error) => (400, json!({"error":error})),
    };
    let raw = serde_json::to_vec(&response.1).map_err(|_| "Réponse locale invalide.".to_owned())?;
    let header = format!("HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.0, if response.0 < 400 { "OK" } else { "Error" }, raw.len());
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|_| "Réponse HTTP locale impossible.".to_owned())?;
    stream
        .write_all(&raw)
        .await
        .map_err(|_| "Réponse HTTP locale impossible.".to_owned())?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn route(
    app: &AppHandle,
    method: &str,
    path: &str,
    authorization: Option<&str>,
    body: &[u8],
) -> (u16, Value) {
    match (method, path) {
        ("POST", "/v1/pair") => pair(app, body),
        ("POST", "/v1/unpair") => {
            let token = match authorized_token(app, authorization) {
                Ok(token) => token,
                Err(error) => return (401, json!({"error":error})),
            };
            drop(token);
            match crate::plugins::delete_secret_from_backend(
                app,
                CREATOR_PLUGIN,
                BRIDGE_SECRET,
                BRIDGE_PERMISSION,
            ) {
                Ok(()) => (200, json!({"revoked":true})),
                Err(error) => (503, json!({"error":error})),
            }
        }
        ("GET", "/v1/status") => {
            let token = match authorized_token(app, authorization) {
                Ok(token) => token,
                Err(error) => return (401, json!({"error":error})),
            };
            drop(token);
            (200, json!({"paired":true,"local":true}))
        }
        ("GET", "/v1/pulsar/scenarios") => {
            let token = match authorized_token(app, authorization) {
                Ok(token) => token,
                Err(error) => return (401, json!({"error":error})),
            };
            drop(token);
            if let Err(error) = ensure_pulsar_streamdeck_control(app) {
                return (403, json!({"error":error}));
            }
            match crate::modules::pulsar::pulsar_list(
                app.state::<crate::modules::pulsar::PulsarState>(),
            ) {
                Ok(scenarios) => {
                    let items = scenarios.into_iter().filter(|scenario| scenario.enabled).take(100)
                        .map(|scenario| json!({"id":scenario.id,"name":scenario.name.chars().filter(|c| !c.is_control()).take(80).collect::<String>()}))
                        .collect::<Vec<_>>();
                    (200, json!({"scenarios":items}))
                }
                Err(error) => (503, json!({"error":error})),
            }
        }
        ("POST", "/v1/action") => {
            if is_unauthenticated_orbit_open(body) {
                if let Err(error) = ensure(app) {
                    return (403, json!({"error":error}));
                }
                return match open_orbit(app, "mission-control") {
                    Ok(result) => (
                        200,
                        serde_json::to_value(result)
                            .unwrap_or_else(|_| json!({"completed":true})),
                    ),
                    Err(error) => (403, json!({"error":error})),
                };
            }
            let token = match authorized_token(app, authorization) {
                Ok(token) => token,
                Err(error) => return (401, json!({"error":error})),
            };
            drop(token);
            let request: ActionRequest = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return (400, json!({"error":"Action Stream Deck invalide."})),
            };
            match perform_action(app, request).await {
                Ok(result) => (
                    200,
                    serde_json::to_value(result).unwrap_or_else(|_| json!({"completed":true})),
                ),
                Err(error) => (403, json!({"error":error})),
            }
        }
        _ => (404, json!({"error":"Route Stream Deck inconnue."})),
    }
}

fn is_unauthenticated_orbit_open(body: &[u8]) -> bool {
    serde_json::from_slice::<OpenOrbitRequest>(body)
        .map(|request| request.action == "orbit.open")
        .unwrap_or(false)
}

fn pair(app: &AppHandle, body: &[u8]) -> (u16, Value) {
    if let Err(error) = ensure(app) {
        return (403, json!({"error":error}));
    }
    let request: PairRequest = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return (400, json!({"error":"Code d’appairage invalide."})),
    };
    if request.code.len() != 6 || !request.code.bytes().all(|b| b.is_ascii_digit()) {
        return (400, json!({"error":"Code d’appairage invalide."}));
    }
    let valid = {
        let Ok(mut state) = state().lock() else {
            return (503, json!({"error":"État d’appairage indisponible."}));
        };
        match state.challenge.as_mut() {
            Some(challenge)
                if challenge.expires > Instant::now()
                    && challenge.attempts < 5
                    && challenge.value == request.code =>
            {
                state.challenge = None;
                true
            }
            Some(challenge) if challenge.expires > Instant::now() && challenge.attempts < 5 => {
                challenge.attempts += 1;
                if challenge.attempts >= 5 {
                    state.challenge = None;
                }
                false
            }
            _ => {
                state.challenge = None;
                false
            }
        }
    };
    if !valid {
        return (
            401,
            json!({"error":"Code invalide, expiré ou déjà utilisé."}),
        );
    }
    let token = uuid::Uuid::new_v4().to_string();
    match crate::plugins::set_secret_from_backend(
        app,
        CREATOR_PLUGIN,
        BRIDGE_SECRET,
        BRIDGE_PERMISSION,
        &token,
    ) {
        Ok(()) => (200, json!({"token":token})),
        Err(error) => (503, json!({"error":error})),
    }
}

fn authorized_token(
    app: &AppHandle,
    authorization: Option<&str>,
) -> Result<Zeroizing<String>, String> {
    ensure(app)?;
    let supplied = authorization.ok_or_else(|| "Appairage Stream Deck requis.".to_owned())?;
    if supplied.len() > 128 {
        return Err("Jeton Stream Deck invalide.".into());
    }
    let stored = crate::plugins::get_secret_from_backend(
        app,
        CREATOR_PLUGIN,
        BRIDGE_SECRET,
        BRIDGE_PERMISSION,
    )?;
    if constant_time_equal(supplied.as_bytes(), stored.as_bytes()) {
        Ok(stored)
    } else {
        Err("Jeton Stream Deck invalide ou révoqué.".into())
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

async fn perform_action(
    app: &AppHandle,
    request: ActionRequest,
) -> Result<BridgeActionResult, String> {
    if !is_supported_action(&request.action) {
        return Err("Cette action Stream Deck n’est pas autorisée.".into());
    }
    if let Some(target) = open_target_for_action(&request.action) {
        return open_orbit(app, target);
    }
    match request.action.as_str() {
        "echo.copyLatest" | "echo.copyPrevious" => {
            copy_echo_entry(app, request.action == "echo.copyPrevious")
        }
        "echo.toggleMonitoring" => toggle_echo_monitoring(app),
        "pulse.open" => {
            crate::plugins::require_plugin_enabled(app, PULSE_PLUGIN)?;
            open_orbit(app, "usage-analytics")
        }
        "pulse.toggleMonitoring" => toggle_pulse(app),
        "pulse.summary" => pulse_summary(app),
        "pulsar.togglePause" => toggle_pulsar_pause(app),
        "pulsar.runScenario" => run_pulsar_scenario(app, request.scenario_id.as_deref()),
        _ => Err("Cette action Stream Deck n’est pas autorisée.".into()),
    }
}

fn open_target_for_action(action: &str) -> Option<&'static str> {
    match action {
        "orbit.open" | "mission.open" => Some("mission-control"),
        "echo.open" => Some("echo"),
        "pulsar.open" => Some("pulsar"),
        "core.open" => Some("core"),
        "timeline.open" => Some("timeline"),
        "settings.open" => Some("settings"),
        "workspaces.open" => Some("workspaces"),
        _ => None,
    }
}

fn is_supported_action(action: &str) -> bool {
    matches!(
        action,
        "echo.copyLatest"
            | "echo.copyPrevious"
            | "echo.toggleMonitoring"
            | "orbit.open"
            | "mission.open"
            | "echo.open"
            | "pulsar.open"
            | "pulsar.togglePause"
            | "pulsar.runScenario"
            | "core.open"
            | "timeline.open"
            | "settings.open"
            | "workspaces.open"
            | "pulse.open"
            | "pulse.toggleMonitoring"
            | "pulse.summary"
    )
}

fn toggle_pulsar_pause(app: &AppHandle) -> Result<BridgeActionResult, String> {
    ensure_pulsar_streamdeck_control(app)?;
    let pulsar = app.state::<crate::modules::pulsar::PulsarState>();
    let paused = !pulsar.global_pause_status()?;
    let global_paused = pulsar.set_global_pause(paused)?;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: None,
        monitoring_enabled: None,
        global_paused: Some(global_paused),
    })
}

fn run_pulsar_scenario(
    app: &AppHandle,
    scenario_id: Option<&str>,
) -> Result<BridgeActionResult, String> {
    ensure_pulsar_streamdeck_control(app)?;
    let id = scenario_id
        .ok_or_else(|| "Choisis un scénario Pulsar dans les paramètres de la touche.".to_owned())?;
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("Identifiant de scénario Pulsar invalide.".into());
    }
    let state = app.state::<crate::modules::pulsar::PulsarState>();
    let scenarios = crate::modules::pulsar::pulsar_list(state)?;
    let scenario = scenarios
        .iter()
        .find(|scenario| scenario.id == id && scenario.enabled)
        .ok_or_else(|| {
            "Ce scénario n’est pas activé dans Pulsar. Actualise la liste de la touche.".to_owned()
        })?;
    let _ = scenario;
    crate::modules::pulsar::pulsar_run(
        app.state::<crate::modules::pulsar::PulsarState>(),
        id.to_owned(),
        Some(true),
    )?;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: None,
        monitoring_enabled: None,
        global_paused: None,
    })
}

fn ensure_pulsar_streamdeck_control(app: &AppHandle) -> Result<(), String> {
    let grant = crate::plugins::require_permission(app, CREATOR_PLUGIN, PULSAR_PERMISSION);
    map_pulsar_permission(grant)
}

fn map_pulsar_permission(grant: Result<(), String>) -> Result<(), String> {
    grant.map_err(|_| {
        "Autorise « Contrôler Pulsar depuis Stream Deck » dans ORBIT → Plugins pour cette action."
            .to_owned()
    })
}

fn copy_echo_entry(app: &AppHandle, previous: bool) -> Result<BridgeActionResult, String> {
    crate::plugins::require_plugin_enabled(app, ECHO_PLUGIN)?;
    crate::plugins::require_permission(app, ECHO_PLUGIN, ECHO_PLUGIN_PERMISSION)?;
    let snapshot =
        crate::modules::echo::echo_snapshot(app.state::<crate::modules::echo::EchoState>())?;
    let mut entries = snapshot.entries;
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if entries.is_empty() {
        return Err("Echo ne contient aucune entrée copiable.".into());
    }
    let selected = select_echo_entry(
        entries.into_iter().map(|entry| entry.id).collect(),
        previous,
    )
    .ok_or_else(|| "Echo ne contient aucune entrée copiable.".to_owned())?;
    crate::modules::echo::echo_copy_entry_from_streamdeck(
        app.state::<crate::modules::echo::EchoState>(),
        selected,
    )?;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: None,
        monitoring_enabled: None,
        global_paused: None,
    })
}

fn select_echo_entry(ids: Vec<String>, previous: bool) -> Option<String> {
    let index = if previous { 1 } else { 0 };
    ids.get(index).cloned()
}

fn toggle_echo_monitoring(app: &AppHandle) -> Result<BridgeActionResult, String> {
    let state = app.state::<crate::modules::echo::EchoState>();
    let snapshot = crate::modules::echo::echo_snapshot(state)?;
    let enable = !snapshot.preferences.monitoring_enabled;
    if enable {
        crate::plugins::require_plugin_enabled(app, ECHO_PLUGIN)?;
        crate::plugins::require_permission(app, ECHO_PLUGIN, ECHO_CAPTURE_PERMISSION)?;
    }
    let state = app.state::<crate::modules::echo::EchoState>();
    crate::modules::echo::echo_set_monitoring(app.clone(), state, enable)?;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: None,
        monitoring_enabled: Some(enable),
        global_paused: None,
    })
}

fn pulse_summary(app: &AppHandle) -> Result<BridgeActionResult, String> {
    crate::plugins::require_permission(app, PULSE_PLUGIN, PULSE_PERMISSION)?;
    let state = app.state::<crate::modules::activity::ActivityPulseState>();
    let snapshot =
        crate::modules::activity::activity_pulse_snapshot(app.clone(), state, "week".into())?;
    Ok(pulse_summary_result(
        snapshot.total_seconds,
        snapshot.preferences.monitoring_enabled,
    ))
}

fn pulse_summary_result(total_seconds: u64, monitoring_enabled: bool) -> BridgeActionResult {
    BridgeActionResult {
        completed: true,
        summary_seconds: Some(total_seconds),
        monitoring_enabled: Some(monitoring_enabled),
        global_paused: None,
    }
}

fn toggle_pulse(app: &AppHandle) -> Result<BridgeActionResult, String> {
    let state = app.state::<crate::modules::activity::ActivityPulseState>();
    let snapshot =
        crate::modules::activity::activity_pulse_snapshot(app.clone(), state, "week".into())?;
    if !snapshot.preferences.monitoring_enabled {
        crate::plugins::require_permission(app, PULSE_PLUGIN, PULSE_PERMISSION)?;
    }
    let state = app.state::<crate::modules::activity::ActivityPulseState>();
    let updated = crate::modules::activity::activity_pulse_set_monitoring(
        app.clone(),
        state,
        !snapshot.preferences.monitoring_enabled,
    )?;
    let snapshot = updated;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: Some(snapshot.total_seconds),
        monitoring_enabled: Some(snapshot.preferences.monitoring_enabled),
        global_paused: None,
    })
}

fn open_orbit(app: &AppHandle, target: &str) -> Result<BridgeActionResult, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Fenêtre ORBIT indisponible.".to_owned())?;
    window
        .unminimize()
        .map_err(|_| "Impossible de restaurer la fenêtre ORBIT.".to_owned())?;
    window
        .show()
        .map_err(|_| "Impossible d’afficher ORBIT.".to_owned())?;
    window
        .set_focus()
        .map_err(|_| "Impossible de mettre ORBIT au premier plan.".to_owned())?;
    let allowed = [
        "mission-control",
        "echo",
        "pulsar",
        "core",
        "timeline",
        "settings",
        "workspaces",
        "usage-analytics",
    ];
    if !allowed.contains(&target) {
        return Err("Ce module ORBIT n’est pas accessible depuis Stream Deck.".into());
    }
    app.emit("orbit://navigate", target)
        .map_err(|_| "Impossible d’ouvrir ce module ORBIT.".to_owned())?;
    Ok(BridgeActionResult {
        completed: true,
        summary_seconds: None,
        monitoring_enabled: None,
        global_paused: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_actions_map_to_the_expected_orbit_sections() {
        assert_eq!(open_target_for_action("orbit.open"), Some("mission-control"));
        assert_eq!(open_target_for_action("mission.open"), Some("mission-control"));
        assert_eq!(open_target_for_action("core.open"), Some("core"));
        assert_eq!(open_target_for_action("pulse.open"), None);
        assert_eq!(open_target_for_action("cmd.exe"), None);
    }
    #[test]
    fn unauthenticated_route_accepts_only_exact_orbit_open_request() {
        assert!(is_unauthenticated_orbit_open(br#"{"action":"orbit.open"}"#));
        assert!(is_unauthenticated_orbit_open(br#"{ "action" : "orbit.open" }"#));
        assert!(!is_unauthenticated_orbit_open(br#"{"action":"mission.open"}"#));
        assert!(!is_unauthenticated_orbit_open(
            br#"{"action":"orbit.open","target":"core"}"#
        ));
        assert!(!is_unauthenticated_orbit_open(
            br#"{"action":"orbit.open","scenarioId":null}"#
        ));
        assert!(!is_unauthenticated_orbit_open(
            br#"{"action":"echo.copyLatest"}"#
        ));
        assert!(!is_unauthenticated_orbit_open(b"not json"));
    }
    #[test]
    fn pulse_summary_preserves_zero_and_exact_seconds() {
        let empty = pulse_summary_result(0, true);
        assert!(empty.completed);
        assert_eq!(empty.summary_seconds, Some(0));
        assert_eq!(empty.monitoring_enabled, Some(true));

        let tracked = pulse_summary_result(3_661, false);
        assert_eq!(tracked.summary_seconds, Some(3_661));
        assert_eq!(tracked.monitoring_enabled, Some(false));
    }
    #[test]
    fn pairing_code_is_six_digits_and_short_lived() {
        let s = state();
        let mut s = s.lock().unwrap();
        let code = "042731".to_string();
        s.challenge = Some(PairingCode {
            value: code.clone(),
            expires: Instant::now() + PAIRING_LIFETIME,
            attempts: 0,
        });
        let p = s.challenge.as_ref().unwrap();
        assert_eq!(p.value.len(), 6);
        assert!(p.expires > Instant::now());
    }
    #[test]
    fn bridge_is_loopback_only_and_uses_fixed_port() {
        assert_eq!(PORT, 45122);
        assert!(IpAddr::V4(Ipv4Addr::LOCALHOST).is_loopback());
    }
    #[test]
    fn bearer_comparison_rejects_different_lengths_and_values() {
        assert!(constant_time_equal(b"token", b"token"));
        assert!(!constant_time_equal(b"token", b"other"));
        assert!(!constant_time_equal(b"x", b"token"));
    }
    #[test]
    fn action_deserializer_rejects_unknown_fields() {
        assert!(
            serde_json::from_str::<ActionRequest>(r#"{"action":"stream.start","extra":true}"#)
                .is_err()
        );
    }
    #[test]
    fn action_allowlist_has_only_local_orbit_operations() {
        assert!(is_supported_action("orbit.open"));
        assert!(is_supported_action("echo.copyPrevious"));
        assert!(is_supported_action("pulse.toggleMonitoring"));
        assert!(is_supported_action("pulsar.togglePause"));
        assert!(!is_supported_action("obs.status"));
        assert!(!is_supported_action("stream.start"));
        assert!(!is_supported_action("cmd.exe"));
    }
    #[test]
    fn pulsar_actions_are_denied_without_the_explicit_streamdeck_grant() {
        let error = map_pulsar_permission(Err("permission absent".into())).unwrap_err();
        assert!(error.contains("Contrôler Pulsar depuis Stream Deck"));
        assert!(map_pulsar_permission(Ok(())).is_ok());
    }
    #[test]
    fn action_deserializer_rejects_arbitrary_input_fields() {
        assert!(serde_json::from_str::<ActionRequest>(r#"{"action":"echo.copyLatest"}"#).is_ok());
        assert!(serde_json::from_str::<ActionRequest>(
            r#"{"action":"echo.copyLatest","text":"private"}"#
        )
        .is_err());
    }
    #[test]
    fn echo_capture_toggle_is_a_fixed_action_and_accepts_no_clipboard_content() {
        let request: ActionRequest =
            serde_json::from_str(r#"{"action":"echo.toggleMonitoring"}"#).unwrap();
        assert_eq!(request.action, "echo.toggleMonitoring");
        assert!(serde_json::from_str::<ActionRequest>(
            r#"{"action":"echo.toggleMonitoring","text":"private"}"#
        )
        .is_err());
    }
    #[test]
    fn echo_selection_is_deterministic_and_never_cycles_after_copy_capture() {
        let newest = vec![
            "newest".to_string(),
            "previous".to_string(),
            "older".to_string(),
        ];
        assert_eq!(
            select_echo_entry(newest.clone(), false).as_deref(),
            Some("newest")
        );
        assert_eq!(select_echo_entry(newest, true).as_deref(), Some("previous"));
        assert_eq!(
            select_echo_entry(
                vec!["newest".into(), "previous".into(), "older".into()],
                true
            )
            .as_deref(),
            Some("previous")
        );
        assert_eq!(select_echo_entry(vec!["only".into()], true), None);
    }
}
