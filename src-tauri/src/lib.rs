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

fn extract_user_prompt_text(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("user") {
        return None;
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
        return None;
    }
    Some(trimmed.chars().take(200).collect())
}

fn extract_first_and_last_user_prompts(jsonl: &str) -> (Option<String>, Option<String>) {
    let mut first: Option<String> = None;
    let mut last: Option<String> = None;
    for line in jsonl.lines() {
        if let Some(p) = extract_user_prompt_text(line) {
            if first.is_none() {
                first = Some(p.clone());
            }
            last = Some(p);
        }
    }
    (first, last)
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
    #[serde(rename = "lastPrompt")]
    last_prompt: Option<String>,
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
            let (first_prompt, last_prompt) = fs::read_to_string(&path)
                .ok()
                .map(|raw| extract_first_and_last_user_prompts(&raw))
                .unwrap_or((None, None));
            items.push(SessionInfo {
                id,
                mtime,
                mtime_ms,
                size: meta.len(),
                first_prompt,
                last_prompt,
            });
        }
    }
    items.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    ListSessionsResult { encoded_dir, exists: true, items }
}

#[derive(Deserialize)]
struct FindSessionsByIdArgs {
    query: String,
    paths: Vec<String>,
}

#[derive(Serialize)]
struct FindSessionsByIdMatch {
    path: String,
    #[serde(rename = "sessionIds")]
    session_ids: Vec<String>,
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    #[serde(rename = "orphan")]
    orphan: bool,
}

#[derive(Serialize)]
struct FindSessionsByIdResult {
    items: Vec<FindSessionsByIdMatch>,
}

#[tauri::command]
fn find_sessions_by_id(args: FindSessionsByIdArgs) -> FindSessionsByIdResult {
    let q = args.query.trim().to_lowercase();
    if q.is_empty() {
        return FindSessionsByIdResult { items: Vec::new() };
    }
    let root = sessions_root();
    let mut encoded_to_path: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for p in &args.paths {
        encoded_to_path.insert(encode_project_path(p), p.clone());
    }
    let mut items: Vec<FindSessionsByIdMatch> = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return FindSessionsByIdResult { items },
    };
    for dir_entry in entries.flatten() {
        let dir = dir_entry.path();
        if !dir.is_dir() {
            continue;
        }
        let encoded = dir_entry.file_name().to_string_lossy().into_owned();
        let mut ids: Vec<String> = Vec::new();
        if let Ok(files) = fs::read_dir(&dir) {
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Some(stem) = fp.file_stem().and_then(|s| s.to_str()) {
                    if stem.to_lowercase().contains(&q) {
                        ids.push(stem.to_string());
                    }
                }
            }
        }
        if ids.is_empty() {
            continue;
        }
        ids.sort();
        let (path, orphan) = match encoded_to_path.get(&encoded) {
            Some(p) => (p.clone(), false),
            None => (encoded.clone(), true),
        };
        items.push(FindSessionsByIdMatch {
            path,
            session_ids: ids,
            encoded_dir: encoded,
            orphan,
        });
    }
    FindSessionsByIdResult { items }
}

fn plugins_root() -> PathBuf {
    home().join(".claude").join("plugins").join("marketplaces")
}

#[derive(Serialize)]
struct PluginHookEvent {
    name: String,
    matcher: Option<String>,
    #[serde(rename = "hookCount")]
    hook_count: usize,
    commands: Vec<String>,
}

#[derive(Serialize)]
struct PluginHook {
    marketplace: String,
    plugin: String,
    #[serde(rename = "pluginId")]
    plugin_id: String,
    description: Option<String>,
    events: Vec<PluginHookEvent>,
    #[serde(rename = "totalHooks")]
    total_hooks: usize,
    #[serde(rename = "hooksPath")]
    hooks_path: String,
}

#[derive(Serialize)]
struct ListPluginHooksResult {
    items: Vec<PluginHook>,
}

