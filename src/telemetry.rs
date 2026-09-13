//! BetterDesk device telemetry and heartbeat-delivered command support.
//!
//! Collection is deliberately best-effort: unsupported or restricted host
//! APIs become compact status codes instead of preventing the main heartbeat.

use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

use hbb_common::{
    base64::{engine::general_purpose::STANDARD, Engine as _},
    config::{self, keys, Config, LocalConfig},
    log,
    sodiumoxide::crypto::{box_, secretbox, sign},
};
use serde_json::{json, Value};

lazy_static::lazy_static! {
    static ref SEEN_COMMANDS: Mutex<HashSet<i64>> = Mutex::new(HashSet::new());
    static ref LAST_ACTIVITY_COLLECTION: Mutex<Option<Instant>> = Mutex::new(None);
}

const MAX_PROCESS_ROWS: usize = 500;
const MAX_DIRECTORY_ROWS: usize = 500;
const MAX_COMMAND_OUTPUT: usize = 16 * 1024;
const MAX_FILE_READ: usize = 1024 * 1024;
const TELEMETRY_SEQUENCE: &str = "betterdesk-telemetry-sequence";
const RESPONSE_PUBLIC_KEY: &str = "betterdesk-telemetry-response-public-key";
const RESPONSE_SECRET_KEY: &str = "betterdesk-telemetry-response-secret-key";

