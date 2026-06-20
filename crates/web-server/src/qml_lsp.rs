use crate::analyzer_paths::resolve_qmlls;
use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use graph_core::AnalyzerStatus;
use parking_lot::RwLock;
use ra_client::{LspLocation, LspNotification};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::time::{timeout, Duration};

const START_RETRY_COOLDOWN: Duration = Duration::from_secs(30);
const QMLLS_AUTO_FALLBACK_MESSAGE: &str =
    "qmlls not found, parser fallback active. Install Qt/qmlls or pass --qmlls-path.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum QmlAnalyzerMode {
    Auto,
    Parser,
    Qmlls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QmllsRuntimeStatus {
    ParserOnly,
    Ready,
    Unavailable,
    Restarting,
    Error,
}

impl QmllsRuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::ParserOnly => "parser only",
            Self::Ready => "qmlls ready",
            Self::Unavailable => "qmlls unavailable",
            Self::Restarting => "qmlls restarting",
            Self::Error => "qmlls error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QmlAnalyzerStatus {
    pub mode: String,
    pub status: String,
    pub message: Option<String>,
}

pub struct QmlLspState {
    binary: PathBuf,
    mode: QmlAnalyzerMode,
    build_dir: Option<PathBuf>,
    no_cmake_calls: bool,
    root: RwLock<PathBuf>,
    client: AsyncMutex<Option<ra_client::RaClient>>,
    opened_files: RwLock<HashSet<PathBuf>>,
    file_versions: RwLock<HashMap<PathBuf, i32>>,
    status: RwLock<QmllsRuntimeStatus>,
    message: RwLock<Option<String>>,
    last_start_failure: RwLock<Option<Instant>>,
    last_warning: RwLock<Option<Instant>>,
    start_attempts: AtomicUsize,
}

