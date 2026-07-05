use anyhow::Result;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::state::AppStateHandle;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorRequest {
    file: PathBuf,
    line: Option<u32>,
    column: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorResponse {
    pub(crate) editor: String,
    pub(crate) file: String,
    pub(crate) line: Option<u32>,
}

pub(crate) async fn open_in_editor(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenEditorRequest>,
) -> impl IntoResponse {
    if !state.enable_editor_open {
        return (
            StatusCode::FORBIDDEN,
            "Opening files in an editor is disabled. Restart with --enable-editor-open to enable it.",
        )
            .into_response();
    }
    let root = state.project_root.read().clone();
    let requested_path = if request.file.is_absolute() {
        request.file.clone()
    } else {
        root.join(&request.file)
    };
    let file = match requested_path.canonicalize() {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to resolve file path: {error}"),
            )
                .into_response();
        }
    };
    if !file.starts_with(&root) {
        return (
            StatusCode::BAD_REQUEST,
            "refusing to open a file outside the current project",
        )
            .into_response();
    }

    match launch_editor(&file, request.line, request.column.unwrap_or(1)).await {
        Ok(editor) => (
            StatusCode::ACCEPTED,
            Json(OpenEditorResponse {
                editor,
                file: file.display().to_string(),
                line: request.line,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to open editor: {error}"),
        )
            .into_response(),
    }
}

pub(crate) async fn launch_editor(file: &Path, line: Option<u32>, column: u32) -> Result<String> {
    let mut errors = Vec::new();
    for candidate in editor_candidates(file, line, column) {
        for command_candidate in command_candidates(candidate) {
            let mut command = Command::new(&command_candidate.program);
            command.args(&command_candidate.args);
            match command.spawn() {
                Ok(_child) => return Ok(command_candidate.label),
                Err(error) => errors.push(format!("{}: {error}", command_candidate.label)),
            }
        }
    }
    anyhow::bail!(
        "no editor command worked. Set RUST_WATCHER_EDITOR, RUST_WATCHER_TERMINAL, VISUAL, or EDITOR. Tried: {}",
        errors.join("; ")
    )
}

pub(crate) struct EditorCommand {
    label: String,
    program: String,
    args: Vec<String>,
}

pub(crate) fn command_candidates(candidate: EditorCommand) -> Vec<EditorCommand> {
    if is_terminal_editor(&candidate.program) {
        return terminal_wrapped_editor_candidates(&candidate);
    }
    vec![candidate]
}

pub(crate) fn is_terminal_editor(program: &str) -> bool {
    matches!(
        Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program),
        "nvim" | "vim" | "vi" | "hx" | "helix" | "kak" | "micro" | "nano"
    )
}

pub(crate) fn terminal_wrapped_editor_candidates(editor: &EditorCommand) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_TERMINAL", "TERMINAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if let Some(candidate) = terminal_command_from_env(env_name, &command, editor) {
                candidates.push(candidate);
            }
        }
    }
    for terminal in [
        "x-terminal-emulator",
        "kitty",
        "alacritty",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ] {
        candidates.push(wrap_editor_in_terminal(
            terminal.to_string(),
            Vec::new(),
            editor,
        ));
    }
    candidates
}

pub(crate) fn terminal_command_from_env(
    env_name: &str,
    command: &str,
    editor: &EditorCommand,
) -> Option<EditorCommand> {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let program = parts.remove(0);
    let mut candidate = wrap_editor_in_terminal(program, parts, editor);
    candidate.label = format!("{env_name}: {}", candidate.label);
    Some(candidate)
}

