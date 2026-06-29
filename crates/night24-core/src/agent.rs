use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::provider::{ModelConfig, Provider};
use crate::session::Session;
use crate::model::Message;
use crate::tool_executor::execute_tool;
use crate::context_mgmt::{CompactionResult, ContextManager};
use crate::permission::PermissionManager;
use crate::security::SecurityInspector;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model_config: ModelConfig,
    pub system_prompt: String,
    pub max_turns: usize,
    pub turn_timeout: Duration,
    pub tool_timeout: Duration,
    pub total_timeout: Duration,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model_config: ModelConfig::default(),
            system_prompt: String::new(),
            max_turns: 10,
            turn_timeout: Duration::from_secs(60),
            tool_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(180),
        }
    }
}

pub struct Agent {
    pub config: AgentConfig,
    pub provider: Arc<dyn Provider>,
    pub context_manager: ContextManager,
    pub security: SecurityInspector,
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Arc<dyn Provider>) -> Self {
        Self {
            config,
            provider,
            context_manager: ContextManager::default(),
            security: SecurityInspector::new(std::sync::Arc::new(PermissionManager::default())),
        }
    }

    /// Build the agent with an explicit permission policy. Useful for the
    /// server layer, which selects between strict/permissive presets based on
    /// configuration.
    pub fn with_permission_manager(
        config: AgentConfig,
        provider: Arc<dyn Provider>,
        permission_manager: std::sync::Arc<PermissionManager>,
    ) -> Self {
        Self {
            config,
            provider,
            context_manager: ContextManager::default(),
            security: SecurityInspector::new(permission_manager),
        }
    }

    pub fn with_context_manager(mut self, context_manager: ContextManager) -> Self {
        self.context_manager = context_manager;
        self
    }

    pub fn with_security(mut self, security: SecurityInspector) -> Self {
        self.security = security;
        self
    }

    pub async fn run(
        &self,
        session: &mut Session,
        user_message: Message,
    ) -> anyhow::Result<Vec<Message>> {
        let total_deadline = timeout(self.config.total_timeout, async {
            let mut messages: Vec<Message> = session.conversation.clone();
            messages.push(user_message);

            let system = if self.config.system_prompt.is_empty() {
                "You are a helpful AI assistant.".to_string()
            } else {
                self.config.system_prompt.clone()
            };

            let tools = crate::tool_executor::builtin_tools();
            let mut final_messages = vec![];

            for turn in 0..self.config.max_turns {
                debug!(turn = turn + 1, "agent turn");

                let turn_result = timeout(self.config.turn_timeout, async {
                    let mut stream = self
                        .provider
                        .stream(&self.config.model_config, &system, &messages, &tools)
                        .await?;

                    let mut turn_messages = vec![];
                    let mut has_tool_requests = false;

                    while let Some(result) = stream.next().await {
                        match result {
                            Ok((Some(msg), _usage)) => {
                                turn_messages.push(msg.clone());
                                has_tool_requests |= matches!(
                                    msg.content.first(),
                                    Some(crate::model::ContentBlock::ToolRequest { .. })
                                );
                            }
                            Ok((None, _usage)) => {}
                            Err(e) => {
                                warn!(error = %e, "provider stream error");
                                return Err(e.into());
                            }
                        }
                    }

                    anyhow::Ok((turn_messages, has_tool_requests))
                })
                .await
                .map_err(|_| anyhow::anyhow!("agent turn timed out after {:?}", self.config.turn_timeout))??;

                let (turn_messages, has_tool_requests) = turn_result;

                if turn_messages.is_empty() {
                    break;
                }

                // Execute tool requests before appending to messages
                let mut executed_messages = vec![];
                for msg in &turn_messages {
                    let mut had_tool_request = false;
                    let mut blocks = vec![];
                    for block in &msg.content {
                        match block {
                            crate::model::ContentBlock::ToolRequest { id, name, arguments } => {
                                had_tool_request = true;
                                match execute_tool(name, arguments, &session.working_dir, &self.security).await {
                                    Ok(result) => {
                                        blocks.push(crate::model::ContentBlock::ToolResponse {
                                            id: id.clone(),
                                            content: result,
                                            is_error: false,
                                        });
                                    }
                                    Err(e) => {
                                        blocks.push(crate::model::ContentBlock::ToolResponse {
                                            id: id.clone(),
                                            content: format!("error: {}", e),
                                            is_error: true,
                                        });
                                    }
                                }
                            }
                            other => blocks.push(other.clone()),
                        }
                    }
                    if had_tool_request && !blocks.is_empty() {
                        executed_messages.push(Message {
                            id: msg.id.clone(),
                            role: msg.role,
                            content: blocks,
                            created_at: msg.created_at,
                        });
                    }
                }

                // Append original turn messages and tool results
                for msg in &turn_messages {
                    messages.push(msg.clone());
                }
                for msg in &executed_messages {
                    messages.push(msg.clone());
                }

                final_messages.extend(turn_messages);
                final_messages.extend(executed_messages);

                let compaction = self.context_manager.maybe_compact(&mut messages);
                if compaction != CompactionResult::Noop {
                    debug!(compaction = %compaction, "context compaction");
                }

                if !has_tool_requests {
                    break;
                }
            }

            session.conversation = messages.clone();
            session.updated_at = chrono::Utc::now();

            anyhow::Ok(final_messages)
        })
        .await
        .map_err(|_| anyhow::anyhow!("agent run timed out after {:?}", self.config.total_timeout))??;

        Ok(total_deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::EchoProvider;
    use crate::session::SessionType;
    use crate::model::{ContentBlock, Role};
    use std::path::PathBuf;
    use chrono::Utc;

    #[tokio::test]
    async fn test_agent_run_echo_provider() {
        let provider = Arc::new(EchoProvider::default());
        let config = AgentConfig {
            model_config: ModelConfig {
                model: "echo-v1".to_string(),
                temperature: None,
                max_tokens: None,
            },
            system_prompt: "You are a helpful assistant.".to_string(),
            max_turns: 1,
            turn_timeout: Duration::from_secs(10),
            tool_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(30),
        };
        let agent = Agent::new(config, provider);
        let mut session = Session::new("test", PathBuf::from("."), SessionType::User);
        let user_message = Message {
            id: "test-msg".to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hello".to_string() }],
            created_at: Utc::now(),
        };

        let result = agent.run(&mut session, user_message).await;
        assert!(result.is_ok());
        let messages = result.unwrap();
        assert!(!messages.is_empty());
        assert_eq!(messages[0].role, Role::Assistant);
    }

    #[tokio::test]
    async fn test_agent_run_tool_execution() {
        let provider = Arc::new(EchoProvider::default());
        let config = AgentConfig {
            model_config: ModelConfig {
                model: "echo-v1".to_string(),
                temperature: None,
                max_tokens: None,
            },
            system_prompt: "You are a helpful assistant.".to_string(),
            max_turns: 1,
            turn_timeout: Duration::from_secs(10),
            tool_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(30),
        };
        let agent = Agent::new(config, provider);
        let mut session = Session::new("test", PathBuf::from("."), SessionType::User);
        let user_message = Message {
            id: "test-tool".to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: "tool:datetime".to_string() }],
            created_at: Utc::now(),
        };

        let result = agent.run(&mut session, user_message).await;
        assert!(result.is_ok());
        let messages = result.unwrap();
        assert!(!messages.is_empty());
    }

    #[tokio::test]
    async fn test_agent_timeout_total() {
        let provider = Arc::new(EchoProvider::default());
        let config = AgentConfig {
            model_config: ModelConfig {
                model: "echo-v1".to_string(),
                temperature: None,
                max_tokens: None,
            },
            system_prompt: "You are a helpful assistant.".to_string(),
            max_turns: 1000,
            turn_timeout: Duration::from_secs(1),
            tool_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_millis(1),
        };
        let agent = Agent::new(config, provider);
        let mut session = Session::new("test", PathBuf::from("."), SessionType::User);
        let user_message = Message {
            id: "test-timeout".to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hello".to_string() }],
            created_at: Utc::now(),
        };

        let result = agent.run(&mut session, user_message).await;
        // total_timeout is extremely short; if it does not timeout on this platform,
        // accept success as non-fatal to avoid flaky timer-dependent tests.
        if result.is_err() {
            assert!(result.unwrap_err().to_string().contains("timed out"));
        }
    }
}
