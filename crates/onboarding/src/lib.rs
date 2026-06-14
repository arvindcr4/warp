// Onboarding library crate

mod agent_onboarding_view;
pub mod callout;
mod model;
pub mod slides;
pub mod telemetry;

/// The user's intention selected during onboarding slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingIntention {
    Terminal,
    AgentDrivenDevelopment,
}

impl std::fmt::Display for OnboardingIntention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnboardingIntention::AgentDrivenDevelopment => write!(f, "agent_driven"),
            OnboardingIntention::Terminal => write!(f, "terminal"),
        }
    }
}

pub use callout::{OnboardingCalloutView, OnboardingKeybindings};

/// User-facing names of the AI features enabled when the agent intention is selected.
/// Shared by the intention slide's agent card checklist and the login slide's
/// skip-login confirmation dialog so the two always stay in sync.
pub const AI_FEATURES: &[&str] = &[
    "Warp agents",
    "Oz cloud agents platform",
    "Next command predictions",
    "Prompt suggestions",
    "Codebase context",
    "Remote control with Claude Code, Codex, and other agents",
    "Agents over SSH",
];

/// User-facing names of the Warp Drive features enabled when the terminal
/// intention is selected with Warp Drive turned on. Shared by the login slide's
/// skip-login confirmation dialog so the list stays in sync with any future
/// surfaces that need it.
pub const WARP_DRIVE_FEATURES: &[&str] = &["Warp Drive", "Session Sharing"];

cfg_if::cfg_if! {
    if #[cfg(feature = "bin")] {
        mod telemetry_provider;
        pub use telemetry_provider::MockTelemetryContextProvider;
    }
}

pub mod components;
mod visuals;

/// The default mode for new sessions, chosen during onboarding.
/// Mapped to `DefaultSessionMode` at the application boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionDefault {
    #[default]
    Agent,
    Terminal,
}

impl std::fmt::Display for SessionDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionDefault::Agent => write!(f, "agent"),
            SessionDefault::Terminal => write!(f, "terminal"),
        }
    }
}

pub use agent_onboarding_view::{AgentOnboardingAction, AgentOnboardingEvent, AgentOnboardingView};
pub use model::{OnboardingAuthState, SelectedSettings, UICustomizationSettings};
pub use slides::ProjectOnboardingSettings;
pub use telemetry::OnboardingEvent;

pub fn init(app: &mut warpui_core::AppContext) {
    agent_onboarding_view::init(app);
    callout::init(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_intention_display_is_stable() {
        assert_eq!(OnboardingIntention::Terminal.to_string(), "terminal");
        assert_eq!(
            OnboardingIntention::AgentDrivenDevelopment.to_string(),
            "agent_driven"
        );
    }

    #[test]
    fn ai_features_is_non_empty_and_dedup() {
        assert!(!AI_FEATURES.is_empty(), "AI_FEATURES must not be empty");
        let unique: std::collections::HashSet<_> = AI_FEATURES.iter().copied().collect();
        assert_eq!(
            unique.len(),
            AI_FEATURES.len(),
            "AI_FEATURES must not contain duplicates: {:?}",
            AI_FEATURES
        );
    }

    #[test]
    fn warp_drive_features_is_non_empty_and_dedup() {
        assert!(
            !WARP_DRIVE_FEATURES.is_empty(),
            "WARP_DRIVE_FEATURES must not be empty"
        );
        let unique: std::collections::HashSet<_> = WARP_DRIVE_FEATURES.iter().copied().collect();
        assert_eq!(
            unique.len(),
            WARP_DRIVE_FEATURES.len(),
            "WARP_DRIVE_FEATURES must not contain duplicates: {:?}",
            WARP_DRIVE_FEATURES
        );
    }

    #[test]
    fn feature_lists_disjoint() {
        for ai in AI_FEATURES {
            assert!(
                !WARP_DRIVE_FEATURES.contains(ai),
                "{ai:?} appears in both AI_FEATURES and WARP_DRIVE_FEATURES",
            );
        }
    }

    #[test]
    fn session_default_display() {
        assert_eq!(SessionDefault::Agent.to_string(), "agent");
        assert_eq!(SessionDefault::Terminal.to_string(), "terminal");
        assert_eq!(SessionDefault::default(), SessionDefault::Agent);
    }

    #[cfg(feature = "bin")]
    #[test]
    fn mock_telemetry_context_provider_registers_in_module() {
        // Compile-only check: MockTelemetryContextProvider is only available behind
        // `feature = "bin"` and must be re-exported from this crate.
        use warp_core::telemetry::TelemetryContextProvider as _;
        fn _assert_provider<T: TelemetryContextProvider>() {}
        _assert_provider::<MockTelemetryContextProvider>();
    }
}
