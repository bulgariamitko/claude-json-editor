use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const MAX_BACKUPS: usize = 10;

fn home() -> PathBuf {
    dirs::home_dir().expect("home directory not found")
}
fn config_path() -> PathBuf {
    home().join(".claude.json")
}
fn backup_dir() -> PathBuf {
    home().join(".claude.json.backups")
}
fn settings_path() -> PathBuf {
    home().join(".claude").join("settings.json")
}
fn skills_dir() -> PathBuf {
    home().join(".claude").join("skills")
}
fn sessions_root() -> PathBuf {
    home().join(".claude").join("projects")
}

fn encode_project_path(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn extract_first_user_prompt(jsonl: &str) -> Option<String> {
    for line in jsonl.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let content = v.get("message").and_then(|m| m.get("content"))?;
        let text = if let Some(s) = content.as_str() {
            s.to_string()
        } else if let Some(arr) = content.as_array() {
            arr.iter()
                .find_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('<') {
            continue;
        }
        let truncated: String = trimmed.chars().take(200).collect();
        return Some(truncated);
    }
    None
}

fn ensure_backup_dir() -> std::io::Result<()> {
    let d = backup_dir();
    if !d.is_dir() {
        fs::create_dir_all(&d)?;
    }
    Ok(())
}

fn now_stamp() -> String {
    let now: DateTime<Local> = Local::now();
    let iso = now.to_rfc3339_opts(SecondsFormat::Secs, false);
    iso.replace([':', '.'], "-")
}

#[derive(Serialize)]
struct BackupItem {
    name: String,
    size: u64,
    mtime: String,
}

fn list_backups_internal() -> Vec<BackupItem> {
    let _ = ensure_backup_dir();
    let mut items = Vec::new();
    let d = backup_dir();
    if let Ok(entries) = fs::read_dir(&d) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("claude.json.bak-") {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false).into())
                .unwrap_or_default();
            items.push(BackupItem {
                name,
                size: meta.len(),
                mtime,
            });
        }
    }
    items.sort_by(|a, b| b.name.cmp(&a.name));
    items
}

fn prune_backups() {
    let items = list_backups_internal();
    for stale in items.iter().skip(MAX_BACKUPS) {
        let _ = fs::remove_file(backup_dir().join(&stale.name));
    }
}

fn backup_current() -> Option<String> {
    let cp = config_path();
    if !cp.is_file() {
        return None;
    }
    if ensure_backup_dir().is_err() {
        return None;
    }
    let name = format!("claude.json.bak-{}", now_stamp());
    let dest = backup_dir().join(&name);
    fs::copy(&cp, &dest).ok()?;
    prune_backups();
    Some(name)
}

fn backup_settings() -> Option<String> {
    let p = settings_path();
    if !p.is_file() {
        return None;
    }
    if ensure_backup_dir().is_err() {
        return None;
    }
    let name = format!("settings.json.bak-{}", now_stamp());
    let dest = backup_dir().join(&name);
    fs::copy(&p, &dest).ok()?;
    Some(name)
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn is_valid_backup_name(name: &str) -> bool {
    name.starts_with("claude.json.bak-")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '+' | '_'))
}

#[derive(Serialize)]
struct Meta {
    #[serde(rename = "configPath")]
    config_path: String,
    #[serde(rename = "backupDir")]
    backup_dir: String,
    #[serde(rename = "settingsPath")]
    settings_path: String,
    #[serde(rename = "skillsDir")]
    skills_dir: String,
    #[serde(rename = "maxBackups")]
    max_backups: usize,
    #[serde(rename = "appVersion")]
    app_version: String,
    #[serde(rename = "runtime")]
    runtime: String,
}

#[tauri::command]
fn meta() -> Meta {
    Meta {
        config_path: config_path().to_string_lossy().into_owned(),
        backup_dir: backup_dir().to_string_lossy().into_owned(),
        settings_path: settings_path().to_string_lossy().into_owned(),
        skills_dir: skills_dir().to_string_lossy().into_owned(),
        max_backups: MAX_BACKUPS,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime: format!("tauri {}", env!("CARGO_PKG_VERSION")),
    }
}

#[tauri::command]
fn load() -> Result<Value, String> {
    let p = config_path();
    if !p.is_file() {
        return Err(format!("Not found: {}", p.display()));
    }
    let raw = fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))?;
    serde_json::from_str::<Value>(&raw).map_err(|e| format!("Invalid JSON in source: {e}"))
}

#[derive(Serialize)]
struct SaveResult {
    ok: bool,
    backup: Option<String>,
    bytes: usize,
}

