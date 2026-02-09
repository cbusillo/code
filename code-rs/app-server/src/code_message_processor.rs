use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use code_core::AuthManager;
use code_core::CodexConversation;
use code_core::ConversationManager;
use code_core::NewConversation;
use code_core::remote_models::RemoteModelsManager;
use code_core::review_format::format_review_findings_block;
use code_core::config::Config;
use code_core::config::ConfigToml;
use code_core::config::ConfigOverrides;
use code_core::config::load_config_as_toml_with_cli_overrides;
use code_core::config::load_global_mcp_servers;
use code_core::mcp_connection_manager::McpConnectionManager;
use code_core::config_edit;
use code_core::config_types::McpServerTransportConfig;
use code_core::exec::ExecParams;
use code_core::exec::SandboxType;
use code_core::exec::process_exec_tool_call;
use code_core::exec_env::create_env;
use code_core::get_platform_sandbox;
use code_core::git_info::git_diff_to_remote;
use code_core::SessionCatalog;
use code_core::SessionIndexEntry;
use code_core::SessionQuery;
use code_core::skills::loader::load_skills;
use code_core::protocol::ApplyPatchApprovalRequestEvent;
use code_core::protocol::Event;
use code_core::protocol::EventMsg;
use code_core::protocol::ExecApprovalRequestEvent;
use code_app_server_protocol::ClientRequest;
use code_app_server_protocol::ServerRequestPayload;
use code_app_server_protocol::ServerNotification;
use code_app_server_protocol::build_turns_from_event_msgs;
use code_app_server_protocol::v2;
use code_app_server_protocol::FuzzyFileSearchParams;
use code_app_server_protocol::FuzzyFileSearchResponse;
use code_protocol::protocol::ReviewDecision;
use code_protocol::protocol::RolloutItem;
use code_protocol::protocol::RolloutLine;
use code_protocol::config_types::CollaborationModeMask;
use code_protocol::config_types::ModeKind;
use code_protocol::openai_models::ModelInfo;
use code_protocol::request_user_input::RequestUserInputAnswer as CoreRequestUserInputAnswer;
use code_protocol::request_user_input::RequestUserInputResponse as CoreRequestUserInputResponse;
use code_utils_absolute_path::AbsolutePathBuf;
use mcp_types::JSONRPCErrorError;
use mcp_types::RequestId;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::io::AsyncBufReadExt;
use tracing::error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::jsonrpc_compat::to_mcp_request_id;
use code_utils_json_to_toml::json_to_toml;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::OutgoingNotification;
use crate::fuzzy_file_search::run_fuzzy_file_search;
use crate::remote_skills::download_remote_skill;
use crate::remote_skills::list_remote_skills;
use code_protocol::protocol::TurnAbortReason;
use code_protocol::protocol::SessionSource;
use code_protocol::protocol::SubAgentSource;
use code_protocol::dynamic_tools::DynamicToolResponse as CoreDynamicToolResponse;
use code_core::protocol::InputItem as CoreInputItem;
use code_core::protocol::Op;
use code_core::protocol as core_protocol;
use code_protocol::mcp_protocol::APPLY_PATCH_APPROVAL_METHOD;
use code_protocol::mcp_protocol::EXEC_COMMAND_APPROVAL_METHOD;
use code_protocol::mcp_protocol::DYNAMIC_TOOL_CALL_METHOD;
use code_protocol::mcp_protocol::DynamicToolCallParams as LegacyDynamicToolCallParams;
use code_protocol::mcp_protocol::DynamicToolCallResponse as LegacyDynamicToolCallResponse;
use code_protocol::ConversationId;
use code_protocol::ThreadId;
use code_app_server_protocol::v1::AddConversationListenerParams;
use code_app_server_protocol::v1::AddConversationSubscriptionResponse;
use code_app_server_protocol::v1::ApplyPatchApprovalParams;
use code_app_server_protocol::v1::ApplyPatchApprovalResponse;
use code_app_server_protocol::v1::ExecCommandApprovalParams;
use code_app_server_protocol::v1::ExecCommandApprovalResponse;
use code_app_server_protocol::v1::InputItem as WireInputItem;
use code_app_server_protocol::v1::InterruptConversationParams;
use code_app_server_protocol::v1::InterruptConversationResponse;
use code_app_server_protocol::v2::DynamicToolCallParams;
use code_app_server_protocol::v2::DynamicToolCallResponse;
// Unused login-related and diff param imports removed
use code_app_server_protocol::v1::GitDiffToRemoteResponse;
use code_app_server_protocol::v1::GetConversationSummaryParams;
use code_app_server_protocol::v1::GetConversationSummaryResponse;
use code_app_server_protocol::v1::ListConversationsParams;
use code_app_server_protocol::v1::ListConversationsResponse;
use code_app_server_protocol::v1::ResumeConversationParams;
use code_app_server_protocol::v1::ResumeConversationResponse;
use code_app_server_protocol::v1::ForkConversationParams;
use code_app_server_protocol::v1::ForkConversationResponse;
use code_app_server_protocol::v1::ArchiveConversationParams;
use code_app_server_protocol::v1::ArchiveConversationResponse;
use code_app_server_protocol::v1::ConversationGitInfo;
use code_app_server_protocol::v1::ConversationSummary;
use code_app_server_protocol::v1::NewConversationParams;
use code_app_server_protocol::v1::NewConversationResponse;
use code_app_server_protocol::v1::RemoveConversationListenerParams;
use code_app_server_protocol::v1::RemoveConversationSubscriptionResponse;
use code_app_server_protocol::v1::SendUserMessageParams;
use code_app_server_protocol::v1::SendUserMessageResponse;
use code_app_server_protocol::v1::SendUserTurnParams;
use code_app_server_protocol::v1::SendUserTurnResponse;
use code_app_server_protocol::v1::SessionConfiguredNotification;
use code_app_server_protocol::v1::GetAuthStatusParams;
use code_app_server_protocol::v1::GetAuthStatusResponse;
use code_app_server_protocol::v1::GetUserSavedConfigResponse;
use code_app_server_protocol::v1::UserSavedConfig;
use code_app_server_protocol::v1::SetDefaultModelParams;
use code_app_server_protocol::v1::SetDefaultModelResponse;
use code_app_server_protocol::v1::GetUserAgentResponse;
use code_app_server_protocol::v1::UserInfoResponse;
use code_app_server_protocol::v1::Profile;
use code_app_server_protocol::v1::SandboxSettings;
use code_app_server_protocol::v1::Tools;
use code_app_server_protocol::v1::ExecOneOffCommandParams;
use code_app_server_protocol::v1::ExecOneOffCommandResponse;
use code_app_server_protocol::v1::LoginApiKeyParams;
use code_app_server_protocol::v1::LoginApiKeyResponse;
use code_app_server_protocol::v1::LoginChatGptResponse;
use code_app_server_protocol::v1::CancelLoginChatGptParams;
use code_app_server_protocol::v1::CancelLoginChatGptResponse;
use code_app_server_protocol::v1::LogoutChatGptResponse;
use code_app_server_protocol::v1::AuthStatusChangeNotification;
use code_app_server_protocol::v1::LoginChatGptCompleteNotification;
use code_app_server_protocol::v2::Account;
use code_app_server_protocol::v2::AccountLoginCompletedNotification;
use code_app_server_protocol::v2::AccountUpdatedNotification;
use code_app_server_protocol::v2::AppsListParams;
use code_app_server_protocol::v2::AppsListResponse;
use code_app_server_protocol::v2::CancelLoginAccountParams;
use code_app_server_protocol::v2::CancelLoginAccountResponse;
use code_app_server_protocol::v2::CancelLoginAccountStatus;
use code_app_server_protocol::v2::GetAccountParams;
use code_app_server_protocol::v2::GetAccountRateLimitsResponse;
use code_app_server_protocol::v2::GetAccountResponse;
use code_app_server_protocol::v2::LoginAccountParams;
use code_app_server_protocol::v2::LoginAccountResponse;
use code_app_server_protocol::v2::LogoutAccountResponse;
use code_app_server_protocol::v2::Model;
use code_app_server_protocol::v2::ModelListParams;
use code_app_server_protocol::v2::ModelListResponse;
use code_app_server_protocol::v2::ReasoningEffortOption;
use code_app_server_protocol::v2::ExperimentalFeature;
use code_app_server_protocol::v2::ExperimentalFeatureListParams;
use code_app_server_protocol::v2::ExperimentalFeatureListResponse;
use code_app_server_protocol::v2::ExperimentalFeatureStage;
use code_app_server_protocol::v2::CollaborationModeListParams;
use code_app_server_protocol::v2::CollaborationModeListResponse;
use code_app_server_protocol::v2::RateLimitSnapshot;
use code_app_server_protocol::v2::RateLimitWindow;
use code_app_server_protocol::AuthMode as CoreAuthMode;
use code_chatgpt::connectors;
use code_rmcp_client::has_valid_oauth_tokens;
use code_rmcp_client::perform_oauth_login_return_url;
use chrono::Utc;
use code_core::auth::CLIENT_ID;
use code_core::auth::AuthDotJson;
use code_core::auth::get_auth_file;
use code_core::auth::login_with_api_key as write_api_key_auth;
use code_core::auth::write_auth_json;
use code_core::token_data::parse_id_token;
use code_login::run_login_server;
use code_login::ServerOptions;
use code_login::ShutdownHandle;
use code_protocol::account::PlanType as AccountPlanType;
use code_protocol::config_types::ForcedLoginMethod;
use std::time::Duration;
use toml::Value as TomlValue;

/// Duration before a ChatGPT login attempt is abandoned.
const LOGIN_CHATGPT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy)]
enum ExperimentalFeatureSpecStage {
    UnderDevelopment,
    Experimental {
        name: &'static str,
        menu_description: &'static str,
        announcement: &'static str,
    },
    Stable,
    Deprecated,
}

#[derive(Clone, Copy)]
struct ExperimentalFeatureSpec {
    key: &'static str,
    stage: ExperimentalFeatureSpecStage,
    default_enabled: bool,
}