fn trim_output(value: String) -> String {
    value.chars().take(MAX_COMMAND_OUTPUT).collect()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn heartbeat() -> Value {
    let mut system = hbb_common::sysinfo::System::new();
    system.refresh_memory();
    system.refresh_cpu();

    let cpus = system.cpus();
    let cpu_percent = if cpus.is_empty() {
        None
    } else {
        Some(cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32)
    };
    let memory_percent = if system.total_memory() == 0 {
        None
    } else {
        Some(system.used_memory() as f64 / system.total_memory() as f64 * 100.0)
    };

    let mut status = serde_json::Map::new();
    status.insert("metrics".to_owned(), json!("ok"));
    status.insert("hardware".to_owned(), json!("ok"));
    status.insert("services".to_owned(), json!("unavailable"));
    status.insert("processes".to_owned(), json!("unavailable"));
    status.insert("events".to_owned(), json!("unavailable"));
    status.insert("activity".to_owned(), json!("permission_denied"));

    let metrics = json!({
        "cpu_percent": cpu_percent,
        "memory_percent": memory_percent,
        "disk_percent": Value::Null,
        "gpu_percent": Value::Null,
        "network": Value::Null,
    });

    let mut payload = json!({
        "telemetry_schema": 1,
        "sample_id": uuid::Uuid::new_v4().to_string(),
        "collected_at": now_rfc3339(),
        "metrics": metrics,
        "status": status,
    });
    if Config::get_option("telemetry-activity-enabled") == "Y" {
        let should_collect = match LAST_ACTIVITY_COLLECTION.lock() {
            Ok(mut last) => {
                if last
                    .map(|value| value.elapsed() >= Duration::from_secs(300))
                    .unwrap_or(true)
                {
                    *last = Some(Instant::now());
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        if should_collect {
            payload["snapshots"] = json!([collect_activity()]);
        }
    }
    payload
}

pub fn seal_payload(
    payload: &Value,
    server_key_id: &str,
    server_public_key: &str,
    device_id: &str,
) -> Result<Value, String> {
    let server_public_bytes = STANDARD
        .decode(server_public_key)
        .map_err(|_| "invalid server telemetry key".to_owned())?;
    let server_public_key = box_::PublicKey::from_slice(&server_public_bytes)
        .ok_or_else(|| "invalid server telemetry key size".to_owned())?;
    let (ephemeral_public, ephemeral_secret) = box_::gen_keypair();
    let shared = box_::precompute(&server_public_key, &ephemeral_secret);
    let shared_key = secretbox::Key(shared.0);
    let nonce = secretbox::gen_nonce();
    let plaintext = serde_json::to_vec(payload).map_err(|err| err.to_string())?;
    let ciphertext = secretbox::seal(&plaintext, &nonce, &shared_key);

    let current_sequence = match LocalConfig::get_option(TELEMETRY_SEQUENCE).parse::<u64>() {
        Ok(value) => value,
        Err(_) => 0,
    };
    let sequence = current_sequence.saturating_add(1);
    LocalConfig::set_option(TELEMETRY_SEQUENCE.to_owned(), sequence.to_string());

    let ephemeral_public = STANDARD.encode(ephemeral_public.0);
    let nonce_value = STANDARD.encode(nonce.0);
    let ciphertext_value = STANDARD.encode(ciphertext);
    let (response_public_key, _) = response_keypair()?;
    let response_public_key_value = STANDARD.encode(response_public_key.0);
    let signing_message = format!(
        "1|{}|{}|{}|{}|{}|{}",
        device_id, sequence, server_key_id, ephemeral_public, nonce_value, ciphertext_value
    ) + "|"
        + &response_public_key_value;
    let key_pair = Config::get_key_pair();
    let secret_key = sign::SecretKey::from_slice(&key_pair.0)
        .ok_or_else(|| "device signing key unavailable".to_owned())?;
    let signature = sign::sign_detached(signing_message.as_bytes(), &secret_key);

    Ok(json!({
        "version": 1,
        "device_id": device_id,
        "sequence": sequence,
        "server_key_id": server_key_id,
        "ephemeral_public_key": ephemeral_public,
        "response_public_key": response_public_key_value,
        "nonce": nonce_value,
        "ciphertext": ciphertext_value,
        "signature": STANDARD.encode(signature.as_ref()),
    }))
}

fn response_keypair() -> Result<(box_::PublicKey, box_::SecretKey), String> {
    let public = STANDARD
        .decode(LocalConfig::get_option(RESPONSE_PUBLIC_KEY))
        .ok()
        .and_then(|bytes| box_::PublicKey::from_slice(&bytes));
    let secret = STANDARD
        .decode(LocalConfig::get_option(RESPONSE_SECRET_KEY))
        .ok()
        .and_then(|bytes| box_::SecretKey::from_slice(&bytes));
    if let (Some(public), Some(secret)) = (public, secret) {
        return Ok((public, secret));
    }
    let (public, secret) = box_::gen_keypair();
    LocalConfig::set_option(RESPONSE_PUBLIC_KEY.to_owned(), STANDARD.encode(public.0));
    LocalConfig::set_option(RESPONSE_SECRET_KEY.to_owned(), STANDARD.encode(secret.0));
    Ok((public, secret))
}

pub fn open_response(envelope: &Value) -> Result<Value, String> {
    let version = envelope
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "response envelope version missing".to_owned())?;
    let device_id = envelope
        .get("device_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope device missing".to_owned())?;
    let sequence = envelope
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| "response envelope sequence missing".to_owned())?;
    let server_key_id = envelope
        .get("server_key_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope key id missing".to_owned())?;
    let ephemeral = envelope
        .get("ephemeral_public_key")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope key missing".to_owned())?;
    let nonce = envelope
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope nonce missing".to_owned())?;
    let ciphertext = envelope
        .get("ciphertext")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope ciphertext missing".to_owned())?;
    let signature = envelope
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| "response envelope signature missing".to_owned())?;

    let signing_message = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        version, device_id, sequence, server_key_id, ephemeral, nonce, ciphertext
    );
    let server_key = STANDARD
        .decode(Config::get_option(keys::OPTION_KEY))
        .map_err(|_| "invalid BetterDesk server key".to_owned())?;
    let server_key = sign::PublicKey::from_slice(&server_key)
        .ok_or_else(|| "invalid BetterDesk server key size".to_owned())?;
    let signed = STANDARD
        .decode(signature)
        .map_err(|_| "invalid response signature".to_owned())?;
    let verified =
        sign::verify(&signed, &server_key).map_err(|_| "response signature mismatch".to_owned())?;
    if verified != signing_message.as_bytes() {
        return Err("response envelope identity mismatch".to_owned());
    }

    let ephemeral = STANDARD
        .decode(ephemeral)
        .map_err(|_| "invalid response ephemeral key".to_owned())?;
    let nonce = STANDARD
        .decode(nonce)
        .map_err(|_| "invalid response nonce".to_owned())?;
    let ciphertext = STANDARD
        .decode(ciphertext)
        .map_err(|_| "invalid response ciphertext".to_owned())?;
    let ephemeral = box_::PublicKey::from_slice(&ephemeral)
        .ok_or_else(|| "invalid response ephemeral key size".to_owned())?;
    let nonce = secretbox::Nonce::from_slice(&nonce)
        .ok_or_else(|| "invalid response nonce size".to_owned())?;
    let (_, response_secret) = response_keypair()?;
    let shared = box_::precompute(&ephemeral, &response_secret);
    let shared_key = secretbox::Key(shared.0);
    let plaintext = secretbox::open(&ciphertext, &nonce, &shared_key)
        .map_err(|_| "response decryption failed".to_owned())?;
    serde_json::from_slice(&plaintext).map_err(|_| "response JSON invalid".to_owned())
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(path) = std::env::var("USERPROFILE") {
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    std::env::var_os("HOME").map(PathBuf::from)
}

fn safe_file_path(raw: Option<&str>) -> Result<PathBuf, &'static str> {
    let root = home_directory()
        .ok_or("unavailable")?
        .canonicalize()
        .map_err(|_| "unavailable")?;
    let requested = raw
        .filter(|value| !value.trim().is_empty() && *value != "/" && *value != "\\")
        .unwrap_or(".");
    let path = PathBuf::from(requested);
    let candidate = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let canonical = candidate.canonicalize().map_err(|_| "unavailable")?;
    if canonical == root || canonical.starts_with(&root) {
        Ok(canonical)
    } else {
        Err("permission_denied")
    }
}