#[tauri::command]
fn save(config: Value) -> Result<SaveResult, String> {
    if !config.is_object() {
        return Err("Body must be a JSON object".into());
    }
    let backup = backup_current();
    let json = pretty_json(&config);
    fs::write(config_path(), &json).map_err(|e| format!("write failed: {e}"))?;
    Ok(SaveResult {
        ok: true,
        backup,
        bytes: json.len(),
    })
}

#[tauri::command]
fn check_paths(paths: Vec<String>) -> HashMap<String, bool> {
    let mut out = HashMap::with_capacity(paths.len());
    for p in paths {
        let exists = Path::new(&p).is_dir();
        out.insert(p, exists);
    }
    out
}

#[derive(Serialize)]
struct BackupsResult {
    dir: String,
    items: Vec<BackupItem>,
}

#[tauri::command]
fn backups() -> BackupsResult {
    BackupsResult {
        dir: backup_dir().to_string_lossy().into_owned(),
        items: list_backups_internal(),
    }
}

#[derive(Deserialize)]
struct RestoreArgs {
    name: String,
}

#[tauri::command]
fn restore(args: RestoreArgs) -> Result<Value, String> {
    if !is_valid_backup_name(&args.name) {
        return Err("Invalid backup name".into());
    }
    let src = backup_dir().join(&args.name);
    if !src.is_file() {
        return Err("Backup not found".into());
    }
    let raw = fs::read_to_string(&src).map_err(|e| format!("read failed: {e}"))?;
    let _: Value =
        serde_json::from_str(&raw).map_err(|e| format!("Backup is not valid JSON: {e}"))?;
    let _ = backup_current();
    fs::write(config_path(), &raw).map_err(|e| format!("write failed: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

#[derive(Serialize)]
struct SettingsLoadResult {
    exists: bool,
    settings: Value,
}

#[tauri::command]
fn settings_load() -> Result<SettingsLoadResult, String> {
    let p = settings_path();
    if !p.is_file() {
        return Ok(SettingsLoadResult {
            exists: false,
            settings: Value::Object(Map::new()),
        });
    }
    let raw = fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))?;
    let parsed: Value = if raw.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(&raw)
            .map_err(|e| format!("Invalid JSON in {}: {e}", p.display()))?
    };
    Ok(SettingsLoadResult {
        exists: true,
        settings: parsed,
    })
}

#[tauri::command]
fn settings_save(settings: Value) -> Result<SaveResult, String> {
    if !settings.is_object() {
        return Err("Body must be a JSON object".into());
    }
    let backup = backup_settings();
    let json = pretty_json(&settings);
    if let Some(parent) = settings_path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(settings_path(), &json).map_err(|e| format!("write failed: {e}"))?;
    Ok(SaveResult {
        ok: true,
        backup,
        bytes: json.len(),
    })
}

fn parse_frontmatter(raw: &str) -> (Map<String, Value>, String) {
    let mut fm = Map::new();
    if !raw.starts_with("---") {
        return (fm, raw.to_string());
    }
    let after_open = &raw[3..];
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    let close = match after_open.find("\n---") {
        Some(i) => i,
        None => return (fm, raw.to_string()),
    };
    let yaml = &after_open[..close];
    let mut rest = &after_open[close + 4..];
    rest = rest.strip_prefix('\r').unwrap_or(rest);
    rest = rest.strip_prefix('\n').unwrap_or(rest);
    for line in yaml.lines() {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_string();
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
                continue;
            }
            let mut val = line[idx + 1..].trim().to_string();
            if val.len() >= 2 {
                let bytes = val.as_bytes();
                if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
                    || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
                {
                    val = val[1..val.len() - 1].to_string();
                }
            }
            fm.insert(key, Value::String(val));
        }
    }
    (fm, rest.to_string())
}

#[derive(Serialize)]
struct SkillSummary {
    name: String,
    path: String,
    size: u64,
    mtime: String,
    frontmatter: Map<String, Value>,
    #[serde(rename = "bodyPreview")]
    body_preview: String,
    #[serde(rename = "bodyLength")]
    body_length: usize,
}

#[derive(Serialize)]
struct SkillsListResult {
    dir: String,
    items: Vec<SkillSummary>,
}

#[tauri::command]
fn skills_list() -> SkillsListResult {
    let dir = skills_dir();
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let skill_file = skill_dir.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            let raw = match fs::read_to_string(&skill_file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (frontmatter, body) = parse_frontmatter(&raw);
            let trimmed = body.trim();
            let preview: String = trimmed.chars().take(280).collect();
            let meta = entry.metadata().ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false))
                .unwrap_or_default();
            items.push(SkillSummary {
                name,
                path: skill_file.to_string_lossy().into_owned(),
                size: raw.len() as u64,
                mtime,
                frontmatter,
                body_preview: preview,
                body_length: body.len(),
            });
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    SkillsListResult {
        dir: dir.to_string_lossy().into_owned(),
        items,
    }
}