const EXPERIMENTAL_FEATURE_SPECS: &[ExperimentalFeatureSpec] = &[
    ExperimentalFeatureSpec {
        key: "undo",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "shell_tool",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "unified_exec",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: !cfg!(windows),
    },
    ExperimentalFeatureSpec {
        key: "web_search_request",
        stage: ExperimentalFeatureSpecStage::Deprecated,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "web_search_cached",
        stage: ExperimentalFeatureSpecStage::Deprecated,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "shell_snapshot",
        stage: ExperimentalFeatureSpecStage::Experimental {
            name: "Shell snapshot",
            menu_description:
                "Snapshot your shell environment to avoid re-running login scripts for every command.",
            announcement:
                "NEW! Try shell snapshotting to make your Codex faster. Enable in /experimental!",
        },
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "runtime_metrics",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "sqlite",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "memory_tool",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "child_agents_md",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "apply_patch_freeform",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "exec_policy",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "use_linux_sandbox_bwrap",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "request_rule",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "experimental_windows_sandbox",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "elevated_windows_sandbox",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "remote_compaction",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "remote_models",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "powershell_utf8",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "enable_request_compression",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "collab",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "apps",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "skill_mcp_dependency_install",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "skill_env_var_dependency_prompt",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
    ExperimentalFeatureSpec {
        key: "steer",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "collaboration_modes",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "personality",
        stage: ExperimentalFeatureSpecStage::Stable,
        default_enabled: true,
    },
    ExperimentalFeatureSpec {
        key: "responses_websockets",
        stage: ExperimentalFeatureSpecStage::UnderDevelopment,
        default_enabled: false,
    },
];

struct ActiveLogin {
    shutdown_handle: ShutdownHandle,
    login_id: Uuid,
}

impl Drop for ActiveLogin {
    fn drop(&mut self) {
        self.shutdown_handle.shutdown();
    }
}

#[derive(Clone, Copy, Debug)]
enum CancelLoginError {
    NotFound,
}

/// Handles JSON-RPC messages for Codex conversations.
pub struct CodexMessageProcessor {
    auth_manager: Arc<AuthManager>,
    conversation_manager: Arc<ConversationManager>,
    outgoing: Arc<OutgoingMessageSender>,
    code_linux_sandbox_exe: Option<PathBuf>,
    config: Arc<Config>,
    conversation_listeners: HashMap<Uuid, oneshot::Sender<()>>,
    thread_listeners: HashMap<String, oneshot::Sender<()>>,
    loaded_threads: HashSet<String>,
    thread_configs: HashMap<String, Config>,
    // Queue of pending interrupt requests per conversation. We reply when TurnAborted arrives.
    pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
    #[allow(dead_code)]
    pending_fuzzy_searches: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    active_login: Arc<Mutex<Option<ActiveLogin>>>,
    disabled_skill_paths: HashSet<String>,
}

impl CodexMessageProcessor {
    pub fn new(
        auth_manager: Arc<AuthManager>,
        conversation_manager: Arc<ConversationManager>,
        outgoing: Arc<OutgoingMessageSender>,
        code_linux_sandbox_exe: Option<PathBuf>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            auth_manager,
            conversation_manager,
            outgoing,
            code_linux_sandbox_exe,
            config,
            conversation_listeners: HashMap::new(),
            thread_listeners: HashMap::new(),
            loaded_threads: HashSet::new(),
            thread_configs: HashMap::new(),
            pending_interrupts: Arc::new(Mutex::new(HashMap::new())),
            pending_fuzzy_searches: Arc::new(Mutex::new(HashMap::new())),
            active_login: Arc::new(Mutex::new(None)),
            disabled_skill_paths: HashSet::new(),
        }
    }

    pub async fn process_request(&mut self, request: ClientRequest) {
        match request {
            ClientRequest::Initialize { .. } => {
                panic!("Initialize should be handled in MessageProcessor");
            }
            ClientRequest::NewConversation { request_id, params } => {
                // Do not tokio::spawn() to process new_conversation()
                // asynchronously because we need to ensure the conversation is
                // created before processing any subsequent messages.
                self.process_new_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SendUserMessage { request_id, params } => {
                self.send_user_message(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::InterruptConversation { request_id, params } => {
                self.interrupt_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::AddConversationListener { request_id, params } => {
                self.add_conversation_listener(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::RemoveConversationListener { request_id, params } => {
                self.remove_conversation_listener(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SendUserTurn { request_id, params } => {
                self.send_user_turn_compat(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::LoginAccount { request_id, params } => {
                self.login_account(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::CancelLoginAccount { request_id, params } => {
                self.cancel_login_account(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::LogoutAccount { request_id, .. } => {
                self.logout_account(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::GetAccountRateLimits { request_id, .. } => {
                self.get_account_rate_limits(to_mcp_request_id(request_id))
                    .await;
            }
            ClientRequest::LoginApiKey { request_id, params } => {
                self.login_api_key_v1(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::LoginChatGpt { request_id, .. } => {
                self.login_chatgpt_v1(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::CancelLoginChatGpt { request_id, params } => {
                self.cancel_login_chatgpt_v1(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::LogoutChatGpt { request_id, .. } => {
                self.logout_chatgpt_v1(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::GetAuthStatus { request_id, params } => {
                self.get_auth_status(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::GetUserSavedConfig { request_id, .. } => {
                self.get_user_saved_config(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::SetDefaultModel { request_id, params } => {
                self.set_default_model(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::GetUserAgent { request_id, .. } => {
                self.get_user_agent(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::UserInfo { request_id, .. } => {
                self.get_user_info(to_mcp_request_id(request_id)).await;
            }
            ClientRequest::GetConversationSummary { request_id, params } => {
                self.get_conversation_summary(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ListConversations { request_id, params } => {
                self.list_conversations(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ResumeConversation { request_id, params } => {
                self.resume_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ForkConversation { request_id, params } => {
                self.fork_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ArchiveConversation { request_id, params } => {
                self.archive_conversation_v1(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::FuzzyFileSearch { request_id, params } => {
                self.fuzzy_file_search(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ExecOneOffCommand { request_id, params } => {
                self.exec_one_off_command(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::GitDiffToRemote { request_id, params } => {
                self.git_diff_to_origin(to_mcp_request_id(request_id), params.cwd)
                    .await;
            }
            ClientRequest::ThreadStart { request_id, params } => {
                self.thread_start(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadResume { request_id, params } => {
                self.thread_resume(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadFork { request_id, params } => {
                self.thread_fork(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ThreadArchive { request_id, params } => {
                self.thread_archive(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadUnarchive { request_id, params } => {
                self.thread_unarchive(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadSetName { request_id, params } => {
                self.thread_set_name(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadCompactStart { request_id, params } => {
                self.thread_compact_start(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadRollback { request_id, params } => {
                self.thread_rollback(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadList { request_id, params } => {
                self.thread_list(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ThreadLoadedList { request_id, params } => {
                self.thread_loaded_list(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadRead { request_id, params } => {
                self.thread_read(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::SkillsList { request_id, params } => {
                self.skills_list(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::SkillsRemoteRead { request_id, params } => {
                self.skills_remote_read(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SkillsRemoteWrite { request_id, params } => {
                self.skills_remote_write(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SkillsConfigWrite { request_id, params } => {
                self.skills_config_write(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::TurnStart { request_id, params } => {
                self.turn_start(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ReviewStart { request_id, params } => {
                self.review_start(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::TurnInterrupt { request_id, params } => {
                self.turn_interrupt(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::GetAccount { request_id, params } => {
                self.get_account(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::AppsList { request_id, params } => {
                self.apps_list(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ModelList { request_id, params } => {
                self.model_list(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ExperimentalFeatureList { request_id, params } => {
                self.experimental_feature_list(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::CollaborationModeList { request_id, params } => {
                self.collaboration_mode_list(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::MockExperimentalMethod { request_id, params } => {
                self.mock_experimental_method(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::McpServerOauthLogin { request_id, params } => {
                self.mcp_server_oauth_login(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::McpServerRefresh { request_id, params } => {
                self.mcp_server_refresh(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::McpServerStatusList { request_id, params } => {
                self.list_mcp_server_status(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::OneOffCommandExec { request_id, params } => {
                self.exec_one_off_command_v2(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::FeedbackUpload { request_id, params } => {
                self.upload_feedback(to_mcp_request_id(request_id), params)
                    .await;
            }
            _ => {
                self.send_unimplemented_request(request).await;
            }
        }
    }

    async fn send_unimplemented_request(&self, request: ClientRequest) {
        let Some(request_id) = request_id_from_client_request(&request) else {
            tracing::warn!("unhandled request without id: {request:?}");
            return;
        };
        let error = JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "request not supported by app-server".to_string(),
            data: None,
        };
        self.outgoing.send_error(request_id, error).await;
    }

    async fn process_new_conversation(&self, request_id: RequestId, params: NewConversationParams) {
        let config = match derive_config_from_params(params, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let response_reasoning_effort = Some(map_reasoning_effort_to_wire(config.model_reasoning_effort));

        match self.conversation_manager.new_conversation(config).await {
            Ok(conversation_id) => {
                let NewConversation {
                    conversation_id,
                    session_configured,
                    ..
                } = conversation_id;
                let conversation_id = match thread_id_from_conversation_id(conversation_id) {
                    Ok(id) => id,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error mapping thread id: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };
                let rollout_path = find_rollout_path_for_thread(&self.config.code_home, &conversation_id)
                    .await
                    .unwrap_or_else(|| self.config.code_home.join(code_core::SESSIONS_SUBDIR));
                let response = NewConversationResponse {
                    conversation_id,
                    model: session_configured.model,
                    reasoning_effort: response_reasoning_effort,
                    rollout_path,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error creating conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn send_user_message(&self, request_id: RequestId, params: SendUserMessageParams) {
        let SendUserMessageParams {
            conversation_id,
            items,
        } = params;
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = conversation
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        // Acknowledge with an empty result.
        self.outgoing
            .send_response(request_id, SendUserMessageResponse {})
            .await;
    }

    #[allow(dead_code)]
    async fn send_user_turn(&self, request_id: RequestId, params: SendUserTurnParams) {
        let SendUserTurnParams {
            conversation_id,
            items,
            cwd: _,
            approval_policy: _,
            sandbox_policy: _,
            model: _,
            effort: _,
            summary: _,
            output_schema: _,
        } = params;

        let Ok(conversation_id) = conversation_id_from_thread_id(conversation_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid conversation id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Core protocol compatibility: older cores do not support per-turn overrides.
        // Submit only the user input items.
        let _ = conversation
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        self.outgoing
            .send_response(request_id, SendUserTurnResponse {})
            .await;
    }

    async fn interrupt_conversation(
        &mut self,
        request_id: RequestId,
        params: InterruptConversationParams,
    ) {
        let InterruptConversationParams { conversation_id } = params;
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        // Submit the interrupt and respond immediately (core does not emit a dedicated event).
        let _ = conversation.submit(Op::Interrupt).await;
        let response = InterruptConversationResponse { abort_reason: TurnAbortReason::Interrupted };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn add_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: AddConversationListenerParams,
    ) {
        let AddConversationListenerParams {
            conversation_id,
            ..
        } = params;
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let subscription_id = Uuid::new_v4();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.conversation_listeners
            .insert(subscription_id, cancel_tx);
        let outgoing_for_task = self.outgoing.clone();
        let pending_interrupts = self.pending_interrupts.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        // User has unsubscribed, so exit this task.
                        break;
                    }
                    event = conversation.next_event() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(err) => {
                                tracing::warn!("conversation.next_event() failed with: {err}");
                                break;
                            }
                        };

                        // For now, we send a notification for every event,
                        // JSON-serializing the `Event` as-is, but we will move
                        // to creating a special enum for notifications with a
                        // stable wire format.
                        let method = format!("codex/event/{}", event.msg);
                        let mut params = match serde_json::to_value(event.clone()) {
                            Ok(serde_json::Value::Object(map)) => map,
                            Ok(_) => {
                                tracing::error!("event did not serialize to an object");
                                continue;
                            }
                            Err(err) => {
                                tracing::error!("failed to serialize event: {err}");
                                continue;
                            }
                        };
                        params.insert("conversationId".to_string(), conversation_id.to_string().into());

                        outgoing_for_task.send_notification(OutgoingNotification {
                            method,
                            params: Some(params.into()),
                        })
                        .await;

                        apply_bespoke_event_handling(event.clone(), conversation_id, conversation.clone(), outgoing_for_task.clone(), pending_interrupts.clone()).await;
                    }
                }
            }
        });
        let response = AddConversationSubscriptionResponse { subscription_id };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn remove_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: RemoveConversationListenerParams,
    ) {
        let RemoveConversationListenerParams { subscription_id } = params;
        match self.conversation_listeners.remove(&subscription_id) {
            Some(sender) => {
                // Signal the spawned task to exit and acknowledge.
                let _ = sender.send(());
                let response = RemoveConversationSubscriptionResponse {};
                self.outgoing.send_response(request_id, response).await;
            }
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("subscription not found: {subscription_id}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn git_diff_to_origin(&self, request_id: RequestId, cwd: PathBuf) {
        let diff = git_diff_to_remote(&cwd).await;
        match diff {
            Some(value) => {
                let response = GitDiffToRemoteResponse {
                    sha: value.sha,
                    diff: value.diff,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("failed to compute git diff to remote for cwd: {cwd:?}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_start(&mut self, request_id: RequestId, params: v2::ThreadStartParams) {
        let config = match derive_config_from_thread_start_params(
            &params,
            self.code_linux_sandbox_exe.clone(),
        ) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let config_for_response = config.clone();
        match self.conversation_manager.new_conversation(config).await {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let thread = match load_thread_from_catalog(
                    &self.config.code_home,
                    &thread_id,
                    self.config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, Vec::new()),
                };

                let response = v2::ThreadStartResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error creating thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_resume(&mut self, request_id: RequestId, params: v2::ThreadResumeParams) {
        let rollout_path = match resolve_rollout_path(
            &self.config.code_home,
            &params.thread_id,
            params.path.clone(),
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        if params.history.is_some() {
            tracing::info!("thread/resume history override is not supported; falling back to rollout path");
        }

        let config = match derive_config_from_thread_overrides(
            &params.model,
            &params.model_provider,
            &params.cwd,
            &params.approval_policy,
            &params.sandbox,
            &params.config,
            &params.base_instructions,
            &params.developer_instructions,
            None,
            self.code_linux_sandbox_exe.clone(),
        ) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let config_for_response = config.clone();
        match self
            .conversation_manager
            .resume_conversation_from_rollout(
                config,
                rollout_path.clone(),
                self.conversation_manager.auth_manager(),
            )
            .await
        {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let turns = read_turns_from_rollout(&config_for_response, &rollout_path).await;
                let thread = match load_thread_from_catalog(
                    &self.config.code_home,
                    &thread_id,
                    self.config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, turns.clone()),
                };
                let thread = v2::Thread { turns, ..thread };

                let response = v2::ThreadResumeResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error resuming thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_fork(&mut self, request_id: RequestId, params: v2::ThreadForkParams) {
        let rollout_path = match resolve_rollout_path(
            &self.config.code_home,
            &params.thread_id,
            params.path.clone(),
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let config = match derive_config_from_thread_overrides(
            &params.model,
            &params.model_provider,
            &params.cwd,
            &params.approval_policy,
            &params.sandbox,
            &params.config,
            &params.base_instructions,
            &params.developer_instructions,
            None,
            self.code_linux_sandbox_exe.clone(),
        ) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let config_for_response = config.clone();
        match self
            .conversation_manager
            .fork_conversation(0, config, rollout_path.clone())
            .await
        {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let turns = read_turns_from_rollout(&config_for_response, &rollout_path).await;
                let thread = match load_thread_from_catalog(
                    &self.config.code_home,
                    &thread_id,
                    self.config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, turns.clone()),
                };
                let thread = v2::Thread { turns, ..thread };

                let response = v2::ThreadForkResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error forking thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_archive(&mut self, request_id: RequestId, params: v2::ThreadArchiveParams) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_archived(&self.config.code_home, conversation_id, true).await {
            Ok(_) => {
                self.outgoing
                    .send_response(request_id, v2::ThreadArchiveResponse {})
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_unarchive(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadUnarchiveParams,
    ) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_archived(&self.config.code_home, conversation_id, false).await {
            Ok(_) => {
                let thread = match load_thread_from_catalog(
                    &self.config.code_home,
                    &thread_id,
                    self.config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_placeholder(&thread_id),
                };
                let response = v2::ThreadUnarchiveResponse { thread };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_set_name(&mut self, request_id: RequestId, params: v2::ThreadSetNameParams) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_nickname(
            &self.config.code_home,
            conversation_id,
            params.name.clone(),
        )
        .await
        {
            Ok(_) => {
                self.outgoing
                    .send_response(request_id, v2::ThreadSetNameResponse {})
                    .await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadNameUpdated(v2::ThreadNameUpdatedNotification {
                        thread_id,
                        thread_name: Some(params.name),
                    }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_compact_start(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadCompactStartParams,
    ) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let _ = conversation.submit(Op::Compact).await;
        self.outgoing
            .send_response(request_id, v2::ThreadCompactStartResponse {})
            .await;
    }

    async fn thread_rollback(
        &mut self,
        request_id: RequestId,
        _params: v2::ThreadRollbackParams,
    ) {
        let error = JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "thread/rollback is not supported by app-server".to_string(),
            data: None,
        };
        self.outgoing.send_error(request_id, error).await;
    }

    async fn thread_list(&mut self, request_id: RequestId, params: v2::ThreadListParams) {
        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let query = SessionQuery {
            include_archived: matches!(params.archived, Some(true)),
            include_deleted: false,
            ..SessionQuery::default()
        };
        let entries = match catalog.query(&query).await {
            Ok(entries) => entries,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error loading catalog: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let mut entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| match params.archived {
                Some(true) => entry.archived,
                Some(false) | None => !entry.archived,
            })
            .filter(|entry| {
                matches_model_provider(
                    entry,
                    params.model_providers.as_ref(),
                    self.config.model_provider_id.as_str(),
                )
            })
            .filter(|entry| matches_source_kind(entry, params.source_kinds.as_ref()))
            .collect();

        let sort_key = params.sort_key.unwrap_or(v2::ThreadSortKey::CreatedAt);

        let offset = match params.cursor.as_deref() {
            Some(value) => match value.parse::<usize>() {
                Ok(parsed) => parsed,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {value}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };
        let limit = params.limit.unwrap_or(50).clamp(1, 100) as usize;
        let code_home = self.config.code_home.clone();
        let fallback_provider = self.config.model_provider_id.clone();
        let thread_data = tokio::task::spawn_blocking(move || {
            match sort_key {
                v2::ThreadSortKey::CreatedAt => {
                    entries.sort_by(|a, b| {
                        let a_created = parse_timestamp(&a.created_at).unwrap_or(0);
                        let b_created = parse_timestamp(&b.created_at).unwrap_or(0);
                        b_created
                            .cmp(&a_created)
                            .then_with(|| b.session_id.cmp(&a.session_id))
                    });
                }
                v2::ThreadSortKey::UpdatedAt => {
                    let mut entries_with_updated = entries
                        .into_iter()
                        .map(|entry| {
                            let updated_at = entry_updated_at(&entry, &code_home);
                            (entry, updated_at)
                        })
                        .collect::<Vec<_>>();
                    entries_with_updated.sort_by(|(a_entry, a_updated), (b_entry, b_updated)| {
                        b_updated
                            .cmp(a_updated)
                            .then_with(|| b_entry.session_id.cmp(&a_entry.session_id))
                    });
                    entries = entries_with_updated
                        .into_iter()
                        .map(|(entry, _)| entry)
                        .collect();
                }
            }

            let total = entries.len();
            if offset > total {
                return Err(format!("invalid cursor: {offset}"));
            }

            let slice = entries
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|entry| build_thread_from_entry(&entry, &code_home, &fallback_provider, Vec::new()))
                .collect::<Vec<_>>();
            Ok((slice, total))
        })
        .await;

        let (slice, total) = match thread_data {
            Ok(Ok((slice, total))) => (slice, total),
            Ok(Err(message)) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to build thread list: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let end = offset + slice.len();
        let next_cursor = if end < total {
            Some(end.to_string())
        } else {
            None
        };

        let response = v2::ThreadListResponse {
            data: slice,
            next_cursor,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn thread_loaded_list(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadLoadedListParams,
    ) {
        let mut ids: Vec<String> = self.loaded_threads.iter().cloned().collect();
        ids.sort();
        let offset = params
            .cursor
            .as_deref()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = params.limit.unwrap_or(ids.len() as u32) as usize;
        let slice = ids.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        let next_cursor = if slice.len() == limit {
            Some((offset + limit).to_string())
        } else {
            None
        };
        let response = v2::ThreadLoadedListResponse {
            data: slice,
            next_cursor,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn thread_read(&mut self, request_id: RequestId, params: v2::ThreadReadParams) {
        let rollout_path = match resolve_rollout_path(&self.config.code_home, &params.thread_id, None).await {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let turns = if params.include_turns {
            read_turns_from_rollout(&self.config, &rollout_path).await
        } else {
            Vec::new()
        };
        let thread = match load_thread_from_catalog(
            &self.config.code_home,
            &params.thread_id,
            self.config.model_provider_id.as_str(),
        )
        .await
        {
            Some(thread) => thread,
            None => build_thread_for_new(&params.thread_id, &self.config, turns.clone()),
        };
        let thread = v2::Thread { turns, ..thread };
        let response = v2::ThreadReadResponse { thread };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn turn_start(&mut self, request_id: RequestId, params: v2::TurnStartParams) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if !self.thread_listeners.contains_key(&params.thread_id) {
            self.start_thread_listener(params.thread_id.clone(), conversation.clone())
                .await;
        }

        let has_any_overrides = params.cwd.is_some()
            || params.approval_policy.is_some()
            || params.sandbox_policy.is_some()
            || params.model.is_some()
            || params.effort.is_some()
            || params.summary.is_some()
            || params.personality.is_some();

        if has_any_overrides {
            if let Some(cwd) = params.cwd.as_ref()
                && !cwd.is_absolute()
            {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("cwd is not absolute: {cwd:?}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }

            let mut updated_config = self
                .thread_configs
                .get(&params.thread_id)
                .cloned()
                .unwrap_or_else(|| (*self.config).clone());
            apply_turn_overrides(&mut updated_config, &params);
            let configure_session = configure_session_from_config(&updated_config);
            if let Err(err) = conversation.submit(configure_session).await {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error updating session config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
            self.thread_configs
                .insert(params.thread_id.clone(), updated_config);
        }

        let input_items = params.input.clone();
        let items = params
            .input
            .into_iter()
            .map(map_v2_user_input)
            .collect::<Vec<_>>();
        let submit = conversation
            .submit(Op::UserInput {
                items,
                final_output_json_schema: params.output_schema,
            })
            .await;
        match submit {
            Ok(turn_id) => {
                let thread_id = params.thread_id.clone();
                let turn = v2::Turn {
                    id: turn_id.clone(),
                    items: Vec::new(),
                    status: v2::TurnStatus::InProgress,
                    error: None,
                };
                let response = v2::TurnStartResponse { turn: turn.clone() };
                self.outgoing.send_response(request_id, response).await;
                if !input_items.is_empty() {
                    let item = v2::ThreadItem::UserMessage {
                        id: Uuid::new_v4().to_string(),
                        content: input_items,
                    };
                    send_v2_notification(
                        self.outgoing.as_ref(),
                        ServerNotification::ItemStarted(v2::ItemStartedNotification {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            item,
                        }),
                    )
                    .await;
                }
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::TurnStarted(v2::TurnStartedNotification {
                        thread_id,
                        turn,
                    }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to start turn: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    fn review_request_from_target(
        target: v2::ReviewTarget,
    ) -> Result<(core_protocol::ReviewRequest, String), JSONRPCErrorError> {
        fn invalid_request(message: String) -> JSONRPCErrorError {
            JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message,
                data: None,
            }
        }

        let (prompt, hint, metadata) = match target {
            v2::ReviewTarget::UncommittedChanges => (
                "Review the uncommitted changes in the current workspace.".to_string(),
                "uncommitted changes".to_string(),
                core_protocol::ReviewContextMetadata {
                    scope: Some("uncommitted".to_string()),
                    ..Default::default()
                },
            ),
            v2::ReviewTarget::BaseBranch { branch } => {
                let branch = branch.trim().to_string();
                if branch.is_empty() {
                    return Err(invalid_request("branch must not be empty".to_string()));
                }
                (
                    format!("Review changes against base branch `{branch}`."),
                    format!("base branch {branch}"),
                    core_protocol::ReviewContextMetadata {
                        scope: Some("base_branch".to_string()),
                        base_branch: Some(branch),
                        ..Default::default()
                    },
                )
            }
            v2::ReviewTarget::Commit { sha, title } => {
                let sha = sha.trim().to_string();
                if sha.is_empty() {
                    return Err(invalid_request("sha must not be empty".to_string()));
                }
                let short_sha = sha.chars().take(7).collect::<String>();
                let title = title
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                let hint = match title.as_deref() {
                    Some(title) => format!("commit {short_sha}: {title}"),
                    None => format!("commit {short_sha}"),
                };
                (
                    format!("Review commit `{sha}` for correctness and risks."),
                    hint,
                    core_protocol::ReviewContextMetadata {
                        scope: Some("commit".to_string()),
                        commit: Some(sha),
                        ..Default::default()
                    },
                )
            }
            v2::ReviewTarget::Custom { instructions } => {
                let instructions = instructions.trim().to_string();
                if instructions.is_empty() {
                    return Err(invalid_request("instructions must not be empty".to_string()));
                }
                (
                    instructions.clone(),
                    instructions,
                    core_protocol::ReviewContextMetadata {
                        scope: Some("custom".to_string()),
                        ..Default::default()
                    },
                )
            }
        };

        let request = core_protocol::ReviewRequest {
            prompt,
            user_facing_hint: hint.clone(),
            metadata: Some(metadata),
        };
        Ok((request, hint))
    }

    fn build_review_turn(turn_id: String, display_text: &str) -> v2::Turn {
        let items = if display_text.is_empty() {
            Vec::new()
        } else {
            vec![v2::ThreadItem::UserMessage {
                id: turn_id.clone(),
                content: vec![v2::UserInput::Text {
                    text: display_text.to_string(),
                    text_elements: Vec::new(),
                }],
            }]
        };

        v2::Turn {
            id: turn_id,
            items,
            status: v2::TurnStatus::InProgress,
            error: None,
        }
    }

    async fn emit_review_started(
        &self,
        request_id: RequestId,
        turn: v2::Turn,
        parent_thread_id: String,
        review_thread_id: String,
    ) {
        self.outgoing
            .send_response(
                request_id,
                v2::ReviewStartResponse {
                    turn: turn.clone(),
                    review_thread_id,
                },
            )
            .await;

        send_v2_notification(
            self.outgoing.as_ref(),
            ServerNotification::TurnStarted(v2::TurnStartedNotification {
                thread_id: parent_thread_id,
                turn,
            }),
        )
        .await;
    }

    async fn review_start(&mut self, request_id: RequestId, params: v2::ReviewStartParams) {
        let v2::ReviewStartParams {
            thread_id,
            target,
            delivery,
        } = params;

        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let Ok(parent_conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {thread_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if !self.thread_listeners.contains_key(&thread_id) {
            self.start_thread_listener(thread_id.clone(), parent_conversation.clone())
                .await;
        }

        let (review_request, display_text) = match Self::review_request_from_target(target) {
            Ok(values) => values,
            Err(error) => {
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        match delivery.unwrap_or(v2::ReviewDelivery::Inline).to_core() {
            code_protocol::protocol::ReviewDelivery::Inline => {
                let turn_id = parent_conversation
                    .submit(Op::Review { review_request })
                    .await;
                match turn_id {
                    Ok(turn_id) => {
                        let turn = Self::build_review_turn(turn_id, &display_text);
                        self.emit_review_started(request_id, turn, thread_id.clone(), thread_id)
                            .await;
                    }
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to start review: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                    }
                }
            }
            code_protocol::protocol::ReviewDelivery::Detached => {
                let mut review_config = (*self.config).clone();
                review_config.model = review_config.review_model.clone();
                let new_conversation = match self
                    .conversation_manager
                    .new_conversation(review_config.clone())
                    .await
                {
                    Ok(conversation) => conversation,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error creating detached review thread: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };

                let review_thread_id = match thread_id_from_conversation_id(new_conversation.conversation_id)
                {
                    Ok(id) => id,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error mapping thread id: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };

                let review_thread_id = review_thread_id.to_string();
                if !self.thread_listeners.contains_key(&review_thread_id) {
                    self.start_thread_listener(
                        review_thread_id.clone(),
                        new_conversation.conversation.clone(),
                    )
                    .await;
                }
                self.thread_configs
                    .insert(review_thread_id.clone(), review_config);

                let turn_id = new_conversation
                    .conversation
                    .submit(Op::Review { review_request })
                    .await;
                match turn_id {
                    Ok(turn_id) => {
                        let turn = Self::build_review_turn(turn_id, &display_text);
                        self.emit_review_started(request_id, turn, thread_id, review_thread_id)
                            .await;
                    }
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to start detached review: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                    }
                }
            }
        }
    }

    async fn turn_interrupt(&mut self, request_id: RequestId, params: v2::TurnInterruptParams) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let _ = conversation.submit(Op::Interrupt).await;
        self.outgoing
            .send_response(request_id, v2::TurnInterruptResponse {})
            .await;
    }

    async fn start_thread_listener(
        &mut self,
        thread_id: String,
        conversation: Arc<CodexConversation>,
    ) {
        if let Some(existing) = self.thread_listeners.remove(&thread_id) {
            let _ = existing.send(());
        }
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.thread_listeners.insert(thread_id.clone(), cancel_tx);
        let outgoing = self.outgoing.clone();
        tokio::spawn(async move {
            let mut state = V2StreamState::new(thread_id.clone());
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        break;
                    }
                    event = conversation.next_event() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(err) => {
                                tracing::warn!("conversation.next_event() failed with: {err}");
                                break;
                            }
                        };
                        handle_v2_event(&mut state, event, outgoing.clone(), conversation.clone()).await;
                    }
                }
            }
        });
    }

    #[allow(dead_code)]
    async fn fuzzy_file_search(&mut self, request_id: RequestId, params: FuzzyFileSearchParams) {
        let FuzzyFileSearchParams {
            query,
            roots,
            cancellation_token,
        } = params;

        let cancel_flag = match cancellation_token.clone() {
            Some(token) => {
                let mut pending_fuzzy_searches = self.pending_fuzzy_searches.lock().await;
                // if a cancellation_token is provided and a pending_request exists for
                // that token, cancel it
                if let Some(existing) = pending_fuzzy_searches.get(&token) {
                    existing.store(true, Ordering::Relaxed);
                }
                let flag = Arc::new(AtomicBool::new(false));
                pending_fuzzy_searches.insert(token.clone(), flag.clone());
                flag
            }
            None => Arc::new(AtomicBool::new(false)),
        };

        let results = match query.as_str() {
            "" => vec![],
            _ => run_fuzzy_file_search(query, roots, cancel_flag.clone()).await,
        };

        if let Some(token) = cancellation_token {
            let mut pending_fuzzy_searches = self.pending_fuzzy_searches.lock().await;
            if let Some(current_flag) = pending_fuzzy_searches.get(&token)
                && Arc::ptr_eq(current_flag, &cancel_flag)
            {
                pending_fuzzy_searches.remove(&token);
            }
        }

        let files = results
            .into_iter()
            .map(|result| code_app_server_protocol::FuzzyFileSearchResult {
                root: result.root,
                path: result.path,
                file_name: result.file_name,
                score: result.score,
                indices: result.indices,
            })
            .collect();
        let response = FuzzyFileSearchResponse { files };
        self.outgoing.send_response(request_id, response).await;
    }

    // ── V2 account/login/start ──────────────────────────────────────
    async fn login_account(&mut self, request_id: RequestId, params: LoginAccountParams) {
        match params {
            LoginAccountParams::ApiKey { api_key } => {
                self.login_api_key_common(request_id, &api_key, true).await;
            }
            LoginAccountParams::Chatgpt => {
                self.login_chatgpt_v2(request_id).await;
            }
            LoginAccountParams::ChatgptAuthTokens {
                id_token,
                access_token,
            } => {
                self.login_chatgpt_auth_tokens(request_id, id_token, access_token)
                    .await;
            }
        }
    }

    /// Shared API-key login logic.  When `v2` is true the response and
    /// notifications use the v2 wire types; otherwise the v1 types are used.
    async fn login_api_key_common(&mut self, request_id: RequestId, api_key: &str, v2: bool) {
        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Chatgpt)
        ) {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "API key login is disabled. Use ChatGPT login instead.".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        // Cancel any active login attempt.
        {
            let mut guard = self.active_login.lock().await;
            if let Some(active) = guard.take() {
                drop(active);
            }
        }

        match write_api_key_auth(&self.config.code_home, api_key) {
            Ok(()) => {
                self.auth_manager.reload();

                if v2 {
                    self.outgoing
                        .send_response(request_id, LoginAccountResponse::ApiKey {})
                        .await;

                    // AccountLoginCompleted notification
                    send_v2_notification(
                        &self.outgoing,
                        ServerNotification::AccountLoginCompleted(
                            AccountLoginCompletedNotification {
                                login_id: None,
                                success: true,
                                error: None,
                            },
                        ),
                    )
                    .await;

                    // AccountUpdated notification
                    let current_mode = self
                        .auth_manager
                        .auth()
                        .as_ref()
                        .map(|a| a.mode);
                    send_v2_notification(
                        &self.outgoing,
                        ServerNotification::AccountUpdated(AccountUpdatedNotification {
                            auth_mode: current_mode,
                        }),
                    )
                    .await;
                } else {
                    self.outgoing
                        .send_response(request_id, LoginApiKeyResponse {})
                        .await;

                    let current_mode = self
                        .auth_manager
                        .auth()
                        .as_ref()
                        .map(|a| a.mode);
                    send_v2_notification(
                        &self.outgoing,
                        ServerNotification::AuthStatusChange(AuthStatusChangeNotification {
                            auth_method: current_mode,
                        }),
                    )
                    .await;
                }
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to save api key: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn login_chatgpt_v2(&mut self, request_id: RequestId) {
        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Api)
        ) {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "ChatGPT login is disabled. Use API key login instead.".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let opts = ServerOptions {
            code_home: self.config.code_home.clone(),
            client_id: CLIENT_ID.to_string(),
            open_browser: false,
            originator: "code-app-server".to_string(),
            ..ServerOptions::new(
                self.config.code_home.clone(),
                CLIENT_ID.to_string(),
                "code-app-server".to_string(),
            )
        };

        match run_login_server(opts) {
            Ok(server) => {
                let login_id = Uuid::new_v4();
                let shutdown_handle = server.cancel_handle();

                // Replace active login if present.
                {
                    let mut guard = self.active_login.lock().await;
                    if let Some(existing) = guard.take() {
                        drop(existing);
                    }
                    *guard = Some(ActiveLogin {
                        shutdown_handle: shutdown_handle.clone(),
                        login_id,
                    });
                }

                let mut auth_url = server.auth_url.clone();
                if let Some(ref ws) = self.config.forced_chatgpt_workspace_id {
                    let sep = if auth_url.contains('?') { '&' } else { '?' };
                    auth_url = format!("{auth_url}{sep}allowed_workspace_id={ws}");
                }

                // Spawn background task to monitor completion.
                let outgoing_clone = self.outgoing.clone();
                let active_login = self.active_login.clone();
                let auth_manager = self.auth_manager.clone();
                tokio::spawn(async move {
                    let (success, error_msg) = match tokio::time::timeout(
                        LOGIN_CHATGPT_TIMEOUT,
                        server.block_until_done(),
                    )
                    .await
                    {
                        Ok(Ok(())) => (true, None),
                        Ok(Err(err)) => (false, Some(format!("Login server error: {err}"))),
                        Err(_elapsed) => {
                            shutdown_handle.shutdown();
                            (false, Some("Login timed out".to_string()))
                        }
                    };

                    // AccountLoginCompleted notification
                    send_v2_notification(
                        &outgoing_clone,
                        ServerNotification::AccountLoginCompleted(
                            AccountLoginCompletedNotification {
                                login_id: Some(login_id.to_string()),
                                success,
                                error: error_msg,
                            },
                        ),
                    )
                    .await;

                    if success {
                        auth_manager.reload();

                        let current_mode = auth_manager
                            .auth()
                            .as_ref()
                            .map(|a| a.mode);
                        send_v2_notification(
                            &outgoing_clone,
                            ServerNotification::AccountUpdated(AccountUpdatedNotification {
                                auth_mode: current_mode,
                            }),
                        )
                        .await;
                    }

                    // Clear the active login if it matches this attempt.
                    let mut guard = active_login.lock().await;
                    if guard.as_ref().map(|l| l.login_id) == Some(login_id) {
                        *guard = None;
                    }
                });

                let response = LoginAccountResponse::Chatgpt {
                    login_id: login_id.to_string(),
                    auth_url,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to start login server: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn login_chatgpt_auth_tokens(
        &mut self,
        request_id: RequestId,
        id_token: String,
        access_token: String,
    ) {
        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Api)
        ) {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "External ChatGPT auth is disabled. Use API key login instead."
                    .to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        // Cancel any active login attempt.
        {
            let mut guard = self.active_login.lock().await;
            if let Some(active) = guard.take() {
                drop(active);
            }
        }

        // Parse the id_token to extract email and plan_type.
        let id_token_info = match parse_id_token(&id_token) {
            Ok(info) => info,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid id_token: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        // Validate forced workspace if configured.
        if let Some(ref forced_ws) = self.config.forced_chatgpt_workspace_id {
            let account_id = id_token_info.get_chatgpt_account_id();
            if account_id.as_deref() != Some(forced_ws.as_str()) {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!(
                        "id_token workspace mismatch: expected {forced_ws}, got {}",
                        account_id.as_deref().unwrap_or("<none>")
                    ),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        }

        // Build TokenData and persist auth so refresh/reload paths can keep using it.
        let tokens = code_core::token_data::TokenData {
            id_token: id_token_info,
            access_token,
            refresh_token: String::new(),
            account_id: None,
        };

        let auth_dot_json = AuthDotJson {
            auth_mode: Some(CoreAuthMode::ChatgptAuthTokens),
            openai_api_key: None,
            tokens: Some(tokens),
            last_refresh: Some(Utc::now()),
        };
        let auth_file = get_auth_file(&self.config.code_home);
        if let Err(err) = write_auth_json(&auth_file, &auth_dot_json) {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to set external auth: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.auth_manager.reload();

        self.outgoing
            .send_response(request_id, LoginAccountResponse::ChatgptAuthTokens {})
            .await;

        // AccountUpdated notification
        send_v2_notification(
            &self.outgoing,
            ServerNotification::AccountUpdated(AccountUpdatedNotification {
                auth_mode: Some(CoreAuthMode::ChatgptAuthTokens),
            }),
        )
        .await;
    }

    // ── V2 account/login/cancel ─────────────────────────────────────
    async fn cancel_login_account(
        &mut self,
        request_id: RequestId,
        params: CancelLoginAccountParams,
    ) {
        let login_id_str = params.login_id;
        match Uuid::parse_str(&login_id_str) {
            Ok(uuid) => {
                let status = match self.cancel_login_chatgpt_common(uuid).await {
                    Ok(()) => CancelLoginAccountStatus::Canceled,
                    Err(CancelLoginError::NotFound) => CancelLoginAccountStatus::NotFound,
                };
                self.outgoing
                    .send_response(request_id, CancelLoginAccountResponse { status })
                    .await;
            }
            Err(_) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid login id: {login_id_str}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn cancel_login_chatgpt_common(
        &mut self,
        login_id: Uuid,
    ) -> std::result::Result<(), CancelLoginError> {
        let mut guard = self.active_login.lock().await;
        if guard.as_ref().map(|l| l.login_id) == Some(login_id) {
            if let Some(active) = guard.take() {
                drop(active);
            }
            Ok(())
        } else {
            Err(CancelLoginError::NotFound)
        }
    }

    // ── V2 account/logout ───────────────────────────────────────────
    async fn logout_account(&mut self, request_id: RequestId) {
        // Cancel any active login attempt.
        {
            let mut guard = self.active_login.lock().await;
            if let Some(active) = guard.take() {
                drop(active);
            }
        }

        if let Err(err) = self.auth_manager.logout() {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("logout failed: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.outgoing
            .send_response(request_id, LogoutAccountResponse {})
            .await;

        let current_mode = self
            .auth_manager
            .auth()
            .as_ref()
            .map(|a| a.mode);
        send_v2_notification(
            &self.outgoing,
            ServerNotification::AccountUpdated(AccountUpdatedNotification {
                auth_mode: current_mode,
            }),
        )
        .await;
    }

    // ── V2 account/read ─────────────────────────────────────────────
    async fn get_account(&self, request_id: RequestId, _params: GetAccountParams) {
        let requires_openai_auth = self.config.model_provider.requires_openai_auth;

        if !requires_openai_auth {
            let response = GetAccountResponse {
                account: None,
                requires_openai_auth,
            };
            self.outgoing.send_response(request_id, response).await;
            return;
        }

        let account = match self.auth_manager.auth() {
            Some(auth) => match auth.mode {
                CoreAuthMode::ApiKey => Some(Account::ApiKey {}),
                CoreAuthMode::Chatgpt | CoreAuthMode::ChatgptAuthTokens => {
                    // Extract email and plan_type from token data.
                    let email = auth.get_email();
                    let plan_type = auth.get_plan_type()
                        .and_then(|s| AccountPlanType::try_from_str(&s));
                    match (email, plan_type) {
                        (Some(email), Some(plan_type)) => {
                            Some(Account::Chatgpt { email, plan_type })
                        }
                        _ => None,
                    }
                }
            },
            None => None,
        };

        let response = GetAccountResponse {
            account,
            requires_openai_auth,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    // ── V2 account/rateLimits/read ──────────────────────────────────
    async fn get_account_rate_limits(&self, request_id: RequestId) {
        let auth = match self.auth_manager.auth() {
            Some(auth) => auth,
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "codex account authentication required to read rate limits"
                        .to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        if !auth.mode.is_chatgpt() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "chatgpt authentication required to read rate limits".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        match self.fetch_rate_limits(&auth).await {
            Ok(snapshot) => {
                self.outgoing
                    .send_response(
                        request_id,
                        GetAccountRateLimitsResponse {
                            rate_limits: snapshot,
                        },
                    )
                    .await;
            }
            Err(error) => {
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn fetch_rate_limits(
        &self,
        auth: &code_core::CodexAuth,
    ) -> std::result::Result<RateLimitSnapshot, JSONRPCErrorError> {
        let access_token = auth.get_token().await.map_err(|err| JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            message: format!("failed to get access token: {err}"),
            data: None,
        })?;
        let account_id = auth.get_account_id().unwrap_or_default();
        let chatgpt_base_url = &self.config.chatgpt_base_url;
        let url = format!("{}/api/codex/usage", chatgpt_base_url.trim_end_matches('/'));
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header("authorization", format!("Bearer {access_token}"))
            .header("chatgpt-account-id", &account_id)
            .send()
            .await
            .map_err(|err| JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to fetch rate limits: {err}"),
                data: None,
            })?;

        if !resp.status().is_success() {
            return Err(JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("rate limits request failed with status {}", resp.status()),
                data: None,
            });
        }

        let body: serde_json::Value = resp.json().await.map_err(|err| JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            message: format!("failed to parse rate limits response: {err}"),
            data: None,
        })?;

        let plan_type_str = body.get("plan_type").and_then(|v| v.as_str());
        let plan_type = plan_type_str.and_then(|s| AccountPlanType::try_from_str(s));

        let rate_limit = body.get("rate_limit");
        let primary = rate_limit
            .and_then(|rl| rl.get("primary_window"))
            .map(|w| RateLimitWindow {
                used_percent: w.get("used_percent").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                window_duration_mins: w
                    .get("limit_window_seconds")
                    .and_then(|v| v.as_i64())
                    .map(|s| s / 60),
                resets_at: w.get("reset_at").and_then(|v| v.as_i64()),
            });
        let secondary = rate_limit
            .and_then(|rl| rl.get("secondary_window"))
            .map(|w| RateLimitWindow {
                used_percent: w.get("used_percent").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                window_duration_mins: w
                    .get("limit_window_seconds")
                    .and_then(|v| v.as_i64())
                    .map(|s| s / 60),
                resets_at: w.get("reset_at").and_then(|v| v.as_i64()),
            });

        Ok(RateLimitSnapshot {
            primary,
            secondary,
            credits: None,
            plan_type,
        })
    }

    // ── V1 loginApiKey ──────────────────────────────────────────────
    async fn login_api_key_v1(&mut self, request_id: RequestId, params: LoginApiKeyParams) {
        self.login_api_key_common(request_id, &params.api_key, false)
            .await;
    }

    // ── V1 loginChatGpt ─────────────────────────────────────────────
    async fn login_chatgpt_v1(&mut self, request_id: RequestId) {
        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Api)
        ) {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "ChatGPT login is disabled. Use API key login instead.".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let opts = ServerOptions {
            code_home: self.config.code_home.clone(),
            client_id: CLIENT_ID.to_string(),
            open_browser: false,
            originator: "code-app-server".to_string(),
            ..ServerOptions::new(
                self.config.code_home.clone(),
                CLIENT_ID.to_string(),
                "code-app-server".to_string(),
            )
        };

        match run_login_server(opts) {
            Ok(server) => {
                let login_id = Uuid::new_v4();
                let shutdown_handle = server.cancel_handle();

                {
                    let mut guard = self.active_login.lock().await;
                    if let Some(existing) = guard.take() {
                        drop(existing);
                    }
                    *guard = Some(ActiveLogin {
                        shutdown_handle: shutdown_handle.clone(),
                        login_id,
                    });
                }

                let mut auth_url = server.auth_url.clone();
                if let Some(ref ws) = self.config.forced_chatgpt_workspace_id {
                    let sep = if auth_url.contains('?') { '&' } else { '?' };
                    auth_url = format!("{auth_url}{sep}allowed_workspace_id={ws}");
                }

                let outgoing_clone = self.outgoing.clone();
                let active_login = self.active_login.clone();
                let auth_manager = self.auth_manager.clone();
                tokio::spawn(async move {
                    let (success, error_msg) = match tokio::time::timeout(
                        LOGIN_CHATGPT_TIMEOUT,
                        server.block_until_done(),
                    )
                    .await
                    {
                        Ok(Ok(())) => (true, None),
                        Ok(Err(err)) => (false, Some(format!("Login server error: {err}"))),
                        Err(_elapsed) => {
                            shutdown_handle.shutdown();
                            (false, Some("Login timed out".to_string()))
                        }
                    };

                    send_v2_notification(
                        &outgoing_clone,
                        ServerNotification::LoginChatGptComplete(
                            LoginChatGptCompleteNotification {
                                login_id,
                                success,
                                error: error_msg,
                            },
                        ),
                    )
                    .await;

                    if success {
                        auth_manager.reload();
                        let current_mode = auth_manager.auth().as_ref().map(|a| a.mode);
                        send_v2_notification(
                            &outgoing_clone,
                            ServerNotification::AuthStatusChange(
                                AuthStatusChangeNotification {
                                    auth_method: current_mode,
                                },
                            ),
                        )
                        .await;
                    }

                    let mut guard = active_login.lock().await;
                    if guard.as_ref().map(|l| l.login_id) == Some(login_id) {
                        *guard = None;
                    }
                });

                let response = LoginChatGptResponse {
                    login_id,
                    auth_url,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to start login server: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    // ── V1 cancelLoginChatGpt ───────────────────────────────────────
    async fn cancel_login_chatgpt_v1(
        &mut self,
        request_id: RequestId,
        params: CancelLoginChatGptParams,
    ) {
        let _ = self.cancel_login_chatgpt_common(params.login_id).await;
        self.outgoing
            .send_response(request_id, CancelLoginChatGptResponse {})
            .await;
    }

    // ── V1 logoutChatGpt ────────────────────────────────────────────
    async fn logout_chatgpt_v1(&mut self, request_id: RequestId) {
        {
            let mut guard = self.active_login.lock().await;
            if let Some(active) = guard.take() {
                drop(active);
            }
        }

        if let Err(err) = self.auth_manager.logout() {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("logout failed: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.outgoing
            .send_response(request_id, LogoutChatGptResponse {})
            .await;

        let current_mode = self.auth_manager.auth().as_ref().map(|a| a.mode);
        send_v2_notification(
            &self.outgoing,
            ServerNotification::AuthStatusChange(AuthStatusChangeNotification {
                auth_method: current_mode,
            }),
        )
        .await;
    }

    // ── V1 getAuthStatus ────────────────────────────────────────────
    async fn get_auth_status(&self, request_id: RequestId, params: GetAuthStatusParams) {
        let requires_openai_auth = self.config.model_provider.requires_openai_auth;

        if !requires_openai_auth {
            let response = GetAuthStatusResponse {
                auth_method: None,
                auth_token: None,
                requires_openai_auth: Some(false),
            };
            self.outgoing.send_response(request_id, response).await;
            return;
        }

        let include_token = params.include_token.unwrap_or(false);

        let (auth_method, auth_token) = match self.auth_manager.auth() {
            Some(auth) => {
                let mode = auth.mode;
                let token = if include_token {
                    match mode {
                        CoreAuthMode::ApiKey => auth.api_key(),
                        CoreAuthMode::Chatgpt | CoreAuthMode::ChatgptAuthTokens => {
                            auth.get_token().await.ok()
                        }
                    }
                } else {
                    None
                };
                (Some(mode), token)
            }
            None => (None, None),
        };

        let response = GetAuthStatusResponse {
            auth_method,
            auth_token,
            requires_openai_auth: None,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn get_user_saved_config(&self, request_id: RequestId) {
        let config_toml = match load_config_as_toml_with_cli_overrides(&self.config.code_home, Vec::new()) {
            Ok(config_toml) => config_toml,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to load user config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let response = GetUserSavedConfigResponse {
            config: user_saved_config_from_toml(config_toml),
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn set_default_model(&self, request_id: RequestId, params: SetDefaultModelParams) {
        let model = params.model.as_deref();
        let effort = params.reasoning_effort.map(reasoning_effort_to_str);
        let overrides = [
            (&["model"][..], model),
            (&["model_reasoning_effort"][..], effort),
        ];

        match config_edit::persist_overrides_and_clear_if_none(
            &self.config.code_home,
            self.config.active_profile.as_deref(),
            &overrides,
        )
        .await
        {
            Ok(()) => {
                self.outgoing
                    .send_response(request_id, SetDefaultModelResponse {})
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to persist model defaults: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn get_user_agent(&self, request_id: RequestId) {
        let response = GetUserAgentResponse {
            user_agent: code_core::default_client::get_code_user_agent(None),
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn get_user_info(&self, request_id: RequestId) {
        let alleged_user_email = self.auth_manager.auth().and_then(|auth| auth.get_email());
        let response = UserInfoResponse { alleged_user_email };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn get_conversation_summary(
        &self,
        request_id: RequestId,
        params: GetConversationSummaryParams,
    ) {
        let path = match params {
            GetConversationSummaryParams::RolloutPath { rollout_path } => {
                if rollout_path.is_absolute() {
                    rollout_path
                } else {
                    self.config.code_home.join(rollout_path)
                }
            }
            GetConversationSummaryParams::ThreadId { conversation_id } => {
                let conversation_id = match conversation_id_from_thread_id(conversation_id) {
                    Ok(conversation_id) => conversation_id,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("invalid conversation id: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };
                match code_core::find_conversation_path_by_id_str(
                    &self.config.code_home,
                    &conversation_id.to_string(),
                )
                .await
                {
                    Ok(Some(path)) => path,
                    Ok(None) => {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "conversation not found".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to resolve conversation path: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                }
            }
        };

        let summary = conversation_summary_from_rollout_path(
            &self.config.code_home,
            &path,
            self.config.model_provider_id.as_str(),
        )
        .await;
        let response = GetConversationSummaryResponse { summary };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn list_conversations(&self, request_id: RequestId, params: ListConversationsParams) {
        let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
        let start = match params.cursor {
            Some(cursor) => match cursor.parse::<usize>() {
                Ok(value) => value,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {cursor}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };

        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let mut entries = match catalog.query(&SessionQuery::default()).await {
            Ok(entries) => entries,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to list conversations: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        if let Some(model_providers) = params.model_providers
            && !model_providers.is_empty()
        {
            entries.retain(|entry| {
                entry
                    .model_provider
                    .as_deref()
                    .or(Some(self.config.model_provider_id.as_str()))
                    .is_some_and(|provider| model_providers.iter().any(|candidate| candidate == provider))
            });
        }

        if start > entries.len() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cursor {start} exceeds total conversations {}", entries.len()),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let end = start.saturating_add(page_size).min(entries.len());
        let items: Vec<ConversationSummary> = entries[start..end]
            .iter()
            .map(|entry| conversation_summary_from_entry(&self.config.code_home, entry, self.config.model_provider_id.as_str()))
            .collect();
        let next_cursor = (end < entries.len()).then(|| end.to_string());

        self.outgoing
            .send_response(request_id, ListConversationsResponse { items, next_cursor })
            .await;
    }

    async fn resume_conversation(&self, request_id: RequestId, params: ResumeConversationParams) {
        let ResumeConversationParams {
            path,
            conversation_id,
            history,
            overrides,
        } = params;

        let config = match derive_config_from_params(
            overrides.unwrap_or_default(),
            self.code_linux_sandbox_exe.clone(),
        ) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let response_reasoning_effort = Some(map_reasoning_effort_to_wire(config.model_reasoning_effort));

        let (new_conversation, rollout_path, initial_messages) = if history.is_some()
            && path.is_none()
            && conversation_id.is_none()
        {
            let new_conversation = match self.conversation_manager.new_conversation(config).await {
                Ok(new_conversation) => new_conversation,
                Err(err) => {
                    let error = JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("failed to resume conversation from history: {err}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            };
            let thread_id = match thread_id_from_conversation_id(new_conversation.conversation_id) {
                Ok(thread_id) => thread_id,
                Err(err) => {
                    let error = JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("error mapping thread id: {err}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            };
            let rollout_path = find_rollout_path_for_thread(&self.config.code_home, &thread_id)
                .await
                .unwrap_or_else(|| self.config.code_home.join(code_core::SESSIONS_SUBDIR));
            (new_conversation, rollout_path, None)
        } else {
            let rollout_path = match resolve_rollout_path_for_v1_resume(
                &self.config.code_home,
                path,
                conversation_id,
            )
            .await
            {
                Ok(path) => path,
                Err(error) => {
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            };

            let new_conversation = match self
                .conversation_manager
                .resume_conversation_from_rollout(
                    config,
                    rollout_path.clone(),
                    self.conversation_manager.auth_manager(),
                )
                .await
            {
                Ok(new_conversation) => new_conversation,
                Err(err) => {
                    let error = JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("failed to resume conversation: {err}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            };

            (
                new_conversation,
                rollout_path.clone(),
                read_event_msgs_from_rollout(&rollout_path).await,
            )
        };

        let thread_id = match thread_id_from_conversation_id(new_conversation.conversation_id) {
            Ok(thread_id) => thread_id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error mapping thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        self.send_session_configured_notification(
            thread_id,
            &new_conversation.session_configured,
            initial_messages.clone(),
            response_reasoning_effort,
            rollout_path.clone(),
        )
        .await;

        self.outgoing
            .send_response(
                request_id,
                ResumeConversationResponse {
                    conversation_id: thread_id,
                    model: new_conversation.session_configured.model,
                    initial_messages,
                    rollout_path,
                },
            )
            .await;
    }

    async fn fork_conversation(&self, request_id: RequestId, params: ForkConversationParams) {
        let rollout_path = match resolve_rollout_path_for_v1_resume(
            &self.config.code_home,
            params.path.clone(),
            params.conversation_id,
        )
        .await
        {
            Ok(path) => path,
            Err(error) => {
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let config = match derive_config_from_params(
            params.overrides.unwrap_or_default(),
            self.code_linux_sandbox_exe.clone(),
        ) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let response_reasoning_effort = Some(map_reasoning_effort_to_wire(config.model_reasoning_effort));

        let new_conversation = match self
            .conversation_manager
            .fork_conversation(0, config, rollout_path.clone())
            .await
        {
            Ok(new_conversation) => new_conversation,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to fork conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let initial_messages = read_event_msgs_from_rollout(&rollout_path).await;

        let thread_id = match thread_id_from_conversation_id(new_conversation.conversation_id) {
            Ok(thread_id) => thread_id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error mapping thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let new_rollout_path = find_rollout_path_for_thread(&self.config.code_home, &thread_id)
            .await
            .unwrap_or_else(|| self.config.code_home.join(code_core::SESSIONS_SUBDIR));

        self.send_session_configured_notification(
            thread_id,
            &new_conversation.session_configured,
            initial_messages.clone(),
            response_reasoning_effort,
            new_rollout_path.clone(),
        )
        .await;

        self.outgoing
            .send_response(
                request_id,
                ForkConversationResponse {
                    conversation_id: thread_id,
                    model: new_conversation.session_configured.model,
                    initial_messages,
                    rollout_path: new_rollout_path,
                },
            )
            .await;
    }

    async fn archive_conversation_v1(
        &self,
        request_id: RequestId,
        params: ArchiveConversationParams,
    ) {
        let source = if params.rollout_path.is_absolute() {
            params.rollout_path
        } else {
            self.config.code_home.join(params.rollout_path)
        };
        let Some(file_name) = source.file_name() else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("invalid rollout path: {}", source.display()),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let archived_dir = self.config.code_home.join(code_core::ARCHIVED_SESSIONS_SUBDIR);
        if let Err(err) = tokio::fs::create_dir_all(&archived_dir).await {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to create archived directory: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }
        let destination = archived_dir.join(file_name);
        if let Err(err) = tokio::fs::rename(&source, &destination).await {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to archive conversation: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.outgoing
            .send_response(request_id, ArchiveConversationResponse {})
            .await;
    }

    async fn exec_one_off_command(&self, request_id: RequestId, params: ExecOneOffCommandParams) {
        if params.command.is_empty() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "command must not be empty".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let cwd = params.cwd.unwrap_or_else(|| self.config.cwd.clone());
        let exec_params = ExecParams {
            command: params.command,
            cwd,
            timeout_ms: params.timeout_ms,
            env: create_env(&self.config.shell_environment_policy),
            with_escalated_permissions: None,
            justification: None,
        };

        let requested_policy = params
            .sandbox_policy
            .map(map_sandbox_policy_from_wire)
            .unwrap_or_else(|| self.config.sandbox_policy.clone());
        let sandbox_type = get_platform_sandbox().unwrap_or(SandboxType::None);

        match process_exec_tool_call(
            exec_params,
            sandbox_type,
            &requested_policy,
            self.config.cwd.as_path(),
            &self.code_linux_sandbox_exe,
            None,
        )
        .await
        {
            Ok(output) => {
                self.outgoing
                    .send_response(
                        request_id,
                        ExecOneOffCommandResponse {
                            exit_code: output.exit_code,
                            stdout: output.stdout.text,
                            stderr: output.stderr.text,
                        },
                    )
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("exec failed: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn exec_one_off_command_v2(&self, request_id: RequestId, params: v2::CommandExecParams) {
        let timeout_ms = match params.timeout_ms {
            Some(timeout_ms) => match u64::try_from(timeout_ms) {
                Ok(timeout_ms) => Some(timeout_ms),
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: "timeout_ms must be greater than or equal to 0".to_string(),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => None,
        };

        let legacy_params = ExecOneOffCommandParams {
            command: params.command,
            timeout_ms,
            cwd: params.cwd,
            sandbox_policy: params.sandbox_policy.map(|policy| policy.to_core()),
        };

        self.exec_one_off_command(request_id, legacy_params).await;
    }

    async fn send_session_configured_notification(
        &self,
        session_id: ThreadId,
        session: &core_protocol::SessionConfiguredEvent,
        initial_messages: Option<Vec<code_protocol::protocol::EventMsg>>,
        reasoning_effort: Option<code_protocol::openai_models::ReasoningEffort>,
        rollout_path: PathBuf,
    ) {
        send_v2_notification(
            &self.outgoing,
            ServerNotification::SessionConfigured(SessionConfiguredNotification {
                session_id,
                model: session.model.clone(),
                reasoning_effort,
                history_log_id: session.history_log_id,
                history_entry_count: initial_messages
                    .as_ref()
                    .map_or(session.history_entry_count, Vec::len),
                initial_messages,
                rollout_path,
            }),
        )
        .await;
    }

    async fn apps_list(&self, request_id: RequestId, params: AppsListParams) {
        let AppsListParams { cursor, limit } = params;

        let connectors = match connectors::list_connectors(&self.config).await {
            Ok(connectors) => connectors,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to list apps: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let total = connectors.len();
        if total == 0 {
            self.outgoing
                .send_response(
                    request_id,
                    AppsListResponse {
                        data: Vec::new(),
                        next_cursor: None,
                    },
                )
                .await;
            return;
        }

        let effective_limit = limit.unwrap_or(total as u32).max(1) as usize;
        let effective_limit = effective_limit.min(total);
        let start = match cursor {
            Some(cursor) => match cursor.parse::<usize>() {
                Ok(idx) => idx,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {cursor}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };

        if start > total {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cursor {start} exceeds total apps {total}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let end = start.saturating_add(effective_limit).min(total);
        let data = connectors[start..end].to_vec();

        let next_cursor = if end < total {
            Some(end.to_string())
        } else {
            None
        };
        self.outgoing
            .send_response(request_id, AppsListResponse { data, next_cursor })
            .await;
    }

    async fn skills_list(&self, request_id: RequestId, params: v2::SkillsListParams) {
        let v2::SkillsListParams { cwds, force_reload: _ } = params;
        let cwds = if cwds.is_empty() {
            vec![self.config.cwd.clone()]
        } else {
            cwds
        };
        let mut disabled_paths = read_disabled_skill_paths(&self.config.code_home).await;
        disabled_paths.extend(
            self.disabled_skill_paths
                .iter()
                .map(PathBuf::from),
        );

        let mut data = Vec::new();
        for cwd in cwds {
            let mut config = (*self.config).clone();
            config.cwd = cwd.clone();
            let outcome = load_skills(&config);
            let skills = outcome
                .skills
                .into_iter()
                .map(|skill| {
                    let enabled = !disabled_paths
                        .iter()
                        .any(|disabled| paths_equivalent(disabled.as_path(), skill.path.as_path()));
                    v2::SkillMetadata {
                        name: skill.name,
                        description: skill.description,
                        short_description: None,
                        interface: None,
                        dependencies: None,
                        path: skill.path,
                        scope: map_core_skill_scope(skill.scope),
                        enabled,
                    }
                })
                .collect();
            let errors = outcome
                .errors
                .into_iter()
                .map(|err| v2::SkillErrorInfo {
                    path: err.path,
                    message: err.message,
                })
                .collect();
            data.push(v2::SkillsListEntry {
                cwd,
                skills,
                errors,
            });
        }

        self.outgoing
            .send_response(request_id, v2::SkillsListResponse { data })
            .await;
    }

    async fn skills_remote_read(&self, request_id: RequestId, _params: v2::SkillsRemoteReadParams) {
        match list_remote_skills(&self.config).await {
            Ok(skills) => {
                let data = skills
                    .into_iter()
                    .map(|skill| v2::RemoteSkillSummary {
                        id: skill.id,
                        name: skill.name,
                        description: skill.description,
                    })
                    .collect();
                self.outgoing
                    .send_response(request_id, v2::SkillsRemoteReadResponse { data })
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to read remote skills: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn skills_remote_write(
        &self,
        request_id: RequestId,
        params: v2::SkillsRemoteWriteParams,
    ) {
        let v2::SkillsRemoteWriteParams {
            hazelnut_id,
            is_preload,
        } = params;
        match download_remote_skill(&self.config, hazelnut_id.as_str(), is_preload).await {
            Ok(downloaded) => {
                self.outgoing
                    .send_response(
                        request_id,
                        v2::SkillsRemoteWriteResponse {
                            id: downloaded.id,
                            name: downloaded.name,
                            path: downloaded.path,
                        },
                    )
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to download remote skill: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn skills_config_write(
        &mut self,
        request_id: RequestId,
        params: v2::SkillsConfigWriteParams,
    ) {
        let v2::SkillsConfigWriteParams { path, enabled } = params;
        match set_skill_config(&self.config.code_home, path.as_path(), enabled).await {
            Ok(()) => {
                let normalized_path = normalize_skill_config_path(path.as_path());
                if enabled {
                    self.disabled_skill_paths.remove(&normalized_path);
                } else {
                    self.disabled_skill_paths.insert(normalized_path);
                }
                self.outgoing
                    .send_response(
                        request_id,
                        v2::SkillsConfigWriteResponse {
                            effective_enabled: enabled,
                        },
                    )
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to update skill settings: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn mcp_server_refresh(&mut self, request_id: RequestId, _params: Option<()>) {
        match load_global_mcp_servers(self.config.code_home.as_path()) {
            Ok(servers) => {
                let refreshed: HashMap<String, _> = servers.into_iter().collect();
                for config in self.thread_configs.values_mut() {
                    config.mcp_servers = refreshed.clone();
                }
                self.outgoing
                    .send_response(request_id, v2::McpServerRefreshResponse {})
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to reload MCP servers: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn mcp_server_oauth_login(
        &self,
        request_id: RequestId,
        params: v2::McpServerOauthLoginParams,
    ) {
        let v2::McpServerOauthLoginParams {
            name,
            scopes,
            timeout_secs,
        } = params;
        let servers = match load_global_mcp_servers(self.config.code_home.as_path()) {
            Ok(servers) => servers,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to load MCP servers: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let Some(server) = servers.get(&name) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("No MCP server named '{name}' found."),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let url = match &server.transport {
            McpServerTransportConfig::StreamableHttp { url, .. } => url.clone(),
            _ => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "OAuth login is only supported for streamable HTTP servers."
                        .to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let scopes = scopes.unwrap_or_default();
        match perform_oauth_login_return_url(
            name.as_str(),
            url.as_str(),
            self.config.code_home.as_path(),
            scopes.as_slice(),
            timeout_secs,
            None,
        )
        .await
        {
            Ok(handle) => {
                let authorization_url = handle.authorization_url().to_string();
                let notification_name = name.clone();
                let outgoing = Arc::clone(&self.outgoing);

                tokio::spawn(async move {
                    let (success, error) = match handle.wait().await {
                        Ok(()) => (true, None),
                        Err(err) => (false, Some(err.to_string())),
                    };

                    send_v2_notification(
                        outgoing.as_ref(),
                        ServerNotification::McpServerOauthLoginCompleted(
                            v2::McpServerOauthLoginCompletedNotification {
                                name: notification_name,
                                success,
                                error,
                            },
                        ),
                    )
                    .await;
                });

                self.outgoing
                    .send_response(
                        request_id,
                        v2::McpServerOauthLoginResponse { authorization_url },
                    )
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to login to MCP server '{name}': {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn list_mcp_server_status(
        &self,
        request_id: RequestId,
        params: v2::ListMcpServerStatusParams,
    ) {
        let servers = match load_global_mcp_servers(self.config.code_home.as_path()) {
            Ok(servers) => servers,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to list MCP servers: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let runtime_snapshot = collect_runtime_mcp_snapshot_by_server(&servers).await;

        let total = servers.len();
        let limit = params.limit.unwrap_or(total as u32).max(1) as usize;
        let effective_limit = limit.min(total);
        let start = match params.cursor {
            Some(cursor) => match cursor.parse::<usize>() {
                Ok(idx) => idx,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {cursor}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };

        if start > total {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cursor {start} exceeds total MCP servers {total}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let mut names: Vec<String> = servers.keys().cloned().collect();
        names.sort();
        let end = start.saturating_add(effective_limit).min(total);

        let data: Vec<v2::McpServerStatus> = names[start..end]
            .iter()
            .map(|name| {
                let tools = runtime_snapshot
                    .tools_by_server
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(tool_name, tool)| {
                        let value = match serde_json::to_value(tool) {
                            Ok(value) => value,
                            Err(err) => {
                                tracing::warn!(
                                    "failed to serialize MCP tool {name}/{tool_name}: {err}"
                                );
                                return None;
                            }
                        };
                        match code_protocol::mcp::Tool::from_mcp_value(value) {
                            Ok(tool) => Some((tool_name, tool)),
                            Err(err) => {
                                tracing::warn!(
                                    "failed to map MCP tool {name}/{tool_name} to protocol type: {err}"
                                );
                                None
                            }
                        }
                    })
                    .collect();
                let resources = runtime_snapshot
                    .resources_by_server
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|resource| {
                        let value = match serde_json::to_value(resource) {
                            Ok(value) => value,
                            Err(err) => {
                                tracing::warn!(
                                    "failed to serialize MCP resource for {name}: {err}"
                                );
                                return None;
                            }
                        };
                        match code_protocol::mcp::Resource::from_mcp_value(value) {
                            Ok(resource) => Some(resource),
                            Err(err) => {
                                tracing::warn!(
                                    "failed to map MCP resource for {name} to protocol type: {err}"
                                );
                                None
                            }
                        }
                    })
                    .collect();
                let resource_templates = runtime_snapshot
                    .resource_templates_by_server
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|template| {
                        let value = match serde_json::to_value(template) {
                            Ok(value) => value,
                            Err(err) => {
                                tracing::warn!(
                                    "failed to serialize MCP resource template for {name}: {err}"
                                );
                                return None;
                            }
                        };
                        match code_protocol::mcp::ResourceTemplate::from_mcp_value(value) {
                            Ok(template) => Some(template),
                            Err(err) => {
                                tracing::warn!(
                                    "failed to map MCP resource template for {name} to protocol type: {err}"
                                );
                                None
                            }
                        }
                    })
                    .collect();
                let auth_status = servers
                    .get(name)
                    .map(|config| {
                        mcp_auth_status_from_config(self.config.code_home.as_path(), name, config)
                    })
                    .unwrap_or(v2::McpAuthStatus::Unsupported);
                v2::McpServerStatus {
                    name: name.clone(),
                    tools,
                    resources,
                    resource_templates,
                    auth_status,
                }
            })
            .collect();

        let next_cursor = if end < total {
            Some(end.to_string())
        } else {
            None
        };

        self.outgoing
            .send_response(request_id, v2::ListMcpServerStatusResponse { data, next_cursor })
            .await;
    }

    async fn upload_feedback(&self, request_id: RequestId, params: v2::FeedbackUploadParams) {
        let thread_id = match params.thread_id {
            Some(thread_id) => {
                if let Err(err) = ThreadId::from_string(thread_id.as_str()) {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid thread id: {err}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
                thread_id
            }
            None => format!("no-active-thread-{}", Uuid::new_v4()),
        };
        tracing::warn!(
            "feedback/upload accepted but no uploader is configured in this build"
        );
        self.outgoing
            .send_response(request_id, v2::FeedbackUploadResponse { thread_id })
            .await;
    }

    async fn model_list(&self, request_id: RequestId, params: ModelListParams) {
        let ModelListParams { cursor, limit } = params;

        let remote_models = RemoteModelsManager::new(
            self.auth_manager.clone(),
            self.config.model_provider.clone(),
            self.config.code_home.clone(),
        );
        let mut model_infos = remote_models.remote_models_snapshot().await;
        model_infos.sort_by_key(|model| model.priority);

        let mut models: Vec<Model> = model_infos
            .into_iter()
            .map(model_info_to_app_server_model)
            .collect();

        if models.is_empty() {
            self.outgoing
                .send_response(
                    request_id,
                    ModelListResponse {
                        data: Vec::new(),
                        next_cursor: None,
                    },
                )
                .await;
            return;
        }

        let default_model_id = if models.iter().any(|model| model.id == self.config.model) {
            self.config.model.clone()
        } else {
            models[0].id.clone()
        };
        for model in &mut models {
            model.is_default = model.id == default_model_id;
        }

        let total = models.len();
        let effective_limit = limit.unwrap_or(total as u32).max(1) as usize;
        let effective_limit = effective_limit.min(total);
        let start = match cursor {
            Some(cursor) => match cursor.parse::<usize>() {
                Ok(idx) => idx,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {cursor}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };

        if start > total {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cursor {start} exceeds total models {total}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let end = start.saturating_add(effective_limit).min(total);
        let data = models[start..end].to_vec();
        let next_cursor = if end < total {
            Some(end.to_string())
        } else {
            None
        };

        self.outgoing
            .send_response(request_id, ModelListResponse { data, next_cursor })
            .await;
    }

    async fn experimental_feature_list(
        &self,
        request_id: RequestId,
        params: ExperimentalFeatureListParams,
    ) {
        let ExperimentalFeatureListParams { cursor, limit } = params;

        let data = EXPERIMENTAL_FEATURE_SPECS
            .iter()
            .map(|spec| {
                let (stage, display_name, description, announcement) = match spec.stage {
                    ExperimentalFeatureSpecStage::Experimental {
                        name,
                        menu_description,
                        announcement,
                    } => (
                        ExperimentalFeatureStage::Beta,
                        Some(name.to_string()),
                        Some(menu_description.to_string()),
                        Some(announcement.to_string()),
                    ),
                    ExperimentalFeatureSpecStage::UnderDevelopment => (
                        ExperimentalFeatureStage::UnderDevelopment,
                        None,
                        None,
                        None,
                    ),
                    ExperimentalFeatureSpecStage::Stable => {
                        (ExperimentalFeatureStage::Stable, None, None, None)
                    }
                    ExperimentalFeatureSpecStage::Deprecated => {
                        (ExperimentalFeatureStage::Deprecated, None, None, None)
                    }
                };

                ExperimentalFeature {
                    name: spec.key.to_string(),
                    stage,
                    display_name,
                    description,
                    announcement,
                    enabled: spec.default_enabled,
                    default_enabled: spec.default_enabled,
                }
            })
            .collect::<Vec<_>>();

        if data.is_empty() {
            self.outgoing
                .send_response(
                    request_id,
                    ExperimentalFeatureListResponse {
                        data: Vec::new(),
                        next_cursor: None,
                    },
                )
                .await;
            return;
        }

        let total = data.len();
        let effective_limit = limit.unwrap_or(total as u32).max(1) as usize;
        let effective_limit = effective_limit.min(total);
        let start = match cursor {
            Some(cursor) => match cursor.parse::<usize>() {
                Ok(idx) => idx,
                Err(_) => {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("invalid cursor: {cursor}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            },
            None => 0,
        };

        if start > total {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cursor {start} exceeds total feature flags {total}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let end = start.saturating_add(effective_limit).min(total);
        let data = data[start..end].to_vec();
        let next_cursor = if end < total {
            Some(end.to_string())
        } else {
            None
        };

        self.outgoing
            .send_response(
                request_id,
                ExperimentalFeatureListResponse { data, next_cursor },
            )
            .await;
    }

    async fn collaboration_mode_list(
        &self,
        request_id: RequestId,
        _params: CollaborationModeListParams,
    ) {
        let data = vec![
            CollaborationModeMask {
                name: "Plan".to_string(),
                mode: Some(ModeKind::Plan),
                model: None,
                reasoning_effort: None,
                developer_instructions: None,
            },
            CollaborationModeMask {
                name: "Default".to_string(),
                mode: Some(ModeKind::Default),
                model: None,
                reasoning_effort: None,
                developer_instructions: None,
            },
        ];
        self.outgoing
            .send_response(request_id, CollaborationModeListResponse { data })
            .await;
    }

    async fn mock_experimental_method(
        &self,
        request_id: RequestId,
        params: v2::MockExperimentalMethodParams,
    ) {
        self.outgoing
            .send_response(
                request_id,
                v2::MockExperimentalMethodResponse {
                    echoed: params.value,
                },
            )
            .await;
    }
}

impl CodexMessageProcessor {
    // Minimal compatibility layer: translate SendUserTurn into our current
    // flow by submitting only the user items. We intentionally do not attempt
    // per‑turn reconfiguration here (model, cwd, approval, sandbox) to avoid
    // destabilizing the session. This preserves behavior and acks the request
    // so clients using the new method continue to function.
    async fn send_user_turn_compat(
        &mut self,
        request_id: RequestId,
        params: SendUserTurnParams,
    ) {
        let SendUserTurnParams {
            conversation_id,
            items,
            cwd,
            approval_policy,
            sandbox_policy,
            model,
            effort,
            summary,
            output_schema,
            ..
        } = params;
        let thread_id = conversation_id.to_string();
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if !cwd.is_absolute() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("cwd is not absolute: {cwd:?}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let mut updated_config = self
            .thread_configs
            .get(&thread_id)
            .cloned()
            .unwrap_or_else(|| (*self.config).clone());
        updated_config.cwd = cwd;
        updated_config.approval_policy = map_ask_for_approval_from_wire(approval_policy);
        updated_config.sandbox_policy = map_sandbox_policy_from_wire(sandbox_policy);
        updated_config.model = model;
        updated_config.model_reasoning_summary = map_reasoning_summary_from_wire(summary);
        if let Some(effort) = effort {
            let core_effort = map_reasoning_effort_from_wire(effort);
            updated_config.model_reasoning_effort = core_effort;
            updated_config.preferred_model_reasoning_effort = Some(core_effort);
        }

        if let Err(err) = conversation
            .submit(configure_session_from_config(&updated_config))
            .await
        {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("error updating session config: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }
        self.thread_configs.insert(thread_id, updated_config);

        // Map wire input items into core protocol items.
        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = conversation
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: output_schema,
            })
            .await;

        // Acknowledge.
        self.outgoing.send_response(request_id, SendUserTurnResponse {}).await;
    }
}

async fn apply_bespoke_event_handling(
    event: Event,
    conversation_id: ConversationId,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
    _pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
) {
    let Event { id: _event_id, msg, .. } = event;
    match msg {
        EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id,
            changes,
            reason,
            grant_root,
        }) => {
            let thread_id = match thread_id_from_conversation_id(conversation_id) {
                Ok(id) => id,
                Err(err) => {
                    error!("failed to map conversation id: {err}");
                    return;
                }
            };
            // Map core FileChange to wire FileChange
            let file_changes: HashMap<PathBuf, code_protocol::protocol::FileChange> = changes
                .into_iter()
                .map(|(p, c)| {
                    let mapped = match c {
                        code_core::protocol::FileChange::Add { content } => {
                            code_protocol::protocol::FileChange::Add { content }
                        }
                        code_core::protocol::FileChange::Delete => {
                            code_protocol::protocol::FileChange::Delete { content: String::new() }
                        }
                        code_core::protocol::FileChange::Update {
                            unified_diff,
                            move_path,
                            original_content,
                            new_content,
                        } => {
                            code_protocol::protocol::FileChange::Update {
                                unified_diff,
                                move_path,
                                original_content,
                                new_content,
                            }
                        }
                    };
                    (p, mapped)
                })
                .collect();

            let params = ApplyPatchApprovalParams {
                conversation_id: thread_id,
                call_id: call_id.clone(),
                file_changes,
                reason,
                grant_root,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(APPLY_PATCH_APPROVAL_METHOD, Some(value))
                .await;
            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            tokio::spawn(async move {
                on_patch_approval_response(approval_id, rx, conversation).await;
            });
        }
        EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            command,
            cwd,
            reason,
        }) => {
            let thread_id = match thread_id_from_conversation_id(conversation_id) {
                Ok(id) => id,
                Err(err) => {
                    error!("failed to map conversation id: {err}");
                    return;
                }
            };
            let parsed_cmd = vec![code_protocol::parse_command::ParsedCommand::Unknown {
                cmd: command_for_display(&command),
            }];
            let params = ExecCommandApprovalParams {
                conversation_id: thread_id,
                call_id: call_id.clone(),
                command,
                cwd,
                reason,
                parsed_cmd,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(EXEC_COMMAND_APPROVAL_METHOD, Some(value))
                .await;

            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            tokio::spawn(async move {
                on_exec_approval_response(approval_id, rx, conversation).await;
            });
        }
        EventMsg::DynamicToolCallRequest(request) => {
            let call_id = request.call_id;
            let params = LegacyDynamicToolCallParams {
                conversation_id,
                turn_id: request.turn_id,
                call_id: call_id.clone(),
                tool: request.tool,
                arguments: request.arguments,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(DYNAMIC_TOOL_CALL_METHOD, Some(value))
                .await;

            tokio::spawn(async move {
                on_dynamic_tool_call_response_legacy(call_id, rx, conversation).await;
            });
        }
        // No special handling needed for interrupts; responses are sent immediately.

        _ => {}
    }
}

fn derive_config_from_params(
    params: NewConversationParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let NewConversationParams {
        model,
        model_provider,
        profile,
        cwd,
        approval_policy,
        sandbox: sandbox_mode,
        config: cli_overrides,
        base_instructions,
        developer_instructions,
        compact_prompt,
        include_apply_patch_tool,
        ..
    } = params;
    let base_instructions = merge_instructions(base_instructions, developer_instructions);
    let overrides = ConfigOverrides {
        model,
        review_model: None,
        config_profile: profile,
        cwd: cwd.map(PathBuf::from),
        approval_policy: approval_policy.map(map_ask_for_approval_from_wire),
        sandbox_mode,
        model_provider,
        code_linux_sandbox_exe,
        base_instructions,
        include_plan_tool: None,
        include_apply_patch_tool,
        include_view_image_tool: None,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
        debug: None,
        tools_web_search_request: None,
        mcp_servers: None,
        experimental_client_tools: None,
        dynamic_tools: None,
        compact_prompt_override: compact_prompt,
        compact_prompt_override_file: None,
    };

    let cli_overrides = cli_overrides
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides).map(sanitize_app_server_config)
}

fn merge_instructions(base: Option<String>, developer: Option<String>) -> Option<String> {
    match (base, developer) {
        (None, None) => None,
        (Some(text), None) | (None, Some(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        (Some(base), Some(dev)) => {
            let base_trim = base.trim();
            let dev_trim = dev.trim();
            if base_trim.is_empty() && dev_trim.is_empty() {
                None
            } else if base_trim.is_empty() {
                Some(dev)
            } else if dev_trim.is_empty() {
                Some(base)
            } else {
                Some(format!("{base}\n\n{dev}"))
            }
        }
    }
}

fn conversation_id_from_thread_id(thread_id: ThreadId) -> Result<ConversationId, uuid::Error> {
    ConversationId::from_string(&thread_id.to_string())
}

fn conversation_id_from_thread_str(thread_id: &str) -> Result<ConversationId, uuid::Error> {
    ConversationId::from_string(thread_id)
}

fn thread_id_from_conversation_id(conversation_id: ConversationId) -> Result<ThreadId, uuid::Error> {
    ThreadId::from_string(&conversation_id.to_string())
}

fn request_id_from_client_request(request: &ClientRequest) -> Option<RequestId> {
    let value = serde_json::to_value(request).ok()?;
    let id_value = value.get("id")?.clone();
    let app_id = serde_json::from_value::<code_app_server_protocol::RequestId>(id_value).ok()?;
    Some(to_mcp_request_id(app_id))
}

fn derive_config_from_thread_start_params(
    params: &v2::ThreadStartParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    derive_config_from_thread_overrides(
        &params.model,
        &params.model_provider,
        &params.cwd,
        &params.approval_policy,
        &params.sandbox,
        &params.config,
        &params.base_instructions,
        &params.developer_instructions,
        params.dynamic_tools.as_ref(),
        code_linux_sandbox_exe,
    )
}

fn derive_config_from_thread_overrides(
    model: &Option<String>,
    model_provider: &Option<String>,
    cwd: &Option<String>,
    approval_policy: &Option<v2::AskForApproval>,
    sandbox_mode: &Option<v2::SandboxMode>,
    cli_overrides: &Option<HashMap<String, serde_json::Value>>,
    base_instructions: &Option<String>,
    developer_instructions: &Option<String>,
    dynamic_tools: Option<&Vec<v2::DynamicToolSpec>>,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let base_instructions = merge_instructions(base_instructions.clone(), developer_instructions.clone());
    let overrides = ConfigOverrides {
        model: model.clone(),
        review_model: None,
        config_profile: None,
        cwd: cwd.as_ref().map(PathBuf::from),
        approval_policy: approval_policy
            .clone()
            .map(|policy| map_ask_for_approval_from_wire(policy.to_core())),
        sandbox_mode: sandbox_mode.clone().map(|mode| mode.to_core()),
        model_provider: model_provider.clone(),
        code_linux_sandbox_exe,
        base_instructions,
        include_plan_tool: None,
        include_apply_patch_tool: None,
        include_view_image_tool: None,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
        debug: None,
        tools_web_search_request: None,
        mcp_servers: None,
        experimental_client_tools: None,
        dynamic_tools: map_dynamic_tools(dynamic_tools),
        compact_prompt_override: None,
        compact_prompt_override_file: None,
    };

    let cli_overrides = cli_overrides
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides).map(sanitize_app_server_config)
}

fn sanitize_app_server_config(mut config: Config) -> Config {
    config.project_doc_max_bytes = 0;
    config.user_instructions = None;
    config
}

fn apply_turn_overrides(config: &mut Config, params: &v2::TurnStartParams) {
    if let Some(cwd) = params.cwd.as_ref() {
        config.cwd = cwd.clone();
    }
    if let Some(policy) = params.approval_policy.clone() {
        config.approval_policy = map_ask_for_approval_from_wire(policy.to_core());
    }
    if let Some(policy) = params.sandbox_policy.as_ref() {
        config.sandbox_policy = map_sandbox_policy_from_wire(policy.to_core());
    }
    if let Some(model) = params.model.as_ref() {
        config.model = model.clone();
        config.model_explicit = true;
    }
    if let Some(effort) = params.effort {
        let effort = map_reasoning_effort_from_wire(effort);
        config.model_reasoning_effort = effort;
        config.preferred_model_reasoning_effort = Some(effort);
    }
    if let Some(summary) = params.summary {
        config.model_reasoning_summary = summary.into();
    }
    if let Some(personality) = params.personality {
        config.model_personality = Some(map_personality_from_wire(personality));
    }
}

fn configure_session_from_config(config: &Config) -> Op {
    Op::ConfigureSession {
        provider: config.model_provider.clone(),
        model: config.model.clone(),
        model_explicit: config.model_explicit,
        model_reasoning_effort: config.model_reasoning_effort,
        preferred_model_reasoning_effort: config.preferred_model_reasoning_effort,
        model_reasoning_summary: config.model_reasoning_summary,
        model_text_verbosity: config.model_text_verbosity,
        user_instructions: config.user_instructions.clone(),
        base_instructions: config.base_instructions.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        disable_response_storage: config.disable_response_storage,
        notify: config.notify.clone(),
        cwd: config.cwd.clone(),
        resume_path: None,
        demo_developer_message: config.demo_developer_message.clone(),
        dynamic_tools: config.dynamic_tools.clone(),
    }
}

fn map_dynamic_tools(
    tools: Option<&Vec<v2::DynamicToolSpec>>,
) -> Option<Vec<code_protocol::dynamic_tools::DynamicToolSpec>> {
    tools.map(|items| {
        items
            .iter()
            .map(|tool| code_protocol::dynamic_tools::DynamicToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
            .collect()
    })
}

fn map_v2_user_input(input: v2::UserInput) -> CoreInputItem {
    match input {
        v2::UserInput::Text { text, .. } => CoreInputItem::Text { text },
        v2::UserInput::Image { url } => CoreInputItem::Image { image_url: url },
        v2::UserInput::LocalImage { path } => CoreInputItem::LocalImage { path },
        v2::UserInput::Skill { name, path } => CoreInputItem::Text {
            text: format!("skill:{name} ({})", path.display()),
        },
        v2::UserInput::Mention { name, path } => CoreInputItem::Text {
            text: format!("mention:{name} ({path})"),
        },
    }
}

async fn send_v2_notification(outgoing: &OutgoingMessageSender, notification: ServerNotification) {
    let method = notification.to_string();
    let params = match notification.to_params() {
        Ok(params) => Some(params),
        Err(err) => {
            error!("failed to serialize notification params: {err}");
            return;
        }
    };
    outgoing
        .send_notification(OutgoingNotification { method, params })
        .await;
}

async fn send_server_request(
    outgoing: &OutgoingMessageSender,
    payload: ServerRequestPayload,
) -> Option<tokio::sync::oneshot::Receiver<mcp_types::Result>> {
    let request = payload.request_with_id(code_app_server_protocol::RequestId::Integer(0));
    let value = serde_json::to_value(request).ok()?;
    let method = value.get("method")?.as_str()?.to_string();
    let params = value.get("params").cloned();
    Some(outgoing.send_request(&method, params).await)
}

fn join_command(command: &[String]) -> String {
    shlex::try_join(command.iter().map(String::as_str))
        .unwrap_or_else(|_| command.join(" "))
}

fn command_for_display(command: &[String]) -> String {
    if command.len() >= 3 && command.get(1).is_some_and(|flag| flag == "-lc") {
        let script = &command[2];
        if let Some(start) = script.find("&& (")
            && script.ends_with(')')
        {
            return script[start + 4..script.len() - 1].trim().to_string();
        }
        return script.clone();
    }
    join_command(command)
}

#[derive(Default)]
struct V2MessageAccumulator {
    id: String,
    text: String,
}

#[derive(Default)]
struct V2ReasoningAccumulator {
    id: String,
    summary: Vec<String>,
    summary_index: i64,
    content: Vec<String>,
    content_index: i64,
}

#[derive(Clone)]
struct CommandExecutionState {
    command: String,
    cwd: PathBuf,
    command_actions: Vec<v2::CommandAction>,
    process_id: Option<String>,
}

struct V2StreamState {
    thread_id: String,
    current_turn_id: Option<String>,
    agent_message: Option<V2MessageAccumulator>,
    reasoning: Option<V2ReasoningAccumulator>,
    file_change_started: HashSet<String>,
    file_change_payloads: HashMap<String, Vec<v2::FileUpdateChange>>,
    command_execs: HashMap<String, CommandExecutionState>,
    last_error: Option<v2::TurnError>,
}

impl V2StreamState {
    fn new(thread_id: String) -> Self {
        Self {
            thread_id,
            current_turn_id: None,
            agent_message: None,
            reasoning: None,
            file_change_started: HashSet::new(),
            file_change_payloads: HashMap::new(),
            command_execs: HashMap::new(),
            last_error: None,
        }
    }

    fn reset_for_turn(&mut self, turn_id: &str) {
        if self.current_turn_id.as_deref() == Some(turn_id) {
            return;
        }
        self.current_turn_id = Some(turn_id.to_string());
        self.agent_message = None;
        self.reasoning = None;
        self.last_error = None;
        self.file_change_started.clear();
        self.file_change_payloads.clear();
        self.command_execs.clear();
    }

    async fn finish_turn(&mut self, turn_id: String, status: v2::TurnStatus, outgoing: &OutgoingMessageSender) {
        self.flush_agent_message(&turn_id, outgoing).await;
        self.flush_reasoning(&turn_id, outgoing).await;
        let error = self.last_error.take();
        send_v2_notification(
            outgoing,
            ServerNotification::TurnCompleted(v2::TurnCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn: v2::Turn {
                    id: turn_id,
                    items: Vec::new(),
                    status,
                    error,
                },
            }),
        )
        .await;
        self.current_turn_id = None;
    }

    async fn flush_agent_message(&mut self, turn_id: &str, outgoing: &OutgoingMessageSender) {
        let Some(acc) = self.agent_message.take() else {
            return;
        };
        let item = v2::ThreadItem::AgentMessage {
            id: acc.id,
            text: acc.text,
        };
        send_v2_notification(
            outgoing,
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.to_string(),
                item,
            }),
        )
        .await;
    }

    async fn flush_reasoning(&mut self, turn_id: &str, outgoing: &OutgoingMessageSender) {
        let Some(reasoning) = self.reasoning.take() else {
            return;
        };
        let item = v2::ThreadItem::Reasoning {
            id: reasoning.id,
            summary: reasoning.summary,
            content: reasoning.content,
        };
        send_v2_notification(
            outgoing,
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.to_string(),
                item,
            }),
        )
        .await;
    }
}

async fn handle_v2_event(
    state: &mut V2StreamState,
    event: Event,
    outgoing: Arc<OutgoingMessageSender>,
    conversation: Arc<CodexConversation>,
) {
    let Event { id: turn_id, msg, .. } = event;
    state.reset_for_turn(&turn_id);

    match msg {
        EventMsg::TaskStarted => {
            // TurnStarted is emitted by the turn/start handler.
        }
        EventMsg::TaskComplete(task_complete) => {
            if let Some(text) = task_complete.last_agent_message {
                state.agent_message = Some(V2MessageAccumulator {
                    id: Uuid::new_v4().to_string(),
                    text,
                });
                state.flush_agent_message(&turn_id, outgoing.as_ref()).await;
            }
            outgoing
                .send_notification(OutgoingNotification {
                    method: "codex/event/task_complete".to_string(),
                    params: Some(serde_json::json!({
                        "threadId": state.thread_id,
                        "turnId": turn_id,
                    })),
                })
                .await;
            let status = if state.last_error.is_some() {
                v2::TurnStatus::Failed
            } else {
                v2::TurnStatus::Completed
            };
            state.finish_turn(turn_id, status, outgoing.as_ref()).await;
        }
        EventMsg::TurnAborted(_) => {
            state.finish_turn(turn_id, v2::TurnStatus::Interrupted, outgoing.as_ref()).await;
        }
        EventMsg::Error(ev) => {
            let error = v2::TurnError {
                message: ev.message,
                codex_error_info: None,
                additional_details: None,
            };
            state.last_error = Some(error.clone());
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::Error(v2::ErrorNotification {
                    error,
                    will_retry: false,
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                }),
            )
            .await;
        }
        EventMsg::AgentMessageDelta(ev) => {
            let acc = state.agent_message.get_or_insert_with(|| V2MessageAccumulator {
                id: Uuid::new_v4().to_string(),
                text: String::new(),
            });
            acc.text.push_str(&ev.delta);
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::AgentMessageDelta(v2::AgentMessageDeltaNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: acc.id.clone(),
                    delta: ev.delta,
                }),
            )
            .await;
        }
        EventMsg::AgentMessage(ev) => {
            let acc = state.agent_message.get_or_insert_with(|| V2MessageAccumulator {
                id: Uuid::new_v4().to_string(),
                text: String::new(),
            });
            acc.text = ev.message;
            state.flush_agent_message(&turn_id, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoning(ev) => {
            handle_reasoning_summary_delta(state, turn_id, ev.text, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningDelta(ev) => {
            handle_reasoning_summary_delta(state, turn_id, ev.delta, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningRawContent(ev) => {
            handle_reasoning_content_delta(state, turn_id, ev.text, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningRawContentDelta(ev) => {
            handle_reasoning_content_delta(state, turn_id, ev.delta, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningSectionBreak(_) => {
            if let Some(reasoning) = state.reasoning.as_mut() {
                reasoning.summary_index += 1;
                reasoning.summary.push(String::new());
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ReasoningSummaryPartAdded(
                        v2::ReasoningSummaryPartAddedNotification {
                            thread_id: state.thread_id.clone(),
                            turn_id: turn_id.clone(),
                            item_id: reasoning.id.clone(),
                            summary_index: reasoning.summary_index,
                        },
                    ),
                )
                .await;
            }
        }
        EventMsg::UserMessage(ev) => {
            let mut content = Vec::new();
            if !ev.message.trim().is_empty() {
                content.push(v2::UserInput::Text {
                    text: ev.message,
                    text_elements: Vec::new(),
                });
            }
            if let Some(images) = ev.images {
                for url in images {
                    content.push(v2::UserInput::Image { url });
                }
            }
            let item = v2::ThreadItem::UserMessage {
                id: Uuid::new_v4().to_string(),
                content,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExecCommandBegin(ev) => {
            let command = join_command(&ev.command);
            let command_actions = ev
                .parsed_cmd
                .into_iter()
                .map(|cmd| v2::CommandAction::from(code_protocol::parse_command::ParsedCommand::from(cmd)))
                .collect::<Vec<_>>();
            if !state.command_execs.contains_key(&ev.call_id) {
                let item = v2::ThreadItem::CommandExecution {
                    id: ev.call_id.clone(),
                    command: command.clone(),
                    cwd: ev.cwd.clone(),
                    process_id: None,
                    status: v2::CommandExecutionStatus::InProgress,
                    command_actions: command_actions.clone(),
                    aggregated_output: None,
                    exit_code: None,
                    duration_ms: None,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }
            state.command_execs.insert(
                ev.call_id,
                CommandExecutionState {
                    command,
                    cwd: ev.cwd,
                    command_actions,
                    process_id: None,
                },
            );
        }
        EventMsg::ExecCommandOutputDelta(ev) => {
            let delta = String::from_utf8_lossy(&ev.chunk).to_string();
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::CommandExecutionOutputDelta(
                    v2::CommandExecutionOutputDeltaNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: ev.call_id,
                        delta,
                    },
                ),
            )
            .await;
        }
        EventMsg::ExecCommandEnd(ev) => {
            let status = if ev.exit_code == 0 {
                v2::CommandExecutionStatus::Completed
            } else {
                v2::CommandExecutionStatus::Failed
            };
            let duration_ms = i64::try_from(ev.duration.as_millis()).ok();
            let (command, cwd, command_actions, process_id) = state
                .command_execs
                .remove(&ev.call_id)
                .map(|state| (state.command, state.cwd, state.command_actions, state.process_id))
                .unwrap_or_else(|| (String::new(), PathBuf::new(), Vec::new(), None));
            let aggregated_output = if ev.stdout.is_empty() && ev.stderr.is_empty() {
                None
            } else {
                Some(format!("{}{}", ev.stdout, ev.stderr))
            };
            let item = v2::ThreadItem::CommandExecution {
                id: ev.call_id,
                command,
                cwd,
                process_id,
                status,
                command_actions,
                aggregated_output,
                exit_code: Some(ev.exit_code),
                duration_ms,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExecApprovalRequest(ev) => {
            let item_id = ev.call_id.clone();
            let command = join_command(&ev.command);
            let first_start = !state.command_execs.contains_key(&item_id);
            state.command_execs.entry(item_id.clone()).or_insert(CommandExecutionState {
                command: command.clone(),
                cwd: ev.cwd.clone(),
                command_actions: Vec::new(),
                process_id: None,
            });
            if first_start {
                let item = v2::ThreadItem::CommandExecution {
                    id: item_id.clone(),
                    command: command.clone(),
                    cwd: ev.cwd.clone(),
                    process_id: None,
                    status: v2::CommandExecutionStatus::InProgress,
                    command_actions: Vec::new(),
                    aggregated_output: None,
                    exit_code: None,
                    duration_ms: None,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }

            let params = v2::CommandExecutionRequestApprovalParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                reason: ev.reason,
                command: Some(command.clone()),
                cwd: Some(ev.cwd.clone()),
                command_actions: None,
                proposed_execpolicy_amendment: None,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::CommandExecutionRequestApproval(params)).await
            {
                let conversation = conversation.clone();
                let outgoing = outgoing.clone();
                let command = command.clone();
                let cwd = ev.cwd.clone();
                let command_actions = Vec::new();
                let thread_id = state.thread_id.clone();
                tokio::spawn(async move {
                    on_command_execution_request_approval_response(
                        thread_id,
                        turn_id.clone(),
                        item_id,
                        command,
                        cwd,
                        command_actions,
                        rx,
                        conversation,
                        outgoing,
                    )
                    .await;
                });
            }
        }
        EventMsg::ApplyPatchApprovalRequest(ev) => {
            let item_id = ev.call_id.clone();
            let changes = convert_patch_changes(&ev.changes);
            state
                .file_change_payloads
                .insert(item_id.clone(), changes.clone());
            if state.file_change_started.insert(item_id.clone()) {
                let item = v2::ThreadItem::FileChange {
                    id: item_id.clone(),
                    changes: changes.clone(),
                    status: v2::PatchApplyStatus::InProgress,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }

            let params = v2::FileChangeRequestApprovalParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                reason: ev.reason,
                grant_root: ev.grant_root,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::FileChangeRequestApproval(params)).await
            {
                let conversation = conversation.clone();
                let outgoing = outgoing.clone();
                let thread_id = state.thread_id.clone();
                let turn_id = turn_id.clone();
                tokio::spawn(async move {
                    on_file_change_request_approval_response(
                        thread_id,
                        turn_id,
                        item_id,
                        changes,
                        rx,
                        conversation,
                        outgoing,
                    )
                    .await;
                });
            }
        }
        EventMsg::PatchApplyBegin(ev) => {
            let changes = convert_patch_changes(&ev.changes);
            state
                .file_change_payloads
                .insert(ev.call_id.clone(), changes.clone());
            if state.file_change_started.insert(ev.call_id.clone()) {
                let item = v2::ThreadItem::FileChange {
                    id: ev.call_id.clone(),
                    changes: changes.clone(),
                    status: v2::PatchApplyStatus::InProgress,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }
        }
        EventMsg::PatchApplyEnd(ev) => {
            let status = if ev.success {
                v2::PatchApplyStatus::Completed
            } else {
                v2::PatchApplyStatus::Failed
            };
            let changes = state
                .file_change_payloads
                .remove(&ev.call_id)
                .unwrap_or_default();
            state.file_change_started.remove(&ev.call_id);
            let item = v2::ThreadItem::FileChange {
                id: ev.call_id.clone(),
                changes,
                status,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::McpToolCallBegin(ev) => {
            let item = v2::ThreadItem::McpToolCall {
                id: ev.call_id.clone(),
                server: ev.invocation.server,
                tool: ev.invocation.tool,
                status: v2::McpToolCallStatus::InProgress,
                arguments: ev.invocation.arguments.unwrap_or(serde_json::Value::Null),
                result: None,
                error: None,
                duration_ms: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemStarted(v2::ItemStartedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::McpToolCallEnd(ev) => {
            let status = if ev.is_success() {
                v2::McpToolCallStatus::Completed
            } else {
                v2::McpToolCallStatus::Failed
            };
            let duration_ms = i64::try_from(ev.duration.as_millis()).ok();
            let (result, error) = match ev.result {
                Ok(value) => {
                    let content = value
                        .content
                        .into_iter()
                        .map(|item| {
                            serde_json::to_value(item).unwrap_or_else(|err| {
                                error!("failed to serialize MCP content block: {err}");
                                serde_json::Value::Null
                            })
                        })
                        .collect();
                    (
                        Some(v2::McpToolCallResult {
                            content,
                            structured_content: value.structured_content,
                        }),
                        None,
                    )
                }
                Err(message) => (
                    None,
                    Some(v2::McpToolCallError { message }),
                ),
            };
            let item = v2::ThreadItem::McpToolCall {
                id: ev.call_id,
                server: ev.invocation.server,
                tool: ev.invocation.tool,
                status,
                arguments: ev.invocation.arguments.unwrap_or(serde_json::Value::Null),
                result,
                error,
                duration_ms,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::WebSearchBegin(ev) => {
            let item = v2::ThreadItem::WebSearch {
                id: ev.call_id,
                query: ev.query.unwrap_or_default(),
                action: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemStarted(v2::ItemStartedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::WebSearchComplete(ev) => {
            let item = v2::ThreadItem::WebSearch {
                id: ev.call_id,
                query: ev.query.unwrap_or_default(),
                action: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ViewImageToolCall(ev) => {
            let item = v2::ThreadItem::ImageView {
                id: ev.call_id,
                path: ev.path.to_string_lossy().into_owned(),
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::TurnDiff(ev) => {
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::TurnDiffUpdated(v2::TurnDiffUpdatedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    diff: ev.unified_diff,
                }),
            )
            .await;
        }
        EventMsg::PlanUpdate(ev) => {
            let plan = ev.plan.into_iter().map(Into::into).collect();
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::TurnPlanUpdated(v2::TurnPlanUpdatedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    explanation: None,
                    plan,
                }),
            )
            .await;
        }
        EventMsg::TokenCount(ev) => {
            if let Some(info) = ev.info {
                let token_usage = thread_token_usage_from_core(&info);
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ThreadTokenUsageUpdated(
                        v2::ThreadTokenUsageUpdatedNotification {
                            thread_id: state.thread_id.clone(),
                            turn_id: turn_id.clone(),
                            token_usage,
                        },
                    ),
                )
                .await;
            }
            if let Some(rate_limits) = ev.rate_limits {
                let notification = v2::AccountRateLimitsUpdatedNotification {
                    rate_limits: rate_limit_snapshot_from_event(&rate_limits),
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::AccountRateLimitsUpdated(notification),
                )
                .await;
            }
        }
        EventMsg::RequestUserInput(ev) => {
            let questions = ev
                .questions
                .into_iter()
                .map(|question| v2::ToolRequestUserInputQuestion {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| v2::ToolRequestUserInputOption {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                })
                .collect();
            let params = v2::ToolRequestUserInputParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: ev.call_id.clone(),
                questions,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::ToolRequestUserInput(params)).await
            {
                let conversation = conversation.clone();
                let response_turn_id = if ev.turn_id.is_empty() {
                    turn_id.clone()
                } else {
                    ev.turn_id.clone()
                };
                tokio::spawn(async move {
                    on_request_user_input_response(response_turn_id, rx, conversation).await;
                });
            }
        }
        EventMsg::DynamicToolCallRequest(ev) => {
            let params = DynamicToolCallParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                call_id: ev.call_id.clone(),
                tool: ev.tool,
                arguments: ev.arguments,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::DynamicToolCall(params)).await
            {
                let conversation = conversation.clone();
                tokio::spawn(async move {
                    on_dynamic_tool_call_response_v2(ev.call_id, rx, conversation).await;
                });
            }
        }
        EventMsg::EnteredReviewMode(review_request) => {
            let review_text = if review_request.user_facing_hint.is_empty() {
                review_request.prompt
            } else {
                review_request.user_facing_hint
            };
            let item = v2::ThreadItem::EnteredReviewMode {
                id: turn_id.clone(),
                review: review_text,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemStarted(v2::ItemStartedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExitedReviewMode(review_event) => {
            let review_text = review_event
                .review_output
                .map(|output| {
                    let mut sections = Vec::new();
                    let explanation = output.overall_explanation.trim();
                    if !explanation.is_empty() {
                        sections.push(explanation.to_string());
                    }
                    if !output.findings.is_empty() {
                        sections.push(format_review_findings_block(&output.findings, None));
                    }
                    let correctness = output.overall_correctness.trim();
                    if !correctness.is_empty() {
                        sections.push(format!("Overall correctness: {correctness}"));
                    }
                    sections.join("\n\n")
                })
                .unwrap_or_default();
            let item = v2::ThreadItem::ExitedReviewMode {
                id: turn_id.clone(),
                review: review_text,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        _ => {}
    }
}

async fn handle_reasoning_summary_delta(
    state: &mut V2StreamState,
    turn_id: String,
    delta: String,
    outgoing: &OutgoingMessageSender,
) {
    let is_new = state.reasoning.is_none();
    let reasoning = state.reasoning.get_or_insert_with(|| V2ReasoningAccumulator {
        id: Uuid::new_v4().to_string(),
        summary: vec![String::new()],
        summary_index: 0,
        content: Vec::new(),
        content_index: 0,
    });
    if is_new {
        send_v2_notification(
            outgoing,
            ServerNotification::ReasoningSummaryPartAdded(v2::ReasoningSummaryPartAddedNotification {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: reasoning.id.clone(),
                summary_index: reasoning.summary_index,
            }),
        )
        .await;
    }
    if reasoning.summary.is_empty() {
        reasoning.summary.push(String::new());
    }
    if let Some(last) = reasoning.summary.last_mut() {
        last.push_str(&delta);
    }
    send_v2_notification(
        outgoing,
        ServerNotification::ReasoningSummaryTextDelta(v2::ReasoningSummaryTextDeltaNotification {
            thread_id: state.thread_id.clone(),
            turn_id,
            item_id: reasoning.id.clone(),
            delta,
            summary_index: reasoning.summary_index,
        }),
    )
    .await;
}

async fn handle_reasoning_content_delta(
    state: &mut V2StreamState,
    turn_id: String,
    delta: String,
    outgoing: &OutgoingMessageSender,
) {
    let reasoning = state.reasoning.get_or_insert_with(|| V2ReasoningAccumulator {
        id: Uuid::new_v4().to_string(),
        summary: Vec::new(),
        summary_index: 0,
        content: vec![String::new()],
        content_index: 0,
    });
    if reasoning.content.is_empty() {
        reasoning.content.push(String::new());
    }
    if let Some(last) = reasoning.content.last_mut() {
        last.push_str(&delta);
    }
    send_v2_notification(
        outgoing,
        ServerNotification::ReasoningTextDelta(v2::ReasoningTextDeltaNotification {
            thread_id: state.thread_id.clone(),
            turn_id,
            item_id: reasoning.id.clone(),
            delta,
            content_index: reasoning.content_index,
        }),
    )
    .await;
}

fn convert_patch_changes(
    changes: &HashMap<PathBuf, code_core::protocol::FileChange>,
) -> Vec<v2::FileUpdateChange> {
    let mut converted = changes
        .iter()
        .map(|(path, change)| v2::FileUpdateChange {
            path: path.to_string_lossy().into_owned(),
            kind: map_patch_change_kind(change),
            diff: format_file_change_diff(change),
        })
        .collect::<Vec<_>>();
    converted.sort_by(|a, b| a.path.cmp(&b.path));
    converted
}

fn map_patch_change_kind(change: &code_core::protocol::FileChange) -> v2::PatchChangeKind {
    match change {
        code_core::protocol::FileChange::Add { .. } => v2::PatchChangeKind::Add,
        code_core::protocol::FileChange::Delete => v2::PatchChangeKind::Delete,
        code_core::protocol::FileChange::Update { move_path, .. } => {
            v2::PatchChangeKind::Update {
                move_path: move_path.clone(),
            }
        }
    }
}

fn format_file_change_diff(change: &code_core::protocol::FileChange) -> String {
    match change {
        code_core::protocol::FileChange::Add { content } => content.clone(),
        code_core::protocol::FileChange::Delete => String::new(),
        code_core::protocol::FileChange::Update { unified_diff, move_path, .. } => {
            if let Some(path) = move_path {
                format!("{unified_diff}\n\nMoved to: {}", path.display())
            } else {
                unified_diff.clone()
            }
        }
    }
}

fn thread_token_usage_from_core(info: &code_core::protocol::TokenUsageInfo) -> v2::ThreadTokenUsage {
    let to_i64 = |value| i64::try_from(value).unwrap_or(i64::MAX);
    let total = v2::TokenUsageBreakdown {
        total_tokens: to_i64(info.total_token_usage.total_tokens),
        input_tokens: to_i64(info.total_token_usage.input_tokens),
        cached_input_tokens: to_i64(info.total_token_usage.cached_input_tokens),
        output_tokens: to_i64(info.total_token_usage.output_tokens),
        reasoning_output_tokens: to_i64(info.total_token_usage.reasoning_output_tokens),
    };
    let last = v2::TokenUsageBreakdown {
        total_tokens: to_i64(info.last_token_usage.total_tokens),
        input_tokens: to_i64(info.last_token_usage.input_tokens),
        cached_input_tokens: to_i64(info.last_token_usage.cached_input_tokens),
        output_tokens: to_i64(info.last_token_usage.output_tokens),
        reasoning_output_tokens: to_i64(info.last_token_usage.reasoning_output_tokens),
    };
    v2::ThreadTokenUsage {
        total,
        last,
        model_context_window: info
            .model_context_window
            .and_then(|window| i64::try_from(window).ok()),
    }
}

fn rate_limit_snapshot_from_event(
    snapshot: &code_core::protocol::RateLimitSnapshotEvent,
) -> v2::RateLimitSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok());
    let primary = v2::RateLimitWindow {
        used_percent: snapshot.primary_used_percent.round() as i32,
        window_duration_mins: i64::try_from(snapshot.primary_window_minutes).ok(),
        resets_at: snapshot
            .primary_reset_after_seconds
            .and_then(|seconds| now.map(|now| now + seconds as i64)),
    };
    let secondary = v2::RateLimitWindow {
        used_percent: snapshot.secondary_used_percent.round() as i32,
        window_duration_mins: i64::try_from(snapshot.secondary_window_minutes).ok(),
        resets_at: snapshot
            .secondary_reset_after_seconds
            .and_then(|seconds| now.map(|now| now + seconds as i64)),
    };
    v2::RateLimitSnapshot {
        primary: Some(primary),
        secondary: Some(secondary),
        credits: None,
        plan_type: None,
    }
}

async fn read_turns_from_rollout(_config: &Config, rollout_path: &std::path::Path) -> Vec<v2::Turn> {
    let file = match tokio::fs::File::open(rollout_path).await {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!("failed to open rollout file {}: {err}", rollout_path.display());
            return Vec::new();
        }
    };
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();
    let mut events = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(event_msg) = parse_event_msg_from_rollout_line(&line) {
            events.push(event_msg);
        }
    }

    build_turns_from_event_msgs(&events)
}

async fn read_event_msgs_from_rollout(
    rollout_path: &std::path::Path,
) -> Option<Vec<code_protocol::protocol::EventMsg>> {
    let file = tokio::fs::File::open(rollout_path).await.ok()?;
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();
    let mut events = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(event_msg) = parse_event_msg_from_rollout_line(&line) {
            events.push(event_msg);
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

fn parse_event_msg_from_rollout_line(
    line: &str,
) -> Option<code_protocol::protocol::EventMsg> {
    if line.trim().is_empty() {
        return None;
    }

    if let Ok(rollout_line) = serde_json::from_str::<RolloutLine>(line)
        && let RolloutItem::Event(event) = rollout_line.item
    {
        return Some(event.msg);
    }

    // Compatibility with legacy app-server fixtures that persisted event lines
    // with `type = "event_msg"` and the event payload directly under `payload`.
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let kind = value.get("type")?.as_str()?;
    if kind != "event_msg" {
        return None;
    }

    serde_json::from_value(value.get("payload")?.clone()).ok()
}

async fn resolve_rollout_path(
    code_home: &std::path::Path,
    thread_id: &str,
    path_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = path_override {
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("rollout path does not exist: {}", path.display()));
    }

    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let entry = catalog
        .find_by_id(thread_id)
        .await
        .map_err(|err| format!("failed to load catalog: {err}"))?
        .ok_or_else(|| format!("no rollout found for thread id {thread_id}"))?;
    Ok(catalog.entry_rollout_path(&entry))
}

async fn find_rollout_path_for_thread(
    code_home: &std::path::Path,
    thread_id: &ThreadId,
) -> Option<PathBuf> {
    let id = thread_id.to_string();
    for _ in 0..20 {
        if let Ok(Some(path)) = code_core::find_conversation_path_by_id_str(code_home, &id).await {
            return Some(path);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    None
}

async fn resolve_rollout_path_for_v1_resume(
    code_home: &std::path::Path,
    path: Option<PathBuf>,
    conversation_id: Option<ThreadId>,
) -> Result<PathBuf, JSONRPCErrorError> {
    if let Some(path) = path {
        let resolved = if path.is_absolute() {
            path
        } else {
            code_home.join(path)
        };
        if resolved.exists() {
            return Ok(resolved);
        }
        return Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: format!("rollout path does not exist: {}", resolved.display()),
            data: None,
        });
    }

    let Some(conversation_id) = conversation_id else {
        return Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "either path or conversationId is required".to_string(),
            data: None,
        });
    };

    match code_core::find_conversation_path_by_id_str(code_home, &conversation_id.to_string()).await {
        Ok(Some(path)) => Ok(path),
        Ok(None) => Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "conversation not found".to_string(),
            data: None,
        }),
        Err(err) => Err(JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            message: format!("failed to resolve conversation path: {err}"),
            data: None,
        }),
    }
}

async fn conversation_summary_from_rollout_path(
    code_home: &std::path::Path,
    path: &std::path::Path,
    fallback_provider: &str,
) -> ConversationSummary {
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    if let Ok(entries) = catalog.query(&SessionQuery::default()).await {
        for entry in entries {
            let entry_path = catalog.entry_rollout_path(&entry);
            if paths_match(&entry_path, path) {
                return conversation_summary_from_entry(code_home, &entry, fallback_provider);
            }
        }
    }

    let preview = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    ConversationSummary {
        conversation_id: ThreadId::new(),
        path: path.to_path_buf(),
        preview,
        timestamp: None,
        updated_at: None,
        model_provider: fallback_provider.to_string(),
        cwd: PathBuf::new(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SessionSource::Unknown,
        git_info: None,
    }
}

fn conversation_summary_from_entry(
    code_home: &std::path::Path,
    entry: &SessionIndexEntry,
    fallback_provider: &str,
) -> ConversationSummary {
    let conversation_id = ThreadId::from_string(&entry.session_id.to_string())
        .unwrap_or_else(|_| ThreadId::new());
    let path = code_home.join(&entry.rollout_path);
    let preview = entry
        .nickname
        .clone()
        .or_else(|| entry.last_user_snippet.clone())
        .unwrap_or_default();
    let updated_at = format_unix_timestamp(entry_updated_at(entry, code_home))
        .or_else(|| non_empty_string(entry.last_event_at.clone()));
    let git_info = read_git_info_from_rollout(&path)
        .map(|git| ConversationGitInfo {
            sha: git.sha,
            branch: git.branch,
            origin_url: git.origin_url,
        })
        .or_else(|| {
            entry.git_branch.clone().map(|branch| ConversationGitInfo {
                sha: None,
                branch: Some(branch),
                origin_url: None,
            })
        });

    ConversationSummary {
        conversation_id,
        path,
        preview,
        timestamp: non_empty_string(entry.created_at.clone()),
        updated_at,
        model_provider: entry
            .model_provider
            .clone()
            .unwrap_or_else(|| fallback_provider.to_string()),
        cwd: entry.cwd_real.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: entry.session_source.clone(),
        git_info,
    }
}

fn paths_match(lhs: &std::path::Path, rhs: &std::path::Path) -> bool {
    lhs == rhs
        || std::fs::canonicalize(lhs)
            .ok()
            .zip(std::fs::canonicalize(rhs).ok())
            .is_some_and(|(left, right)| left == right)
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn format_unix_timestamp(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}

async fn load_thread_from_catalog(
    code_home: &std::path::Path,
    thread_id: &str,
    fallback_provider: &str,
) -> Option<v2::Thread> {
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let entry = catalog.find_by_id(thread_id).await.ok()??;
    let code_home = code_home.to_path_buf();
    let fallback_provider = fallback_provider.to_string();
    tokio::task::spawn_blocking(move || {
        build_thread_from_entry(&entry, &code_home, &fallback_provider, Vec::new())
    })
    .await
    .ok()
}

fn build_thread_for_new(thread_id: &str, config: &Config, turns: Vec<v2::Turn>) -> v2::Thread {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0);
    v2::Thread {
        id: thread_id.to_string(),
        preview: String::new(),
        model_provider: config.model_provider_id.clone(),
        created_at: now,
        updated_at: now,
        path: None,
        cwd: config.cwd.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::from(SessionSource::Mcp),
        git_info: None,
        turns,
    }
}

fn build_thread_placeholder(thread_id: &str) -> v2::Thread {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0);
    v2::Thread {
        id: thread_id.to_string(),
        preview: String::new(),
        model_provider: "unknown".to_string(),
        created_at: now,
        updated_at: now,
        path: None,
        cwd: PathBuf::new(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::Unknown,
        git_info: None,
        turns: Vec::new(),
    }
}

fn build_thread_from_entry(
    entry: &SessionIndexEntry,
    code_home: &std::path::Path,
    fallback_provider: &str,
    turns: Vec<v2::Turn>,
) -> v2::Thread {
    let created_at = parse_timestamp(&entry.created_at).unwrap_or(0);
    let updated_at = entry_updated_at(entry, code_home);
    let preview = entry
        .nickname
        .clone()
        .or_else(|| entry.last_user_snippet.clone())
        .unwrap_or_default();
    let git_info = read_git_info_from_rollout(code_home.join(&entry.rollout_path).as_path())
        .or_else(|| {
            entry.git_branch.clone().map(|branch| v2::GitInfo {
                sha: None,
                branch: Some(branch),
                origin_url: None,
            })
        });
    v2::Thread {
        id: entry.session_id.to_string(),
        preview,
        model_provider: entry
            .model_provider
            .clone()
            .unwrap_or_else(|| fallback_provider.to_string()),
        created_at,
        updated_at,
        path: Some(code_home.join(&entry.rollout_path)),
        cwd: entry.cwd_real.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::from(entry.session_source.clone()),
        git_info,
        turns,
    }
}

fn entry_updated_at(entry: &SessionIndexEntry, code_home: &std::path::Path) -> i64 {
    let from_mtime = std::fs::metadata(code_home.join(&entry.rollout_path))
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok());
    from_mtime.unwrap_or_else(|| {
        parse_timestamp(&entry.last_event_at).unwrap_or_else(|| parse_timestamp(&entry.created_at).unwrap_or(0))
    })
}

fn read_git_info_from_rollout(path: &std::path::Path) -> Option<v2::GitInfo> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let rollout_line: RolloutLine = match serde_json::from_str(&line) {
            Ok(rollout_line) => rollout_line,
            Err(_) => continue,
        };
        if let RolloutItem::SessionMeta(meta_line) = rollout_line.item {
            return meta_line.git.map(|git| v2::GitInfo {
                sha: git.commit_hash,
                branch: git.branch,
                origin_url: git.repository_url,
            });
        }
    }
    None
}

fn parse_timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|dt| dt.unix_timestamp())
}

fn matches_model_provider(
    entry: &SessionIndexEntry,
    providers: Option<&Vec<String>>,
    fallback_provider: &str,
) -> bool {
    let Some(providers) = providers else {
        return true;
    };
    if providers.is_empty() {
        return true;
    }
    let provider = entry
        .model_provider
        .as_deref()
        .unwrap_or(fallback_provider);
    providers.iter().any(|candidate| candidate == provider)
}

fn matches_source_kind(
    entry: &SessionIndexEntry,
    kinds: Option<&Vec<v2::ThreadSourceKind>>,
) -> bool {
    let Some(kinds) = kinds else {
        return matches!(entry.session_source, SessionSource::Cli | SessionSource::VSCode);
    };
    if kinds.is_empty() {
        return matches!(entry.session_source, SessionSource::Cli | SessionSource::VSCode);
    }

    let entry_kind = thread_source_kind(entry);
    if kinds.iter().any(|kind| kind == &entry_kind) {
        return true;
    }
    if matches!(entry_kind, v2::ThreadSourceKind::SubAgentReview | v2::ThreadSourceKind::SubAgentCompact | v2::ThreadSourceKind::SubAgentThreadSpawn | v2::ThreadSourceKind::SubAgentOther)
        && kinds.iter().any(|kind| kind == &v2::ThreadSourceKind::SubAgent)
    {
        return true;
    }
    false
}

fn thread_source_kind(entry: &SessionIndexEntry) -> v2::ThreadSourceKind {
    match &entry.session_source {
        SessionSource::Cli => v2::ThreadSourceKind::Cli,
        SessionSource::VSCode => v2::ThreadSourceKind::VsCode,
        SessionSource::Exec => v2::ThreadSourceKind::Exec,
        SessionSource::Mcp => v2::ThreadSourceKind::AppServer,
        SessionSource::SubAgent(sub) => match sub {
            SubAgentSource::Review => v2::ThreadSourceKind::SubAgentReview,
            SubAgentSource::Compact => v2::ThreadSourceKind::SubAgentCompact,
            SubAgentSource::ThreadSpawn { .. } => v2::ThreadSourceKind::SubAgentThreadSpawn,
            SubAgentSource::Other(_) => v2::ThreadSourceKind::SubAgentOther,
        },
        SessionSource::Unknown => v2::ThreadSourceKind::Unknown,
    }
}

async fn set_catalog_archived(
    code_home: &std::path::Path,
    conversation_id: ConversationId,
    archived: bool,
) -> Result<(), String> {
    let session_id = Uuid::from(conversation_id).to_string();
    if archived {
        let source = code_core::find_conversation_path_by_id_str(code_home, &session_id)
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "thread not found".to_string())?;
        let Some(file_name) = source.file_name() else {
            return Err(format!("invalid rollout path: {}", source.display()));
        };
        let destination_dir = code_home.join(code_core::ARCHIVED_SESSIONS_SUBDIR);
        tokio::fs::create_dir_all(&destination_dir)
            .await
            .map_err(|err| err.to_string())?;
        let destination = destination_dir.join(file_name);
        tokio::fs::rename(&source, &destination)
            .await
            .map_err(|err| err.to_string())
    } else {
        let scan_code_home = code_home.to_path_buf();
        let session_id_for_scan = session_id.clone();
        let source = tokio::task::spawn_blocking(move || {
            find_archived_thread_path_by_id_str(&scan_code_home, &session_id_for_scan)
        })
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?
            .ok_or_else(|| "thread not found".to_string())?;
        let Some(file_name) = source.file_name() else {
            return Err(format!("invalid rollout path: {}", source.display()));
        };
        let destination_dir = code_home.join(code_core::SESSIONS_SUBDIR);
        tokio::fs::create_dir_all(&destination_dir)
            .await
            .map_err(|err| err.to_string())?;
        let destination = destination_dir.join(file_name);
        tokio::fs::rename(&source, &destination)
            .await
            .map_err(|err| err.to_string())
    }
}

fn find_archived_thread_path_by_id_str(
    code_home: &std::path::Path,
    id_str: &str,
) -> std::io::Result<Option<PathBuf>> {
    if Uuid::parse_str(id_str).is_err() {
        return Ok(None);
    }

    let root = code_home.join(code_core::ARCHIVED_SESSIONS_SUBDIR);
    if !root.exists() {
        return Ok(None);
    }

    let mut stack = vec![root];
    let mut fallback = None;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.contains(id_str) {
                continue;
            }
            if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
            {
                return Ok(Some(path));
            }
            fallback = Some(path);
        }
    }

    Ok(fallback)
}

async fn set_catalog_nickname(
    code_home: &std::path::Path,
    conversation_id: ConversationId,
    nickname: String,
) -> Result<(), String> {
    let session_id = Uuid::from(conversation_id);
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let updated = catalog
        .set_nickname(session_id, Some(nickname))
        .await
        .map_err(|err| err.to_string())?;
    if updated {
        Ok(())
    } else {
        Err("thread not found".to_string())
    }
}

async fn on_patch_approval_response(
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    codex: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            if let Err(submit_err) = codex
                .submit(Op::PatchApproval {
                    id: approval_id.clone(),
                    decision: core_protocol::ReviewDecision::Denied,
                })
                .await
            {
                error!("failed to submit denied PatchApproval after request failure: {submit_err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<ApplyPatchApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ApplyPatchApprovalResponse: {err}");
            ApplyPatchApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = codex
        .submit(Op::PatchApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

async fn on_dynamic_tool_call_response_legacy(
    call_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            let fallback = CoreDynamicToolResponse {
                content_items: vec![code_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText {
                    text: "dynamic tool request failed".to_string(),
                }],
                success: false,
            };
            if let Err(err) = conversation
                .submit(Op::DynamicToolResponse {
                    id: call_id.clone(),
                    response: fallback,
                })
                .await
            {
                error!("failed to submit DynamicToolResponse: {err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<LegacyDynamicToolCallResponse>(value).unwrap_or_else(|err| {
        error!("failed to deserialize DynamicToolCallResponse: {err}");
        LegacyDynamicToolCallResponse {
            output: "dynamic tool response was invalid".to_string(),
            success: false,
        }
    });

    let response = CoreDynamicToolResponse {
        content_items: vec![
            code_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText {
                text: response.output,
            },
        ],
        success: response.success,
    };
    if let Err(err) = conversation
        .submit(Op::DynamicToolResponse {
            id: call_id,
            response,
        })
        .await
    {
        error!("failed to submit DynamicToolResponse: {err}");
    }
}

async fn on_exec_approval_response(
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            return;
        }
    };

    // Try to deserialize `value` and then make the appropriate call to `codex`.
    let response =
        serde_json::from_value::<ExecCommandApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ExecCommandApprovalResponse: {err}");
            // If we cannot deserialize the response, we deny the request to be
            // conservative.
            ExecCommandApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = conversation
        .submit(Op::ExecApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}

async fn on_dynamic_tool_call_response_v2(
    call_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            return;
        }
    };

    let response =
        serde_json::from_value::<DynamicToolCallResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize DynamicToolCallResponse: {err}");
            DynamicToolCallResponse {
                content_items: Vec::new(),
                success: false,
            }
        });

    let response = CoreDynamicToolResponse {
        content_items: response
            .content_items
            .into_iter()
            .map(Into::into)
            .collect(),
        success: response.success,
    };
    if let Err(err) = conversation
        .submit(Op::DynamicToolResponse {
            id: call_id,
            response,
        })
        .await
    {
        error!("failed to submit DynamicToolResponse: {err}");
    }
}

async fn on_request_user_input_response(
    turn_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            let empty = CoreRequestUserInputResponse {
                answers: HashMap::new(),
            };
            if let Err(err) = conversation
                .submit(Op::UserInputAnswer {
                    id: turn_id,
                    response: empty,
                })
                .await
            {
                error!("failed to submit UserInputAnswer: {err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<v2::ToolRequestUserInputResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ToolRequestUserInputResponse: {err}");
            v2::ToolRequestUserInputResponse {
                answers: HashMap::new(),
            }
        });
    let response = CoreRequestUserInputResponse {
        answers: response
            .answers
            .into_iter()
            .map(|(id, answer)| {
                (
                    id,
                    CoreRequestUserInputAnswer {
                        answers: answer.answers,
                    },
                )
            })
            .collect(),
    };

    if let Err(err) = conversation
        .submit(Op::UserInputAnswer {
            id: turn_id,
            response,
        })
        .await
    {
        error!("failed to submit UserInputAnswer: {err}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_command_execution_request_approval_response(
    thread_id: String,
    turn_id: String,
    item_id: String,
    command: String,
    cwd: PathBuf,
    command_actions: Vec<v2::CommandAction>,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
) {
    #[derive(Deserialize)]
    struct LegacyCommandExecutionApprovalResponse {
        decision: code_protocol::protocol::ReviewDecision,
    }

    let response = receiver.await;
    let (decision, completion_status) = match response {
        Ok(value) => {
            if let Ok(response) =
                serde_json::from_value::<v2::CommandExecutionRequestApprovalResponse>(value.clone())
            {
                map_command_execution_approval_decision(response.decision)
            } else if let Ok(response) =
                serde_json::from_value::<LegacyCommandExecutionApprovalResponse>(value)
            {
                map_legacy_command_execution_approval_decision(response.decision)
            } else {
                error!("failed to deserialize CommandExecutionRequestApprovalResponse");
                (
                    core_protocol::ReviewDecision::Denied,
                    Some(v2::CommandExecutionStatus::Declined),
                )
            }
        }
        Err(err) => {
            error!("request failed: {err:?}");
            (
                core_protocol::ReviewDecision::Denied,
                Some(v2::CommandExecutionStatus::Failed),
            )
        }
    };

    if let Some(status) = completion_status {
        let item = v2::ThreadItem::CommandExecution {
            id: item_id.clone(),
            command,
            cwd,
            process_id: None,
            status,
            command_actions,
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        };
        send_v2_notification(
            outgoing.as_ref(),
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id,
                turn_id: turn_id.clone(),
                item,
            }),
        )
        .await;
    }

    if let Err(err) = conversation
        .submit(Op::ExecApproval {
            id: item_id,
            decision,
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_file_change_request_approval_response(
    thread_id: String,
    turn_id: String,
    item_id: String,
    changes: Vec<v2::FileUpdateChange>,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
) {
    let response = receiver.await;
    let (decision, completion_status) = match response {
        Ok(value) => {
            let response = serde_json::from_value::<v2::FileChangeRequestApprovalResponse>(value)
                .unwrap_or_else(|err| {
                    error!("failed to deserialize FileChangeRequestApprovalResponse: {err}");
                    v2::FileChangeRequestApprovalResponse {
                        decision: v2::FileChangeApprovalDecision::Decline,
                    }
                });
            map_file_change_approval_decision(response.decision)
        }
        Err(err) => {
            error!("request failed: {err:?}");
            (
                core_protocol::ReviewDecision::Denied,
                Some(v2::PatchApplyStatus::Failed),
            )
        }
    };

    if let Some(status) = completion_status {
        let item = v2::ThreadItem::FileChange {
            id: item_id.clone(),
            changes,
            status,
        };
        send_v2_notification(
            outgoing.as_ref(),
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id,
                turn_id: turn_id.clone(),
                item,
            }),
        )
        .await;
    }

    if let Err(err) = conversation
        .submit(Op::PatchApproval {
            id: item_id,
            decision,
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

fn model_info_to_app_server_model(info: ModelInfo) -> Model {
    let supports_personality = info.supports_personality();
    let model_id = info.slug.clone();
    Model {
        id: model_id.clone(),
        model: model_id,
        upgrade: info.upgrade.map(|upgrade| upgrade.model),
        display_name: info.display_name,
        description: info.description.unwrap_or_default(),
        supported_reasoning_efforts: info
            .supported_reasoning_levels
            .iter()
            .map(|preset| ReasoningEffortOption {
                reasoning_effort: preset.effort,
                description: preset.description.clone(),
            })
            .collect(),
        default_reasoning_effort: info
            .default_reasoning_level
            .unwrap_or(code_protocol::openai_models::ReasoningEffort::Medium),
        input_modalities: info.input_modalities,
        supports_personality,
        is_default: false,
    }
}

fn map_command_execution_approval_decision(
    decision: v2::CommandExecutionApprovalDecision,
) -> (core_protocol::ReviewDecision, Option<v2::CommandExecutionStatus>) {
    match decision {
        v2::CommandExecutionApprovalDecision::Accept => {
            (core_protocol::ReviewDecision::Approved, None)
        }
        v2::CommandExecutionApprovalDecision::AcceptForSession => {
            (core_protocol::ReviewDecision::ApprovedForSession, None)
        }
        v2::CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. } => {
            (core_protocol::ReviewDecision::Approved, None)
        }
        v2::CommandExecutionApprovalDecision::Decline => (
            core_protocol::ReviewDecision::Denied,
            Some(v2::CommandExecutionStatus::Declined),
        ),
        v2::CommandExecutionApprovalDecision::Cancel => (
            core_protocol::ReviewDecision::Abort,
            Some(v2::CommandExecutionStatus::Declined),
        ),
    }
}

fn map_legacy_command_execution_approval_decision(
    decision: code_protocol::protocol::ReviewDecision,
) -> (core_protocol::ReviewDecision, Option<v2::CommandExecutionStatus>) {
    match decision {
        code_protocol::protocol::ReviewDecision::Approved => {
            (core_protocol::ReviewDecision::Approved, None)
        }
        code_protocol::protocol::ReviewDecision::ApprovedForSession => {
            (core_protocol::ReviewDecision::ApprovedForSession, None)
        }
        code_protocol::protocol::ReviewDecision::Denied => (
            core_protocol::ReviewDecision::Denied,
            Some(v2::CommandExecutionStatus::Declined),
        ),
        code_protocol::protocol::ReviewDecision::Abort => (
            core_protocol::ReviewDecision::Abort,
            Some(v2::CommandExecutionStatus::Declined),
        ),
    }
}

fn map_file_change_approval_decision(
    decision: v2::FileChangeApprovalDecision,
) -> (core_protocol::ReviewDecision, Option<v2::PatchApplyStatus>) {
    match decision {
        v2::FileChangeApprovalDecision::Accept => (core_protocol::ReviewDecision::Approved, None),
        v2::FileChangeApprovalDecision::AcceptForSession => {
            (core_protocol::ReviewDecision::ApprovedForSession, None)
        }
        v2::FileChangeApprovalDecision::Decline => (
            core_protocol::ReviewDecision::Denied,
            Some(v2::PatchApplyStatus::Declined),
        ),
        v2::FileChangeApprovalDecision::Cancel => (
            core_protocol::ReviewDecision::Abort,
            Some(v2::PatchApplyStatus::Declined),
        ),
    }
}

fn map_review_decision_from_wire(d: code_protocol::protocol::ReviewDecision) -> core_protocol::ReviewDecision {
    match d {
        code_protocol::protocol::ReviewDecision::Approved => core_protocol::ReviewDecision::Approved,
        code_protocol::protocol::ReviewDecision::ApprovedForSession => core_protocol::ReviewDecision::ApprovedForSession,
        code_protocol::protocol::ReviewDecision::Denied => core_protocol::ReviewDecision::Denied,
        code_protocol::protocol::ReviewDecision::Abort => core_protocol::ReviewDecision::Abort,
    }
}

fn map_ask_for_approval_from_wire(a: code_protocol::protocol::AskForApproval) -> core_protocol::AskForApproval {
    match a {
        code_protocol::protocol::AskForApproval::UnlessTrusted => core_protocol::AskForApproval::UnlessTrusted,
        code_protocol::protocol::AskForApproval::OnFailure => core_protocol::AskForApproval::OnFailure,
        code_protocol::protocol::AskForApproval::OnRequest => core_protocol::AskForApproval::OnRequest,
        code_protocol::protocol::AskForApproval::Never => core_protocol::AskForApproval::Never,
    }
}

fn map_ask_for_approval_to_wire(a: core_protocol::AskForApproval) -> code_protocol::protocol::AskForApproval {
    match a {
        core_protocol::AskForApproval::UnlessTrusted => code_protocol::protocol::AskForApproval::UnlessTrusted,
        core_protocol::AskForApproval::OnFailure => code_protocol::protocol::AskForApproval::OnFailure,
        core_protocol::AskForApproval::OnRequest => code_protocol::protocol::AskForApproval::OnRequest,
        core_protocol::AskForApproval::Never => code_protocol::protocol::AskForApproval::Never,
    }
}

fn user_saved_config_from_toml(config: ConfigToml) -> UserSavedConfig {
    let model_verbosity = config
        .model_text_verbosity
        .map(map_verbosity_to_wire)
        .or_else(|| config.model.as_ref().map(|_| code_protocol::config_types::Verbosity::Medium));
    let sandbox_settings = config.sandbox_workspace_write.map(|settings| SandboxSettings {
        writable_roots: settings
            .writable_roots
            .into_iter()
            .filter_map(|root| AbsolutePathBuf::from_absolute_path(root).ok())
            .collect(),
        network_access: Some(settings.network_access),
        exclude_tmpdir_env_var: Some(settings.exclude_tmpdir_env_var),
        exclude_slash_tmp: Some(settings.exclude_slash_tmp),
    });
    let tools = config.tools.map(|tools| Tools {
        web_search: tools.web_search,
        view_image: tools.view_image,
    });

    UserSavedConfig {
        approval_policy: config.approval_policy.map(map_ask_for_approval_to_wire),
        sandbox_mode: config.sandbox_mode,
        sandbox_settings,
        forced_chatgpt_workspace_id: config.forced_chatgpt_workspace_id,
        forced_login_method: config.forced_login_method,
        model: config.model,
        model_reasoning_effort: config
            .model_reasoning_effort
            .map(map_reasoning_effort_to_wire),
        model_reasoning_summary: config
            .model_reasoning_summary
            .map(map_reasoning_summary_to_wire),
        model_verbosity,
        tools,
        profile: config.profile,
        profiles: config
            .profiles
            .into_iter()
            .map(|(name, profile)| (name, profile_to_wire(profile)))
            .collect(),
    }
}

fn profile_to_wire(profile: code_core::config_profile::ConfigProfile) -> Profile {
    let model_verbosity = profile
        .model_text_verbosity
        .map(map_verbosity_to_wire)
        .or_else(|| profile.model.as_ref().map(|_| code_protocol::config_types::Verbosity::Medium));
    Profile {
        model: profile.model,
        model_provider: profile.model_provider,
        approval_policy: profile.approval_policy.map(map_ask_for_approval_to_wire),
        model_reasoning_effort: profile
            .model_reasoning_effort
            .map(map_reasoning_effort_to_wire),
        model_reasoning_summary: profile
            .model_reasoning_summary
            .map(map_reasoning_summary_to_wire),
        model_verbosity,
        chatgpt_base_url: profile.chatgpt_base_url,
    }
}

fn map_reasoning_effort_to_wire(
    effort: code_core::config_types::ReasoningEffort,
) -> code_protocol::openai_models::ReasoningEffort {
    match effort {
        code_core::config_types::ReasoningEffort::None => {
            code_protocol::openai_models::ReasoningEffort::None
        }
        code_core::config_types::ReasoningEffort::Minimal => {
            code_protocol::openai_models::ReasoningEffort::Minimal
        }
        code_core::config_types::ReasoningEffort::Low => {
            code_protocol::openai_models::ReasoningEffort::Low
        }
        code_core::config_types::ReasoningEffort::Medium => {
            code_protocol::openai_models::ReasoningEffort::Medium
        }
        code_core::config_types::ReasoningEffort::High => {
            code_protocol::openai_models::ReasoningEffort::High
        }
        code_core::config_types::ReasoningEffort::XHigh => {
            code_protocol::openai_models::ReasoningEffort::XHigh
        }
    }
}

fn map_reasoning_summary_to_wire(
    summary: code_core::config_types::ReasoningSummary,
) -> code_protocol::config_types::ReasoningSummary {
    match summary {
        code_core::config_types::ReasoningSummary::Auto => {
            code_protocol::config_types::ReasoningSummary::Auto
        }
        code_core::config_types::ReasoningSummary::Concise => {
            code_protocol::config_types::ReasoningSummary::Concise
        }
        code_core::config_types::ReasoningSummary::Detailed => {
            code_protocol::config_types::ReasoningSummary::Detailed
        }
        code_core::config_types::ReasoningSummary::None => {
            code_protocol::config_types::ReasoningSummary::None
        }
    }
}

fn map_reasoning_summary_from_wire(
    summary: code_protocol::config_types::ReasoningSummary,
) -> code_core::config_types::ReasoningSummary {
    match summary {
        code_protocol::config_types::ReasoningSummary::Auto => {
            code_core::config_types::ReasoningSummary::Auto
        }
        code_protocol::config_types::ReasoningSummary::Concise => {
            code_core::config_types::ReasoningSummary::Concise
        }
        code_protocol::config_types::ReasoningSummary::Detailed => {
            code_core::config_types::ReasoningSummary::Detailed
        }
        code_protocol::config_types::ReasoningSummary::None => {
            code_core::config_types::ReasoningSummary::None
        }
    }
}

fn map_verbosity_to_wire(
    verbosity: code_core::config_types::TextVerbosity,
) -> code_protocol::config_types::Verbosity {
    match verbosity {
        code_core::config_types::TextVerbosity::Low => code_protocol::config_types::Verbosity::Low,
        code_core::config_types::TextVerbosity::Medium => {
            code_protocol::config_types::Verbosity::Medium
        }
        code_core::config_types::TextVerbosity::High => {
            code_protocol::config_types::Verbosity::High
        }
    }
}

fn reasoning_effort_to_str(effort: code_protocol::openai_models::ReasoningEffort) -> &'static str {
    match effort {
        code_protocol::openai_models::ReasoningEffort::Minimal => "minimal",
        code_protocol::openai_models::ReasoningEffort::Low => "low",
        code_protocol::openai_models::ReasoningEffort::Medium => "medium",
        code_protocol::openai_models::ReasoningEffort::High => "high",
        code_protocol::openai_models::ReasoningEffort::XHigh => "xhigh",
        code_protocol::openai_models::ReasoningEffort::None => "none",
    }
}

fn map_reasoning_effort_from_wire(
    effort: code_protocol::openai_models::ReasoningEffort,
) -> code_core::config_types::ReasoningEffort {
    match effort {
        code_protocol::openai_models::ReasoningEffort::None => {
            code_core::config_types::ReasoningEffort::None
        }
        code_protocol::openai_models::ReasoningEffort::Minimal => {
            code_core::config_types::ReasoningEffort::Minimal
        }
        code_protocol::openai_models::ReasoningEffort::Low => {
            code_core::config_types::ReasoningEffort::Low
        }
        code_protocol::openai_models::ReasoningEffort::Medium => {
            code_core::config_types::ReasoningEffort::Medium
        }
        code_protocol::openai_models::ReasoningEffort::High => {
            code_core::config_types::ReasoningEffort::High
        }
        code_protocol::openai_models::ReasoningEffort::XHigh => {
            code_core::config_types::ReasoningEffort::XHigh
        }
    }
}

fn map_personality_from_wire(
    personality: code_protocol::config_types::Personality,
) -> code_core::config_types::Personality {
    match personality {
        code_protocol::config_types::Personality::None => code_core::config_types::Personality::None,
        code_protocol::config_types::Personality::Friendly => {
            code_core::config_types::Personality::Friendly
        }
        code_protocol::config_types::Personality::Pragmatic => {
            code_core::config_types::Personality::Pragmatic
        }
    }
}

fn map_sandbox_policy_to_wire(
    policy: &core_protocol::SandboxPolicy,
) -> code_protocol::protocol::SandboxPolicy {
    match policy {
        core_protocol::SandboxPolicy::DangerFullAccess => {
            code_protocol::protocol::SandboxPolicy::DangerFullAccess
        }
        core_protocol::SandboxPolicy::ReadOnly => code_protocol::protocol::SandboxPolicy::ReadOnly,
        core_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        } => code_protocol::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_roots
                .iter()
                .filter_map(|root| AbsolutePathBuf::from_absolute_path(root).ok())
                .collect(),
            network_access: *network_access,
            exclude_tmpdir_env_var: *exclude_tmpdir_env_var,
            exclude_slash_tmp: *exclude_slash_tmp,
            allow_git_writes: *allow_git_writes,
        },
    }
}

fn map_sandbox_policy_from_wire(
    policy: code_protocol::protocol::SandboxPolicy,
) -> core_protocol::SandboxPolicy {
    match policy {
        code_protocol::protocol::SandboxPolicy::DangerFullAccess => {
            core_protocol::SandboxPolicy::DangerFullAccess
        }
        code_protocol::protocol::SandboxPolicy::ReadOnly => core_protocol::SandboxPolicy::ReadOnly,
        code_protocol::protocol::SandboxPolicy::ExternalSandbox { .. } => {
            core_protocol::SandboxPolicy::DangerFullAccess
        }
        code_protocol::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        } => core_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_roots
                .into_iter()
                .map(|root| root.into_path_buf())
                .collect(),
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        },
    }
}

fn map_core_skill_scope(scope: code_core::skills::model::SkillScope) -> v2::SkillScope {
    match scope {
        code_core::skills::model::SkillScope::User => v2::SkillScope::User,
        code_core::skills::model::SkillScope::Repo => v2::SkillScope::Repo,
        code_core::skills::model::SkillScope::System => v2::SkillScope::System,
    }
}

fn mcp_auth_status_from_config(
    code_home: &Path,
    name: &str,
    config: &code_core::config_types::McpServerConfig,
) -> v2::McpAuthStatus {
    match &config.transport {
        McpServerTransportConfig::Stdio { .. } => v2::McpAuthStatus::Unsupported,
        McpServerTransportConfig::StreamableHttp {
            bearer_token: Some(_),
            ..
        } => v2::McpAuthStatus::BearerToken,
        McpServerTransportConfig::StreamableHttp {
            bearer_token: None,
            url,
        } => {
            if has_valid_oauth_tokens(code_home, name, url.as_str()) {
                v2::McpAuthStatus::OAuth
            } else {
                v2::McpAuthStatus::NotLoggedIn
            }
        }
    }
}

fn normalize_skill_config_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

async fn read_disabled_skill_paths(code_home: &Path) -> Vec<PathBuf> {
    let read_path = code_core::config::resolve_code_path_for_read(code_home, Path::new("config.toml"));
    let contents = match tokio::fs::read_to_string(read_path).await {
        Ok(contents) => contents,
        Err(_) => return Vec::new(),
    };

    let value = match contents.parse::<TomlValue>() {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let mut disabled_paths = Vec::new();
    let Some(root) = value.as_table() else {
        return disabled_paths;
    };
    let Some(skills) = root.get("skills").and_then(TomlValue::as_table) else {
        return disabled_paths;
    };
    let Some(entries) = skills.get("config").and_then(TomlValue::as_array) else {
        return disabled_paths;
    };

    for entry in entries {
        let Some(entry_table) = entry.as_table() else {
            continue;
        };
        let Some(path) = entry_table.get("path").and_then(TomlValue::as_str) else {
            continue;
        };
        let enabled = entry_table
            .get("enabled")
            .and_then(TomlValue::as_bool)
            .unwrap_or(false);
        if !enabled {
            disabled_paths.push(PathBuf::from(path));
        }
    }

    disabled_paths
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    let left = normalize_path_for_compare(left);
    let right = normalize_path_for_compare(right);
    if left == right {
        return true;
    }

    strip_private_prefix(left.as_path()) == strip_private_prefix(right.as_path())
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn strip_private_prefix(path: &Path) -> PathBuf {
    match path.strip_prefix("/private") {
        Ok(stripped) => Path::new("/").join(stripped),
        Err(_) => path.to_path_buf(),
    }
}

async fn set_skill_config(code_home: &Path, path: &Path, enabled: bool) -> anyhow::Result<()> {
    let config_path = code_home.join("config.toml");
    let read_path = code_core::config::resolve_code_path_for_read(code_home, Path::new("config.toml"));
    let contents = match tokio::fs::read_to_string(read_path).await {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };

    let mut root = if contents.trim().is_empty() {
        TomlValue::Table(toml::Table::new())
    } else {
        contents.parse::<TomlValue>()?
    };

    let normalized_path = normalize_skill_config_path(path);
    let root_table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config.toml root must be a table"))?;

    let skills_value = root_table
        .entry("skills")
        .or_insert_with(|| TomlValue::Table(toml::Table::new()));
    if !skills_value.is_table() {
        *skills_value = TomlValue::Table(toml::Table::new());
    }
    let skills_table = skills_value
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("skills must be a table"))?;

    let config_value = skills_table
        .entry("config")
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    if !config_value.is_array() {
        *config_value = TomlValue::Array(Vec::new());
    }
    let config_entries = config_value
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("skills.config must be an array"))?;

    config_entries.retain(|entry| {
        entry
            .as_table()
            .and_then(|table| table.get("path"))
            .and_then(TomlValue::as_str)
            .is_none_or(|existing| {
                normalize_skill_config_path(Path::new(existing)) != normalized_path
            })
    });

    if !enabled {
        let mut entry = toml::Table::new();
        entry.insert("path".to_string(), TomlValue::String(normalized_path));
        entry.insert("enabled".to_string(), TomlValue::Boolean(false));
        config_entries.push(TomlValue::Table(entry));
    }

    if enabled && config_entries.is_empty() {
        skills_table.remove("config");
    }
    if skills_table.is_empty() {
        root_table.remove("skills");
    }

    let serialized = toml::to_string_pretty(&root)?;
    tokio::fs::create_dir_all(code_home).await?;
    tokio::fs::write(config_path, serialized).await?;
    Ok(())
}

struct RuntimeMcpSnapshot {
    tools_by_server: HashMap<String, HashMap<String, mcp_types::Tool>>,
    resources_by_server: HashMap<String, Vec<mcp_types::Resource>>,
    resource_templates_by_server: HashMap<String, Vec<mcp_types::ResourceTemplate>>,
}

async fn collect_runtime_mcp_snapshot_by_server(
    servers: &std::collections::BTreeMap<String, code_core::config_types::McpServerConfig>,
) -> RuntimeMcpSnapshot {
    if servers.is_empty() {
        return RuntimeMcpSnapshot {
            tools_by_server: HashMap::new(),
            resources_by_server: HashMap::new(),
            resource_templates_by_server: HashMap::new(),
        };
    }

    let server_map: HashMap<String, code_core::config_types::McpServerConfig> =
        servers.iter().map(|(name, config)| (name.clone(), config.clone())).collect();

    match McpConnectionManager::new(server_map, HashSet::new()).await {
        Ok((manager, startup_errors)) => {
            for (name, error) in startup_errors {
                tracing::warn!(
                    "MCP startup issue while collecting status for {name}: {}",
                    error.message
                );
            }
            let tools_by_server = manager.list_tools_with_details_by_server();
            let resources_by_server = manager.list_all_resources().await;
            let resource_templates_by_server = manager.list_all_resource_templates().await;
            RuntimeMcpSnapshot {
                tools_by_server,
                resources_by_server,
                resource_templates_by_server,
            }
        }
        Err(err) => {
            tracing::warn!("failed to collect MCP tools for status list: {err}");
            RuntimeMcpSnapshot {
                tools_by_server: HashMap::new(),
                resources_by_server: HashMap::new(),
                resource_templates_by_server: HashMap::new(),
            }
        }
    }
}

// Unused legacy mappers removed to avoid warnings.
