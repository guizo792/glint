//! Extension host. An extension is a program in its own folder, speaking
//! line-delimited JSON-RPC on stdin and stdout. See guide/extensions.md.
//!
//! What this protects against: an extension that hangs, crashes or writes garbage cannot affect
//! Glint, sees only the text typed after its own trigger word, and can only ask Glint for the
//! effects its manifest declares. What it cannot protect against: an extension is a normal
//! program running with the user's rights, so it can do anything the user can. That is why
//! nothing runs until the user enables it.
use glint_core::engine::{Item, ItemKind, Provider, Target};
use glint_core::query::{Mode, Query};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

const QUERY_TIMEOUT: Duration = Duration::from_millis(800);
const RUN_TIMEOUT: Duration = Duration::from_secs(5);
/// The first answer after a start includes launching the program, which can take well over the
/// normal limit on a cold or busy machine (a Node script took more than 800 ms on a CI runner).
const STARTUP_ALLOWANCE: Duration = Duration::from_secs(5);
const MAX_LINE: u64 = 1024 * 1024;
const MAX_ITEMS: usize = 50;
/// Failures in a row before an extension is left stopped until reload.
const MAX_FAILURES: u32 = 3;
const SCORE: u32 = 9 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// First word typed to reach this extension.
    pub trigger: String,
    /// Program and arguments, run with the extension's folder as working directory.
    pub command: Vec<String>,
    /// `clipboard` and `open_url` are enforced by Glint. Others, such as `network` or
    /// `filesystem`, are declarations shown to the user that Glint cannot enforce.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !id_ok {
            return Err("id must use only a-z, 0-9 and -".to_string());
        }
        if self.trigger.is_empty() || self.trigger.contains(char::is_whitespace) {
            return Err("trigger must be one word".to_string());
        }
        if self.command.is_empty() {
            return Err("command must not be empty".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub folder: String,
    pub manifest: Manifest,
    pub enabled: bool,
    /// Why the extension stopped, if it did.
    pub error: Option<String>,
}

/// Something an extension asks Glint to do.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    Copy { text: String },
    OpenUrl { url: String },
}

#[derive(Deserialize)]
struct WireItem {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: String,
}

/// A running extension process.
struct Running {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Slot {
    running: Option<Running>,
    failures: u32,
    error: Option<String>,
}

struct Extension {
    folder: PathBuf,
    manifest: Manifest,
    slot: Mutex<Slot>,
}

impl Extension {
    fn spawn(&self) -> Result<Running, String> {
        let program = &self.manifest.command[0];
        // A relative path means inside the extension's folder, not Glint's working directory.
        let program_path = if program.contains(['/', '\\']) {
            self.folder.join(program)
        } else {
            PathBuf::from(program)
        };
        let mut command = Command::new(program_path);
        command
            .args(&self.manifest.command[1..])
            .current_dir(&self.folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut buf = Vec::new();
                let read = reader.by_ref().take(MAX_LINE).read_until(b'\n', &mut buf);
                // End of output, or a line too long to be a reply: treat as closed.
                let too_long = buf.len() as u64 == MAX_LINE && buf.last() != Some(&b'\n');
                if read.map_or(true, |n| n == 0) || too_long {
                    return;
                }
                if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                    return;
                }
            }
        });
        Ok(Running {
            child,
            stdin,
            lines,
            next_id: 1,
        })
    }

    /// Send one request. Any failure stops the process; it restarts on the next call, up to
    /// [`MAX_FAILURES`] in a row.
    fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let mut slot = self.slot.lock().unwrap();
        if slot.failures >= MAX_FAILURES {
            return Err(slot.error.clone().unwrap_or_else(|| "stopped".to_string()));
        }
        let mut timeout = timeout;
        if slot.running.is_none() {
            timeout = timeout.max(STARTUP_ALLOWANCE);
            match self.spawn() {
                Ok(running) => slot.running = Some(running),
                Err(e) => return Err(fail(&mut slot, e)),
            }
        }
        let running = slot.running.as_mut().unwrap();
        let id = running.next_id;
        running.next_id += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let written = writeln!(running.stdin, "{request}").and_then(|()| running.stdin.flush());
        if written.is_err() {
            return Err(fail(&mut slot, "the extension closed".to_string()));
        }
        match read_reply(&running.lines, id, timeout) {
            Ok(reply) => {
                slot.failures = 0;
                slot.error = None;
                Ok(reply)
            }
            Err(e) => Err(fail(&mut slot, e)),
        }
    }
}

