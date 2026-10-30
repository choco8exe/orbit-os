use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::{self, Read, Write}, net::{SocketAddr, TcpStream}, path::{Path, PathBuf}, process::Command, time::Duration};
use zeroize::Zeroizing;
#[cfg(windows)]
use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

const SERVICE: &str = "com.orbitos.nebula.plugin";
const ACCOUNT: &str = "com.orbitos.creator-suite:creator.streamdeck.pair-token";
const MAX_INPUT: usize = 2048;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct Input { code: Option<String>, port: Option<u16>, action: Option<String>, scenario_id: Option<String> }
#[derive(Serialize)]
struct Output { ok: bool, #[serde(skip_serializing_if="Option::is_none")] error: Option<&'static str>, #[serde(skip_serializing_if="Option::is_none")] data: Option<Value> }

fn main() {
    let result = run();
    let (ok, error, data) = match result { Ok(data) => (true, None, data), Err(message) => (false, Some(message), None) };
    let output = Output { ok, error, data };
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &output);
    let _ = stdout.write_all(b"\n");
}

fn run() -> Result<Option<Value>, &'static str> {
    let operation = std::env::args().nth(1).ok_or("operation")?;
    if std::env::args().nth(2).is_some() { return Err("operation"); }
    let input = read_input()?;
    match operation.as_str() {
        "pair" => {
            let code = input.code.as_deref().ok_or("pair")?;
            if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) { return Err("pair"); }
            let port = valid_port(input.port.ok_or("port")?)?;
            let response = request(port, "POST", "/v1/pair", None, Some(json!({"code":code})))?;
            let token = response.get("token").and_then(Value::as_str).ok_or("pair")?;
            if token.len() < 32 || token.len() > 128 { return Err("pair"); }
            let secret = Zeroizing::new(token.to_owned());
            entry()?.set_password(secret.as_str()).map_err(|_| "credential-write")?;
            Ok(None)
        }
        "status" => {
            let secret = get_secret()?;
            let value = request(45122, "GET", "/v1/status", Some(secret.as_str()), None)?;
            Ok(Some(status_summary(&value)?))
        }
        "scenarios" => {
            let secret = get_secret()?;
            let value = request(45122, "GET", "/v1/pulsar/scenarios", Some(secret.as_str()), None)?;
            let scenarios = value.get("scenarios").and_then(Value::as_array).ok_or("response")?;
            let safe = scenarios.iter().filter_map(|scenario| {
                let id = scenario.get("id")?.as_str()?;
                let name = scenario.get("name")?.as_str()?;
                if id.len() > 64 || name.len() > 160 { return None; }
                Some(json!({"id":id,"name":name.chars().filter(|c| !c.is_control()).take(80).collect::<String>()}))
            }).take(100).collect::<Vec<_>>();
            Ok(Some(json!({"scenarios":safe})))
        }
        "unpair" => {
            let secret = get_secret().ok();
            let remote_revoked = secret.as_ref().is_some_and(|token| request(45122, "POST", "/v1/unpair", Some(token.as_str()), Some(json!({}))).is_ok());
            match entry()?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(Some(json!({"remoteRevoked":remote_revoked}))),
                Err(_) => Err("credential-delete"),
            }
        }
        "action" => {
            let action = input.action.as_deref().ok_or("action")?;
            validate_action(action, input.scenario_id.as_deref())?;
            let port = valid_port(input.port.unwrap_or(45122))?;
            let value = if action == "orbit.open" {
                open_orbit_action(port)?
            } else {
                let secret = get_secret()?;
                let body = json!({"action":action,"scenarioId":input.scenario_id});
                request(port, "POST", "/v1/action", Some(secret.as_str()), Some(body))?
            };
            Ok(Some(json!({"completed":value.get("completed"),"summarySeconds":value.get("summarySeconds"),"monitoringEnabled":value.get("monitoringEnabled"),"globalPaused":value.get("globalPaused")})))
        }
        _ => Err("operation"),
    }
}

