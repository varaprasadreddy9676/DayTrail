#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContext {
    pub provider_configured: bool,
    pub queued_tasks: usize,
    pub unclosed_loops: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMode {
    ConfigureProvider,
    FollowUp,
    PlanWork,
    Summarize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDecision {
    pub mode: AgentMode,
    pub reason: String,
}

pub fn decide_next_action(context: &AgentContext) -> AgentDecision {
    if !context.provider_configured {
        return AgentDecision {
            mode: AgentMode::ConfigureProvider,
            reason: "AI provider is not configured".to_string(),
        };
    }

    if context.unclosed_loops > 0 {
        return AgentDecision {
            mode: AgentMode::FollowUp,
            reason: format!("{} unclosed loop(s) need attention", context.unclosed_loops),
        };
    }

    if context.queued_tasks > 0 {
        return AgentDecision {
            mode: AgentMode::PlanWork,
            reason: format!("{} queued task(s) are ready to plan", context.queued_tasks),
        };
    }

    AgentDecision {
        mode: AgentMode::Summarize,
        reason: "no pending actions".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prioritizes_configuration_then_followups_then_task_planning() {
        let mut context = AgentContext {
            provider_configured: false,
            queued_tasks: 3,
            unclosed_loops: 2,
        };
        assert_eq!(
            decide_next_action(&context).mode,
            AgentMode::ConfigureProvider
        );

        context.provider_configured = true;
        assert_eq!(decide_next_action(&context).mode, AgentMode::FollowUp);

        context.unclosed_loops = 0;
        assert_eq!(decide_next_action(&context).mode, AgentMode::PlanWork);

        context.queued_tasks = 0;
        assert_eq!(decide_next_action(&context).mode, AgentMode::Summarize);
    }
}