fn fail(slot: &mut Slot, message: String) -> String {
    slot.running = None;
    slot.failures += 1;
    slot.error = Some(message.clone());
    message
}

fn read_reply(lines: &Receiver<String>, id: u64, timeout: Duration) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = lines.recv_timeout(left).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => "the extension did not answer in time".to_string(),
            mpsc::RecvTimeoutError::Disconnected => "the extension stopped".to_string(),
        })?;
        let reply: Value = serde_json::from_str(&line)
            .map_err(|_| "the extension wrote something that is not JSON".to_string())?;
        if reply.get("id") != Some(&json!(id)) {
            continue; // A late reply to an earlier request.
        }
        if let Some(error) = reply.get("error") {
            return Err(format!("the extension reported an error: {error}"));
        }
        return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// All installed extensions. Implements [`Provider`], so results merge with built-in sources.
pub struct Host {
    root: PathBuf,
    extensions: RwLock<HashMap<String, Extension>>,
    enabled: RwLock<HashSet<String>>,
}

impl Host {
    pub fn new(root: PathBuf, enabled: &[String]) -> Host {
        let host = Host {
            root,
            extensions: RwLock::new(HashMap::new()),
            enabled: RwLock::new(enabled.iter().cloned().collect()),
        };
        host.reload();
        host
    }

    /// Re-read every manifest and stop all processes, so edits take effect. Also the way to
    /// retry an extension that stopped after repeated failures.
    pub fn reload(&self) {
        let mut found = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let folder = entry.path();
                let Some(name) = folder
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(String::from)
                else {
                    continue;
                };
                if let Ok(manifest) = read_manifest(&folder) {
                    found.insert(
                        name,
                        Extension {
                            folder,
                            manifest,
                            slot: Mutex::default(),
                        },
                    );
                }
            }
        }
        *self.extensions.write().unwrap() = found;
    }

    pub fn set_enabled(&self, enabled: &[String]) {
        let next: HashSet<String> = enabled.iter().cloned().collect();
        // Stop anything no longer allowed to run.
        for (name, ext) in self.extensions.read().unwrap().iter() {
            if !next.contains(name) {
                ext.slot.lock().unwrap().running = None;
            }
        }
        *self.enabled.write().unwrap() = next;
    }

    pub fn list(&self) -> Vec<Installed> {
        let enabled = self.enabled.read().unwrap();
        let mut list: Vec<Installed> = self
            .extensions
            .read()
            .unwrap()
            .iter()
            .map(|(folder, ext)| Installed {
                folder: folder.clone(),
                manifest: ext.manifest.clone(),
                enabled: enabled.contains(folder),
                error: ext.slot.lock().unwrap().error.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.folder.cmp(&b.folder));
        list
    }

    /// Run a result and return the effects its manifest allows. Undeclared effects are errors.
    pub fn run(&self, folder: &str, id: &str) -> Result<Vec<Effect>, String> {
        if !self.enabled.read().unwrap().contains(folder) {
            return Err("That extension is not enabled.".to_string());
        }
        let extensions = self.extensions.read().unwrap();
        let ext = extensions
            .get(folder)
            .ok_or("That extension is not installed.")?;
        let reply = ext.call("run", json!({"id": id}), RUN_TIMEOUT)?;
        let effects: Vec<Effect> = match reply.get("effects") {
            Some(effects) => serde_json::from_value(effects.clone())
                .map_err(|_| "the extension sent an unknown effect".to_string())?,
            None => Vec::new(),
        };
        for effect in &effects {
            check_effect(effect, &ext.manifest)?;
        }
        Ok(effects)
    }

    /// Install from an HTTPS Git URL. The extension stays disabled until the user enables it.
    pub fn install(&self, url: &str) -> Result<String, String> {
        let folder = folder_for_url(url)?;
        let dest = self.root.join(&folder);
        if dest.exists() {
            return Err(format!("{folder} is already installed."));
        }
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let mut command = Command::new("git");
        command
            .args(["clone", "--depth", "1", "--", url])
            .arg(&dest);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("could not run git: {e}"))?;
        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        if let Err(e) = read_manifest(&dest) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("That repository is not a Glint extension: {e}"));
        }
        self.reload();
        Ok(folder)
    }

    /// Delete an installed extension.
    pub fn remove(&self, folder: &str) -> Result<(), String> {
        let mut extensions = self.extensions.write().unwrap();
        // Only names from the listing are accepted, so this cannot reach outside the root.
        let ext = extensions
            .remove(folder)
            .ok_or("That extension is not installed.")?;
        ext.slot.lock().unwrap().running = None;
        std::fs::remove_dir_all(&ext.folder).map_err(|e| e.to_string())
    }
}