#[tauri::command]
fn list_plugin_hooks() -> ListPluginHooksResult {
    let root = plugins_root();
    let mut items: Vec<PluginHook> = Vec::new();
    let marketplaces = match fs::read_dir(&root) {
        Ok(m) => m,
        Err(_) => return ListPluginHooksResult { items },
    };
    for mkt in marketplaces.flatten() {
        let mkt_dir = mkt.path();
        if !mkt_dir.is_dir() { continue; }
        let mkt_name = mkt.file_name().to_string_lossy().into_owned();
        let plugins_dir = mkt_dir.join("plugins");
        if !plugins_dir.is_dir() { continue; }
        let plugins = match fs::read_dir(&plugins_dir) {
            Ok(p) => p,
            Err(_) => continue,
        };
        for plugin in plugins.flatten() {
            let plugin_dir = plugin.path();
            if !plugin_dir.is_dir() { continue; }
            let plugin_name = plugin.file_name().to_string_lossy().into_owned();
            let hooks_path = plugin_dir.join("hooks").join("hooks.json");
            if !hooks_path.is_file() { continue; }
            let raw = match fs::read_to_string(&hooks_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let parsed: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let description = parsed.get("description").and_then(|d| d.as_str()).map(String::from);
            let mut events: Vec<PluginHookEvent> = Vec::new();
            let mut total = 0usize;
            if let Some(hooks_obj) = parsed.get("hooks").and_then(|h| h.as_object()) {
                for (event_name, val) in hooks_obj {
                    if let Some(arr) = val.as_array() {
                        for matcher_entry in arr {
                            let matcher = matcher_entry.get("matcher")
                                .and_then(|m| m.as_str())
                                .map(String::from);
                            let mut commands = Vec::new();
                            if let Some(hook_arr) = matcher_entry.get("hooks").and_then(|h| h.as_array()) {
                                for h in hook_arr {
                                    if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                                        commands.push(cmd.to_string());
                                    }
                                }
                            }
                            total += commands.len();
                            events.push(PluginHookEvent {
                                name: event_name.clone(),
                                matcher,
                                hook_count: commands.len(),
                                commands,
                            });
                        }
                    }
                }
            }
            if events.is_empty() { continue; }
            let plugin_id = format!("{}@{}", plugin_name, mkt_name);
            items.push(PluginHook {
                marketplace: mkt_name.clone(),
                plugin: plugin_name,
                plugin_id,
                description,
                events,
                total_hooks: total,
                hooks_path: hooks_path.to_string_lossy().into_owned(),
            });
        }
    }
    items.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    ListPluginHooksResult { items }
}

#[derive(Serialize)]
struct MemoryFile {
    name: String,
    size: u64,
    mtime: String,
    #[serde(rename = "mtimeMs")]
    mtime_ms: i64,
    frontmatter: Map<String, Value>,
    #[serde(rename = "bodyPreview")]
    body_preview: String,
    #[serde(rename = "isIndex")]
    is_index: bool,
}

#[derive(Serialize)]
struct MemoryProject {
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    #[serde(rename = "dirPath")]
    dir_path: String,
    #[serde(rename = "dirMtimeMs")]
    dir_mtime_ms: i64,
    files: Vec<MemoryFile>,
}

#[derive(Serialize)]
struct ListMemoriesResult {
    projects: Vec<MemoryProject>,
}

