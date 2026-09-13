//! Session model / mode / effort / fast-mode preference types and warm helpers.
//!
//! Warm applies model + collaboration via [`apply_model_and_collaboration_prefs`].
//! Run's cancel-aware `apply_session_preferences` (model/collab/effort/fast) remains
//! on `RunOnceContext` in `run.rs` because it is tightly coupled to cancellation
//! and `agent:completed` emission.

use crate::features::agent::acp::client::timed_acp_request;
use crate::features::agent::acp::updates::{
    collaboration_from_config_options, models_from_config_options,
};
use agent_client_protocol::schema::v1::{
    SessionConfigId, SessionConfigOption, SessionConfigOptionValue, SessionId,
    SetSessionConfigOptionRequest,
};
use agent_client_protocol::{Agent, ConnectionTo};

/// User-selected session preferences applied after the session opens.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunPreferences {
    pub model_id: Option<String>,
    pub collaboration_mode_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub prefer_highest_reasoning_effort: bool,
    pub fast_mode: Option<bool>,
}

impl RunPreferences {
    pub fn resolve_reasoning_effort(
        &self,
        choices: &[crate::features::agent::models::AgentEffortChoice],
    ) -> Option<String> {
        if self.reasoning_effort.is_some() {
            return self.reasoning_effort.clone();
        }
        if !self.prefer_highest_reasoning_effort {
            return None;
        }
        // ACP supplies no rank. Keep this known ordering in sync with reasoning-effort.ts.
        let order = [
            "none", "off", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ];
        let mut highest = None;
        for choice in choices {
            let rank = order
                .iter()
                .position(|id| *id == choice.id.to_ascii_lowercase())?;
            if highest.as_ref().is_none_or(|(best, _)| rank > *best) {
                highest = Some((rank, choice.id.clone()));
            }
        }
        highest.map(|(_, id)| id)
    }
}

/// Apply preferred model (free-form) and collaboration mode after session/new.
/// Failures are logged and do not abort the warm path.
pub(crate) async fn apply_model_and_collaboration_prefs(
    connection: &ConnectionTo<Agent>,
    runtime_session_id: &str,
    agent_id: &str,
    acp_session_id: &SessionId,
    mut config_options: Vec<SessionConfigOption>,
    preferred_model: Option<String>,
    preferred_collab: Option<String>,
) -> Vec<SessionConfigOption> {
    if let Some(ev) = models_from_config_options(runtime_session_id, agent_id, &config_options) {
        // Attempt preferred model even when not in the advertised catalog
        // (third-party / gateway free-form ids).
        if let Some(pref) = preferred_model {
            if pref != ev.current_id {
                let listed = ev.models.iter().any(|m| m.id == pref);
                match timed_acp_request(
                    "set model",
                    connection
                        .send_request(SetSessionConfigOptionRequest::new(
                            acp_session_id.clone(),
                            SessionConfigId::new(ev.config_id.as_str()),
                            SessionConfigOptionValue::value_id(pref.clone()),
                        ))
                        .block_task(),
                )
                .await
                {
                    Ok(response) => {
                        config_options = response.config_options;
                    }
                    Err(e) => {
                        log::debug!(
                            target: "agentero::agent",
                            "agent={} warm set model failed (listed={}): pref={} err={}",
                            agent_id,
                            listed,
                            pref,
                            e
                        );
                    }
                }
            }
        }
    }
    if let Some(pref) = preferred_collab {
        if let Some(ev) =
            collaboration_from_config_options(runtime_session_id, agent_id, &config_options)
        {
            if pref != ev.current_id && ev.modes.iter().any(|mode| mode.id == pref) {
                match timed_acp_request(
                    "set collaboration mode",
                    connection
                        .send_request(SetSessionConfigOptionRequest::new(
                            acp_session_id.clone(),
                            SessionConfigId::new(ev.config_id.as_str()),
                            SessionConfigOptionValue::value_id(pref.clone()),
                        ))
                        .block_task(),
                )
                .await
                {
                    Ok(response) => {
                        config_options = response.config_options;
                    }
                    Err(e) => {
                        log::debug!(
                            target: "agentero::agent",
                            "agent={} warm set collaboration mode failed: pref={} err={}",
                            agent_id,
                            pref,
                            e
                        );
                    }
                }
            }
        }
    }
    config_options
}

#[cfg(test)]
mod tests {
    use super::RunPreferences;
    use crate::features::agent::models::AgentEffortChoice;

    fn choices(ids: &[&str]) -> Vec<AgentEffortChoice> {
        ids.iter()
            .map(|id| AgentEffortChoice {
                id: id.to_string(),
                name: id.to_string(),
                description: None,
            })
            .collect()
    }

    #[test]
    fn resolves_highest_after_model_setup_without_warm_options() {
        let prefs = RunPreferences {
            prefer_highest_reasoning_effort: true,
            ..Default::default()
        };
        assert_eq!(
            prefs
                .resolve_reasoning_effort(&choices(&["xhigh", "low", "high"]))
                .as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            prefs
                .resolve_reasoning_effort(&choices(&["low", "max", "xhigh"]))
                .as_deref(),
            Some("max")
        );
    }

    #[test]
    fn explicit_effort_takes_precedence_over_highest() {
        let prefs = RunPreferences {
            reasoning_effort: Some("medium".into()),
            prefer_highest_reasoning_effort: true,
            ..Default::default()
        };
        assert_eq!(
            prefs
                .resolve_reasoning_effort(&choices(&["low", "medium", "xhigh"]))
                .as_deref(),
            Some("medium")
        );
    }

    #[test]
    fn no_override_for_other_callers_or_unknown_orderings() {
        assert!(RunPreferences::default()
            .resolve_reasoning_effort(&choices(&["low", "high"]))
            .is_none());
        let prefs = RunPreferences {
            prefer_highest_reasoning_effort: true,
            ..Default::default()
        };
        assert!(prefs
            .resolve_reasoning_effort(&choices(&["deep", "high"]))
            .is_none());
        assert!(prefs.resolve_reasoning_effort(&[]).is_none());
    }
}
