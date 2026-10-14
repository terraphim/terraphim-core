//! Multi-agent coordination domain.

use serde::{Deserialize, Serialize};
#[cfg(feature = "typescript")]
use tsify::Tsify;

use ahash::AHashMap;

use crate::conversation::ContextItem;

/// Multi-agent context for coordinating between different AI agents
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct MultiAgentContext {
    /// Unique identifier for the multi-agent session
    pub session_id: String,
    /// Agents participating in this context
    pub agents: Vec<AgentInfo>,
    /// Shared context items available to all agents
    pub shared_context: Vec<ContextItem>,
    /// Agent-specific context
    pub agent_contexts: AHashMap<String, Vec<ContextItem>>,
    /// Communication log between agents
    pub agent_communications: Vec<AgentCommunication>,
    /// When this session was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When this session was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl MultiAgentContext {
    /// Creates a new multi-agent context with a fresh session ID and empty agent/shared-context lists.
    pub fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            agents: Vec::new(),
            shared_context: Vec::new(),
            agent_contexts: AHashMap::new(),
            agent_communications: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Add an agent to the session
    pub fn add_agent(&mut self, agent: AgentInfo) {
        self.agents.push(agent.clone());
        self.agent_contexts.insert(agent.id, Vec::new());
        self.updated_at = chrono::Utc::now();
    }

    /// Add context for a specific agent
    pub fn add_agent_context(&mut self, agent_id: &str, context: ContextItem) {
        if let Some(contexts) = self.agent_contexts.get_mut(agent_id) {
            contexts.push(context);
            self.updated_at = chrono::Utc::now();
        }
    }

    /// Record communication between agents
    pub fn record_communication(
        &mut self,
        from_agent: &str,
        to_agent: Option<&str>,
        message: String,
    ) {
        let communication = AgentCommunication {
            from_agent: from_agent.to_string(),
            to_agent: to_agent.map(|s| s.to_string()),
            message,
            timestamp: chrono::Utc::now(),
        };
        self.agent_communications.push(communication);
        self.updated_at = chrono::Utc::now();
    }
}

impl Default for MultiAgentContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about an AI agent in a multi-agent context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct AgentInfo {
    /// Unique identifier for the agent
    pub id: String,
    /// Human-readable name of the agent
    pub name: String,
    /// Role/specialty of the agent
    pub role: String,
    /// Capabilities or description of what this agent does
    pub capabilities: Vec<String>,
    /// Model or provider powering this agent
    pub model: Option<String>,
}

/// Communication between agents in a multi-agent context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(Tsify))]
#[cfg_attr(feature = "typescript", tsify(into_wasm_abi, from_wasm_abi))]
pub struct AgentCommunication {
    /// ID of the agent sending the message
    pub from_agent: String,
    /// ID of the agent receiving the message (None for broadcast)
    pub to_agent: Option<String>,
    /// The communication message
    pub message: String,
    /// When this communication occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