#[tauri::command]
fn list_memories() -> ListMemoriesResult {
    let root = sessions_root();
    // Build encoded->canonical map.
    let mut encoded_to_path: HashMap<String, String> = HashMap::new();
    if let Ok(raw) = fs::read_to_string(config_path()) {
        if let Ok(cfg) = serde_json::from_str::<Value>(&raw) {
            if let Some(projects) = cfg.get("projects").and_then(|p| p.as_object()) {
                for (path, _) in projects {
                    encoded_to_path.insert(encode_project_path(path), path.clone());
                }
            }
        }
    }
    let mut projects = Vec::new();
    let dirs = match fs::read_dir(&root) {
        Ok(d) => d,
        Err(_) => return ListMemoriesResult { projects },
    };
    for entry in dirs.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() { continue; }
        let mem_dir = project_dir.join("memory");
        if !mem_dir.is_dir() { continue; }
        let encoded = entry.file_name().to_string_lossy().into_owned();
        let dir_mtime_ms = mem_dir.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64).unwrap_or(0);
        let mut files = Vec::new();
        if let Ok(items) = fs::read_dir(&mem_dir) {
            for f in items.flatten() {
                let p = f.path();
                let name = f.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') { continue; } // skip hidden / lock files
                if p.extension().and_then(|s| s.to_str()) != Some("md") { continue; }
                let raw = match fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let (fm, body) = parse_frontmatter(&raw);
                let meta = match f.metadata() { Ok(m) => m, Err(_) => continue };
                let mtime_ms = meta.modified().ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64).unwrap_or(0);
                let mtime = meta.modified().ok()
                    .map(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false))
                    .unwrap_or_default();
                let trimmed = body.trim();
                let preview: String = trimmed.chars().take(220).collect();
                files.push(MemoryFile {
                    name: name.clone(),
                    size: meta.len(),
                    mtime,
                    mtime_ms,
                    frontmatter: fm,
                    body_preview: preview,
                    is_index: name == "MEMORY.md",
                });
            }
        }
        if files.is_empty() { continue; }
        // Sort: index file last, others by mtime desc.
        files.sort_by(|a, b| {
            match (a.is_index, b.is_index) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => b.mtime_ms.cmp(&a.mtime_ms),
            }
        });
        let project_path = encoded_to_path.get(&encoded).cloned();
        projects.push(MemoryProject {
            project_path,
            encoded_dir: encoded,
            dir_path: mem_dir.to_string_lossy().into_owned(),
            dir_mtime_ms,
            files,
        });
    }
    projects.sort_by(|a, b| b.dir_mtime_ms.cmp(&a.dir_mtime_ms));
    ListMemoriesResult { projects }
}

#[derive(Deserialize)]
struct ReadMemoryArgs {
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    name: String,
}

#[derive(Serialize)]
struct ReadMemoryResult {
    raw: String,
    frontmatter: Map<String, Value>,
    body: String,
}

fn is_safe_memory_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && name.ends_with(".md")
}

#[tauri::command]
fn read_memory(args: ReadMemoryArgs) -> Result<ReadMemoryResult, String> {
    if !is_safe_memory_name(&args.name) {
        return Err("Invalid memory file name".into());
    }
    if args.encoded_dir.contains('/') || args.encoded_dir.contains("..") {
        return Err("Invalid project dir".into());
    }
    let p = sessions_root().join(&args.encoded_dir).join("memory").join(&args.name);
    if !p.is_file() {
        return Err("Memory not found".into());
    }
    let raw = fs::read_to_string(&p).map_err(|e| format!("read failed: {e}"))?;
    let (fm, body) = parse_frontmatter(&raw);
    Ok(ReadMemoryResult { raw, frontmatter: fm, body })
}

#[derive(Deserialize)]
struct DeleteMemoryArgs {
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    name: String,
}

#[tauri::command]
fn delete_memory(args: DeleteMemoryArgs) -> Result<Value, String> {
    if !is_safe_memory_name(&args.name) {
        return Err("Invalid memory file name".into());
    }
    if args.encoded_dir.contains('/') || args.encoded_dir.contains("..") {
        return Err("Invalid project dir".into());
    }
    let p = sessions_root().join(&args.encoded_dir).join("memory").join(&args.name);
    if !p.is_file() {
        return Err("Memory not found".into());
    }
    let _ = ensure_backup_dir();
    let backup_name = format!("memory-{}-{}-{}", args.encoded_dir, args.name, now_stamp());
    let _ = fs::copy(&p, backup_dir().join(&backup_name));
    fs::remove_file(&p).map_err(|e| format!("delete failed: {e}"))?;
    Ok(serde_json::json!({ "ok": true, "backup": backup_name }))
}

#[derive(Deserialize)]
struct OpenTerminalArgs {
    path: String,
}

