//! Opt-in Adaptive Execution Guard.
//!
//! AlpenCat never owns the application's worker threads or Cgroup. Callers supply
//! resource events, evaluation feedback, and the actual work to admit. No
//! background observers, hidden task scheduling, or automatic OS mutations.
use alpencat_resource::{Freshness, Reading, ResourceSnapshot, ResourceState};
use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Only surface a trusted capacity observation. Never recommend or enforce.
    Observe,
    /// Calculate an application-owned limit without changing admissions.
    Recommend,
    /// Change the shared admission gate only after the application opts in.
    Enforce,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    ZeroLimit,
    InconsistentLimits,
    InvalidThreshold,
    InvalidFeedbackBudget,
}

#[derive(Debug, Clone)]
pub struct GuardConfig {
    pub mode: Mode,
    /// Application-owned profile for an adequately provisioned resource regime.
    pub normal_limit: usize,
    /// Application-owned profile for a limited CPU regime.
    pub constrained_limit: usize,
    /// The safe starting limit, also applied on missing/expired resource data.
    pub fail_safe_limit: usize,
    /// CPU-equivalent quota below which the constrained profile applies.
    pub constrained_below_cpus: f64,
    /// Prevent repeated UPWARD adjustments; reductions and fail-safe are immediate.
    pub upward_cooldown: Duration,
    /// Resource data older than this is expired rather than trusted.
    pub max_observation_age: Duration,
    /// Reject a new profile when app-supplied same-regime p95 regresses past this.
    pub max_p95_regression_pct: f64,
    pub max_cpu_regression_pct: f64,
    pub max_wall_regression_pct: f64,
}
impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Observe,
            normal_limit: 4,
            constrained_limit: 2,
            fail_safe_limit: 2,
            constrained_below_cpus: 1.0,
            upward_cooldown: Duration::from_secs(5),
            max_observation_age: Duration::from_secs(2),
            max_p95_regression_pct: 5.0,
            max_cpu_regression_pct: 5.0,
            max_wall_regression_pct: 5.0,
        }
    }
}
impl GuardConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if [
            self.normal_limit,
            self.constrained_limit,
            self.fail_safe_limit,
        ]
        .contains(&0)
        {
            return Err(ConfigError::ZeroLimit);
        }
        if self.constrained_limit > self.normal_limit || self.fail_safe_limit > self.normal_limit {
            return Err(ConfigError::InconsistentLimits);
        }
        if !self.constrained_below_cpus.is_finite() || self.constrained_below_cpus <= 0.0 {
            return Err(ConfigError::InvalidThreshold);
        }
        if [
            self.max_p95_regression_pct,
            self.max_cpu_regression_pct,
            self.max_wall_regression_pct,
        ]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(ConfigError::InvalidFeedbackBudget);
        }
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug)]
struct GateState {
    limit: usize,
    in_flight: usize,
}
#[derive(Debug)]
struct GateInner {
    state: Mutex<GateState>,
    capacity_changed: Condvar,
}
/// A cloneable admission limiter. It never creates, terminates, or resizes threads.
/// Decreasing the limit never interrupts existing work; it blocks *new* permits.
#[derive(Debug, Clone)]
pub struct AdmissionGate {
    shared: Arc<GateInner>,
}
impl AdmissionGate {
    pub fn new(limit: usize) -> Result<Self, ConfigError> {
        if limit == 0 {
            return Err(ConfigError::ZeroLimit);
        }
        Ok(Self {
            shared: Arc::new(GateInner {
                state: Mutex::new(GateState {
                    limit,
                    in_flight: 0,
                }),
                capacity_changed: Condvar::new(),
            }),
        })
    }
    fn set_limit(&self, new: usize) {
        debug_assert!(new > 0);
        let mut state = lock(&self.shared.state);
        state.limit = new;
        self.shared.capacity_changed.notify_all();
    }
    pub fn limit(&self) -> usize {
        lock(&self.shared.state).limit
    }
    pub fn in_flight(&self) -> usize {
        lock(&self.shared.state).in_flight
    }
    pub fn try_acquire(&self) -> Option<Permit> {
        let mut state = lock(&self.shared.state);
        if state.in_flight >= state.limit {
            return None;
        }
        state.in_flight += 1;
        Some(Permit {
            shared: Arc::clone(&self.shared),
        })
    }
    /// Wait at most timeout. No implicit queue; the application owns requests
    /// and decides whether a timeout should be retried or rejected.
    pub fn acquire_timeout(&self, timeout: Duration) -> Option<Permit> {
        let start = Instant::now();
        let mut state = lock(&self.shared.state);
        loop {
            if state.in_flight < state.limit {
                state.in_flight += 1;
                return Some(Permit {
                    shared: Arc::clone(&self.shared),
                });
            }
            let remaining = timeout.checked_sub(start.elapsed())?;
            let result = self.shared.capacity_changed.wait_timeout(state, remaining);
            state = match result {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}
/// Release happens even on panic unwinding; permits remain valid if Guard is dropped.
#[derive(Debug)]
pub struct Permit {
    shared: Arc<GateInner>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        assert!(state.in_flight > 0, "admission count underflow");
        state.in_flight -= 1;
        self.shared.capacity_changed.notify_one();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regime {
    Normal,
    Constrained,
}
#[derive(Debug)]
struct Controller {
    regime: Option<Regime>,
    last_change: Option<Instant>,
    rollback_target: Option<usize>,
    blocked_in_regime: bool,
    rollback_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    TrustedNormal,
    TrustedConstrained,
    UntrustedFallback,
    UpwardCooldown,
    FeedbackSuppressed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub mode: Mode,
    pub freshness: Freshness,
    pub known_cpu_equivalents: Option<f64>,
    /// None for Observe; otherwise the opt-in proposed admission ceiling.
    pub recommended_limit: Option<usize>,
    pub active_limit: usize,
    pub changed: bool,
    pub reason: Reason,
    pub rollbacks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub p95: Duration,
    pub cpu_time: Duration,
    pub wall_time: Duration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackResult {
    NotEnforcing,
    NoPendingSwitch,
    Accepted,
    RolledBack,
}
fn regresses(baseline: Duration, actual: Duration, allowed_pct: f64) -> bool {
    !baseline.is_zero()
        && actual.as_secs_f64() > baseline.as_secs_f64() * (1.0 + allowed_pct / 100.0)
}

/// Guard decision and admission control are synchronized; resource collection
/// itself must be explicit in ResourceState. Share this Guard across request tasks.
#[derive(Debug)]
pub struct Guard {
    config: GuardConfig,
    gate: AdmissionGate,
    inner: Mutex<Controller>,
}
impl Guard {
    pub fn new(config: GuardConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        let gate = AdmissionGate::new(config.fail_safe_limit)?;
        Ok(Self {
            config,
            gate,
            inner: Mutex::new(Controller {
                regime: None,
                last_change: None,
                rollback_target: None,
                blocked_in_regime: false,
                rollback_count: 0,
            }),
        })
    }
    pub fn gate(&self) -> AdmissionGate {
        self.gate.clone()
    }
    /// Reads *only* the caller-owned cache and age; never refreshes the OS.
    pub fn evaluate(&self, source: &ResourceState) -> Decision {
        let view = source.cached_with_max_age(self.config.max_observation_age);
        self.evaluate_observation(view.freshness, view.fresh())
    }
    /// Explicit injection point for application-owned event adapters and tests.
    /// Untrusted snapshots are never accepted even if the caller passes Some.
    pub fn evaluate_observation(
        &self,
        freshness: Freshness,
        snapshot: Option<&ResourceSnapshot>,
    ) -> Decision {
        let capacity = if freshness == Freshness::Fresh {
            snapshot.and_then(|s| {
                if !s.limits_complete || s.membership_changed_during_read {
                    return None;
                }
                match &s.effective_cpu_capacity {
                    Reading::Known(n) if n.is_finite() && *n > 0.0 => Some(*n),
                    _ => None,
                }
            })
        } else {
            None
        };
        let regime = capacity.map(|n| {
            if n < self.config.constrained_below_cpus {
                Regime::Constrained
            } else {
                Regime::Normal
            }
        });
        let proposed = match regime {
            Some(Regime::Constrained) => self.config.constrained_limit,
            Some(Regime::Normal) => self.config.normal_limit,
            None => self.config.fail_safe_limit,
        };
        let mut reason = match regime {
            Some(Regime::Constrained) => Reason::TrustedConstrained,
            Some(Regime::Normal) => Reason::TrustedNormal,
            None => Reason::UntrustedFallback,
        };
        let mut controller = lock(&self.inner);
        if controller.regime != regime {
            controller.regime = regime;
            controller.blocked_in_regime = false;
            controller.rollback_target = None;
        }
        let mut changed = false;
        if self.config.mode == Mode::Enforce {
            let current = self.gate.limit();
            let mut target = proposed;
            if controller.blocked_in_regime && regime.is_some() {
                target = current;
                reason = Reason::FeedbackSuppressed;
            } else if target > current
                && controller
                    .last_change
                    .is_some_and(|time| time.elapsed() < self.config.upward_cooldown)
            {
                target = current;
                reason = Reason::UpwardCooldown;
            }
            if target != current {
                self.gate.set_limit(target);
                controller.rollback_target = if regime.is_some() {
                    Some(current)
                } else {
                    None
                };
                controller.last_change = Some(Instant::now());
                changed = true;
            }
        }
        Decision {
            mode: self.config.mode,
            freshness,
            known_cpu_equivalents: capacity,
            recommended_limit: (self.config.mode != Mode::Observe).then_some(proposed),
            active_limit: self.gate.limit(),
            changed,
            reason,
            rollbacks: controller.rollback_count,
        }
    }
    /// Caller must supply comparable baseline/candidate outcomes collected
    /// in the SAME resource regime; never compare pre/post Cgroup transition.
    /// Reverts at most once per regime. This evaluates a policy, not request
    /// correctness or real user SLO attainment.
    pub fn report_comparison(&self, baseline: Outcome, candidate: Outcome) -> FeedbackResult {
        if self.config.mode != Mode::Enforce {
            return FeedbackResult::NotEnforcing;
        }
        let mut controller = lock(&self.inner);
        let Some(previous) = controller.rollback_target.take() else {
            return FeedbackResult::NoPendingSwitch;
        };
        let violation = regresses(
            baseline.p95,
            candidate.p95,
            self.config.max_p95_regression_pct,
        ) || regresses(
            baseline.cpu_time,
            candidate.cpu_time,
            self.config.max_cpu_regression_pct,
        ) || regresses(
            baseline.wall_time,
            candidate.wall_time,
            self.config.max_wall_regression_pct,
        );
        if violation {
            self.gate.set_limit(previous);
            controller.blocked_in_regime = true;
            controller.last_change = Some(Instant::now());
            controller.rollback_count += 1;
            FeedbackResult::RolledBack
        } else {
            FeedbackResult::Accepted
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn observed(cpu: f64) -> ResourceSnapshot {
        let mut s = alpencat_resource::snapshot();
        s.limits_complete = true;
        s.membership_changed_during_read = false;
        s.effective_cpu_capacity = Reading::Known(cpu);
        s
    }
    fn enforce() -> Guard {
        Guard::new(GuardConfig {
            mode: Mode::Enforce,
            upward_cooldown: Duration::from_secs(3600),
            ..GuardConfig::default()
        })
        .unwrap()
    }
    #[test]
    fn invalid_config_is_rejected() {
        let c = GuardConfig {
            normal_limit: 0,
            ..GuardConfig::default()
        };
        assert!(matches!(Guard::new(c), Err(ConfigError::ZeroLimit)));
        let c = GuardConfig {
            constrained_below_cpus: f64::NAN,
            ..GuardConfig::default()
        };
        assert!(matches!(Guard::new(c), Err(ConfigError::InvalidThreshold)));
    }
    #[test]
    fn observe_and_recommend_do_not_enforce() {
        let normal = observed(2.0);
        let passive = Guard::new(GuardConfig::default()).unwrap();
        let p = passive.evaluate_observation(Freshness::Fresh, Some(&normal));
        assert_eq!(p.recommended_limit, None);
        assert_eq!(p.known_cpu_equivalents, Some(2.0));
        assert_eq!(p.active_limit, 2);
        let recommender = Guard::new(GuardConfig {
            mode: Mode::Recommend,
            ..GuardConfig::default()
        })
        .unwrap();
        let d = recommender.evaluate_observation(Freshness::Fresh, Some(&normal));
        assert_eq!(d.recommended_limit, Some(4));
        assert_eq!(d.active_limit, 2);
        assert!(!d.changed);
    }
    #[test]
    fn enforce_changes_profiles_and_expired_data_fails_safe() {
        let guard = enforce();
        let high = guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        assert_eq!(high.active_limit, 4);
        assert!(high.changed);
        let low = guard.evaluate_observation(Freshness::Fresh, Some(&observed(0.5)));
        assert_eq!(low.active_limit, 2);
        assert_eq!(low.reason, Reason::TrustedConstrained);
        let invalid = guard.evaluate_observation(Freshness::Expired, Some(&observed(2.0)));
        assert_eq!(invalid.active_limit, 2);
        assert_eq!(invalid.reason, Reason::UntrustedFallback);
        let cooldown = guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        assert_eq!(cooldown.active_limit, 2);
        assert_eq!(cooldown.reason, Reason::UpwardCooldown);
    }
    #[test]
    fn unknown_or_partial_cgroup_fails_closed() {
        let guard = enforce();
        guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        let mut missing = observed(2.0);
        missing.limits_complete = false;
        assert_eq!(
            guard
                .evaluate_observation(Freshness::Fresh, Some(&missing))
                .active_limit,
            2
        );
        missing.limits_complete = true;
        missing.effective_cpu_capacity = Reading::Unknown;
        assert_eq!(
            guard
                .evaluate_observation(Freshness::Fresh, Some(&missing))
                .active_limit,
            2
        );
    }
    #[test]
    fn feedback_rolls_back_and_suppresses_until_regime_changes() {
        let guard = enforce();
        guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        guard.evaluate_observation(Freshness::Fresh, Some(&observed(0.5)));
        let reference = Outcome {
            p95: Duration::from_millis(100),
            cpu_time: Duration::from_secs(1),
            wall_time: Duration::from_secs(2),
        };
        let worse = Outcome {
            p95: Duration::from_millis(120),
            ..reference
        };
        assert_eq!(
            guard.report_comparison(reference, worse),
            FeedbackResult::RolledBack
        );
        assert_eq!(guard.gate().limit(), 4);
        let decision = guard.evaluate_observation(Freshness::Fresh, Some(&observed(0.5)));
        assert_eq!(decision.reason, Reason::FeedbackSuppressed);
        assert_eq!(decision.active_limit, 4);
        assert_eq!(decision.rollbacks, 1);
        assert_eq!(
            guard.report_comparison(reference, worse),
            FeedbackResult::NoPendingSwitch
        );
        let changed = guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        assert_eq!(changed.active_limit, 4);
        let restored = guard.evaluate_observation(Freshness::Fresh, Some(&observed(0.5)));
        assert_eq!(restored.active_limit, 2);
    }
    #[test]
    fn successful_feedback_clears_pending_switch() {
        let guard = enforce();
        guard.evaluate_observation(Freshness::Fresh, Some(&observed(2.0)));
        let metric = Outcome {
            p95: Duration::from_millis(100),
            cpu_time: Duration::from_millis(500),
            wall_time: Duration::from_millis(600),
        };
        assert_eq!(
            guard.report_comparison(metric, metric),
            FeedbackResult::Accepted
        );
        assert_eq!(
            guard.report_comparison(metric, metric),
            FeedbackResult::NoPendingSwitch
        );
    }
    #[test]
    fn gate_enforces_limit_and_does_not_revoke_running_permits() {
        let gate = AdmissionGate::new(2).unwrap();
        let p1 = gate.try_acquire().unwrap();
        let p2 = gate.try_acquire().unwrap();
        assert!(gate.try_acquire().is_none());
        gate.set_limit(1);
        assert_eq!(gate.in_flight(), 2);
        drop(p1);
        assert!(gate.try_acquire().is_none());
        drop(p2);
        let p3 = gate.try_acquire().unwrap();
        assert_eq!(gate.in_flight(), 1);
        drop(p3);
        assert_eq!(gate.in_flight(), 0);
    }
    #[test]
    fn blocked_waiters_observe_release_and_timeout() {
        let gate = AdmissionGate::new(1).unwrap();
        let p = gate.try_acquire().unwrap();
        assert!(gate.acquire_timeout(Duration::from_millis(5)).is_none());
        let copy = gate.clone();
        let success = Arc::new(AtomicUsize::new(0));
        let updated = Arc::clone(&success);
        let handle = std::thread::spawn(move || {
            let permit = copy.acquire_timeout(Duration::from_secs(2)).unwrap();
            updated.fetch_add(1, Ordering::SeqCst);
            drop(permit);
        });
        drop(p);
        handle.join().unwrap();
        assert_eq!(success.load(Ordering::SeqCst), 1);
        assert_eq!(gate.in_flight(), 0);
    }
}
