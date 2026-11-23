use super::*;
use std::io::Read;

/// One empty directory per test case. Do not add tempfile for these three cases.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("zb-ai-clients-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// When no sidecar ships, give no path. The UI then shows "this version does not include it", and does not build a command
/// that points to a missing file for the user to run into.
#[test]
#[cfg(not(all(windows, feature = "store")))]
fn missing_sidecar_gives_no_path() {
    let dir = Scratch::new("missing");
    let found = locate_sidecar(dir.path(), &dir.path().join("data"));
    assert!(found.path.is_none());
    assert!(found.data_dir_env.is_none());
    assert!(found.cli_path.is_none());
}

/// The CLI and the MCP are found separately: if only the CLI ships, its path is still given.
#[test]
#[cfg(not(all(windows, feature = "store")))]
fn the_cli_is_found_on_its_own() {
    let dir = Scratch::new("cli");
    let cli = dir
        .path()
        .join(format!("zeppbridge-cli{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&cli, b"stub").unwrap();
    let found = locate_sidecar(dir.path(), &dir.path().join("data"));
    assert!(found.path.is_none());
    assert_eq!(
        found.cli_path.as_deref(),
        Some(cli.to_string_lossy().as_ref())
    );
}

/// No environment variable when the app database is next to the sidecar. When the database is elsewhere (dev build, fallback directory),
/// it must be set. Otherwise the AI reads an empty database and thinks there is "no data".
#[test]
#[cfg(not(all(windows, feature = "store")))]
fn data_dir_env_only_when_the_library_is_elsewhere() {
    let dir = Scratch::new("env");
    let exe = dir
        .path()
        .join(format!("zeppbridge-mcp{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&exe, b"stub").unwrap();

    let beside = locate_sidecar(dir.path(), &dir.path().join("data"));
    assert_eq!(beside.path.as_deref(), Some(exe.to_string_lossy().as_ref()));
    assert!(beside.data_dir_env.is_none());

    let elsewhere = dir.path().join("elsewhere");
    let moved = locate_sidecar(dir.path(), &elsewhere);
    assert_eq!(
        moved.data_dir_env.as_deref(),
        Some(elsewhere.to_string_lossy().as_ref())
    );
}

/// The package really has the manifest and the file that entry_point points to. The start command points to the copy in the install directory.
#[test]
fn bundle_carries_manifest_and_entry_point() {
    let dir = Scratch::new("bundle");
    let exe = dir.path().join("zeppbridge-mcp.exe");
    std::fs::write(&exe, b"binary").unwrap();
    let bytes = build_bundle(&exe, Some("D:\\data"), false).unwrap();

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    let entry = manifest["server"]["entry_point"]
        .as_str()
        .unwrap()
        .to_string();
    let mut binary = Vec::new();
    archive
        .by_name(&entry)
        .unwrap()
        .read_to_end(&mut binary)
        .unwrap();
    assert_eq!(binary, b"binary");
    let config = &manifest["server"]["mcp_config"];
    assert_eq!(config["command"], exe.to_string_lossy().as_ref());
    // By default, read the full database: no --scope.
    assert_eq!(config["args"], serde_json::json!([]));
    let task = bundle_manifest("x", None, true);
    assert_eq!(
        task["server"]["mcp_config"]["args"],
        serde_json::json!(["--scope", "task"])
    );
    assert_eq!(config["env"]["ZEPPBRIDGE_DATA_DIR"], "D:\\data");
}

#[cfg(all(windows, feature = "store"))]
#[test]
fn store_mcp_uses_its_unique_windowsapps_alias_for_both_commands() {
    let data_dir = Path::new(
        r"C:\Users\example\AppData\Local\Packages\LincStudio.ZeppBridge_abcd\LocalState\ZeppBridge\data",
    );
    let sidecar = current_sidecar(data_dir);
    let expected_dir = zeppbridge_core::paths::windows_apps_alias_dir().unwrap();
    let expected_mcp = expected_dir.join("zeppbridge-store-mcp.exe");
    let expected_cli = expected_dir.join("zeppbridge-store-cli.exe");
    assert_eq!(
        sidecar.path.as_deref(),
        Some(expected_mcp.to_string_lossy().as_ref())
    );
    assert_eq!(
        sidecar.cli_path.as_deref(),
        Some(expected_cli.to_string_lossy().as_ref())
    );
    assert_eq!(
        sidecar.data_dir_env.as_deref(),
        Some(data_dir.to_string_lossy().as_ref())
    );

    let manifest = bundle_manifest(
        sidecar.path.as_deref().unwrap(),
        sidecar.data_dir_env.as_deref(),
        false,
    );
    assert_eq!(
        manifest["server"]["mcp_config"]["command"],
        expected_mcp.to_string_lossy().as_ref()
    );
    assert_eq!(
        manifest["server"]["mcp_config"]["env"]["ZEPPBRIDGE_DATA_DIR"],
        data_dir.to_string_lossy().as_ref()
    );
}

/// Users who upgraded from 2.x registered the zeppbridge-mcp from the old toolkit in their AI clients. The three clients each use one format.
/// An entry that points to the current sidecar does not count (also with different case or slash direction). A project path that does not name zeppbridge-mcp does not count.
#[test]
fn stale_clients_are_the_ones_pointing_at_another_mcp_program() {
    use super::stale::{stale_clients, ClientConfigRoots};
    let dir = Scratch::new("stale");
    let home = dir.path().join("home");
    let appdata = dir.path().join("appdata");
    let sidecar = dir.path().join("app").join("zeppbridge-mcp.exe");
    let roots = ClientConfigRoots {
        home: Some(home.clone()),
        appdata: Some(appdata.clone()),
    };
    assert!(stale_clients(&roots, &sidecar).is_empty(), "nothing installed");

    // Claude Code: an old path registered at project level; the user level points to the current sidecar (with changed case and slashes).
    std::fs::create_dir_all(&home).unwrap();
    let current = sidecar.to_string_lossy().to_uppercase().replace('\\', "/");
    let claude_json = serde_json::json!({
        "mcpServers": { "zeppbridge": { "command": current, "args": ["--scope", "task"] } },
        "projects": { "C:/work/zeppbridge": { "mcpServers": {
            "zb": { "command": "cmd", "args": ["/c", "D:\\tools\\ZeppBridge-2.4\\zeppbridge-mcp.exe"] }
        } } }
    });
    std::fs::write(home.join(".claude.json"), claude_json.to_string()).unwrap();
    // Codex: only the project trust table names zeppbridge, which does not count.
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex").join("config.toml"),
        "[projects.'c:\\users\\me\\zeppbridge']\ntrust_level = \"trusted\"\n",
    )
    .unwrap();
    assert_eq!(stale_clients(&roots, &sidecar), ["claude_code"]);

    // Codex registered an old path (TOML double quotes with escapes); Claude Desktop installed an extension that points to an old path.
    std::fs::write(
        home.join(".codex").join("config.toml"),
        "[mcp_servers.zeppbridge]\ncommand = \"D:\\\\old\\\\zeppbridge-mcp.exe\"\nargs = [\"--scope\", \"task\"]\n",
    )
    .unwrap();
    let extension = appdata
        .join("Claude")
        .join("Claude Extensions")
        .join("local.mcpb.zeppbridge");
    std::fs::create_dir_all(&extension).unwrap();
    std::fs::write(
        extension.join("manifest.json"),
        serde_json::json!({ "server": { "mcp_config": { "command": "D:/old/zeppbridge-mcp.exe" } } })
            .to_string(),
    )
    .unwrap();
    assert_eq!(
        stale_clients(&roots, &sidecar),
        ["claude_desktop", "claude_code", "codex"]
    );

    // After all of them change to the current copy, none is reported.
    let toml_path = sidecar.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        home.join(".codex").join("config.toml"),
        format!("[mcp_servers.zeppbridge]\ncommand = \"{toml_path}\"\n"),
    )
    .unwrap();
    std::fs::remove_dir_all(appdata.join("Claude")).unwrap();
    std::fs::write(home.join(".claude.json"), "{\"mcpServers\":{}}").unwrap();
    assert!(stale_clients(&roots, &sidecar).is_empty());
}

/// By default, read the full database: clients that point to this copy but whose read scope does not match the switch must get a reconnect warning.
/// Also detects OpenCode (v2 `mcp.servers`), Grok (same TOML as Codex), Factory and Qwen Code (`mcpServers`).
#[test]
fn scope_stale_clients_are_the_ones_registered_with_the_other_scope() {
    use super::stale::{scope_stale_clients, stale_clients, ClientConfigRoots};
    let dir = Scratch::new("scope-stale");
    let home = dir.path().join("home");
    let sidecar = dir.path().join("app").join("zeppbridge-mcp.exe");
    let current = sidecar.to_string_lossy().to_string();
    let roots = ClientConfigRoots {
        home: Some(home.clone()),
        appdata: None,
    };
    std::fs::create_dir_all(home.join(".grok")).unwrap();
    std::fs::create_dir_all(home.join(".config").join("opencode")).unwrap();
    std::fs::create_dir_all(home.join(".factory")).unwrap();
    std::fs::create_dir_all(home.join(".qwen")).unwrap();
    std::fs::write(
        home.join(".claude.json"),
        serde_json::json!({ "mcpServers": { "zeppbridge": { "command": current, "args": ["--scope", "task"] } } })
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        home.join(".grok").join("config.toml"),
        format!(
            "[mcp_servers.zeppbridge]\ncommand = '{current}'\nargs = []\n\n[mcp_servers.zeppbridge.env]\nX = \"1\"\n"
        ),
    )
    .unwrap();
    std::fs::write(
        home.join(".config").join("opencode").join("opencode.json"),
        serde_json::json!({ "mcp": { "servers": { "zeppbridge": {
            "type": "local", "command": [current, "--scope=task"]
        } } } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        home.join(".factory").join("mcp.json"),
        serde_json::json!({ "mcpServers": { "zeppbridge": {
            "type": "stdio", "command": "D:/old/zeppbridge-mcp.exe", "args": []
        } } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        home.join(".qwen").join("settings.json"),
        serde_json::json!({ "mcpServers": { "zeppbridge": { "command": current, "args": [] } } })
            .to_string(),
    )
    .unwrap();
    assert_eq!(stale_clients(&roots, &sidecar), ["factory"]);
    assert_eq!(
        scope_stale_clients(&roots, &sidecar, false),
        ["claude_code", "opencode"]
    );
    assert_eq!(
        scope_stale_clients(&roots, &sidecar, true),
        ["grok", "qwen"]
    );

    // macOS: the Claude Desktop config is under ~/Library/Application Support/Claude (no APPDATA).
    let mac_claude = home
        .join("Library")
        .join("Application Support")
        .join("Claude");
    std::fs::create_dir_all(&mac_claude).unwrap();
    std::fs::write(
        mac_claude.join("claude_desktop_config.json"),
        serde_json::json!({ "mcpServers": { "zeppbridge": {
            "command": current, "args": ["--scope", "task"]
        } } })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        scope_stale_clients(&roots, &sidecar, false),
        ["claude_desktop", "claude_code", "opencode"]
    );
}