#[tauri::command]
fn open_terminal(args: OpenTerminalArgs) -> Result<(), String> {
    let path = args.path;
    if path.is_empty() {
        return Err("Empty path".into());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal", &path])
            .spawn()
            .map_err(|e| format!("Failed to launch Terminal: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // `start cmd /K "cd /d <path>"` opens a new console at that directory.
        std::process::Command::new("cmd")
            .args(["/C", "start", "cmd", "/K", &format!("cd /d \"{}\"", path)])
            .spawn()
            .map_err(|e| format!("Failed to launch cmd: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Best-effort: try common terminal emulators in order.
        let attempts: Vec<(&str, Vec<String>)> = vec![
            ("gnome-terminal", vec![format!("--working-directory={}", path)]),
            ("konsole",        vec!["--workdir".into(), path.clone()]),
            ("xfce4-terminal", vec![format!("--working-directory={}", path)]),
            ("alacritty",      vec!["--working-directory".into(), path.clone()]),
            ("kitty",          vec!["--directory".into(), path.clone()]),
            ("tilix",          vec!["--working-directory".into(), path.clone()]),
            ("xterm",          vec!["-e".into(), format!("cd '{}' && exec $SHELL", path.replace('\'', "'\\''"))]),
        ];
        for (term, term_args) in attempts {
            if std::process::Command::new(term).args(&term_args).spawn().is_ok() {
                return Ok(());
            }
        }
        return Err("No supported terminal emulator found (tried gnome-terminal, konsole, xfce4-terminal, alacritty, kitty, tilix, xterm)".into());
    }

    #[allow(unreachable_code)]
    Err("Unsupported platform".into())
}

#[derive(Deserialize)]
struct DeleteSessionArgs {
    path: String,
    #[serde(rename = "sessionId")]
    session_id: String,
}

#[tauri::command]
fn delete_session(args: DeleteSessionArgs) -> Result<Value, String> {
    if !args.session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Invalid session id".into());
    }
    let encoded = encode_project_path(&args.path);
    let file = sessions_root().join(&encoded).join(format!("{}.jsonl", args.session_id));
    if !file.is_file() {
        return Err("Session file not found".into());
    }
    // Move to backup dir instead of hard delete, so a mistake is recoverable.
    let _ = ensure_backup_dir();
    let backup_name = format!("session-{}-{}.jsonl", args.session_id, now_stamp());
    let backup_path = backup_dir().join(&backup_name);
    let bytes = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    fs::copy(&file, &backup_path).map_err(|e| format!("backup failed: {e}"))?;
    fs::remove_file(&file).map_err(|e| format!("delete failed: {e}"))?;
    // If a sidecar dir exists (Claude Code sometimes stores per-session aux files), remove it too.
    let sidecar_dir = sessions_root().join(&encoded).join(&args.session_id);
    if sidecar_dir.is_dir() {
        let _ = fs::remove_dir_all(&sidecar_dir);
    }
    Ok(serde_json::json!({
        "ok": true,
        "backup": backup_name,
        "bytes": bytes,
    }))
}

#[derive(Deserialize)]
struct ListProjectImagesArgs {
    path: String,
    #[serde(rename = "maxImages")]
    max_images: Option<usize>,
}

#[derive(Serialize)]
struct ProjectImage {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "lineNo")]
    line_no: usize,
    #[serde(rename = "mtimeMs")]
    mtime_ms: i64,
    role: Option<String>,
    #[serde(rename = "mediaType")]
    media_type: String,
    data: String,
}

#[derive(Serialize)]
struct ListProjectImagesResult {
    images: Vec<ProjectImage>,
    truncated: bool,
    #[serde(rename = "sessionsScanned")]
    sessions_scanned: usize,
}

fn collect_images_from_line(
    line: &str,
    line_no: usize,
    session_id: &str,
    mtime_ms: i64,
    out: &mut Vec<ProjectImage>,
    limit: usize,
) -> bool {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let role = v.get("message").and_then(|m| m.get("role")).and_then(|r| r.as_str()).map(String::from);
    let content = match v.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return false,
    };
    let blocks = match content.as_array() {
        Some(a) => a,
        None => return false,
    };
    let mut stop = false;
    for b in blocks {
        if b.get("type").and_then(|t| t.as_str()) != Some("image") { continue; }
        let src = match b.get("source") {
            Some(s) => s,
            None => continue,
        };
        if src.get("type").and_then(|t| t.as_str()) != Some("base64") { continue; }
        let media_type = src.get("media_type").and_then(|t| t.as_str()).unwrap_or("image/png").to_string();
        let data = match src.get("data").and_then(|d| d.as_str()) {
            Some(d) => d.to_string(),
            None => continue,
        };
        out.push(ProjectImage {
            session_id: session_id.to_string(),
            line_no,
            mtime_ms,
            role: role.clone(),
            media_type,
            data,
        });
        if out.len() >= limit {
            stop = true;
            break;
        }
    }
    stop
}