/// `orbit.open` is the single Stream Deck action allowed to work before pairing.
/// It is a fixed navigation action with no user data. When the local bridge is
/// reachable, let ORBIT focus its existing window. Only when the app is not
/// running do we launch its registered executable directly.
fn open_orbit_action(port: u16) -> Result<Value, &'static str> {
    // This exact fixed body is the only unauthenticated Stream Deck request.
    let body = orbit_open_request_body();
    let secret = get_secret();
    match secret {
        Ok(secret) => match request(port, "POST", "/v1/action", Some(secret.as_str()), Some(body.clone())) {
            Ok(value) => Ok(value),
            Err("orbit-unavailable") => launch_orbit_and_open(port, &body, Some(secret.as_str())),
            Err("orbit-rejected") => match request(port, "POST", "/v1/action", None, Some(body.clone())) {
                Ok(value) => Ok(value),
                Err("orbit-unavailable") => launch_orbit_and_open(port, &body, None),
                Err(_) => Err("not-paired"),
            },
            Err(error) => Err(error),
        },
        Err("not-paired") => match request(port, "POST", "/v1/action", None, Some(body.clone())) {
            Ok(value) => Ok(value),
            Err("orbit-unavailable") => launch_orbit_and_open(port, &body, None),
            Err(_) => Err("not-paired"),
        },
        Err(error) => Err(error),
    }
}

fn orbit_open_request_body() -> Value { json!({"action":"orbit.open"}) }

fn launch_orbit_and_open(port: u16, body: &Value, token: Option<&str>) -> Result<Value, &'static str> {
    launch_registered_orbit()?;
    for _ in 0..32 {
        if bridge_is_reachable(port) {
            return request(port, "POST", "/v1/action", token, Some(body.clone()));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("orbit-unavailable")
}

/// Side-effect-free local probe used only after launching ORBIT. It sends no
/// credentials and treats the bridge's unauthenticated 401 status as ready.
fn bridge_is_reachable(port: u16) -> bool {
    let Ok(port) = valid_port(port) else { return false };
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200)) else { return false };
    if stream.set_read_timeout(Some(Duration::from_millis(300))).is_err() { return false; }
    let request = format!("GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() { return false; }
    let mut response = [0u8; 512];
    stream.read(&mut response).is_ok_and(|read| read > 0)
}

fn candidate_executable(install_location: &str) -> Result<PathBuf, &'static str> {
    let location = install_location.trim().trim_matches('"');
    let root = PathBuf::from(location);
    if !root.is_absolute() { return Err("orbit-not-installed"); }
    Ok(root.join("orbit-os.exe"))
}

#[cfg(windows)]
fn registered_orbit_executable() -> Result<PathBuf, &'static str> {
    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\ORBIT OS";
    for hive in [CURRENT_USER, LOCAL_MACHINE] {
        let Ok(key) = hive.open(UNINSTALL_KEY) else { continue };
        let Ok(location) = key.get_string("InstallLocation") else { continue };
        let Ok(candidate) = candidate_executable(&location) else { continue };
        let Ok(root) = Path::new(&location.trim().trim_matches('"')).canonicalize() else { continue };
        let Ok(executable) = candidate.canonicalize() else { continue };
        if !executable.is_file()
            || executable.file_name().and_then(|name| name.to_str()).is_none_or(|name| !name.eq_ignore_ascii_case("orbit-os.exe"))
            || executable.parent() != Some(root.as_path())
        { continue; }
        return Ok(executable);
    }
    Err("orbit-not-installed")
}

#[cfg(windows)]
fn launch_registered_orbit() -> Result<(), &'static str> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let executable = registered_orbit_executable()?;
    Command::new(executable)
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|_| "orbit-launch")
}

#[cfg(not(windows))]
fn launch_registered_orbit() -> Result<(), &'static str> { Err("orbit-not-installed") }

fn read_input() -> Result<Input, &'static str> {
    let mut bytes = Vec::new();
    io::stdin().take((MAX_INPUT + 1) as u64).read_to_end(&mut bytes).map_err(|_| "input")?;
    if bytes.len() > MAX_INPUT { return Err("input-size"); }
    serde_json::from_slice(&bytes).map_err(|_| "input")
}

fn entry() -> Result<Entry, &'static str> { Entry::new(SERVICE, ACCOUNT).map_err(|_| "credential-store") }
fn get_secret() -> Result<Zeroizing<String>, &'static str> { entry()?.get_password().map(Zeroizing::new).map_err(|_| "not-paired") }
fn valid_port(port: u16) -> Result<u16, &'static str> { if port == 45122 { Ok(port) } else { Err("port") } }

