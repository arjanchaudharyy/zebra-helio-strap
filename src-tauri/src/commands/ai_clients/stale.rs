//! Whether the zeppbridge-mcp registered in local AI clients is **this copy**, and whether its read scope is correct.
//!
//! Users who upgraded from 2.x usually registered the old toolkit's `zeppbridge-mcp` in Claude Desktop / Claude Code / Codex.
//! The old program cannot read the upgraded database, so the AI side only reports "cannot open database". The default now reads the full database,
//! so entries registered with `--scope task`, or not reconnected after the user turned on "shared tasks only", also need a warning to copy again.
//!
//! This module only reads these clients' config files on this device. It **returns only client names**: no paths, and it does not touch other config entries.
//! If a file cannot be read or parsed, treat it as not registered.

use serde_json::Value;
use std::path::{Path, PathBuf};

const MCP_PROGRAM: &str = "zeppbridge-mcp";

/// Where to find the configs of these clients. `appdata` exists only on Windows (the Claude Desktop config is there).
pub(crate) struct ClientConfigRoots {
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
}

impl ClientConfigRoots {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            home: var("USERPROFILE").or_else(|| var("HOME")),
            appdata: var("APPDATA"),
        }
    }
}

/// One registration: start command and arguments (all taken as strings).
#[derive(Debug, Default)]
struct Server {
    command: Option<String>,
    args: Vec<String>,
}

impl Server {
    fn strings(&self) -> impl Iterator<Item = &String> {
        self.command.iter().chain(self.args.iter())
    }

    /// The arguments include `--scope task` / `--scope=task`.
    fn task_scoped(&self) -> bool {
        self.args
            .windows(2)
            .any(|pair| pair[0] == "--scope" && pair[1] == "task")
            || self.args.iter().any(|arg| arg == "--scope=task")
    }
}

/// Services registered by each client, in the order `claude_desktop` / `claude_code` / `codex` / `opencode` / `grok` / `factory` / `qwen`.
fn registered(roots: &ClientConfigRoots) -> Vec<(&'static str, Vec<Server>)> {
    let mut out = Vec::new();
    let mut claude_desktop_dirs = Vec::new();
    if let Some(appdata) = &roots.appdata {
        claude_desktop_dirs.push(appdata.join("Claude"));
    }
    if let Some(home) = &roots.home {
        claude_desktop_dirs.push(
            home.join("Library")
                .join("Application Support")
                .join("Claude"),
        );
    }
    if !claude_desktop_dirs.is_empty() {
        let mut servers = Vec::new();
        for claude in claude_desktop_dirs {
            servers.extend(json_servers(
                &claude.join("claude_desktop_config.json"),
                &[],
            ));
            servers.extend(extension_servers(&claude.join("Claude Extensions")));
        }
        out.push(("claude_desktop", servers));
    }
    if let Some(home) = &roots.home {
        out.push((
            "claude_code",
            json_servers(&home.join(".claude.json"), &["projects"]),
        ));
        out.push((
            "codex",
            toml_servers(&home.join(".codex").join("config.toml")),
        ));
        out.push((
            "opencode",
            opencode_servers(&home.join(".config").join("opencode").join("opencode.json")),
        ));
        out.push((
            "grok",
            toml_servers(&home.join(".grok").join("config.toml")),
        ));
        out.push((
            "factory",
            json_servers(&home.join(".factory").join("mcp.json"), &[]),
        ));
        // Qwen Code user-level settings: `mcpServers.<name>` is `{command, args, env}`, the same format as Claude.
        out.push((
            "qwen",
            json_servers(&home.join(".qwen").join("settings.json"), &[]),
        ));
    }
    out
}

/// Clients that registered another zeppbridge-mcp.
pub(crate) fn stale_clients(roots: &ClientConfigRoots, sidecar: &Path) -> Vec<&'static str> {
    let current = normalize(&sidecar.to_string_lossy());
    registered(roots)
        .into_iter()
        .filter(|(_, servers)| {
            servers
                .iter()
                .flat_map(Server::strings)
                .filter(|text| mentions_program(text))
                .any(|text| normalize(text) != current)
        })
        .map(|(client, _)| client)
        .collect()
}