#[tauri::command]
fn list_project_images(args: ListProjectImagesArgs) -> ListProjectImagesResult {
    let limit = args.max_images.unwrap_or(200);
    let encoded = encode_project_path(&args.path);
    let dir = sessions_root().join(&encoded);
    let mut images: Vec<ProjectImage> = Vec::new();
    let mut sessions_scanned = 0usize;
    let mut truncated = false;
    if !dir.is_dir() {
        return ListProjectImagesResult { images, truncated, sessions_scanned };
    }
    let mut files: Vec<(PathBuf, i64, String)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for f in entries.flatten() {
            let p = f.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") { continue; }
            let id = match p.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let meta = match f.metadata() { Ok(m) => m, Err(_) => continue };
            let mtime_ms = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64).unwrap_or(0);
            files.push((p, mtime_ms, id));
        }
    }
    // Newest sessions first so the freshest images appear first.
    files.sort_by(|a, b| b.1.cmp(&a.1));
    'outer: for (path, mtime_ms, id) in files {
        sessions_scanned += 1;
        let raw = match fs::read_to_string(&path) { Ok(s) => s, Err(_) => continue };
        for (idx, line) in raw.lines().enumerate() {
            if !line.contains("\"image\"") { continue; }
            let stop = collect_images_from_line(line, idx + 1, &id, mtime_ms, &mut images, limit);
            if stop {
                truncated = true;
                break 'outer;
            }
        }
    }
    ListProjectImagesResult { images, truncated, sessions_scanned }
}

#[derive(Deserialize)]
struct SearchSessionsArgs {
    query: String,
    #[serde(rename = "maxResults")]
    max_results: Option<usize>,
    #[serde(rename = "maxPerSession")]
    max_per_session: Option<usize>,
}

#[derive(Serialize)]
struct SearchSnippet {
    #[serde(rename = "lineNo")]
    line_no: usize,
    role: Option<String>,
    before: String,
    matched: String,
    after: String,
}

#[derive(Serialize)]
struct SearchHit {
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    #[serde(rename = "encodedDir")]
    encoded_dir: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    mtime: String,
    #[serde(rename = "mtimeMs")]
    mtime_ms: i64,
    snippets: Vec<SearchSnippet>,
}

#[derive(Serialize)]
struct SearchSessionsResult {
    hits: Vec<SearchHit>,
    #[serde(rename = "filesScanned")]
    files_scanned: usize,
    truncated: bool,
}

fn extract_text_content(line: &str) -> String {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return line.to_string(),
    };
    let content = match v.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return line.to_string(),
    };
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for b in arr {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                if !out.is_empty() {
                    out.push_str("\n");
                }
                out.push_str(t);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    line.to_string()
}

