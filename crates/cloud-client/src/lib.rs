use anyhow::{bail, Context, Result};
use graph_core::{
    CloudWorkspace, CreateWorkspaceRevisionRequest, CreateWorkspaceRevisionResponse, LanguageId,
    WorkspaceFileEntry, WorkspaceRevision, WorkspaceSyncPlanRequest, WorkspaceSyncPlanResponse,
};
use reqwest::StatusCode;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

const CONFIG_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "package.json",
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "tsconfig.json",
    "pyproject.toml",
    "uv.lock",
    "requirements.txt",
];

#[derive(Debug, Clone)]
pub struct CloudClientConfig {
    pub base_url: Url,
}

#[derive(Clone)]
pub struct CloudClient {
    config: CloudClientConfig,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct LocalSyncFile {
    pub absolute_path: PathBuf,
    pub entry: WorkspaceFileEntry,
}

#[derive(Debug, Clone)]
pub struct SyncProjectRequest {
    pub project_root: PathBuf,
    pub workspace_id: Option<String>,
    pub display_name: Option<String>,
    pub base_revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncProjectResult {
    pub workspace: CloudWorkspace,
    pub revision: WorkspaceRevision,
    pub files_count: usize,
    pub uploaded_blobs: usize,
    pub skipped_blobs: usize,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
}

impl CloudClient {
    pub fn new(config: CloudClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    pub async fn create_workspace(&self, display_name: &str) -> Result<CloudWorkspace> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CreateWorkspaceRequest<'a> {
            display_name: &'a str,
        }

        self.post_json("api/workspaces", &CreateWorkspaceRequest { display_name })
            .await
    }

    pub async fn get_workspace(&self, workspace_id: &str) -> Result<CloudWorkspace> {
        self.get_json(&format!("api/workspaces/{workspace_id}"))
            .await
    }

    pub async fn create_sync_plan(
        &self,
        workspace_id: &str,
        request: WorkspaceSyncPlanRequest,
    ) -> Result<WorkspaceSyncPlanResponse> {
        self.post_json(
            &format!("api/workspaces/{workspace_id}/sync-plan"),
            &request,
        )
        .await
    }

    pub async fn upload_blob(
        &self,
        workspace_id: &str,
        content_hash: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let response = self
            .http
            .put(self.url(&format!(
                "api/workspaces/{workspace_id}/blobs/{content_hash}"
            ))?)
            .body(bytes)
            .send()
            .await
            .context("failed to upload blob")?;
        ensure_success(response).await.map(|_| ())
    }

    pub async fn create_revision(
        &self,
        workspace_id: &str,
        request: CreateWorkspaceRevisionRequest,
    ) -> Result<CreateWorkspaceRevisionResponse> {
        self.post_json(
            &format!("api/workspaces/{workspace_id}/revisions"),
            &request,
        )
        .await
    }

    pub async fn sync_project(&self, request: SyncProjectRequest) -> Result<SyncProjectResult> {
        let files = collect_sync_files(&request.project_root)?;
        let display_name = request.display_name.clone().unwrap_or_else(|| {
            request
                .project_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
                .to_string()
        });
        let workspace = match request.workspace_id.as_deref() {
            Some(workspace_id) => self.get_workspace(workspace_id).await?,
            None => self.create_workspace(&display_name).await?,
        };
        let entries = files
            .iter()
            .map(|file| file.entry.clone())
            .collect::<Vec<_>>();
        let plan = self
            .create_sync_plan(
                &workspace.id,
                WorkspaceSyncPlanRequest {
                    base_revision: request.base_revision.clone(),
                    files: entries.clone(),
                },
            )
            .await?;
        let missing = plan.missing_hashes.into_iter().collect::<HashSet<_>>();
        let mut local_files_by_hash = HashMap::new();
        for file in &files {
            local_files_by_hash
                .entry(file.entry.content_hash.clone())
                .or_insert(file);
        }
        let local_hashes = local_files_by_hash.keys().cloned().collect::<HashSet<_>>();
        let mut uploaded_hashes = HashSet::new();
        let mut uploaded_bytes = 0u64;

        for content_hash in &missing {
            if let Some(file) = local_files_by_hash.get(content_hash) {
                let bytes = fs::read(&file.absolute_path)
                    .with_context(|| format!("failed to read {}", file.absolute_path.display()))?;
                uploaded_bytes += bytes.len() as u64;
                self.upload_blob(&workspace.id, &file.entry.content_hash, bytes)
                    .await?;
                uploaded_hashes.insert(content_hash.clone());
            }
        }

        let revision_response = self
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: request.base_revision,
                    files: entries,
                },
            )
            .await?;
        let total_bytes = files.iter().map(|file| file.entry.size_bytes).sum();
        let summary = sync_summary(&local_hashes, &uploaded_hashes);
        Ok(SyncProjectResult {
            workspace: revision_response.workspace,
            revision: revision_response.revision,
            files_count: files.len(),
            uploaded_blobs: summary.uploaded_blobs,
            skipped_blobs: summary.skipped_blobs,
            total_bytes,
            uploaded_bytes,
        })
    }

    async fn get_json<T>(&self, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .http
            .get(self.url(path)?)
            .send()
            .await
            .with_context(|| format!("failed to GET {path}"))?;
        ensure_success(response)
            .await?
            .json()
            .await
            .context("invalid JSON")
    }

    async fn post_json<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let response = self
            .http
            .post(self.url(path)?)
            .json(body)
            .send()
            .await
            .with_context(|| format!("failed to POST {path}"))?;
        ensure_success(response)
            .await?
            .json()
            .await
            .context("invalid JSON")
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.config
            .base_url
            .join(path)
            .with_context(|| format!("failed to build cloud API URL for {path}"))
    }
}

