use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{paths::set_private_permissions, render::sanitize};

pub const MAX_PROMPT_BYTES: usize = 128 * 1024;
pub const MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_DRAFT_BYTES: usize = 16 * 1024;

pub fn validate_prompt(prompt: &str) -> Result<(), &'static str> {
    if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES {
        return Err("prompt is outside the local assistant safety limit");
    }
    Ok(())
}

pub fn sanitize_draft(value: &str) -> Result<String, &'static str> {
    let value = sanitize::text(value);
    if value.trim().is_empty() || value.len() > MAX_DRAFT_BYTES {
        return Err("assistant draft is outside the local assistant safety limit");
    }
    Ok(value)
}

pub fn validate_workspace(path: &Path) -> Result<PathBuf, &'static str> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "configured local assistant workspace is unavailable")?;
    if canonical != path || !canonical.is_dir() {
        return Err("configured local assistant workspace is not a canonical directory");
    }
    Ok(canonical)
}

pub fn scratch_directory(data_dir: &Path) -> Result<PathBuf, &'static str> {
    let root = data_dir.join("agent-scratch");
    fs::create_dir_all(&root).map_err(|_| "could not prepare local assistant scratch space")?;
    set_private_permissions(&root).map_err(|_| "could not secure local assistant scratch space")?;
    let path = root.join(Uuid::new_v4().to_string());
    fs::create_dir(&path).map_err(|_| "could not prepare local assistant scratch space")?;
    set_private_permissions(&path).map_err(|_| "could not secure local assistant scratch space")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{MAX_DRAFT_BYTES, MAX_PROMPT_BYTES, sanitize_draft, validate_prompt};

    #[test]
    fn strict_limits_and_terminal_sanitization_apply_to_local_content() {
        assert!(validate_prompt("context").is_ok());
        assert!(validate_prompt(&"x".repeat(MAX_PROMPT_BYTES + 1)).is_err());
        assert!(sanitize_draft("answer\u{1b}[2J").unwrap().contains("[2J"));
        assert!(sanitize_draft(&"x".repeat(MAX_DRAFT_BYTES + 1)).is_err());
    }
}
