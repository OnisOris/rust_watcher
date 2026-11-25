//! Camera policy, independent of rendering and physical scanning.
use super::*;
impl State {
    pub(super) fn pause_camera(&mut self, now: Instant) {
        self.manual_until = Some(now + Duration::from_secs(15));
    }
    pub(super) fn camera_active(&self, now: Instant) -> bool {
        self.follow
            && self.auto == Auto::Follow
            && self.manual_until.is_none_or(|until| now >= until)
    }
    pub(super) fn update_focus(&mut self, now: Instant) {
        if !self.follow
            && self
                .focus
                .as_ref()
                .is_some_and(|p| self.snapshot.files.contains_key(p))
        {
            return;
        }
        let Some(hottest) = self.snapshot.hottest().cloned() else {
            self.focus = None;
            self.candidate = None;
            return;
        };
        let Some(current) = self
            .focus
            .as_ref()
            .filter(|p| self.snapshot.files.contains_key(*p))
            .cloned()
        else {
            self.focus = Some(hottest);
            self.candidate = None;
            return;
        };
        let candidate_cluster = self.snapshot.clusters.first().cloned().unwrap_or_default();
        let current_cluster = current.parent().unwrap_or(Path::new(""));
        if candidate_cluster == current_cluster {
            self.candidate = None;
            // Keep the same leaf inside a stable cluster; heat ranks its neighbors.
            return;
        }
        let cluster_volume = |p: &Path| {
            self.snapshot
                .files
                .iter()
                .filter(|(f, _)| f.parent().unwrap_or(Path::new("")) == p)
                .map(|(_, f)| f.volume)
                .sum::<f64>()
        };
        let old = cluster_volume(current_cluster);
        let new = cluster_volume(&candidate_cluster);
        if new <= 0. || (old > 0. && new < old * 1.5) {
            self.candidate = None;
            return;
        }
        match &self.candidate {
            Some((p, since))
                if p == &candidate_cluster
                    && now.saturating_duration_since(*since) >= Duration::from_secs(1) =>
            {
                self.focus = self
                    .snapshot
                    .ranked_files
                    .iter()
                    .find(|p| p.parent().unwrap_or(Path::new("")) == candidate_cluster)
                    .cloned();
                self.candidate = None;
            }
            Some((p, _)) if p == &candidate_cluster => {}
            _ => self.candidate = Some((candidate_cluster, now)),
        }
    }
}
