use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
use std::{env, path::{Path, PathBuf}};

/// Called from main before the application starts threads or resolves resources.
/// A bundle carries code/resources only; all mutable state lives in user storage.
/// Product version shown in the About window and by the installer.
pub const PRODUCT_VERSION: &str = "0.0.1-beta1";

// Assign this process to a job with KILL_ON_JOB_CLOSE so the OS terminates
// leftover children (language servers, agents, terminals) on any exit path.
// Upstream relies on drop-time kills, which Windows teardown does not always reach.
#[cfg(windows)]
fn reap_descendants_on_exit() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let Ok(job) = CreateJobObjectW(None, None) else { return };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .is_ok();
        let assigned = configured && AssignProcessToJobObject(job, GetCurrentProcess()).is_ok();
        if assigned {
            // Closing the handle now would terminate our own tree via KILL_ON_JOB_CLOSE.
            std::mem::forget(job);
        } else {
            let _ = CloseHandle(job);
        }
    }
}

pub fn configure() -> Result<Option<PathBuf>> {
    #[cfg(windows)]
    reap_descendants_on_exit();

    let executable = env::current_exe()?;
    let Some(contents) = executable.parent().and_then(Path::parent) else { return Ok(None); };
    let resources = contents.join("Resources");
    let root = resources.join("runtime");
    if !root.join("zepi-runtime.json").is_file() {
        // Dev layout: the exe lives under <root>/.local/native-target/<profile>/.
        // Expose the checkout as ZEPI_ROOT so the zepi UI activates without a wrapper.
        let dev_root = executable
            .parent()
            .and_then(|dir| std::fs::read_to_string(dir.join("zepi-root.txt")).ok())
            .map(|content| std::path::PathBuf::from(content.trim()))
            .filter(|root| root.join("packages/acp/src/cli.ts").is_file())
            .or_else(|| {
                executable
                    .ancestors()
                    .find(|dir| dir.join("packages/acp/src/cli.ts").is_file())
                    .map(std::path::PathBuf::from)
            });
        if let Some(dev_root) = dev_root {
            // Bundled node in <root>/deps/node and browsers in <root>/deps/browsers stay
            // with the executable; state location is resolved below.
            let exe_dir = executable.parent().expect("executable has a parent").to_path_buf();
            // Dev checkouts (zepi-root.txt next to the exe) keep state beside the exe.
            // An installed bundle stores state in LOCALAPPDATA so updates never
            // overwrite threads and settings; runtime payload stays with the exe.
            let dev_marker = exe_dir.join("zepi-root.txt").is_file();
            let state_root = if dev_marker {
                exe_dir.clone()
            } else {
                env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| exe_dir.clone())
                    .join("Zepi")
            };
            let zed_dir = if dev_marker {
                state_root.join(".zed")
            } else {
                state_root.join("zed")
            };
            let pi_dir = if dev_marker {
                state_root.join(".pi").join("agent")
            } else {
                state_root.join("pi")
            };
            if !dev_marker {
                let legacy = [
                    (exe_dir.join(".zed"), zed_dir.clone()),
                    (exe_dir.join(".pi").join("agent"), pi_dir.clone()),
                ];
                for (from, to) in legacy {
                    if from.is_dir() && !to.exists() {
                        std::fs::create_dir_all(to.parent().expect("state dir has a parent")).ok();
                        std::fs::rename(&from, &to).ok();
                    }
                }
            }
            std::fs::create_dir_all(&pi_dir).ok();
            let payload = payload_dir(&exe_dir);
            let bundled_node = payload.join("node").join("node.exe");
            // SAFETY: startup is single-threaded, before GPUI/runtime worker creation.
            unsafe {
                env::set_var("ZEPI_ROOT", &dev_root);
                if bundled_node.is_file() {
                    env::set_var("ZEPI_NODE", &bundled_node);
                    let mut search = vec![bundled_node.parent().expect("node.exe has a parent").to_path_buf()];
                    if let Some(path) = env::var_os("PATH") {
                        search.extend(env::split_paths(&path));
                    }
                    if let Ok(joined) = env::join_paths(search) {
                        env::set_var("PATH", joined);
                    }
                } else if env::var_os("ZEPI_NODE").is_none() {
                    if let Some(node) = env::var_os("PATH").and_then(|paths| {
                        env::split_paths(&paths)
                            .find_map(|dir| ["node.exe", "node"].iter().map(|name| dir.join(name)).find(|candidate| candidate.is_file()).map(|candidate| candidate.into_os_string()))
                    }) {
                        env::set_var("ZEPI_NODE", node);
                    }
                }
                env::set_var("ZEPI_PI_CONFIG_DIR", &pi_dir);
                env::set_var("ZEPI_INSTALLED_RUNTIME", "1");
                let browsers = payload.join("browsers");
                if browsers.is_dir() {
                    env::set_var("PLAYWRIGHT_BROWSERS_PATH", browsers);
                }
            }
            return Ok(Some(zed_dir));
        }
        return Ok(None);
    }
    let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("zepi-runtime.json"))?)?;
    ensure!(manifest["version"] == 1 && manifest["pi"] == "1.5.0", "Unsupported Zepi runtime manifest");
    let helpers = contents.join("Helpers");
    let node = helpers.join("node");
    ensure!(node.is_file() && root.join("packages/acp/src/cli.js").is_file(), "Bundled agent runtime is incomplete");
    let data = env::var_os("ZEPI_USER_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        paths::home_dir().join("Library/Application Support/Zepi")
    });
    let pi = env::var_os("ZEPI_PI_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| data.join("pi"));
    std::fs::create_dir_all(&pi)?;
    let mut search = vec![helpers, root.join("node_modules/.bin")];
    if let Some(path) = env::var_os("PATH") { search.extend(env::split_paths(&path)); }
    // SAFETY: startup is single-threaded, before GPUI/runtime worker creation.
    unsafe {
        env::set_var("ZEPI_ROOT", &root);
        env::set_var("ZEPI_NODE", &node);
        env::set_var("ZEPI_PI_CONFIG_DIR", &pi);
        env::set_var("PI_CODING_AGENT_DIR", &pi);
        env::set_var("ZEPI_NATIVE_SOURCE_ROOT", resources.join("editor"));
        env::set_var("ZEPI_INSTALLED_RUNTIME", "1");
        env::set_var("PLAYWRIGHT_BROWSERS_PATH", resources.join("browsers"));
        env::set_var("PATH", env::join_paths(search)?);
    }
    Ok(Some(data.join("native")))
}