/// Registered this copy, but the read scope does not match the current setting (`task_only` is on without `--scope task`, or the reverse).
pub(crate) fn scope_stale_clients(
    roots: &ClientConfigRoots,
    sidecar: &Path,
    task_only: bool,
) -> Vec<&'static str> {
    let current = normalize(&sidecar.to_string_lossy());
    registered(roots)
        .into_iter()
        .filter(|(_, servers)| {
            servers.iter().any(|server| {
                server.strings().any(|text| normalize(text) == current)
                    && server.task_scoped() != task_only
            })
        })
        .map(|(client, _)| client)
        .collect()
}

/// For comparison: case, slash direction and quotes at both ends do not count as differences.
fn normalize(path: &str) -> String {
    path.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .replace('/', "\\")
        .to_lowercase()
}

fn mentions_program(text: &str) -> bool {
    text.to_lowercase().contains(MCP_PROGRAM)
}

fn read_json(file: &Path) -> Option<Value> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Each entry in one `mcpServers` object (`command` string + `args` array).
fn servers_in(servers: Option<&Value>, out: &mut Vec<Server>) {
    for server in servers
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|map| map.values())
    {
        out.push(Server {
            command: server
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_string),
            args: string_list(server.get("args")),
        });
    }
}

/// JSON config: the top-level `mcpServers`, and the `mcpServers` under each object in `nested`
/// (the Claude Code `.claude.json` adds one more level per project).
fn json_servers(file: &Path, nested: &[&str]) -> Vec<Server> {
    let Some(root) = read_json(file) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    servers_in(root.get("mcpServers"), &mut out);
    for key in nested {
        for scope in root
            .get(*key)
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|map| map.values())
        {
            servers_in(scope.get("mcpServers"), &mut out);
        }
    }
    out
}

/// OpenCode: `mcp.<name>` (old) or `mcp.servers.<name>` (v2). `command` is one array of "program + arguments".
fn opencode_servers(file: &Path) -> Vec<Server> {
    let Some(root) = read_json(file) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |entry: &Value| {
        let mut command = string_list(entry.get("command")).into_iter();
        if let Some(program) = command.next() {
            out.push(Server {
                command: Some(program),
                args: command.collect(),
            });
        }
    };
    for (name, entry) in root
        .get("mcp")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if name == "servers" && entry.get("command").is_none() {
            for server in entry.as_object().into_iter().flat_map(|map| map.values()) {
                push(server);
            }
        } else {
            push(entry);
        }
    }
    out
}

/// Extensions (`.mcpb`) installed in Claude Desktop: one directory per extension. In `manifest.json`,
/// `server.mcp_config` is the start command.
fn extension_servers(dir: &Path) -> Vec<Server> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(manifest) = read_json(&entry.path().join("manifest.json")) else {
            continue;
        };
        if let Some(config) = manifest.pointer("/server/mcp_config") {
            let mut servers = serde_json::Map::new();
            servers.insert(String::new(), config.clone());
            servers_in(Some(&Value::Object(servers)), &mut out);
        }
    }
    out
}

/// The `config.toml` of Codex / Grok: only reads the `command` / `args` lines in `[mcp_servers.*]` tables.
/// No TOML parser library. Only two formats are accepted: `"..."` (backslash escapes) and `'...'` (literal).
fn toml_servers(file: &Path) -> Vec<Server> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut out: Vec<Server> = Vec::new();
    let mut in_server = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_server = line.trim_start_matches('[').starts_with("mcp_servers.");
            if in_server {
                out.push(Server::default());
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(server) = out.last_mut().filter(|_| in_server) else {
            continue;
        };
        match key.trim() {
            "command" => server.command = toml_strings(value).into_iter().next(),
            "args" => server.args = toml_strings(value),
            _ => {}
        }
    }
    out
}

fn toml_strings(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut text = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => text.extend(chars.next()),
                        '"' => break,
                        other => text.push(other),
                    }
                }
                out.push(text);
            }
            '\'' => out.push(chars.by_ref().take_while(|&c| c != '\'').collect()),
            _ => {}
        }
    }
    out
}
