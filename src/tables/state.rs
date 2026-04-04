use spacetimedb::{Identity, ScheduleAt, SpacetimeType, Timestamp};

use crate::api::content_http::{
    process_round_content_request, process_villain_speech_request, process_villain_tts_request,
};
use crate::api::http_wrappers::{process_armoriq_request, process_gemini_validator_request};
use crate::reducers::content::{
    _round_content_callback, _villain_speech_callback, _villain_tts_callback,
};
use crate::reducers::terminal::{_armoriq_callback, _gemini_validator_callback};

/// The singleton game row used by the non-room legacy reducers in this scaffold.
pub const DEFAULT_GAME_ID: u64 = 1;

/// Room-backed games start at a separate range so they never collide with the legacy default row.
pub const ROOM_GAME_ID_OFFSET: u64 = 10_000;

/// The singleton configuration row holding secrets and remote endpoint settings.
pub const ACTIVE_SERVER_CONFIG_KEY: u8 = 1;

/// The singleton configuration row holding optional ElevenLabs voice settings.
pub const ACTIVE_VOICE_CONFIG_KEY: u8 = 1;

/// The singleton row key storing which identity is allowed to run admin reducers.
pub const MODULE_OWNER_KEY: u8 = 1;

/// Public lifecycle state for a multiplayer room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum RoomStatus {
    WaitingForPlayers,
    Ready,
    Terminated,
}

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

/// Public lifecycle state for room-scoped clue/manual generation and villain speech artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum GenerationStatus {
    Idle,
    PendingGemini,
    PendingTts,
    Failed,
    Succeeded,
}

/// Fine-grained request state tracked for clue/manual generation jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum RoundGenerationPhase {
    PendingGeminiSkeleton,
    PendingGeminiExpansion,
    Failed,
    Succeeded,
}