fn collect_processes() -> Value {
    let mut system = hbb_common::sysinfo::System::new();
    system.refresh_processes();
    let processes = system
        .processes()
        .iter()
        .take(MAX_PROCESS_ROWS)
        .map(|(pid, process)| {
            json!({
                "pid": pid.to_string(),
                "name": process.name(),
                "cpu_percent": process.cpu_usage(),
                "cpu": process.cpu_usage(),
                "memory_bytes": process.memory(),
                "memory_mb": process.memory() as f64 / 1048576.0,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "kind": "processes",
        "sample_id": uuid::Uuid::new_v4().to_string(),
        "status": "ok",
        "collected_at": now_rfc3339(),
        "data": {"processes": processes},
    })
}

fn command_output(program: &str, args: &[&str]) -> Result<String, &'static str> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|_| "unsupported")?;
    if !output.status.success() {
        return Err("permission_denied");
    }
    Ok(trim_output(
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

fn collect_services() -> Value {
    #[cfg(windows)]
    let result = command_output("sc.exe", &["query", "state=", "all"]);
    #[cfg(target_os = "linux")]
    let result = command_output(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--all",
            "--no-legend",
            "--no-pager",
        ],
    );
    #[cfg(target_os = "macos")]
    let result = command_output("launchctl", &["list"]);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let result: Result<String, &'static str> = Err("unsupported");

    match result {
        Ok(raw) => json!({
            "kind": "services",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": "ok",
            "collected_at": now_rfc3339(),
            "data": {"services": parse_service_rows(&raw)},
        }),
        Err(status) => json!({
            "kind": "services",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": status,
            "collected_at": now_rfc3339(),
            "data": {},
        }),
    }
}

fn parse_service_rows(raw: &str) -> Vec<Value> {
    let mut services = Vec::new();
    let mut current_name = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        #[cfg(windows)]
        if let Some(name) = trimmed.strip_prefix("SERVICE_NAME:") {
            current_name = name.trim().to_owned();
            continue;
        }
        #[cfg(windows)]
        if trimmed.starts_with("STATE") && !current_name.is_empty() {
            let state = trimmed
                .split_whitespace()
                .last()
                .unwrap_or("UNKNOWN")
                .to_owned();
            services.push(json!({
                "name": current_name,
                "display_name": current_name,
                "status": state,
                "start_type": "unknown",
            }));
            current_name.clear();
            continue;
        }
        #[cfg(not(windows))]
        {
            let fields = trimmed.split_whitespace().collect::<Vec<_>>();
            if fields.len() >= 3 && fields[0].ends_with(".service") {
                services.push(json!({
                    "name": fields[0],
                    "display_name": fields[0],
                    "status": fields[2],
                    "start_type": "unknown",
                }));
            } else if fields.len() >= 3 && fields[2].contains('.') {
                services.push(json!({
                    "name": fields[2],
                    "display_name": fields[2],
                    "status": fields[1],
                    "start_type": "unknown",
                }));
            }
        }
        if services.len() >= MAX_PROCESS_ROWS {
            break;
        }
    }
    services
}

fn collect_events() -> Value {
    #[cfg(windows)]
    let result = command_output("wevtutil.exe", &["qe", "System", "/c:100", "/f:text"]);
    #[cfg(target_os = "linux")]
    let result = command_output(
        "journalctl",
        &["-n", "100", "--no-pager", "-o", "short-iso"],
    );
    #[cfg(target_os = "macos")]
    let result = command_output("log", &["show", "--last", "1h", "--style", "compact"]);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let result: Result<String, &'static str> = Err("unsupported");

    match result {
        Ok(raw) => json!({
            "kind": "events",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": "ok",
            "collected_at": now_rfc3339(),
            "data": {"events": raw.lines().take(100).map(|line| json!({
                "time": "",
                "level": "info",
                "source": "",
                "message": line,
            })).collect::<Vec<_>>()},
        }),
        Err(status) => json!({
            "kind": "events",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": status,
            "collected_at": now_rfc3339(),
            "data": {},
        }),
    }
}

fn collect_activity() -> Value {
    let result = {
        #[cfg(windows)]
        {
            command_output(
                "powershell.exe",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "(Get-Process | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1 -ExpandProperty ProcessName)",
                ],
            )
        }
        #[cfg(target_os = "linux")]
        {
            command_output("xdotool", &["getactivewindow", "getwindowname"])
        }
        #[cfg(target_os = "macos")]
        {
            command_output(
                "osascript",
                &[
                    "-e",
                    "tell application \"System Events\" to get name of first application process whose frontmost is true",
                ],
            )
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Err("unsupported")
        }
    };
    match result {
        Ok(app_name) if !app_name.trim().is_empty() => json!({
            "kind": "activity",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": "ok",
            "collected_at": now_rfc3339(),
            "data": {"apps": [{"name": app_name.trim(), "seconds": 0}]},
        }),
        Ok(_) => json!({
            "kind": "activity",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": "unavailable",
            "collected_at": now_rfc3339(),
            "data": {},
        }),
        Err(status) => json!({
            "kind": "activity",
            "sample_id": uuid::Uuid::new_v4().to_string(),
            "status": status,
            "collected_at": now_rfc3339(),
            "data": {},
        }),
    }
}

fn browse_files(args: &Value) -> Value {
    let path = args.get("path").and_then(Value::as_str);
    let path = match safe_file_path(path) {
        Ok(path) => path,
        Err(status) => {
            return json!({"status": status, "data": {}});
        }
    };
    let mut entries = Vec::new();
    let read_dir = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(_) => return json!({"status": "permission_denied", "data": {}}),
    };
    for entry in read_dir.flatten().take(MAX_DIRECTORY_ROWS) {
        let entry_path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry_path.to_string_lossy(),
            "is_dir": metadata.is_dir(),
            "size": if metadata.is_file() { metadata.len() } else { 0 },
        }));
    }
    json!({
        "status": "ok",
        "data": {
            "path": path.to_string_lossy(),
            "entries": entries,
        },
    })
}