pub fn prepare_settings(directory: &Path) -> Result<()> {
    let root = PathBuf::from(env::var_os("ZEPI_ROOT").context("Missing bundled runtime")?);
    let node = env::var("ZEPI_NODE")?;
    let pi = PathBuf::from(env::var_os("ZEPI_PI_CONFIG_DIR").context("Missing pi config dir")?);
    std::fs::create_dir_all(&pi)?;
    // Ship provider/model defaults with the repo; never overwrite user edits.
    for name in ["models.json", "settings.json"] {
        let target = pi.join(name);
        let source = root.join("native").join("pi-defaults").join(name);
        if !target.exists() && source.is_file() {
            std::fs::copy(source, target)?;
        }
    }
    sync_skills(&root, &pi)?;
    // PowerShell replaces bash, which needs Git for Windows, and the native file tools
    // keep search off the shell. An explicit entry from the operator wins.
    let pi_settings = pi.join("settings.json");
    if pi_settings.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&pi_settings) {
            if let Ok(mut value) = serde_json::from_str::<Value>(&raw) {
                let changed = match value.get_mut("defaultTools") {
                    None => {
                        value["defaultTools"] = Value::Array(REQUIRED_TOOLS.iter().map(|tool| json!(tool)).collect());
                        true
                    }
                    Some(list) => {
                        list.as_array_mut()
                            .is_some_and(|items| ensure_tools(items, REQUIRED_TOOLS))
                    }
                };
                if changed {
                    std::fs::write(&pi_settings, serde_json::to_vec_pretty(&value)?)?;
                }
            }
        }
    }
    let path = directory.join("config/settings.json");
    // Existing settings may contain JSONC and user formatting, so only the runtime node
    // path written by an earlier build is patched; everything else is left untouched.
    if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        let patched = raw.replace(STALE_NODE_PATH, CURRENT_NODE_PATH);
        if patched != raw {
            std::fs::write(&path, patched)?;
        }
        return Ok(());
    }
    let mut settings = json!({});
    let defaults: Value = serde_json::from_slice(&std::fs::read(root.join("native/appearance.json"))?)?;
    fn merge(target: &mut Value, defaults: &Value) {
        if let (Some(target), Some(defaults)) = (target.as_object_mut(), defaults.as_object()) {
            for (key, value) in defaults {
                if let Some(current) = target.get_mut(key) { merge(current, value); }
                else { target.insert(key.clone(), value.clone()); }
            }
        }
    }
    merge(&mut settings, &defaults);
    merge(&mut settings, &json!({"telemetry":{"metrics":false,"diagnostics":false},"auto_update":false,
        "disable_ai":false,"auto_install_extensions":{"html":false}}));
    // office viewer knobs, shared by the slides and the future word viewer
    merge(&mut settings, &json!({"zepi":{"officeViewer":{"reloadDebounceMs":600,
        "rasterCacheCap":12,"maxRasterPixels":6000000,"slideGap":14,"zoomStep":10}}}));
    settings["node"] = json!({"path":node,"ignore_system_version":false});
    settings["agent_servers"]["zepi-pi"] = json!({"type":"custom","command":node,
        "args":[root.join("packages/acp/src/cli.js")],"env":{"ZEPI_ROOT":root,"PI_CODING_AGENT_DIR":env::var("ZEPI_PI_CONFIG_DIR")?}});
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&settings)?)?;
    std::fs::rename(temporary, path)?;
    // user-editable overrides for the slides viewer hotkeys
    let keymap = directory.join("config/keymap.json");
    if !keymap.exists() {
        std::fs::write(
            &keymap,
            serde_json::to_vec_pretty(&json!([
                {"context": "OfficeView", "bindings": {
                    "up": "office_viewer::PagePrev",
                    "down": "office_viewer::PageNext",
                    "ctrl-f": "office_viewer::ToggleSearch",
                    "f3": "office_viewer::SearchNext",
                    "shift-f3": "office_viewer::SearchPrev",
                    "ctrl-c": "office_viewer::CopySelection",
                    "escape": "office_viewer::ClearSelection",
                    "ctrl-enter": "office_viewer::SendSlideText",
                    "ctrl-shift-enter": "office_viewer::SendRegionImage",
                    "ctrl-shift-c": "office_viewer::CopyRegionImage"
                }},
                {"context": "SheetView", "bindings": {
                    "ctrl-s": "sheet_viewer::Save",
                    "enter": "sheet_viewer::EditCell",
                    "escape": "sheet_viewer::CancelEdit",
                    "up": "sheet_viewer::MoveUp",
                    "down": "sheet_viewer::MoveDown",
                    "left": "sheet_viewer::MoveLeft",
                    "right": "sheet_viewer::MoveRight",
                    "ctrl-=": "sheet_viewer::ZoomIn",
                    "ctrl--": "sheet_viewer::ZoomOut",
                    "ctrl-f": "sheet_viewer::SheetSearch"
                }},
                {"context": "SheetSearch", "bindings": {
                    "enter": "sheet_viewer::SheetSearchNext",
                    "escape": "sheet_viewer::SheetSearch"
                }},
                {"context": "SheetView", "bindings": {
                    "delete": "sheet_viewer::SheetClear",
                    "ctrl-0": "sheet_viewer::SheetZoomReset",
                    "ctrl-a": "sheet_viewer::SheetSelectAll",
                    "ctrl-c": "sheet_viewer::SheetCopy",
                    "ctrl-x": "sheet_viewer::SheetCut",
                    "ctrl-v": "sheet_viewer::SheetPaste",
                    "ctrl-z": "sheet_viewer::SheetUndo",
                    "ctrl-y": "sheet_viewer::SheetRedo",
                    "ctrl-enter": "sheet_viewer::SheetSend"
                }},
                {"context": "SheetEditing", "bindings": {
                    "enter": "sheet_viewer::CommitEdit",
                    "alt-enter": "sheet_viewer::SheetEditNewline",
                    "ctrl-s": "sheet_viewer::Save",
                    "escape": "sheet_viewer::CancelEdit"
                }},
                {"context": "OfficeSearch", "bindings": {
                    "escape": "office_viewer::ToggleSearch",
                    "enter": "office_viewer::SearchNext"
                }}
            ]))?
            .to_vec(),
        )?;
    }
    Ok(())
}