/// Fine-grained request state tracked for villain speech generation and optional TTS jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpacetimeType)]
pub enum VillainSpeechPhase {
    PendingGemini,
    PendingTts,
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

/// Sequence allocator used to mint unique room identifiers and room-scoped game ids.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = room_sequence, private)]
pub struct RoomSequence {
    #[primary_key]
    #[auto_inc]
    pub room_seq: u64,
    pub created_at: Timestamp,
}

/// Public room metadata consumed by clients before they enter the actual game loop.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = game_room, public)]
pub struct GameRoom {
    #[primary_key]
    pub room_id: String,
    #[index(btree)]
    pub game_id: u64,
    pub host_identity: Identity,
    pub player_one: Option<Identity>,
    pub player_two: Option<Identity>,
    pub status: RoomStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub terminated_at: Option<Timestamp>,
}

/// Per-player room lifecycle receipt so clients can observe the latest room action they triggered.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = room_ticket, public)]
pub struct RoomTicket {
    #[primary_key]
    pub owner_identity: Identity,
    pub room_id: Option<String>,
    pub room_status: Option<RoomStatus>,
    pub updated_at: Timestamp,
}

/// Private owner record used to authorize operational/admin reducers.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = module_owner, private)]
pub struct ModuleOwner {
    #[primary_key]
    pub owner_key: u8,
    pub owner_identity: Identity,
    pub created_at: Timestamp,
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

/// Optional ElevenLabs configuration used when villain speech should also be synthesized to audio.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = voice_config, private)]
pub struct VoiceConfig {
    #[primary_key]
    pub config_key: u8,
    pub elevenlabs_api_base_url: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_default_voice_id: String,
    pub elevenlabs_default_model_id: String,
}

/// Public artifact row holding the latest clue/manual generation output for a room and round.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = round_content_artifact, public)]
pub struct RoundContentArtifact {
    #[primary_key]
    pub artifact_key: String,
    #[index(btree)]
    pub room_id: String,
    #[index(btree)]
    pub game_id: u64,
    pub round_key: String,
    pub status: GenerationStatus,
    pub request_payload_json: String,
    pub response_payload_json: Option<String>,
    pub hidden_answer_candidate: Option<String>,
    pub active_request_id: Option<u64>,
    pub last_error: Option<String>,
    pub updated_at: Timestamp,
}

/// Public artifact row holding the latest villain speech payload for a room and scope.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = villain_speech_artifact, public)]
pub struct VillainSpeechArtifact {
    #[primary_key]
    pub artifact_key: String,
    #[index(btree)]
    pub room_id: String,
    #[index(btree)]
    pub game_id: u64,
    pub round_key: Option<String>,
    pub status: GenerationStatus,
    pub request_payload_json: String,
    pub speech_cues_json: Option<String>,
    pub selected_cue_id: Option<String>,
    pub selected_speech_text: Option<String>,
    pub audio_base64: Option<String>,
    pub mime_type: Option<String>,
    pub tts_provider: Option<String>,
    pub active_request_id: Option<u64>,
    pub last_error: Option<String>,
    pub updated_at: Timestamp,
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
    pub retries: Option<u32>,
}

/// Durable request row for room-scoped clue/manual generation.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = round_generation_request, private)]
pub struct RoundGenerationRequest {
    #[primary_key]
    #[auto_inc]
    pub request_id: u64,
    pub artifact_key: String,
    #[index(btree)]
    pub room_id: String,
    #[index(btree)]
    pub game_id: u64,
    pub round_key: String,
    #[index(btree)]
    pub player_identity: Identity,
    pub request_payload_json: String,
    pub response_schema_json: String,
    pub phase: RoundGenerationPhase,
    pub skeleton_payload_json: Option<String>,
    pub response_payload_json: Option<String>,
    pub hidden_answer_candidate: Option<String>,
    pub error_message: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub retries: Option<u32>,
}

/// Durable request row for villain speech generation and optional audio synthesis.
#[derive(Debug, Clone)]
#[spacetimedb::table(accessor = villain_speech_request, private)]
pub struct VillainSpeechRequest {
    #[primary_key]
    #[auto_inc]
    pub request_id: u64,
    pub artifact_key: String,
    #[index(btree)]
    pub room_id: String,
    #[index(btree)]
    pub game_id: u64,
    #[index(btree)]
    pub player_identity: Identity,
    pub round_key: Option<String>,
    pub request_payload_json: String,
    pub phase: VillainSpeechPhase,
    pub speech_cues_json: Option<String>,
    pub selected_cue_id: Option<String>,
    pub selected_speech_text: Option<String>,
    pub audio_base64: Option<String>,
    pub mime_type: Option<String>,
    pub tts_provider: Option<String>,
    pub error_message: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub retries: Option<u32>,
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

/// Queue row that schedules the outbound clue/manual generation Gemini procedure.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = round_generation_request_schedule,
    private,
    scheduled(process_round_content_request)
)]
pub struct RoundGenerationRequestSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
}

/// Queue row that schedules the reducer-side handling of a clue/manual Gemini result.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = round_generation_callback_schedule,
    private,
    scheduled(_round_content_callback)
)]
pub struct RoundGenerationCallbackSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
    pub status_code: u16,
    pub response_body: String,
    pub transport_error: Option<String>,
}

/// Queue row that schedules the outbound villain speech Gemini procedure.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = villain_speech_request_schedule,
    private,
    scheduled(process_villain_speech_request)
)]
pub struct VillainSpeechRequestSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
}

/// Queue row that schedules the reducer-side handling of a villain speech Gemini result.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = villain_speech_callback_schedule,
    private,
    scheduled(_villain_speech_callback)
)]
pub struct VillainSpeechCallbackSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
    pub status_code: u16,
    pub response_body: String,
    pub transport_error: Option<String>,
}

/// Queue row that schedules the outbound ElevenLabs TTS procedure for villain speech.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = villain_tts_request_schedule,
    private,
    scheduled(process_villain_tts_request)
)]
pub struct VillainTtsRequestSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
}

/// Queue row that schedules the reducer-side handling of a villain TTS result.
#[derive(Debug, Clone)]
#[spacetimedb::table(
    accessor = villain_tts_callback_schedule,
    private,
    scheduled(_villain_tts_callback)
)]
pub struct VillainTtsCallbackSchedule {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub request_id: u64,
    pub status_code: u16,
    pub response_body_base64: String,
    pub mime_type: Option<String>,
    pub transport_error: Option<String>,
}
