use crate::analyzer_paths::resolve_qmlls;
use anyhow::Result;
use clap::ValueEnum;
use graph_core::AnalyzerStatus;
use ra_client::{
    LspLocation, LspNotification, LspRuntime, LspRuntimeConfig, LspRuntimeMode, LspRuntimeStatus,
};
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;

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
    mode: QmlAnalyzerMode,
    runtime: LspRuntime,
}

impl QmlLspState {
    pub fn new(
        binary: PathBuf,
        mode: QmlAnalyzerMode,
        build_dir: Option<PathBuf>,
        no_cmake_calls: bool,
        root: PathBuf,
    ) -> Self {
        let runtime_mode = match mode {
            QmlAnalyzerMode::Auto => LspRuntimeMode::Auto,
            QmlAnalyzerMode::Parser => LspRuntimeMode::ParserOnly,
            QmlAnalyzerMode::Qmlls => LspRuntimeMode::Required,
        };
        let args = qmlls_args(build_dir.as_ref(), no_cmake_calls);
        Self {
            mode,
            runtime: LspRuntime::new(LspRuntimeConfig {
                analyzer_id: "qmlls",
                process_name: "qmlls",
                default_language_id: "qml",
                binary,
                args,
                mode: runtime_mode,
                fallback_message: QMLLS_AUTO_FALLBACK_MESSAGE,
                resolver: resolve_qmlls,
                root,
            }),
        }
    }

    pub fn is_parser_only(&self) -> bool {
        self.runtime.is_parser_only()
    }

    pub fn status_record(&self) -> QmlAnalyzerStatus {
        QmlAnalyzerStatus {
            mode: match self.mode {
                QmlAnalyzerMode::Auto => "auto",
                QmlAnalyzerMode::Parser => "parser",
                QmlAnalyzerMode::Qmlls => "qmlls",
            }
            .to_string(),
            status: QmllsRuntimeStatus::from(self.runtime.status())
                .as_str()
                .to_string(),
            message: self.runtime.message(),
        }
    }

    pub fn should_log_start_failure(&self) -> bool {
        self.runtime.should_log_start_failure()
    }

    #[cfg(test)]
    fn start_attempts(&self) -> usize {
        self.runtime.start_attempts()
    }

    pub async fn set_root(&self, root: PathBuf) {
        self.runtime.set_root(root).await;
    }

    pub async fn subscribe_notifications(&self) -> Result<broadcast::Receiver<LspNotification>> {
        self.runtime.subscribe_notifications().await
    }

    pub async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        self.runtime.sync_changed_file(file, Some("qml")).await
    }

    pub async fn open_document(&self, file: &Path) -> Result<()> {
        self.runtime.open_document(file, Some("qml")).await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>> {
        self.runtime
            .references(file, line, character, Some("qml"))
            .await
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .definition(file, line, character, Some("qml"))
            .await
    }
}

impl From<LspRuntimeStatus> for QmllsRuntimeStatus {
    fn from(status: LspRuntimeStatus) -> Self {
        match status {
            LspRuntimeStatus::ParserOnly => Self::ParserOnly,
            LspRuntimeStatus::Ready => Self::Ready,
            LspRuntimeStatus::Unavailable => Self::Unavailable,
            LspRuntimeStatus::Restarting => Self::Restarting,
            LspRuntimeStatus::Error => Self::Error,
        }
    }
}

fn qmlls_args(build_dir: Option<&PathBuf>, no_cmake_calls: bool) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(build_dir) = build_dir {
        args.push("--build-dir".to_string());
        args.push(build_dir.display().to_string());
    }
    if no_cmake_calls {
        args.push("--no-cmake-calls".to_string());
    }
    args
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

    #[test]
    fn qmlls_startup_args_include_build_dir_and_no_cmake_calls() {
        let build_dir = PathBuf::from("/tmp/qml-build");

        assert_eq!(
            qmlls_args(Some(&build_dir), true),
            vec![
                "--build-dir".to_string(),
                "/tmp/qml-build".to_string(),
                "--no-cmake-calls".to_string()
            ]
        );
    }
}
