use spacetimedb::{Identity, ScheduleAt, SpacetimeType, Timestamp};

use crate::api::http_wrappers::{process_armoriq_request, process_gemini_validator_request};
use crate::reducers::terminal::{_armoriq_callback, _gemini_validator_callback};

/// The singleton game row used by the reducers in this scaffold.
pub const DEFAULT_GAME_ID: u64 = 1;

/// The singleton configuration row holding secrets and remote endpoint settings.
pub const ACTIVE_SERVER_CONFIG_KEY: u8 = 1;

/// High-level lifecycle state for the terminal-validation loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum TerminalStatus {
    Idle,
    PendingArmorIq,
    PendingGeminiValidator,
    Rejected,
    Failed,
    Succeeded,
}

/// Fine-grained request state tracked per submitted terminal attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum TerminalRequestPhase {
    PendingArmorIq,
    PendingGeminiValidator,
    Rejected,
    Failed,
    Succeeded,
}

/// Public game-state surface clients can subscribe to without seeing secrets.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = game_state, public)]
pub struct GameState {
    #[primary_key]
    pub game_id: u64,
    pub player_one: Option<Identity>,
    pub player_two: Option<Identity>,
    pub villain_name: String,
    pub is_processing_terminal: bool,
    pub active_terminal_request: Option<u64>,
    pub terminal_status: TerminalStatus,
    pub last_terminal_result: Option<bool>,
    pub last_terminal_message: Option<String>,
    pub last_terminal_actor: Option<Identity>,
    pub updated_at: Timestamp,
}

/// Private secrets for a game session, intentionally isolated from the public state row.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = game_secret, private)]
pub struct GameSecret {
    #[primary_key]
    pub game_id: u64,
    pub hidden_answer: String,
    pub updated_at: Timestamp,
}

/// Server-owned configuration for external integrations.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = server_config, private)]
pub struct ServerConfig {
    #[primary_key]
    pub config_key: u8,
    pub armoriq_verify_url: String,
    pub armoriq_api_key_header: String,
    pub armoriq_api_key: String,
    pub local_llm_relay_base_url: Option<String>,
    pub gemini_api_base_url: String,
    pub gemini_api_key: String,
    pub gemini_validator_model: String,
    pub gemini_clue_generator_model: String,
    pub gemini_villain_model: String,
}

/// Durable audit row for every terminal submission and each asynchronous transition it takes.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = terminal_request, private)]
pub struct TerminalRequest {
    #[primary_key]
    #[auto_inc]
    pub request_id: u64,
    #[index(btree)]
    pub game_id: u64,
    #[index(btree)]
    pub player_identity: Identity,
    pub phase: TerminalRequestPhase,
    pub player_input: String,
    pub hidden_answer_snapshot: String,
    pub armoriq_allowed: Option<bool>,
    pub armoriq_block_reason: Option<String>,
    pub armoriq_raw_response: Option<String>,
    pub gemini_raw_response: Option<String>,
    pub validator_success: Option<bool>,
    pub validator_reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Queue row that schedules the outbound ArmorIQ HTTP procedure.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = armoriq_request_schedule, private, scheduled(process_armoriq_request))]
pub struct ArmoriqRequestSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
}

/// Queue row that schedules the reducer-side handling of an ArmorIQ HTTP result.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = armoriq_callback_schedule, private, scheduled(_armoriq_callback))]
pub struct ArmoriqCallbackSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
    pub status_code: u16,
    pub response_body: String,
    pub transport_error: Option<String>,
}

/// Queue row that schedules the outbound Gemini validator HTTP procedure.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = gemini_validator_request_schedule,
    private,
    scheduled(process_gemini_validator_request)
)]
pub struct GeminiValidatorRequestSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
}

/// Queue row that schedules the reducer-side handling of a Gemini validator HTTP result.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = gemini_validator_callback_schedule,
    private,
    scheduled(_gemini_validator_callback)
)]
pub struct GeminiValidatorCallbackSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
    pub status_code: u16,
    pub response_body: String,
    pub transport_error: Option<String>,
}
