use super::*;
pub(super) async fn open_source(app: &mut App, tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>) {
    let s = &mut app.activity;
    if let Some(path) = s.selected_path() {
        let root = &app.project.workspace_root;
        if !s.data.files.get(&path).copied().unwrap_or(false) || !root.join(&path).is_file() {
            app.notice = Some(format!("Deleted / unavailable: {}", path.display()));
            return;
        }
        if activity::relative(root, &path).is_none() || !activity::watcher::admitted(root, &path) {
            app.notice = Some("Source outside current observation scope".into());
            return;
        }
        // Extend the existing Explorer index with an observed new location.
        if let Some(tree) = &app.explorer.tree {
            if !tree.entries.iter().any(|e| e.path == path) {
                let mut tree = tree.clone();
                for p in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                    if !tree.entries.iter().any(|e| e.path == p) {
                        tree.entries.push(crate::repository::RepoEntry {
                            path: p.to_path_buf(),
                            kind: if p == path {
                                crate::repository::RepoEntryKind::File
                            } else {
                                crate::repository::RepoEntryKind::Directory
                            },
                            size: None,
                        });
                    }
                }
                app.explorer.apply_tree(tree);
            }
        }
        super::super::event::open_exact_file(app, tx, path, None).await;
    }
}