fn read_manifest(folder: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(folder.join("manifest.json")).map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    manifest.validate()?;
    Ok(manifest)
}

fn check_effect(effect: &Effect, manifest: &Manifest) -> Result<(), String> {
    let declared = |p: &str| manifest.permissions.iter().any(|d| d == p);
    match effect {
        Effect::Copy { .. } if !declared("clipboard") => {
            Err("This extension did not declare the clipboard permission.".to_string())
        }
        Effect::OpenUrl { .. } if !declared("open_url") => {
            Err("This extension did not declare the open_url permission.".to_string())
        }
        Effect::OpenUrl { url } if !url.starts_with("https://") && !url.starts_with("http://") => {
            Err("Extensions can only open http and https links.".to_string())
        }
        _ => Ok(()),
    }
}

fn folder_for_url(url: &str) -> Result<String, String> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err("Only https:// Git URLs can be installed.".to_string());
    };
    // The part after the host names the folder, so a bare host has no usable name.
    let (_, name) = rest
        .trim_end_matches('/')
        .rsplit_once('/')
        .unwrap_or(("", ""));
    let name = name.trim_end_matches(".git");
    let safe = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    if safe {
        Ok(name.to_string())
    } else {
        Err("That URL does not end in a usable folder name.".to_string())
    }
}

