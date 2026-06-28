use crate::{
    LspCallHierarchyIncomingCall, LspCallHierarchyItem, LspCallHierarchyOutgoingCall, LspClient,
    LspGotoDefinitionResponse, LspLocation, LspNotification,
};
use anyhow::{anyhow, Context, Result};
use graph_core::DiscoveredSymbol;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::time::{timeout, Duration};

pub const START_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

pub type BinaryResolver = fn(&Path, &Path) -> PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspRuntimeMode {
    Auto,
    ParserOnly,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspRuntimeStatus {
    ParserOnly,
    Ready,
    Unavailable,
    Restarting,
    Error,
}

pub struct LspRuntime {
    analyzer_id: &'static str,
    process_name: &'static str,
    default_language_id: &'static str,
    binary: PathBuf,
    args: Vec<String>,
    mode: LspRuntimeMode,
    fallback_message: &'static str,
    resolver: BinaryResolver,
    root: RwLock<PathBuf>,
    client: AsyncMutex<Option<LspClient>>,
    opened_files: RwLock<HashSet<PathBuf>>,
    file_versions: RwLock<HashMap<PathBuf, i32>>,
    status: RwLock<LspRuntimeStatus>,
    message: RwLock<Option<String>>,
    last_start_failure: RwLock<Option<Instant>>,
    last_warning: RwLock<Option<Instant>>,
    start_attempts: AtomicUsize,
}

impl LspRuntime {
    pub fn new(config: LspRuntimeConfig) -> Self {
        let initial = if config.mode == LspRuntimeMode::ParserOnly {
            LspRuntimeStatus::ParserOnly
        } else {
            LspRuntimeStatus::Unavailable
        };
        Self {
            analyzer_id: config.analyzer_id,
            process_name: config.process_name,
            default_language_id: config.default_language_id,
            binary: config.binary,
            args: config.args,
            mode: config.mode,
            fallback_message: config.fallback_message,
            resolver: config.resolver,
            root: RwLock::new(config.root),
            client: AsyncMutex::new(None),
            opened_files: RwLock::new(HashSet::new()),
            file_versions: RwLock::new(HashMap::new()),
            status: RwLock::new(initial),
            message: RwLock::new(None),
            last_start_failure: RwLock::new(None),
            last_warning: RwLock::new(None),
            start_attempts: AtomicUsize::new(0),
        }
    }

    pub fn is_parser_only(&self) -> bool {
        self.mode == LspRuntimeMode::ParserOnly
    }

    pub fn status(&self) -> LspRuntimeStatus {
        *self.status.read()
    }

    pub fn message(&self) -> Option<String> {
        self.message.read().clone()
    }

    pub fn should_log_start_failure(&self) -> bool {
        let mut last_warning = self.last_warning.write();
        let should_log = last_warning
            .as_ref()
            .is_none_or(|instant| instant.elapsed() >= START_RETRY_COOLDOWN);
        if should_log {
            *last_warning = Some(Instant::now());
        }
        should_log
    }

    pub fn start_attempts(&self) -> usize {
        self.start_attempts.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn file_version(&self, file: &Path) -> Option<i32> {
        self.file_versions
            .read()
            .get(&normalize_path(file))
            .copied()
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    #[cfg(test)]
    pub fn insert_open_file_for_test(&self, file: PathBuf, version: i32) {
        let file = normalize_path(&file);
        self.opened_files.write().insert(file.clone());
        self.file_versions.write().insert(file, version);
    }

    #[cfg(test)]
    pub fn is_file_open(&self, file: &Path) -> bool {
        self.opened_files.read().contains(&normalize_path(file))
    }

    pub async fn set_root(&self, root: PathBuf) {
        *self.root.write() = root;
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_mut() {
            let _ = client.shutdown().await;
        }
        *client = None;
        self.opened_files.write().clear();
        self.file_versions.write().clear();
        *self.status.write() = if self.mode == LspRuntimeMode::ParserOnly {
            LspRuntimeStatus::ParserOnly
        } else {
            LspRuntimeStatus::Unavailable
        };
        *self.message.write() = None;
        *self.last_start_failure.write() = None;
        *self.last_warning.write() = None;
    }

    pub async fn subscribe_notifications(&self) -> Result<broadcast::Receiver<LspNotification>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        Ok(guard.as_ref().unwrap().subscribe_notifications())
    }

    pub async fn open_document(&self, file: &Path, language_id: Option<&str>) -> Result<()> {
        self.ensure_document_open(file, language_id).await
    }

    pub async fn sync_changed_file(&self, file: &Path, language_id: Option<&str>) -> Result<i32> {
        let file = normalize_path(file);
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        if !self.opened_files.read().contains(&file) {
            self.ensure_document_open(&file, language_id).await?;
            return Ok(*self.file_versions.read().get(&file).unwrap_or(&1));
        }
        let version = self.increment_file_version(&file);
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = async {
            let client = guard.as_ref().unwrap();
            client.did_change(&file, text.clone(), version).await?;
            client.did_save(&file, Some(text)).await
        }
        .await;
        self.handle_request_result(&mut guard, result.map(|_| version))
    }

    pub async fn document_symbols(
        &self,
        file: &Path,
        language_id: Option<&str>,
    ) -> Result<Vec<DiscoveredSymbol>> {
        self.ensure_document_open(file, language_id).await?;
        self.with_client_request(|client| {
            let file = file.to_path_buf();
            Box::pin(async move { client.document_symbols(&file).await })
        })
        .await
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
        language_id: Option<&str>,
    ) -> Result<Vec<LspCallHierarchyItem>> {
        self.ensure_document_open(file, language_id).await?;
        self.with_client_request(|client| {
            let file = file.to_path_buf();
            Box::pin(async move { client.prepare_call_hierarchy(&file, line, character).await })
        })
        .await
    }

    pub async fn incoming_calls(
        &self,
        item: &LspCallHierarchyItem,
    ) -> Result<Vec<LspCallHierarchyIncomingCall>> {
        self.with_client_request(|client| {
            let item = item.clone();
            Box::pin(async move { client.incoming_calls(&item).await })
        })
        .await
    }

    pub async fn outgoing_calls(
        &self,
        item: &LspCallHierarchyItem,
    ) -> Result<Vec<LspCallHierarchyOutgoingCall>> {
        self.with_client_request(|client| {
            let item = item.clone();
            Box::pin(async move { client.outgoing_calls(&item).await })
        })
        .await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
        language_id: Option<&str>,
    ) -> Result<Vec<LspLocation>> {
        self.ensure_document_open(file, language_id).await?;
        self.with_client_request(|client| {
            let file = file.to_path_buf();
            Box::pin(async move { client.references(&file, line, character).await })
        })
        .await
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
        language_id: Option<&str>,
    ) -> Result<Option<LspGotoDefinitionResponse>> {
        self.ensure_document_open(file, language_id).await?;
        self.with_client_request(|client| {
            let file = file.to_path_buf();
            Box::pin(async move { client.definition(&file, line, character).await })
        })
        .await
    }

    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
        language_id: Option<&str>,
    ) -> Result<Option<LspGotoDefinitionResponse>> {
        self.ensure_document_open(file, language_id).await?;
        self.with_client_request(|client| {
            let file = file.to_path_buf();
            Box::pin(async move { client.type_definition(&file, line, character).await })
        })
        .await
    }

    async fn ensure_document_open(&self, file: &Path, language_id: Option<&str>) -> Result<()> {
        let file = normalize_path(file);
        if self.opened_files.read().contains(&file) {
            return Ok(());
        }
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let version = *self.file_versions.write().entry(file.clone()).or_insert(1);
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .did_open_with_language(
                &file,
                text,
                version,
                language_id.unwrap_or(self.default_language_id),
            )
            .await;
        if result.is_ok() {
            self.opened_files.write().insert(file);
        }
        self.handle_request_result(&mut guard, result)
    }

    async fn ensure_started_locked(&self, client: &mut Option<LspClient>) -> Result<()> {
        if self.mode == LspRuntimeMode::ParserOnly {
            *self.status.write() = LspRuntimeStatus::ParserOnly;
            return Err(anyhow!(
                "{} analyzer is configured for parser-only mode",
                self.analyzer_id
            ));
        }
        if client.is_some() {
            return Ok(());
        }
        if self.mode == LspRuntimeMode::Auto
            && self
                .last_start_failure
                .read()
                .as_ref()
                .is_some_and(|instant| instant.elapsed() < START_RETRY_COOLDOWN)
        {
            return Err(anyhow!(
                "{}",
                self.message()
                    .unwrap_or_else(|| self.fallback_message.to_string())
            ));
        }
        let root = self.root.read().clone();
        let binary = (self.resolver)(&self.binary, &root);
        self.start_attempts.fetch_add(1, Ordering::SeqCst);
        let started = timeout(
            Duration::from_secs(8),
            LspClient::start_with_options(
                &binary,
                self.args.clone(),
                &root,
                self.default_language_id,
                self.process_name,
            ),
        )
        .await;
        match started {
            Ok(Ok(started)) => {
                *client = Some(started);
                self.opened_files.write().clear();
                *self.status.write() = LspRuntimeStatus::Ready;
                *self.message.write() = None;
                *self.last_start_failure.write() = None;
                *self.last_warning.write() = None;
                Ok(())
            }
            Ok(Err(error)) => self.handle_start_error(error),
            Err(_) => self.handle_start_error(anyhow!("{} initialize timed out", self.analyzer_id)),
        }
    }

    async fn with_client_request<T, F>(&self, request: F) -> Result<T>
    where
        F: for<'a> FnOnce(
            &'a LspClient,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>,
        >,
    {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = request(guard.as_ref().unwrap()).await;
        self.handle_request_result(&mut guard, result)
    }

    fn handle_request_result<T>(
        &self,
        client: &mut Option<LspClient>,
        result: Result<T>,
    ) -> Result<T> {
        if result.is_err() {
            *client = None;
            *self.status.write() = LspRuntimeStatus::Restarting;
        }
        result
    }

    fn handle_start_error(&self, error: anyhow::Error) -> Result<()> {
        let message = error.to_string();
        *self.message.write() = Some(if self.mode == LspRuntimeMode::Auto {
            self.fallback_message.to_string()
        } else {
            message.clone()
        });
        *self.last_start_failure.write() = Some(Instant::now());
        *self.status.write() = if self.mode == LspRuntimeMode::Auto {
            LspRuntimeStatus::Unavailable
        } else {
            LspRuntimeStatus::Error
        };
        if self.mode == LspRuntimeMode::Auto {
            Err(anyhow!(
                "{} unavailable; using parser fallback: {message}",
                self.analyzer_id
            ))
        } else {
            Err(anyhow!(
                "{} is required but unavailable: {message}",
                self.analyzer_id
            ))
        }
    }

    fn increment_file_version(&self, file: &Path) -> i32 {
        let mut versions = self.file_versions.write();
        let version = versions.entry(normalize_path(file)).or_insert(1);
        *version += 1;
        *version
    }
}

pub struct LspRuntimeConfig {
    pub analyzer_id: &'static str,
    pub process_name: &'static str,
    pub default_language_id: &'static str,
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub mode: LspRuntimeMode,
    pub fallback_message: &'static str,
    pub resolver: BinaryResolver,
    pub root: PathBuf,
}

pub fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_ROOT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn missing_binary_resolver(configured: &Path, _root: &Path) -> PathBuf {
        configured.to_path_buf()
    }

    fn temp_root() -> PathBuf {
        let id = TEST_ROOT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-lsp-runtime-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn runtime(mode: LspRuntimeMode, root: PathBuf) -> LspRuntime {
        LspRuntime::new(LspRuntimeConfig {
            analyzer_id: "mock-lsp",
            process_name: "mock-lsp",
            default_language_id: "plain",
            binary: PathBuf::from("/definitely/missing/mock-lsp"),
            args: Vec::new(),
            mode,
            fallback_message: "mock missing, parser fallback active.",
            resolver: missing_binary_resolver,
            root,
        })
    }

    #[tokio::test]
    async fn respects_cooldown_after_failed_start() {
        let state = runtime(LspRuntimeMode::Auto, temp_root());

        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
    }

    #[tokio::test]
    async fn resets_cooldown_on_root_change() {
        let state = runtime(LspRuntimeMode::Auto, temp_root());

        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
        state.set_root(temp_root()).await;
        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 2);
    }

    #[tokio::test]
    async fn required_missing_analyzer_reports_error_status() {
        let state = runtime(LspRuntimeMode::Required, temp_root());

        let error = state
            .subscribe_notifications()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("mock-lsp is required but unavailable"));
        assert_eq!(state.status(), LspRuntimeStatus::Error);
    }

    #[tokio::test]
    async fn tracks_opened_files_and_versions_for_changed_files() {
        let root = temp_root();
        let file = root.join("main.mock");
        std::fs::write(&file, "one").unwrap();
        let state = runtime(LspRuntimeMode::Auto, root);
        state.insert_open_file_for_test(file.clone(), 7);

        assert!(state.is_file_open(&file));
        assert!(state.sync_changed_file(&file, Some("plain")).await.is_err());
        assert_eq!(state.file_version(&file), Some(8));
    }
}