#[derive(Deserialize)]
struct SkillReadArgs {
    name: String,
}

#[derive(Serialize)]
struct SkillReadResult {
    name: String,
    path: String,
    frontmatter: Map<String, Value>,
    body: String,
    raw: String,
}

#[tauri::command]
fn skill_read(args: SkillReadArgs) -> Result<SkillReadResult, String> {
    if !is_valid_skill_name(&args.name) {
        return Err("Invalid skill name".into());
    }
    let p = skills_dir().join(&args.name).join("SKILL.md");
    if !p.is_file() {
        return Err("Skill not found".into());
    }
    let raw = fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))?;
    let (frontmatter, body) = parse_frontmatter(&raw);
    Ok(SkillReadResult {
        name: args.name,
        path: p.to_string_lossy().into_owned(),
        frontmatter,
        body,
        raw,
    })
}

#[derive(Deserialize)]
struct SkillWriteArgs {
    name: String,
    content: String,
}

#[derive(Serialize)]
struct SkillWriteResult {
    ok: bool,
    path: String,
    bytes: usize,
}

#[tauri::command]
fn skill_write(args: SkillWriteArgs) -> Result<SkillWriteResult, String> {
    if !is_valid_skill_name(&args.name) {
        return Err("Invalid skill name".into());
    }
    let skill_dir = skills_dir().join(&args.name);
    fs::create_dir_all(&skill_dir).map_err(|e| format!("mkdir failed: {e}"))?;
    let p = skill_dir.join("SKILL.md");
    if p.is_file() {
        let _ = ensure_backup_dir();
        let backup_name = format!("SKILL-{}-{}.md", args.name, now_stamp());
        let _ = fs::copy(&p, backup_dir().join(backup_name));
    }
    fs::write(&p, &args.content).map_err(|e| format!("write failed: {e}"))?;
    Ok(SkillWriteResult {
        ok: true,
        path: p.to_string_lossy().into_owned(),
        bytes: args.content.len(),
    })
}

#[derive(Deserialize)]
struct SkillDeleteArgs {
    name: String,
}

#[tauri::command]
fn skill_delete(args: SkillDeleteArgs) -> Result<Value, String> {
    if !is_valid_skill_name(&args.name) {
        return Err("Invalid skill name".into());
    }
    let skill_dir = skills_dir().join(&args.name);
    let skill_file = skill_dir.join("SKILL.md");
    if !skill_file.is_file() {
        return Err("Skill not found".into());
    }
    let _ = ensure_backup_dir();
    let backup_name = format!("SKILL-deleted-{}-{}.md", args.name, now_stamp());
    let _ = fs::copy(&skill_file, backup_dir().join(backup_name));
    fs::remove_file(&skill_file).map_err(|e| format!("delete failed: {e}"))?;
    let _ = fs::remove_dir(&skill_dir);
    Ok(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct ListSessionsArgs {
    path: String,
}

#[derive(Serialize)]
struct SessionInfo {
    id: String,
    mtime: String,
    #[serde(rename = "mtimeMs")]
    mtime_ms: i64,
    size: u64,
    #[serde(rename = "firstPrompt")]
    first_prompt: Option<String>,
}

#[derive(Serialize)]
struct ListSessionsResult {
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    exists: bool,
    items: Vec<SessionInfo>,
}

#[tauri::command]
fn list_sessions(args: ListSessionsArgs) -> ListSessionsResult {
    let encoded = encode_project_path(&args.path);
    let dir = sessions_root().join(&encoded);
    let encoded_dir = dir.to_string_lossy().into_owned();
    if !dir.is_dir() {
        return ListSessionsResult { encoded_dir, exists: false, items: Vec::new() };
    }
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = meta.modified().ok();
            let mtime_ms = modified
                .as_ref()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let mtime = modified
                .map(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false))
                .unwrap_or_default();
            let first_prompt = fs::read_to_string(&path)
                .ok()
                .and_then(|raw| extract_first_user_prompt(&raw));
            items.push(SessionInfo {
                id,
                mtime,
                mtime_ms,
                size: meta.len(),
                first_prompt,
            });
        }
    }
    items.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    ListSessionsResult { encoded_dir, exists: true, items }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            meta,
            load,
            save,
            check_paths,
            backups,
            restore,
            settings_load,
            settings_save,
            skills_list,
            skill_read,
            skill_write,
            skill_delete,
            list_sessions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