fn make_snippet(text: &str, lower_text: &str, q_lower: &str, line_no: usize, role: Option<String>) -> Option<SearchSnippet> {
    let pos = lower_text.find(q_lower)?;
    // map byte position from lower_text back to text (same length since to_lowercase mostly preserves byte len for ASCII)
    // safer: clamp on char boundaries of original text
    let mut start = pos;
    while start > 0 && !text.is_char_boundary(start) { start -= 1; }
    let mut end = pos + q_lower.len();
    while end < text.len() && !text.is_char_boundary(end) { end += 1; }
    let ctx = 80;
    let mut before_start = start.saturating_sub(ctx);
    while before_start > 0 && !text.is_char_boundary(before_start) { before_start -= 1; }
    let mut after_end = (end + ctx).min(text.len());
    while after_end < text.len() && !text.is_char_boundary(after_end) { after_end += 1; }
    Some(SearchSnippet {
        line_no,
        role,
        before: text[before_start..start].to_string(),
        matched: text[start..end].to_string(),
        after: text[end..after_end].to_string(),
    })
}

#[tauri::command]
fn search_sessions(args: SearchSessionsArgs) -> SearchSessionsResult {
    let query = args.query.trim();
    let max_results = args.max_results.unwrap_or(100);
    let max_per_session = args.max_per_session.unwrap_or(3);
    if query.is_empty() {
        return SearchSessionsResult { hits: Vec::new(), files_scanned: 0, truncated: false };
    }
    let q_lower = query.to_lowercase();

    // Build encoded->canonical map from .claude.json projects
    let mut encoded_to_path: HashMap<String, String> = HashMap::new();
    if let Ok(raw) = fs::read_to_string(config_path()) {
        if let Ok(cfg) = serde_json::from_str::<Value>(&raw) {
            if let Some(projects) = cfg.get("projects").and_then(|p| p.as_object()) {
                for (path, _) in projects {
                    encoded_to_path.insert(encode_project_path(path), path.clone());
                }
            }
        }
    }

    let root = sessions_root();
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut files_scanned = 0usize;
    let mut truncated = false;
    let dirs = match fs::read_dir(&root) {
        Ok(d) => d,
        Err(_) => return SearchSessionsResult { hits, files_scanned, truncated },
    };
    'outer: for dir_entry in dirs.flatten() {
        let dir_path = dir_entry.path();
        if !dir_path.is_dir() { continue; }
        let encoded_dir = dir_entry.file_name().to_string_lossy().into_owned();
        let project_path = encoded_to_path.get(&encoded_dir).cloned();
        let files = match fs::read_dir(&dir_path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") { continue; }
            files_scanned += 1;
            let raw = match fs::read_to_string(&p) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut snippets: Vec<SearchSnippet> = Vec::new();
            for (idx, line) in raw.lines().enumerate() {
                if snippets.len() >= max_per_session { break; }
                // Fast pre-check on the raw line (case-insensitive).
                if !line.to_lowercase().contains(&q_lower) { continue; }
                let v: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let role = v.get("message").and_then(|m| m.get("role")).and_then(|r| r.as_str()).map(String::from);
                // Skip non-conversation entries (snapshots, permission-mode markers, etc.)
                let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if typ != "user" && typ != "assistant" { continue; }
                let text = extract_text_content(line);
                let lower_text = text.to_lowercase();
                if let Some(s) = make_snippet(&text, &lower_text, &q_lower, idx + 1, role) {
                    snippets.push(s);
                }
            }
            if snippets.is_empty() { continue; }
            let id = match p.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let meta = match f.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = meta.modified().ok();
            let mtime_ms = modified.as_ref()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64).unwrap_or(0);
            let mtime = modified
                .map(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false))
                .unwrap_or_default();
            hits.push(SearchHit {
                project_path: project_path.clone(),
                encoded_dir: encoded_dir.clone(),
                session_id: id,
                mtime,
                mtime_ms,
                snippets,
            });
            if hits.len() >= max_results {
                truncated = true;
                break 'outer;
            }
        }
    }
    hits.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    SearchSessionsResult { hits, files_scanned, truncated }
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
            find_sessions_by_id,
            search_sessions,
            delete_session,
            list_project_images,
            open_terminal,
            list_memories,
            read_memory,
            delete_memory,
            list_plugin_hooks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
