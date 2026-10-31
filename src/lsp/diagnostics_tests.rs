use super::*;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

const A: &str = "file:///workspace/a.rs";
const B: &str = "file:///workspace/b.rs";

#[derive(Default)]
struct Harness {
    state: Arc<Mutex<NotificationState>>,
    changed: Notify,
    error: Mutex<Option<String>>,
}

impl Harness {
    fn event(&self, method: &str, params: Value) {
        record_notification(method, params, &self.state);
        self.changed.notify_waiters();
    }

    fn publish(&self, uri: &str, severities: &[u32]) {
        let diagnostics: Vec<_> = severities.iter().map(|severity| json!({
            "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}},
            "severity": severity, "message": "fixture diagnostic"
        })).collect();
        self.event(
            "textDocument/publishDiagnostics",
            json!({"uri": uri, "diagnostics": diagnostics}),
        );
    }

    fn status(&self, quiescent: bool) {
        self.event("experimental/serverStatus", json!({"quiescent": quiescent}));
    }

    fn progress(&self, kind: &str) {
        self.event(
            "$/progress",
            json!({"token": "rust-analyzer/flycheck/0", "value": {"kind": kind}}),
        );
    }

    async fn wait(&self, expected: &HashSet<String>) -> Result<()> {
        wait_for_diagnostics_state(
            &self.state,
            &self.changed,
            &self.error,
            expected,
            0,
            Duration::from_secs(5),
            Some(0),
        )
        .await
    }
}

async fn pending(future: Pin<&mut impl Future<Output = Result<()>>>) {
    let mut future = future;
    std::future::poll_fn(|cx| {
        assert!(
            future.as_mut().poll(cx).is_pending(),
            "wait completed prematurely"
        );
        Poll::Ready(())
    })
    .await;
}

async fn quiet() {
    tokio::time::advance(Duration::from_millis(750)).await;
}

#[tokio::test(start_paused = true)]
async fn initial_empty_publication_waits_for_analysis_and_check() {
    let h = Harness::default();
    let expected = HashSet::from([A.into()]);
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    h.status(false);
    h.publish(A, &[]);
    quiet().await;
    pending(wait.as_mut()).await;
    h.progress("begin");
    h.publish(A, &[1, 2]);
    h.status(true);
    quiet().await;
    pending(wait.as_mut()).await; // quiescent does not complete flycheck
    h.progress("end");
    quiet().await;
    wait.await.unwrap();
    assert_eq!(h.state.lock().unwrap().diagnostics[A].len(), 2);
}

#[tokio::test(start_paused = true)]
async fn latest_publication_replaces_previous_and_restarts_quiet_window() {
    let h = Harness::default();
    let expected = HashSet::from([A.into()]);
    h.status(true);
    h.progress("begin");
    h.publish(A, &[1]);
    h.progress("end");
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    tokio::time::advance(Duration::from_millis(400)).await;
    h.publish(A, &[1, 2]);
    tokio::time::advance(Duration::from_millis(400)).await;
    pending(wait.as_mut()).await;
    quiet().await;
    wait.await.unwrap();
    assert_eq!(h.state.lock().unwrap().diagnostics[A].len(), 2);
}

#[tokio::test(start_paused = true)]
async fn all_documents_are_required_and_retained() {
    let h = Harness::default();
    let expected = HashSet::from([A.into(), B.into()]);
    h.status(true);
    h.progress("begin");
    h.publish(A, &[1]);
    h.progress("end");
    quiet().await;
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    h.publish(B, &[2]);
    quiet().await;
    wait.await.unwrap();
    let state = h.state.lock().unwrap();
    assert_eq!(
        state.diagnostics[A][0].severity,
        Some(lsp_types::DiagnosticSeverity::ERROR)
    );
    assert_eq!(
        state.diagnostics[B][0].severity,
        Some(lsp_types::DiagnosticSeverity::WARNING)
    );
}

#[tokio::test(start_paused = true)]
async fn clean_project_can_finish_with_empty_diagnostics() {
    let h = Harness::default();
    h.publish(A, &[]);
    h.progress("begin");
    h.progress("end");
    h.status(true);
    quiet().await;
    h.wait(&HashSet::from([A.into()])).await.unwrap();
    assert!(h.state.lock().unwrap().diagnostics[A].is_empty());
}

#[tokio::test(start_paused = true)]
async fn process_exit_wakes_waiter_and_returns_error() {
    let h = Harness::default();
    let expected = HashSet::from([A.into()]);
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    *h.error.lock().unwrap() = Some("rust-analyzer stdout closed".into());
    h.changed.notify_waiters();
    assert!(wait
        .await
        .unwrap_err()
        .to_string()
        .contains("stdout closed"));
}

#[tokio::test(start_paused = true)]
async fn timeout_has_bounded_wait_and_state_context() {
    let h = Harness::default();
    let expected = HashSet::from([A.into()]);
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    tokio::time::advance(Duration::from_secs(5)).await;
    let error = wait.await.unwrap_err().to_string();
    assert!(error.contains("timed out"), "{error}");
    assert!(error.contains("missing_documents=1/1"), "{error}");
    assert!(error.contains("completed_checks=0"), "{error}");
}

#[tokio::test(start_paused = true)]
async fn subsequent_analysis_cycle_blocks_previously_ready_snapshot() {
    let h = Harness::default();
    let expected = HashSet::from([A.into()]);
    h.status(true);
    h.publish(A, &[]);
    h.progress("begin");
    h.progress("end");
    h.status(false);
    h.progress("begin");
    quiet().await;
    let wait = h.wait(&expected);
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    h.publish(A, &[1]);
    h.progress("end");
    h.status(true);
    quiet().await;
    wait.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn old_check_end_does_not_confirm_new_check_request() {
    let h = Harness::default();
    h.status(true);
    h.publish(A, &[]);
    h.progress("begin");
    let after = h.state.lock().unwrap().started_checks;
    h.progress("end");
    quiet().await;
    let expected = HashSet::from([A.into()]);
    let wait = wait_for_diagnostics_state(
        &h.state,
        &h.changed,
        &h.error,
        &expected,
        0,
        Duration::from_secs(5),
        Some(after),
    );
    tokio::pin!(wait);
    pending(wait.as_mut()).await;
    h.progress("begin");
    h.progress("end");
    quiet().await;
    wait.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn malformed_diagnostic_notification_is_an_error() {
    let h = Harness::default();
    h.event(
        "textDocument/publishDiagnostics",
        json!({"uri": A, "diagnostics": "invalid"}),
    );
    let error = h.wait(&HashSet::from([A.into()])).await.unwrap_err();
    assert!(error.to_string().contains("invalid publishDiagnostics"));
}