// Payload paths as they appear inside settings.json (backslashes are JSON-escaped).
/// Copies bundled skills into the user's pi skills directory whenever their content
/// changes; the whole `zepi` subtree is replaced, so edits made there are lost.
fn sync_skills(root: &Path, pi: &Path) -> Result<()> {
    let source = root.join("native/pi-defaults/skills");
    if !source.is_dir() {
        return Ok(());
    }
    let target = pi.join("skills/zepi");
    let stamp = format!("{:016x}", digest(&source)?);
    if std::fs::read_to_string(target.join(".stamp")).is_ok_and(|old| old.trim() == stamp) {
        return Ok(());
    }
    if target.is_dir() {
        std::fs::remove_dir_all(&target)?;
    }
    copy_dir(&source, &target).context("copying bundled skills")?;
    std::fs::write(target.join(".stamp"), stamp)?;
    Ok(())
}

fn digest(dir: &Path) -> std::io::Result<u64> {
    fn mix(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn walk(base: &Path, dir: &Path, hash: &mut u64) -> std::io::Result<()> {
        let mut names: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        names.sort();
        for name in names {
            let path = dir.join(&name);
            mix(hash, path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/").as_bytes());
            if path.is_dir() {
                walk(base, &path, hash)?;
            } else {
                mix(hash, &std::fs::read(&path)?);
            }
        }
        Ok(())
    }
    let mut hash = 0xcbf2_9ce4_8422_2325;
    walk(dir, dir, &mut hash)?;
    Ok(hash)
}

/// Tool selection Zepi expects for the agent; names the operator already mentions are left alone.
const REQUIRED_TOOLS: [&str; 6] = ["-bash", "+powershell", "+grep", "+find", "+ls", "+codemode"];

fn ensure_tools(items: &mut Vec<Value>, required: [&str; 6]) -> bool {
    let mut changed = false;
    for tool in required {
        let name = tool.trim_start_matches(['+', '-']);
        let mentioned = items.iter().any(|item| {
            item.as_str()
                .is_some_and(|entry| entry.trim_start_matches(['+', '-']) == name)
        });
        if !mentioned {
            items.push(json!(tool));
            changed = true;
        }
    }
    changed
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

const STALE_NODE_PATH: &str = "\\\\.pi\\\\node\\\\node.exe";
const CURRENT_NODE_PATH: &str = "\\\\deps\\\\node\\\\node.exe";

/// Bundled runtime payload next to the executable. Older builds kept it in a pi-named
/// `.pi` directory, so its subdirectories are moved to `deps` on first start.
fn payload_dir(exe_dir: &Path) -> PathBuf {
    let payload = exe_dir.join("deps");
    let legacy = exe_dir.join(".pi");
    for name in ["node", "browsers"] {
        let (from, to) = (legacy.join(name), payload.join(name));
        if from.is_dir() && !to.exists() {
            std::fs::create_dir_all(&payload).ok();
            std::fs::rename(&from, &to).ok();
        }
    }
    std::fs::remove_dir(&legacy).ok();
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_dir_moves_bundled_assets_and_keeps_legacy_state() {
        let root = std::env::temp_dir().join(format!("zepi-payload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [".pi/node", ".pi/browsers", ".pi/agent"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }

        let payload = payload_dir(&root);

        assert_eq!(payload, root.join("deps"));
        assert!(payload.join("node").is_dir());
        assert!(payload.join("browsers").is_dir());
        assert!(root.join(".pi/agent").is_dir(), "legacy pi state must stay in place");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_skills_installs_bundled_skills_once_per_version() {
        let root = std::env::temp_dir().join(format!("zepi-skills-{}", std::process::id()));
        let pi = root.join("pi");
        let source = root.join("bundle/native/pi-defaults/skills");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(source.join("pptx")).unwrap();
        std::fs::write(source.join("pptx/SKILL.md"), "bundled").unwrap();

        sync_skills(&root.join("bundle"), &pi).unwrap();
        assert_eq!(std::fs::read_to_string(pi.join("skills/zepi/pptx/SKILL.md")).unwrap(), "bundled");

        std::fs::write(pi.join("skills/zepi/pptx/SKILL.md"), "local edit").unwrap();
        sync_skills(&root.join("bundle"), &pi).unwrap();
        assert_eq!(
            std::fs::read_to_string(pi.join("skills/zepi/pptx/SKILL.md")).unwrap(),
            "local edit",
            "unchanged bundle must not overwrite local edits"
        );

        std::fs::write(source.join("pptx/SKILL.md"), "bundled v2").unwrap();
        sync_skills(&root.join("bundle"), &pi).unwrap();
        assert_eq!(std::fs::read_to_string(pi.join("skills/zepi/pptx/SKILL.md")).unwrap(), "bundled v2");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_tools_adds_missing_tools_and_respects_explicit_choices() {
        let mut items = vec![json!("+bash"), json!("-codemode")];

        assert!(ensure_tools(&mut items, REQUIRED_TOOLS));
        let entries: Vec<&str> = items.iter().filter_map(|value| value.as_str()).collect();
        assert!(entries.contains(&"+powershell") && entries.contains(&"+grep"));
        assert!(!entries.contains(&"-bash"), "an explicit +bash must win over -bash");
        assert!(!entries.contains(&"+codemode"), "an explicit -codemode must stay in place");
        assert!(!ensure_tools(&mut items, REQUIRED_TOOLS), "a second pass must not change anything");
    }
}