fn read_file(args: &Value) -> Value {
    let path = match safe_file_path(args.get("path").and_then(Value::as_str)) {
        Ok(path) => path,
        Err(status) => return json!({"status": status, "data": {}}),
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let requested = args
        .get("length")
        .and_then(Value::as_u64)
        .unwrap_or(64 * 1024)
        .min(MAX_FILE_READ as u64) as usize;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return json!({"status": "permission_denied", "data": {}}),
    };
    let start = (offset as usize).min(bytes.len());
    let end = start.saturating_add(requested).min(bytes.len());
    json!({
        "status": "ok",
        "data": {
            "path": path.to_string_lossy(),
            "offset": start,
            "total": bytes.len(),
            "data": STANDARD.encode(&bytes[start..end]),
        },
    })
}

fn service_control(args: &Value) -> Value {
    let name = match args.get("name").and_then(Value::as_str) {
        Some(name) if !name.is_empty() && name.len() <= 128 => name,
        _ => return json!({"status": "error", "code": "service_name_required"}),
    };
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
    {
        return json!({"status": "error", "code": "invalid_service_name"});
    }
    let action = args.get("action").and_then(Value::as_str).unwrap_or("");
    #[cfg(windows)]
    let result = match action {
        "start" => command_output("sc.exe", &["start", name]),
        "stop" => command_output("sc.exe", &["stop", name]),
        "restart" => {
            let _ = command_output("sc.exe", &["stop", name]);
            command_output("sc.exe", &["start", name])
        }
        "startup" => {
            let startup = match args.get("startup_type").and_then(Value::as_str) {
                Some("auto") => "auto",
                Some("manual") => "demand",
                Some("disabled") => "disabled",
                _ => return json!({"status": "error", "code": "invalid_startup_type"}),
            };
            command_output("sc.exe", &["config", name, "start=", startup])
        }
        _ => Err("unsupported"),
    };
    #[cfg(target_os = "linux")]
    let result = match action {
        "start" | "stop" | "restart" => command_output("systemctl", &[action, name]),
        "startup" => match args.get("startup_type").and_then(Value::as_str) {
            Some("enabled") => command_output("systemctl", &["enable", name]),
            Some("disabled") => command_output("systemctl", &["disable", name]),
            _ => Err("error"),
        },
        _ => Err("unsupported"),
    };
    #[cfg(target_os = "macos")]
    let result: Result<String, &'static str> = Err("unsupported");
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let result: Result<String, &'static str> = Err("unsupported");

    match result {
        Ok(output) => json!({"status": "ok", "data": {"output": output}}),
        Err(status) => json!({"status": status, "data": {}}),
    }
}

