use std::env;
use std::path::{Path, PathBuf};

pub const TYPESCRIPT_LS_COMMAND: &str = "typescript-language-server";
pub const TY_COMMAND: &str = "ty";
pub const RUST_ANALYZER_COMMAND: &str = "rust-analyzer";

pub fn resolve_typescript_language_server(configured: &Path, project_root: &Path) -> PathBuf {
    if is_default_command(configured, TYPESCRIPT_LS_COMMAND) {
        find_nearest_project_binary(project_root, "node_modules/.bin", TYPESCRIPT_LS_COMMAND)
            .or_else(|| repo_frontend_binary(TYPESCRIPT_LS_COMMAND))
            .or_else(|| find_in_path(TYPESCRIPT_LS_COMMAND))
            .unwrap_or_else(|| PathBuf::from(TYPESCRIPT_LS_COMMAND))
    } else {
        resolve_explicit_path(configured, project_root)
    }
}

pub fn resolve_ty(configured: &Path, project_root: &Path) -> PathBuf {
    if is_default_command(configured, TY_COMMAND) {
        find_nearest_project_binary(project_root, ".venv/bin", TY_COMMAND)
            .or_else(|| find_nearest_project_binary(project_root, ".venv/Scripts", TY_COMMAND))
            .or_else(|| find_in_path(TY_COMMAND))
            .unwrap_or_else(|| PathBuf::from(TY_COMMAND))
    } else {
        resolve_explicit_path(configured, project_root)
    }
}

pub fn resolve_rust_analyzer(configured: &Path, project_root: &Path) -> PathBuf {
    if is_default_command(configured, RUST_ANALYZER_COMMAND) {
        find_in_path(RUST_ANALYZER_COMMAND).unwrap_or_else(|| PathBuf::from(RUST_ANALYZER_COMMAND))
    } else {
        resolve_explicit_path(configured, project_root)
    }
}

fn resolve_explicit_path(configured: &Path, project_root: &Path) -> PathBuf {
    if configured.is_absolute() {
        return configured.to_path_buf();
    }
    if path_has_separator(configured) {
        let cwd_candidate = env::current_dir()
            .ok()
            .map(|cwd| cwd.join(configured))
            .filter(|path| path.exists());
        return cwd_candidate.unwrap_or_else(|| project_root.join(configured));
    }
    configured.to_path_buf()
}

fn is_default_command(configured: &Path, command: &str) -> bool {
    configured == Path::new(command)
}

fn path_has_separator(path: &Path) -> bool {
    path.components().count() > 1
}

fn find_nearest_project_binary(
    project_root: &Path,
    directory: &str,
    command: &str,
) -> Option<PathBuf> {
    for root in project_root.ancestors() {
        if let Some(path) = executable_candidate(root.join(directory), command) {
            return Some(path);
        }
    }
    None
}

fn repo_frontend_binary(command: &str) -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontend = manifest_dir.join("../../frontend/node_modules/.bin");
    executable_candidate(frontend, command)
}

fn find_in_path(command: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    find_in_paths(command, env::split_paths(&paths))
}

fn find_in_paths(command: &str, paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for directory in paths {
        if let Some(path) = executable_candidate(directory, command) {
            return Some(path);
        }
    }
    None
}

fn executable_candidate(directory: PathBuf, command: &str) -> Option<PathBuf> {
    executable_names(command)
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}

fn executable_names(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            command.to_string(),
            format!("{command}.cmd"),
            format!("{command}.exe"),
        ]
    } else {
        vec![command.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root() -> PathBuf {
        let root = env::temp_dir().join(format!("rust-watcher-path-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn resolves_project_typescript_language_server() {
        let root = temp_root();
        let binary = root.join("node_modules/.bin").join(TYPESCRIPT_LS_COMMAND);
        touch(&binary);

        assert_eq!(
            resolve_typescript_language_server(Path::new(TYPESCRIPT_LS_COMMAND), &root),
            binary
        );
    }

    #[test]
    fn resolves_parent_typescript_language_server() {
        let root = temp_root();
        let child = root.join("packages/app");
        std::fs::create_dir_all(&child).unwrap();
        let binary = root.join("node_modules/.bin").join(TYPESCRIPT_LS_COMMAND);
        touch(&binary);

        assert_eq!(
            resolve_typescript_language_server(Path::new(TYPESCRIPT_LS_COMMAND), &child),
            binary
        );
    }

    #[test]
    fn explicit_absolute_typescript_language_server_is_respected() {
        let root = temp_root();
        let binary = root.join("tools/typescript-language-server");
        touch(&binary);

        assert_eq!(resolve_typescript_language_server(&binary, &root), binary);
    }

    #[test]
    fn resolves_venv_ty() {
        let root = temp_root();
        let binary = root.join(".venv/bin").join(TY_COMMAND);
        touch(&binary);

        assert_eq!(resolve_ty(Path::new(TY_COMMAND), &root), binary);
    }

    #[test]
    fn resolves_parent_venv_ty() {
        let root = temp_root();
        let child = root.join("src/package");
        std::fs::create_dir_all(&child).unwrap();
        let binary = root.join(".venv/bin").join(TY_COMMAND);
        touch(&binary);

        assert_eq!(resolve_ty(Path::new(TY_COMMAND), &child), binary);
    }

    #[test]
    fn path_lookup_finds_binary_in_path() {
        let root = temp_root();
        let binary = root.join("bin").join("sample-analyzer");
        touch(&binary);

        assert_eq!(
            find_in_paths("sample-analyzer", [root.join("bin")]),
            Some(binary)
        );
    }
}