impl QmlLspState {
    pub fn new(
        binary: PathBuf,
        mode: QmlAnalyzerMode,
        build_dir: Option<PathBuf>,
        no_cmake_calls: bool,
        root: PathBuf,
    ) -> Self {
        let initial = if mode == QmlAnalyzerMode::Parser {
            QmllsRuntimeStatus::ParserOnly
        } else {
            QmllsRuntimeStatus::Unavailable
        };
        Self {
            binary,
            mode,
            build_dir,
            no_cmake_calls,
            root: RwLock::new(root),
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
        self.mode == QmlAnalyzerMode::Parser
    }

    pub fn status_record(&self) -> QmlAnalyzerStatus {
        QmlAnalyzerStatus {
            mode: match self.mode {
                QmlAnalyzerMode::Auto => "auto",
                QmlAnalyzerMode::Parser => "parser",
                QmlAnalyzerMode::Qmlls => "qmlls",
            }
            .to_string(),
            status: self.status.read().as_str().to_string(),
            message: self.message.read().clone(),
        }
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

    #[cfg(test)]
    fn start_attempts(&self) -> usize {
        self.start_attempts.load(Ordering::SeqCst)
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
        *self.status.write() = if self.mode == QmlAnalyzerMode::Parser {
            QmllsRuntimeStatus::ParserOnly
        } else {
            QmllsRuntimeStatus::Unavailable
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

    pub async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        let file = normalize_path(file);
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        if !self.opened_files.read().contains(&file) {
            self.ensure_document_open(&file).await?;
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
        if result.is_err() {
            *guard = None;
            *self.status.write() = QmllsRuntimeStatus::Restarting;
        }
        result.map(|_| version)
    }

    pub async fn open_document(&self, file: &Path) -> Result<()> {
        self.ensure_document_open(file).await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>> {
        self.ensure_document_open(file).await?;
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .references(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = QmllsRuntimeStatus::Restarting;
        }
        result
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.ensure_document_open(file).await?;
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .definition(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = QmllsRuntimeStatus::Restarting;
        }
        result
    }

    async fn ensure_document_open(&self, file: &Path) -> Result<()> {
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
            .did_open_with_language(&file, text, version, "qml")
            .await;
        if result.is_ok() {
            self.opened_files.write().insert(file);
        } else {
            *guard = None;
            *self.status.write() = QmllsRuntimeStatus::Restarting;
        }
        result
    }

    async fn ensure_started_locked(&self, client: &mut Option<ra_client::RaClient>) -> Result<()> {
        if self.mode == QmlAnalyzerMode::Parser {
            *self.status.write() = QmllsRuntimeStatus::ParserOnly;
            return Err(anyhow!("QML analyzer is configured for parser-only mode"));
        }
        if client.is_some() {
            return Ok(());
        }
        if self.mode == QmlAnalyzerMode::Auto
            && self
                .last_start_failure
                .read()
                .as_ref()
                .is_some_and(|instant| instant.elapsed() < START_RETRY_COOLDOWN)
        {
            return Err(anyhow!(
                "{}",
                self.message
                    .read()
                    .clone()
                    .unwrap_or_else(|| QMLLS_AUTO_FALLBACK_MESSAGE.into())
            ));
        }
        let root = self.root.read().clone();
        let binary = resolve_qmlls(&self.binary, &root);
        let mut args = Vec::new();
        if let Some(build_dir) = &self.build_dir {
            args.push("--build-dir".to_string());
            args.push(build_dir.display().to_string());
        }
        if self.no_cmake_calls {
            args.push("--no-cmake-calls".to_string());
        }
        self.start_attempts.fetch_add(1, Ordering::SeqCst);
        let started = timeout(
            Duration::from_secs(8),
            ra_client::RaClient::start_with_options(&binary, args, &root, "qml", "qmlls"),
        )
        .await;
        match started {
            Ok(Ok(started)) => {
                *client = Some(started);
                self.opened_files.write().clear();
                *self.status.write() = QmllsRuntimeStatus::Ready;
                *self.message.write() = None;
                *self.last_start_failure.write() = None;
                *self.last_warning.write() = None;
                Ok(())
            }
            Ok(Err(error)) => self.handle_start_error(error),
            Err(_) => self.handle_start_error(anyhow!("qmlls initialize timed out")),
        }
    }

    fn handle_start_error(&self, error: anyhow::Error) -> Result<()> {
        let message = error.to_string();
        *self.message.write() = Some(if self.mode == QmlAnalyzerMode::Auto {
            QMLLS_AUTO_FALLBACK_MESSAGE.into()
        } else {
            message.clone()
        });
        *self.last_start_failure.write() = Some(Instant::now());
        *self.status.write() = if self.mode == QmlAnalyzerMode::Auto {
            QmllsRuntimeStatus::Unavailable
        } else {
            QmllsRuntimeStatus::Error
        };
        if self.mode == QmlAnalyzerMode::Auto {
            Err(anyhow!(
                "qmlls unavailable; using parser fallback: {message}"
            ))
        } else {
            Err(anyhow!("qmlls is required but unavailable: {message}"))
        }
    }

    fn increment_file_version(&self, file: &Path) -> i32 {
        let mut versions = self.file_versions.write();
        let version = versions.entry(normalize_path(file)).or_insert(1);
        *version += 1;
        *version
    }
}

pub fn status_to_analyzer_status(status: &str) -> AnalyzerStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("ready") {
        AnalyzerStatus::Ready
    } else if status.contains("restart") || status.contains("starting") {
        AnalyzerStatus::Starting
    } else if status.contains("error") {
        AnalyzerStatus::Error
    } else if status.contains("unavailable") || status.contains("parser only") {
        AnalyzerStatus::Fallback
    } else {
        AnalyzerStatus::Stale
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_mode_reports_parser_only_status() {
        let state = QmlLspState::new(
            PathBuf::from("qmlls"),
            QmlAnalyzerMode::Parser,
            None,
            true,
            PathBuf::from("."),
        );
        let status = state.status_record();
        assert_eq!(status.mode, "parser");
        assert_eq!(status.status, "parser only");
    }

    #[tokio::test]
    async fn qmlls_unavailable_in_auto_reports_fallback_and_uses_cooldown() {
        let missing = std::env::temp_dir().join(format!("missing-qmlls-{}", uuid::Uuid::new_v4()));
        let state = QmlLspState::new(
            missing,
            QmlAnalyzerMode::Auto,
            None,
            true,
            PathBuf::from("."),
        );

        assert!(state.subscribe_notifications().await.is_err());
        let status = state.status_record();
        assert_eq!(status.status, "qmlls unavailable");
        assert_eq!(
            status.message.as_deref(),
            Some("qmlls not found, parser fallback active. Install Qt/qmlls or pass --qmlls-path.")
        );
        assert_eq!(state.start_attempts(), 1);

        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
    }
}