pub fn collect_sync_files(root: impl AsRef<Path>) -> Result<Vec<LocalSyncFile>> {
    let root = root.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to canonicalize project root {}",
            root.as_ref().display()
        )
    })?;
    let mut files = Vec::new();
    collect_sync_files_inner(&root, &root, &mut files)?;
    files.sort_by(|left, right| left.entry.path.cmp(&right.entry.path));
    Ok(files)
}

fn collect_sync_files_inner(
    root: &Path,
    current: &Path,
    files: &mut Vec<LocalSyncFile>,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if !project_indexer::is_ignored_path(path.strip_prefix(root).unwrap_or(&path)) {
                collect_sync_files_inner(root, &path, files)?;
            }
            continue;
        }
        if !is_sync_file(&path) {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let relative_path = project_indexer::relative_to(root, &path);
        let language = language_for_path(&path);
        files.push(LocalSyncFile {
            absolute_path: path,
            entry: WorkspaceFileEntry {
                path: relative_path,
                content_hash: sha256_content_hash(&bytes),
                size_bytes: bytes.len() as u64,
                language,
            },
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncSummary {
    uploaded_blobs: usize,
    skipped_blobs: usize,
}

fn sync_summary(local_hashes: &HashSet<String>, uploaded_hashes: &HashSet<String>) -> SyncSummary {
    SyncSummary {
        uploaded_blobs: uploaded_hashes.len(),
        skipped_blobs: local_hashes.len().saturating_sub(uploaded_hashes.len()),
    }
}

fn is_sync_file(path: &Path) -> bool {
    language_for_path(path).is_some()
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| CONFIG_FILES.contains(&name))
}

pub fn sha256_content_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub fn language_for_path(path: &Path) -> Option<LanguageId> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => Some(LanguageId::Rust),
        Some("py") => Some(LanguageId::Python),
        Some("ts" | "tsx") => Some(LanguageId::TypeScript),
        Some("js" | "jsx") => Some(LanguageId::JavaScript),
        Some("qml") => Some(LanguageId::Qml),
        _ => None,
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let text = response.text().await.unwrap_or_default();
    if status == StatusCode::NOT_FOUND {
        bail!("cloud API resource not found: {text}");
    }
    bail!("cloud API request failed with {status}: {text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-cloud-client-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(root: &Path, path: &str, content: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn hash_format_uses_sha256_prefix() {
        assert_eq!(
            sha256_content_hash(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn collect_sync_files_includes_sources_and_configs_but_skips_ignored_dirs() {
        let root = test_root("collection");
        write_file(&root, "Cargo.toml", "[package]\nname = \"demo\"");
        write_file(&root, "src/main.rs", "fn main() {}");
        write_file(&root, "src/lib.rs", "pub fn lib() {}");
        write_file(&root, "frontend/App.tsx", "export function App() {}");
        write_file(&root, "scripts/tool.py", "print('hi')");
        write_file(&root, "qml/Main.qml", "Item {}");
        write_file(&root, "target/generated.rs", "fn generated() {}");
        write_file(
            &root,
            "node_modules/pkg/index.ts",
            "export const ignored = true",
        );

        let files = collect_sync_files(&root).unwrap();
        let paths = files
            .iter()
            .map(|file| file.entry.path.as_str())
            .collect::<Vec<_>>();

        assert!(paths.contains(&"Cargo.toml"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"frontend/App.tsx"));
        assert!(paths.contains(&"scripts/tool.py"));
        assert!(paths.contains(&"qml/Main.qml"));
        assert!(!paths.contains(&"target/generated.rs"));
        assert!(!paths.contains(&"node_modules/pkg/index.ts"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn language_inference_matches_supported_extensions() {
        assert_eq!(
            language_for_path(Path::new("main.rs")),
            Some(LanguageId::Rust)
        );
        assert_eq!(
            language_for_path(Path::new("tool.py")),
            Some(LanguageId::Python)
        );
        assert_eq!(
            language_for_path(Path::new("api.ts")),
            Some(LanguageId::TypeScript)
        );
        assert_eq!(
            language_for_path(Path::new("App.tsx")),
            Some(LanguageId::TypeScript)
        );
        assert_eq!(
            language_for_path(Path::new("utils.js")),
            Some(LanguageId::JavaScript)
        );
        assert_eq!(
            language_for_path(Path::new("view.jsx")),
            Some(LanguageId::JavaScript)
        );
        assert_eq!(
            language_for_path(Path::new("Main.qml")),
            Some(LanguageId::Qml)
        );
        assert_eq!(language_for_path(Path::new("Cargo.toml")), None);
    }

    #[test]
    fn sync_summary_counts_unique_uploaded_and_skipped_blobs() {
        let local_hashes = HashSet::from([
            "sha256:one".to_string(),
            "sha256:two".to_string(),
            "sha256:three".to_string(),
        ]);
        let uploaded_hashes = HashSet::from(["sha256:one".to_string(), "sha256:three".to_string()]);

        assert_eq!(
            sync_summary(&local_hashes, &uploaded_hashes),
            SyncSummary {
                uploaded_blobs: 2,
                skipped_blobs: 1,
            }
        );
    }
}