impl Provider for Host {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        if fuzzy || q.mode != Mode::All {
            return Vec::new();
        }
        let (first, rest) = q.text.split_once(' ').unwrap_or((&q.text, ""));
        let enabled = self.enabled.read().unwrap();
        let extensions = self.extensions.read().unwrap();
        let Some((folder, ext)) = extensions
            .iter()
            .find(|(f, e)| enabled.contains(*f) && e.manifest.trigger.eq_ignore_ascii_case(first))
        else {
            return Vec::new();
        };
        let Ok(reply) = ext.call("query", json!({"text": rest.trim()}), QUERY_TIMEOUT) else {
            return Vec::new();
        };
        let items: Vec<WireItem> = reply
            .get("items")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        items
            .into_iter()
            .take(limit.min(MAX_ITEMS))
            .enumerate()
            .map(|(i, wire)| {
                let mut item = Item::new(
                    format!("ext:{folder}:{}", wire.id),
                    wire.title,
                    wire.subtitle,
                    ItemKind::Extension,
                    Target::Extension {
                        extension: folder.clone(),
                        id: wire.id,
                    },
                );
                item.score = SCORE - i as u32;
                item
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glint_core::query::parse;
    use std::fs;

    /// A scripted extension: behaviour is chosen by the text it receives.
    const SCRIPT: &str = r#"
import json, os, sys, time
for line in sys.stdin:
    req = json.loads(line)
    params = req["params"]
    if req["method"] == "query":
        text = params["text"]
        with open("seen.txt", "a") as f:
            f.write(text + "\n")
        if text == "hang":
            time.sleep(60)
        if text == "crash":
            sys.exit(1)
        if text == "garbage":
            print("this is not json", flush=True)
            continue
        if text == "huge":
            print("x" * 2000000, flush=True)
            continue
        if text == "many":
            items = [{"id": str(i), "title": "n" + str(i)} for i in range(500)]
        else:
            items = [{"id": "a", "title": "Echo " + text, "subtitle": "sub"}]
        print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": {"items": items}}), flush=True)
    else:
        effects = {
            "copy": [{"type": "copy", "text": "copied"}],
            "url": [{"type": "open_url", "url": "https://example.com"}],
            "badurl": [{"type": "open_url", "url": "file:///c:/windows/system32/calc.exe"}],
            "unknown": [{"type": "format_disk"}],
        }[params["id"]]
        print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": {"effects": effects}}), flush=True)
"#;

    fn root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glint-ext-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn add(root: &Path, folder: &str, permissions: &[&str]) {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.py"), SCRIPT).unwrap();
        let manifest = json!({
            "id": folder, "name": folder, "version": "1.0.0", "trigger": "echo",
            "command": ["python", "-u", "main.py"], "permissions": permissions,
        });
        fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    }

    fn host(tag: &str, permissions: &[&str]) -> (PathBuf, Host) {
        let dir = root(tag);
        add(&dir, "demo", permissions);
        let host = Host::new(dir.clone(), &["demo".to_string()]);
        (dir, host)
    }

    fn titles(host: &Host, input: &str) -> Vec<String> {
        host.query(&parse(input), 20, false)
            .into_iter()
            .map(|i| i.title)
            .collect()
    }

    fn finish(dir: PathBuf, host: Host) {
        drop(host);
        std::thread::sleep(Duration::from_millis(100));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn answers_only_to_its_trigger_and_keeps_one_process() {
        let (dir, host) = host("trigger", &[]);
        assert_eq!(titles(&host, "echo hello"), ["Echo hello"]);
        assert_eq!(titles(&host, "ECHO again"), ["Echo again"]);
        assert!(titles(&host, "other text").is_empty());
        finish(dir, host);
    }

    #[test]
    fn sees_only_text_after_the_trigger_and_nothing_else_the_user_types() {
        let (dir, host) = host("private", &[]);
        titles(&host, "my secret search");
        titles(&host, "echo visible");
        titles(&host, "another private search");
        let seen = fs::read_to_string(dir.join("demo").join("seen.txt")).unwrap();
        assert_eq!(seen.trim(), "visible");
        finish(dir, host);
    }

    #[test]
    fn disabled_extensions_never_start() {
        let dir = root("disabled");
        add(&dir, "demo", &[]);
        let host = Host::new(dir.clone(), &[]);
        assert!(titles(&host, "echo hello").is_empty());
        assert!(host.run("demo", "copy").is_err());
        assert!(!dir.join("demo").join("seen.txt").exists());
        assert!(!host.list()[0].enabled);
        finish(dir, host);
    }

    #[test]
    fn disabling_stops_answering() {
        let (dir, host) = host("toggle", &[]);
        assert_eq!(titles(&host, "echo a").len(), 1);
        host.set_enabled(&[]);
        assert!(titles(&host, "echo a").is_empty());
        finish(dir, host);
    }

    #[test]
    fn a_hung_extension_is_killed_quickly_then_restarted() {
        let (dir, host) = host("hang", &[]);
        let start = Instant::now();
        assert!(titles(&host, "echo hang").is_empty());
        // The first call after a start waits out the startup allowance before giving up.
        assert!(
            start.elapsed() < STARTUP_ALLOWANCE + Duration::from_secs(3),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(titles(&host, "echo fine"), ["Echo fine"]);
        finish(dir, host);
    }

    #[test]
    fn a_slow_starting_extension_gets_time_to_answer_its_first_request() {
        let (dir, host) = host("slow-start", &[]);
        // Takes longer to start than the normal query limit, then behaves normally.
        let script = format!(
            "import time\ntime.sleep(1.0)\n{}",
            fs::read_to_string(dir.join("demo").join("main.py")).unwrap()
        );
        fs::write(dir.join("demo").join("main.py"), script).unwrap();
        host.reload();
        assert_eq!(
            titles(&host, "echo first"),
            ["Echo first"],
            "{:?}",
            host.list()[0].error
        );
        assert_eq!(titles(&host, "echo second"), ["Echo second"]);
        assert!(host.list()[0].error.is_none());
        finish(dir, host);
    }

    #[test]
    fn crashes_garbage_and_oversized_replies_cannot_break_the_host() {
        let (dir, host) = host("bad", &[]);
        for bad in ["echo crash", "echo garbage", "echo huge"] {
            assert!(titles(&host, bad).is_empty(), "{bad}");
            assert_eq!(titles(&host, "echo ok"), ["Echo ok"], "after {bad}");
        }
        finish(dir, host);
    }

    #[test]
    fn repeated_failures_stop_it_until_reload_and_the_reason_is_visible() {
        let (dir, host) = host("repeat", &[]);
        for _ in 0..MAX_FAILURES {
            titles(&host, "echo crash");
        }
        assert!(titles(&host, "echo ok").is_empty());
        assert!(host.list()[0].error.is_some());
        host.reload();
        assert_eq!(titles(&host, "echo ok"), ["Echo ok"]);
        finish(dir, host);
    }

    #[test]
    fn a_missing_program_is_reported() {
        let dir = root("missing");
        add(&dir, "demo", &[]);
        let manifest = json!({"id": "demo", "name": "d", "version": "1", "trigger": "echo",
            "command": ["glint-no-such-program"]});
        fs::write(dir.join("demo/manifest.json"), manifest.to_string()).unwrap();
        let host = Host::new(dir.clone(), &["demo".to_string()]);
        assert!(titles(&host, "echo hi").is_empty());
        assert!(host.list()[0]
            .error
            .as_deref()
            .unwrap()
            .contains("could not start"));
        finish(dir, host);
    }

    #[test]
    fn limits_the_number_of_results() {
        let (dir, host) = host("many", &[]);
        assert_eq!(titles(&host, "echo many").len(), 20);
        finish(dir, host);
    }

    #[test]
    fn effects_require_declared_permissions() {
        let (dir, none) = host("noperm", &[]);
        assert!(none.run("demo", "copy").unwrap_err().contains("clipboard"));
        assert!(none.run("demo", "url").unwrap_err().contains("open_url"));
        finish(dir, none);

        let (dir, full) = host("perm", &["clipboard", "open_url"]);
        assert_eq!(
            full.run("demo", "copy").unwrap(),
            [Effect::Copy {
                text: "copied".to_string()
            }]
        );
        assert_eq!(full.run("demo", "url").unwrap().len(), 1);
        finish(dir, full);
    }

    #[test]
    fn dangerous_or_unknown_effects_are_refused() {
        let (dir, host) = host("danger", &["clipboard", "open_url"]);
        assert!(host.run("demo", "badurl").unwrap_err().contains("http"));
        assert!(host
            .run("demo", "unknown")
            .unwrap_err()
            .contains("unknown effect"));
        finish(dir, host);
    }

    #[test]
    fn invalid_manifests_are_ignored() {
        let dir = root("invalid");
        add(&dir, "good", &[]);
        for (folder, text) in [
            ("broken", "{not json".to_string()),
            ("badid", json!({"id": "../evil", "name": "x", "version": "1", "trigger": "t", "command": ["x"]}).to_string()),
            ("badtrigger", json!({"id": "a", "name": "x", "version": "1", "trigger": "two words", "command": ["x"]}).to_string()),
            ("nocommand", json!({"id": "a", "name": "x", "version": "1", "trigger": "t", "command": []}).to_string()),
        ] {
            fs::create_dir_all(dir.join(folder)).unwrap();
            fs::write(dir.join(folder).join("manifest.json"), text).unwrap();
        }
        let host = Host::new(dir.clone(), &[]);
        let list = host.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].folder, "good");
        finish(dir, host);
    }

    #[test]
    fn install_only_accepts_https_and_safe_names() {
        for bad in [
            "http://example.com/a.git",
            "file:///c:/evil",
            "--upload-pack=evil",
            "git@github.com:a/b.git",
            "https://example.com/",
            "https://example.com/..",
            "https://example.com/a%2Fb",
        ] {
            assert!(folder_for_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            folder_for_url("https://example.com/me/ext.git").unwrap(),
            "ext"
        );
        assert_eq!(
            folder_for_url("https://example.com/me/ext/").unwrap(),
            "ext"
        );
    }

    /// Runs a shipped example the way a user would: one search, then Enter on the first result.
    fn try_example(folder: &str, input: &str) -> Vec<Effect> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions");
        let host = Host::new(root, &[folder.to_string()]);
        let items = host.query(&parse(input), 20, false);
        assert!(!items.is_empty(), "{folder}: {:?}", host.list());
        let Target::Extension { extension, id } = &items[0].target else {
            panic!("wrong target");
        };
        host.run(extension, id).unwrap()
    }

    fn copied(effects: Vec<Effect>) -> String {
        match &effects[..] {
            [Effect::Copy { text }] => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn python_example_generates_a_uuid() {
        let text = copied(try_example("uuid-python", "uuid"));
        assert_eq!(text.len(), 36);
        assert_eq!(text.matches('-').count(), 4);
    }

    #[test]
    fn typescript_example_reads_a_timestamp() {
        let text = copied(try_example("timestamp-typescript", "ts 1700000000"));
        assert_eq!(text, "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn rust_example_encodes_text() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/base64-rust");
        let status = Command::new(std::env::var("CARGO").unwrap_or("cargo".to_string()))
            .args(["build", "--release", "--quiet"])
            .current_dir(&dir)
            .env_remove("CARGO_TARGET_DIR")
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(copied(try_example("base64-rust", "b64 hello")), "aGVsbG8=");
    }

    #[test]
    fn remove_deletes_only_listed_extensions() {
        let (dir, host) = host("remove", &[]);
        assert!(host.remove("..").is_err());
        assert!(host.remove("not-there").is_err());
        titles(&host, "echo running");
        std::thread::sleep(Duration::from_millis(50));
        host.remove("demo").unwrap();
        assert!(!dir.join("demo").exists());
        assert!(host.list().is_empty());
        finish(dir, host);
    }
}