pub(crate) fn wrap_editor_in_terminal(
    terminal: String,
    mut terminal_args: Vec<String>,
    editor: &EditorCommand,
) -> EditorCommand {
    let terminal_name = Path::new(&terminal)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&terminal);
    let editor_program = editor.program.clone();
    let editor_args = editor.args.clone();

    match terminal_name {
        "wezterm" => {
            terminal_args.push("start".into());
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "gnome-terminal" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "xfce4-terminal" => {
            terminal_args.push("--disable-server".into());
            terminal_args.push("--command".into());
            terminal_args.push(shell_join(&editor.program, &editor.args));
        }
        "kitty" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        _ => {
            terminal_args.push("-e".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
    }

    EditorCommand {
        label: format!("{terminal_name} -> {}", editor.label),
        program: terminal,
        args: terminal_args,
    }
}

pub(crate) fn shell_join(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .map(|part| shell_quote(&part))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub(crate) fn editor_candidates(file: &Path, line: Option<u32>, column: u32) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_EDITOR", "VISUAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if !command.trim().is_empty() {
                candidates.push(editor_command_from_env(
                    env_name, &command, file, line, column,
                ));
            }
        }
    }

    let file_text = file.display().to_string();
    let line_col = match line {
        Some(line) => format!("{file_text}:{line}:{column}"),
        None => file_text.clone(),
    };
    let line_only = match line {
        Some(line) => format!("{file_text}:{line}"),
        None => file_text.clone(),
    };
    for program in ["code", "codium", "code-insiders"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: if line.is_some() {
                vec!["--goto".into(), line_col.clone()]
            } else {
                vec![file_text.clone()]
            },
        });
    }
    candidates.push(EditorCommand {
        label: "zed".into(),
        program: "zed".into(),
        args: vec![line_only],
    });
    for program in ["rustrover", "idea"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: match line {
                Some(line) => vec!["--line".into(), line.to_string(), file_text.clone()],
                None => vec![file_text.clone()],
            },
        });
    }
    candidates.push(EditorCommand {
        label: "xdg-open".into(),
        program: "xdg-open".into(),
        args: vec![file_text],
    });
    if let Ok(command) = std::env::var("EDITOR") {
        if !command.trim().is_empty() {
            candidates.push(editor_command_from_env(
                "EDITOR", &command, file, line, column,
            ));
        }
    }
    candidates
}

pub(crate) fn editor_command_from_env(
    env_name: &str,
    command: &str,
    file: &Path,
    line: Option<u32>,
    column: u32,
) -> EditorCommand {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let program = parts
        .first()
        .cloned()
        .unwrap_or_else(|| command.to_string());
    let file_text = file.display().to_string();
    let line_text = line.unwrap_or(1).to_string();
    let column_text = column.to_string();
    let had_template = parts.iter().any(|part| {
        part.contains("{file}") || part.contains("{line}") || part.contains("{column}")
    });
    if !parts.is_empty() {
        parts.remove(0);
    }
    let mut args = parts
        .into_iter()
        .map(|part| {
            part.replace("{file}", &file_text)
                .replace("{line}", &line_text)
                .replace("{column}", &column_text)
        })
        .collect::<Vec<_>>();
    if !had_template {
        args.extend(default_editor_args(
            &program, &args, &file_text, line, column,
        ));
    }
    EditorCommand {
        label: format!("{env_name}={program}"),
        program,
        args,
    }
}

pub(crate) fn default_editor_args(
    program: &str,
    existing_args: &[String],
    file: &str,
    line: Option<u32>,
    column: u32,
) -> Vec<String> {
    let program_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    match (program_name, line) {
        ("code" | "codium" | "code-insiders", Some(line)) => {
            if existing_args.iter().any(|arg| arg == "--goto") {
                vec![format!("{file}:{line}:{column}")]
            } else {
                vec!["--goto".into(), format!("{file}:{line}:{column}")]
            }
        }
        ("zed", Some(line)) => vec![format!("{file}:{line}")],
        ("idea" | "rustrover", Some(line)) => vec!["--line".into(), line.to_string(), file.into()],
        ("nvim" | "vim" | "vi" | "kak", Some(line)) => {
            vec![format!("+{line}"), file.into()]
        }
        ("nano" | "micro", Some(line)) => vec![format!("+{line},{column}"), file.into()],
        ("hx" | "helix", Some(line)) => vec![format!("{file}:{line}:{column}")],
        _ => vec![file.into()],
    }
}
