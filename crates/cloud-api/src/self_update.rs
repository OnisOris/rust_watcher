use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::warn;

use crate::auth::require_cloud_auth;
use crate::errors::ApiError;
use crate::state::{CloudApiState, SelfUpdateConfig};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudUpdateStatusResponse {
    pub(crate) current_version: String,
    pub(crate) latest_version: Option<String>,
    pub(crate) update_available: bool,
    pub(crate) updating: bool,
    pub(crate) release_url: Option<String>,
    pub(crate) asset_name: Option<String>,
    pub(crate) message: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct GithubReleaseResponse {
    pub(crate) tag_name: String,
    pub(crate) html_url: Option<String>,
    pub(crate) assets: Vec<GithubReleaseAssetResponse>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct GithubReleaseAssetResponse {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
}
#[derive(Debug, Clone)]
pub(crate) struct LatestReleaseAsset {
    pub(crate) tag_name: String,
    pub(crate) html_url: Option<String>,
    pub(crate) asset_name: String,
    pub(crate) download_url: String,
}

pub(crate) async fn cloud_update_status(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    match cloud_update_status_response(&state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_update_apply(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    let release = match latest_release_asset(&state.update_config).await {
        Ok(Some(release)) => release,
        Ok(None) => {
            return ApiError::BadRequest("release artifact not found".into()).into_response()
        }
        Err(error) => return error.into_response(),
    };
    let current_version = current_app_version(&state.update_config);
    if release.tag_name == current_version {
        return Json(CloudUpdateStatusResponse {
            current_version,
            latest_version: Some(release.tag_name),
            update_available: false,
            updating: false,
            release_url: release.html_url,
            asset_name: Some(release.asset_name),
            message: Some("Already up to date".into()),
        })
        .into_response();
    }
    {
        let mut update_state = state.update_state.write();
        if update_state.running {
            return ApiError::Conflict("update already running".into()).into_response();
        }
        update_state.running = true;
        update_state.last_message = Some(format!("Updating to {}", release.tag_name));
    }
    start_self_update(state.clone(), release.clone());
    Json(CloudUpdateStatusResponse {
        current_version,
        latest_version: Some(release.tag_name),
        update_available: true,
        updating: true,
        release_url: release.html_url,
        asset_name: Some(release.asset_name),
        message: Some("Update started. The service will restart shortly.".into()),
    })
    .into_response()
}
pub(crate) async fn cloud_update_status_response(
    state: &CloudApiState,
) -> Result<CloudUpdateStatusResponse, ApiError> {
    let current_version = current_app_version(&state.update_config);
    let release = latest_release_asset(&state.update_config).await?;
    let update_state = state.update_state.read().clone();
    Ok(CloudUpdateStatusResponse {
        update_available: release
            .as_ref()
            .is_some_and(|release| release.tag_name != current_version),
        latest_version: release.as_ref().map(|release| release.tag_name.clone()),
        release_url: release
            .as_ref()
            .and_then(|release| release.html_url.clone()),
        asset_name: release.as_ref().map(|release| release.asset_name.clone()),
        current_version,
        updating: update_state.running,
        message: update_state.last_message,
    })
}
pub(crate) async fn latest_release_asset(
    config: &SelfUpdateConfig,
) -> Result<Option<LatestReleaseAsset>, ApiError> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        config.repository
    );
    let release = reqwest::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, "rust-watcher-cloud-api")
        .send()
        .await
        .map_err(|error| {
            ApiError::BadRequest(format!("failed to query GitHub release: {error}"))
        })?;
    if release.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release = release
        .error_for_status()
        .map_err(|error| ApiError::BadRequest(format!("failed to query GitHub release: {error}")))?
        .json::<GithubReleaseResponse>()
        .await
        .map_err(|error| {
            ApiError::BadRequest(format!("invalid GitHub release response: {error}"))
        })?;
    Ok(release
        .assets
        .into_iter()
        .find(|asset| {
            asset.name.starts_with(&config.asset_prefix) && asset.name.ends_with(".tar.gz")
        })
        .map(|asset| LatestReleaseAsset {
            tag_name: release.tag_name,
            html_url: release.html_url,
            asset_name: asset.name,
            download_url: asset.browser_download_url,
        }))
}
pub(crate) fn current_app_version(config: &SelfUpdateConfig) -> String {
    std::fs::read_to_string(config.app_root.join("VERSION"))
        .ok()
        .and_then(|version| {
            version
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")))
}
pub(crate) fn start_self_update(state: CloudApiState, release: LatestReleaseAsset) {
    tokio::spawn(async move {
        let result = run_self_update_script(&state.update_config, &release).await;
        if let Err(error) = result {
            warn!(%error, "self update failed");
            let mut update_state = state.update_state.write();
            update_state.running = false;
            update_state.last_message = Some(format!("Update failed: {error}"));
        }
    });
}
pub(crate) async fn run_self_update_script(
    config: &SelfUpdateConfig,
    release: &LatestReleaseAsset,
) -> Result<()> {
    let script = r#"
set -euo pipefail
workdir="$(mktemp -d)"
cleanup() {
  rm -rf "${workdir}"
}
trap cleanup EXIT

archive="${workdir}/update.tar.gz"
curl -fsSL "${ASSET_URL}" -o "${archive}"
tar -xzf "${archive}" -C "${workdir}"
package_dir="$(find "${workdir}" -mindepth 1 -maxdepth 1 -type d | head -n 1)"

test -x "${package_dir}/bin/cloud-api"
test -x "${package_dir}/bin/local-agent"
test -f "${package_dir}/frontend/dist/index.html"

mkdir -p "${APP_ROOT}/target/release" "${APP_ROOT}/frontend"
install -m 755 "${package_dir}/bin/cloud-api" "${APP_ROOT}/target/release/cloud-api"
install -m 755 "${package_dir}/bin/local-agent" "${APP_ROOT}/target/release/local-agent"
rm -rf "${APP_ROOT}/frontend/dist"
cp -a "${package_dir}/frontend/dist" "${APP_ROOT}/frontend/dist"
cp "${package_dir}/VERSION" "${APP_ROOT}/VERSION"

systemctl --user restart "${SERVICE_NAME}"
"#;
    let output = Command::new("bash")
        .arg("-lc")
        .arg(script)
        .env("ASSET_URL", &release.download_url)
        .env("APP_ROOT", &config.app_root)
        .env("SERVICE_NAME", &config.service_name)
        .output()
        .await
        .context("failed to run self update script")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("self update script failed: {stderr}");
}