fn validate_action(action: &str, scenario_id: Option<&str>) -> Result<(), &'static str> {
    match action {
        "pulsar.runScenario" => {
            let id = scenario_id.ok_or("scenario-id")?;
            if valid_scenario_id(id) { Ok(()) } else { Err("scenario-id") }
        }
        "orbit.open" | "mission.open" | "echo.open" | "pulsar.open" | "pulsar.togglePause" | "core.open" | "timeline.open" | "settings.open" | "workspaces.open" | "echo.copyLatest" | "echo.copyPrevious" | "echo.toggleMonitoring" | "pulse.open" | "pulse.toggleMonitoring" | "pulse.summary" => {
            if scenario_id.is_some() { Err("scenario-id") } else { Ok(()) }
        }
        _ => Err("action"),
    }
}

fn valid_scenario_id(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(index, byte)| {
        if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    })
}

fn status_summary(value: &Value) -> Result<Value, &'static str> {
    let paired = value.get("paired").and_then(Value::as_bool).ok_or("response")?;
    let local = value.get("local").and_then(Value::as_bool).ok_or("response")?;
    Ok(json!({"paired":paired,"local":local}))
}

fn request(port: u16, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> Result<Value, &'static str> {
    let port = valid_port(port)?;
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|_| "orbit-unavailable")?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|_| "io")?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|_| "io")?;
    let body = body.map(|value| serde_json::to_vec(&value).map_err(|_| "input")).transpose()?.unwrap_or_default();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\nContent-Length: {}\r\n", body.len());
    if let Some(token) = token { request.push_str(&format!("Authorization: Bearer {token}\r\n")); }
    request.push_str("Content-Type: application/json\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|_| "io")?;
    stream.write_all(&body).map_err(|_| "io")?;
    let mut response = Vec::new();
    stream.take(16384).read_to_end(&mut response).map_err(|_| "io")?;
    let split = response.windows(4).position(|window| window == b"\r\n\r\n").ok_or("response")? + 4;
    let headers = std::str::from_utf8(&response[..split]).map_err(|_| "response")?;
    if !headers.lines().next().is_some_and(|line| line.contains(" 200 ")) {
        let parsed: Value = serde_json::from_slice(&response[split..]).unwrap_or(Value::Null);
        return Err(if parsed.get("error").and_then(Value::as_str).is_some_and(|e| e.contains("permission") || e.contains("autorisation")) { "permission" } else { "orbit-rejected" });
    }
    serde_json::from_slice(&response[split..]).map_err(|_| "response")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn accepts_only_fixed_action_enums() {
        assert!(validate_action("orbit.open", None).is_ok());
        assert!(validate_action("echo.toggleMonitoring", None).is_ok());
        assert!(validate_action("pulse.summary", None).is_ok());
        assert!(validate_action("pulsar.togglePause", None).is_ok());
        assert!(validate_action("cmd.exe", None).is_err());
        assert!(validate_action("echo.toggleMonitoring;cmd.exe", None).is_err());
        assert!(validate_action("stream.start", None).is_err());
        assert!(validate_action("audio.toggleMute", None).is_err());
        assert!(validate_action("pulsar.runScenario", None).is_err());
        assert!(validate_action("pulsar.runScenario", Some("cmd.exe")).is_err());
        assert!(validate_action("pulsar.runScenario", Some("7f9c23e4-11fa-4d3a-a77a-4981af014ef0")).is_ok());
        assert!(!valid_scenario_id("7f9c23e4-11fa-4d3a-a77a-4981af014efz"));
    }
    #[test] fn port_is_constrained_to_the_fixed_orbit_listener() { assert_eq!(valid_port(45122).unwrap(), 45122); assert!(valid_port(0).is_err()); assert!(valid_port(45123).is_err()); }
    #[test] fn status_summary_reads_only_the_real_bridge_fields() {
        assert_eq!(status_summary(&json!({"paired":true,"local":true})).unwrap(), json!({"paired":true,"local":true}));
        assert_eq!(status_summary(&json!({"paired":false,"local":true})).unwrap(), json!({"paired":false,"local":true}));
        assert!(status_summary(&json!({"connected":true,"streaming":false,"recording":false})).is_err());
    }
    #[test] fn orbit_launcher_uses_only_an_absolute_registered_install_location() {
        assert_eq!(
            candidate_executable(r#""C:\Program Files\ORBIT OS""#).unwrap(),
            PathBuf::from(r"C:\Program Files\ORBIT OS\orbit-os.exe"),
        );
        assert!(candidate_executable(r"relative\ORBIT OS").is_err());
    }
    #[test] fn unauthenticated_navigation_request_contains_only_the_fixed_action() {
        assert_eq!(orbit_open_request_body(), json!({"action":"orbit.open"}));
    }
}