fn terminate_process(args: &Value) -> Value {
    let pid = match args.get("pid").and_then(Value::as_u64) {
        Some(pid) if pid > 0 && pid <= usize::MAX as u64 => pid as usize,
        _ => return json!({"status": "error", "code": "pid_required"}),
    };
    let mut system = hbb_common::sysinfo::System::new();
    system.refresh_processes();
    let process = match system.process(pid.into()) {
        Some(process) => process,
        None => return json!({"status": "unavailable", "code": "process_not_found"}),
    };
    if process.kill() {
        json!({"status": "ok", "data": {"pid": pid}})
    } else {
        json!({"status": "permission_denied", "data": {"pid": pid}})
    }
}

fn execute_command(command: &str, args: &Value) -> Value {
    match command {
        "collect.hardware" => json!({
            "snapshot": {
                "kind": "hardware",
                "sample_id": uuid::Uuid::new_v4().to_string(),
                "status": "ok",
                "collected_at": now_rfc3339(),
                "data": crate::common::get_sysinfo(),
            }
        }),
        "collect.metrics" => {
            let envelope = heartbeat();
            json!({
                "snapshot": {
                    "kind": "metrics",
                    "sample_id": envelope["sample_id"],
                    "status": envelope["status"]["metrics"],
                    "collected_at": envelope["collected_at"],
                    "data": envelope["metrics"],
                }
            })
        }
        "collect.services" => json!({"snapshot": collect_services()}),
        "collect.processes" => json!({"snapshot": collect_processes()}),
        "collect.events" => json!({"snapshot": collect_events()}),
        "collect.activity" => {
            let enabled = Config::get_option("telemetry-activity-enabled");
            if enabled != "Y" {
                json!({"status": "permission_denied", "code": "activity_opt_in_required", "data": {}})
            } else {
                json!({"snapshot": collect_activity()})
            }
        }
        "files.browse" => {
            let listing = browse_files(args);
            json!({
                "snapshot": {
                    "kind": "files",
                    "sample_id": uuid::Uuid::new_v4().to_string(),
                    "status": listing["status"],
                    "collected_at": now_rfc3339(),
                    "data": listing["data"],
                }
            })
        }
        "files.read" => read_file(args),
        "service.control" => service_control(args),
        "process.terminate" => terminate_process(args),
        _ => json!({"status": "unsupported", "code": "command_not_supported", "data": {}}),
    }
}

pub fn process_commands(commands: &[Value]) -> Vec<Value> {
    let mut results = Vec::new();
    for command in commands {
        let id = match command.get("id").and_then(Value::as_i64) {
            Some(id) if id > 0 => id,
            _ => continue,
        };
        {
            let mut seen = match SEEN_COMMANDS.lock() {
                Ok(seen) => seen,
                Err(_) => continue,
            };
            if seen.contains(&id) {
                continue;
            }
            seen.insert(id);
            if seen.len() > 256 {
                seen.clear();
                seen.insert(id);
            }
        }

        let command_name = command.get("command").and_then(Value::as_str).unwrap_or("");
        let args = command.get("args").cloned().unwrap_or_else(|| json!({}));
        let result = if config::is_incoming_only() && command_name != "collect.metrics" {
            json!({"status": "permission_denied", "code": "support_agent_restricted", "data": {}})
        } else {
            execute_command(command_name, &args)
        };
        let status = result
            .get("status")
            .and_then(Value::as_str)
            .or_else(|| result.get("snapshot").and_then(|_| Some("ok")))
            .unwrap_or("error");
        results.push(json!({
            "id": id,
            "status": status,
            "result": result,
        }));
    }
    if !results.is_empty() {
        log::debug!("BetterDesk telemetry commands completed: {}", results.len());
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_has_bounded_contract_fields() {
        let value = heartbeat();
        assert_eq!(value["telemetry_schema"], 1);
        assert!(value["sample_id"].as_str().is_some());
        assert!(value["metrics"].is_object());
        assert!(value["status"].is_object());
    }

    #[test]
    fn file_path_cannot_escape_home() {
        if home_directory().is_some() {
            assert!(safe_file_path(Some("..")).is_err());
        }
    }
}
