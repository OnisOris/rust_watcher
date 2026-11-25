//! Absolute volume scale. Agent notifications never contribute measured heat.
use super::content::LineDelta;
use std::time::Duration;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Live,
    Session,
}
impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Live => Self::Session,
            Self::Session => Self::Live,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "LIVE · 120s",
            Self::Session => "SESSION",
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Totals {
    pub added: u64,
    pub deleted: u64,
    pub unknown: usize,
    pub measured: usize,
}
impl Totals {
    pub fn churn(self) -> u64 {
        self.added.saturating_add(self.deleted)
    }
    pub fn add(&mut self, d: Option<LineDelta>) {
        if let Some(d) = d {
            self.measured += 1;
            self.added = self.added.saturating_add(u64::from(d.added));
            self.deleted = self.deleted.saturating_add(u64::from(d.deleted));
        } else {
            self.unknown += 1;
        }
    }
    pub fn merge(&mut self, other: Self) {
        self.added = self.added.saturating_add(other.added);
        self.deleted = self.deleted.saturating_add(other.deleted);
        self.unknown += other.unknown;
        self.measured += other.measured;
    }
}
pub fn weighted(churn: u64, age: Duration, mode: Mode) -> f64 {
    if mode == Mode::Session {
        return churn as f64;
    }
    if age >= Duration::from_secs(120) {
        return 0.;
    }
    churn as f64 * 2f64.powf(-age.as_secs_f64() / 45.)
}
pub fn level(volume: f64) -> u32 {
    if volume <= 0. {
        0
    } else if volume < 8. {
        1
    } else if volume < 32. {
        2
    } else if volume < 96. {
        3
    } else if volume < 256. {
        4
    } else {
        5
    }
}
pub fn bar(level: u32) -> &'static str {
    match level {
        0 => "····",
        1 => "░···",
        2 => "▒░··",
        3 => "▓▒░·",
        4 => "█▓▒░",
        _ => "████",
    }
}
