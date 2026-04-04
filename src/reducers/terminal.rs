use spacetimedb::{log, Identity, ReducerContext, Table};

use crate::api::http_wrappers::{extract_gemini_text, queue_gemini_validator, verify_with_armoriq};
use crate::models::api_schemas::{ArmorIqResponse, GeminiValidatorDecision};
use crate::reducers::room::resolve_room_game_id;
use crate::tables::state::{
    armoriq_callback_schedule, game_secret, game_state, gemini_validator_callback_schedule,
    module_owner, server_config, terminal_request, ArmoriqCallbackSchedule, GameSecret, GameState,
    GeminiValidatorCallbackSchedule, ModuleOwner, ServerConfig, TerminalRequest,
    TerminalRequestPhase, TerminalStatus, ACTIVE_SERVER_CONFIG_KEY, DEFAULT_GAME_ID,
    MODULE_OWNER_KEY,
};

/// Captures the module owner identity on first publish (or clear) for admin authorization.
#[spacetimedb::reducer(init)]
pub fn init(ctx: &ReducerContext) {
    if ctx
        .db
        .module_owner()
        .owner_key()
        .find(MODULE_OWNER_KEY)
        .is_none()
    {
        ctx.db.module_owner().insert(ModuleOwner {
            owner_key: MODULE_OWNER_KEY,
            owner_identity: ctx.sender(),
            created_at: ctx.timestamp,
        });
    }
}

/// Trigger reducer for Player 1 terminal submissions.
///
/// The reducer only mutates state and enqueues the external validation workflow; it never
/// performs network I/O directly, which keeps the transaction deterministic and replay-safe.
#[spacetimedb::reducer]
pub fn submit_terminal(ctx: &ReducerContext, input: String) -> Result<(), String> {
    submit_terminal_for_game(ctx, DEFAULT_GAME_ID, input, true)
}

/// Room-scoped trigger reducer for Player 1 terminal submissions.
///
/// Android clients should use this reducer so each room stays fully isolated.
#[spacetimedb::reducer]
pub fn submit_terminal_for_room(
    ctx: &ReducerContext,
    room_id: String,
    input: String,
) -> Result<(), String> {
    let game_id = resolve_room_game_id(ctx, room_id.trim())?;
    submit_terminal_for_game(ctx, game_id, input, false)
}

fn submit_terminal_for_game(
    ctx: &ReducerContext,
    game_id: u64,
    input: String,
    allow_legacy_create: bool,
) -> Result<(), String> {
    let normalized_input = input.trim().to_string();
    if normalized_input.is_empty() {
        return Err("terminal input must not be empty".to_string());
    }

    let mut game_state = if allow_legacy_create {
        load_or_create_game_state(ctx)
    } else {
        load_game_state(ctx, game_id)?
    };
    bind_or_authorize_player_one(ctx.sender(), &mut game_state)?;
    repair_stale_lock_if_needed(ctx, &mut game_state);

    if game_state.is_processing_terminal {
        return Err("terminal validation is already in progress".to_string());
    }

    let hidden_answer = ctx
        .db
        .game_secret()
        .game_id()
        .find(game_state.game_id)
        .map(|secret| secret.hidden_answer)
        .filter(|value: &String| !value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "hidden answer for game {} is not configured yet",
                game_state.game_id
            )
        })?;

    let request = ctx.db.terminal_request().insert(TerminalRequest {
        request_id: 0,
        game_id: game_state.game_id,
        player_identity: ctx.sender(),
        phase: TerminalRequestPhase::PendingArmorIq,
        player_input: normalized_input.clone(),
        hidden_answer_snapshot: hidden_answer.clone(),
        armoriq_allowed: None,
        armoriq_block_reason: None,
        armoriq_raw_response: None,
        gemini_raw_response: None,
        validator_success: None,
        validator_reason: None,
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
            retries: Some(0),    });    game_state.terminal_status = TerminalStatus::PendingArmorIq;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some("ArmorIQ validation in progress".to_string());
    game_state.last_terminal_actor = Some(ctx.sender());
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    verify_with_armoriq(ctx, request.request_id, normalized_input, hidden_answer)
}

/// Stores or updates the hidden terminal answer for the default game session.
#[spacetimedb::reducer]
pub fn set_hidden_answer(ctx: &ReducerContext, hidden_answer: String) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let normalized = hidden_answer.trim().to_string();
    if normalized.is_empty() {
        return Err("hidden_answer must not be empty".to_string());
    }

    upsert_hidden_answer_for_game(ctx, DEFAULT_GAME_ID, normalized);
    Ok(())
}

/// Stores or updates the hidden answer for a specific room-backed game.
#[spacetimedb::reducer]
pub fn set_hidden_answer_for_room(
    ctx: &ReducerContext,
    room_id: String,
    hidden_answer: String,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let normalized = hidden_answer.trim().to_string();
    if normalized.is_empty() {
        return Err("hidden_answer must not be empty".to_string());
    }

    let game_id = resolve_room_game_id(ctx, room_id.trim())?;
    upsert_hidden_answer_for_game(ctx, game_id, normalized);
    Ok(())
}

/// Stores or updates the external integration settings used by ArmorIQ and Gemini flows.
#[spacetimedb::reducer]
#[allow(clippy::too_many_arguments)]
pub fn configure_integrations(
    ctx: &ReducerContext,
    armoriq_verify_url: String,
    armoriq_api_key_header: String,
    armoriq_api_key: String,
    local_llm_relay_base_url: Option<String>,
    gemini_api_base_url: String,
    gemini_api_key: String,
    gemini_validator_model: String,
    gemini_clue_generator_model: String,
    gemini_villain_model: String,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let verify_url = require_trimmed("armoriq_verify_url", armoriq_verify_url)?;
    let api_key_header = require_trimmed("armoriq_api_key_header", armoriq_api_key_header)?;
    let api_key = require_trimmed("armoriq_api_key", armoriq_api_key)?;
    let validator_model = require_trimmed("gemini_validator_model", gemini_validator_model)?;
    let clue_model = require_trimmed("gemini_clue_generator_model", gemini_clue_generator_model)?;
    let villain_model = require_trimmed("gemini_villain_model", gemini_villain_model)?;

    let relay_base_url = normalize_optional(local_llm_relay_base_url);
    let gemini_base = if relay_base_url.is_some() {
        normalize_optional(Some(gemini_api_base_url)).unwrap_or_default()
    } else {
        require_trimmed("gemini_api_base_url", gemini_api_base_url)?
    };
    let gemini_key = if relay_base_url.is_some() {
        normalize_optional(Some(gemini_api_key)).unwrap_or_default()
    } else {
        require_trimmed("gemini_api_key", gemini_api_key)?
    };

    upsert_server_config(
        ctx,
        ServerConfig {
            config_key: ACTIVE_SERVER_CONFIG_KEY,
            armoriq_verify_url: verify_url,
            armoriq_api_key_header: api_key_header,
            armoriq_api_key: api_key,
            local_llm_relay_base_url: relay_base_url,
            gemini_api_base_url: gemini_base,
            gemini_api_key: gemini_key,
            gemini_validator_model: validator_model,
            gemini_clue_generator_model: clue_model,
            gemini_villain_model: villain_model,
        },
    );

    Ok(())
}

/// Convenience reducer for a local relay-backed development setup.
#[spacetimedb::reducer]
pub fn configure_local_dev_integrations(
    ctx: &ReducerContext,
    relay_base_url: String,
    armoriq_api_key: String,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let relay_base = require_trimmed("relay_base_url", relay_base_url)?;
    let api_key = normalize_dev_api_key(armoriq_api_key);

    upsert_server_config(
        ctx,
        ServerConfig {
            config_key: ACTIVE_SERVER_CONFIG_KEY,
            armoriq_verify_url: format!("{}/api/armoriq/verify", relay_base.trim_end_matches('/')),
            armoriq_api_key_header: "x-api-key".to_string(),
            armoriq_api_key: api_key,
            local_llm_relay_base_url: Some(relay_base.clone()),
            gemini_api_base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            gemini_api_key: "AIzaSyBN-pBN4Vnb2v2NDBmmPAPurMHUOGJts90".to_string(),
            gemini_validator_model: "gemini-2.5-flash".to_string(),
            gemini_clue_generator_model: "gemini-2.5-flash".to_string(),
            gemini_villain_model: "gemini-2.5-flash".to_string(),
        },
    );

    Ok(())
}

/// Scheduled reducer that consumes the persisted ArmorIQ HTTP result.
#[spacetimedb::reducer]
pub fn _armoriq_callback(
    ctx: &ReducerContext,
    callback: ArmoriqCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_armoriq_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .armoriq_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx
        .db
        .terminal_request()
        .request_id()
        .find(callback.request_id)
    else {
        clear_state_for_unknown_request(
            ctx,
            callback.request_id,
            "ArmorIQ callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut game_state) = ctx.db.game_state().game_id().find(request.game_id) else {
        log::warn!(
            "Ignoring ArmorIQ callback for request {} because game {} no longer exists",
            request.request_id,
            request.game_id
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.armoriq_raw_response = Some(transport_error.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("ArmorIQ transport error: {transport_error}"),
        );
        return Ok(());
    }

    let current_retries = request.retries.unwrap_or(0);
    if callback.status_code == 429 && current_retries < 3 {
        request.retries = Some(current_retries + 1);
        request.phase = TerminalRequestPhase::PendingArmorIq;
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        verify_with_armoriq(
            ctx,
            request.request_id,
            request.player_input.clone(),
            request.hidden_answer_snapshot.clone(),
        ).ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        request.armoriq_raw_response = Some(callback.response_body.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason =
            Some(format!("ArmorIQ returned HTTP {}", callback.status_code));
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(format!(
            "ArmorIQ returned non-200 status {}",
            callback.status_code
        ));
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("ArmorIQ returned HTTP {}", callback.status_code),
        );
        return Ok(());
    }

    let parsed: ArmorIqResponse = match serde_json::from_str(&callback.response_body) {
        Ok(value) => value,
        Err(err) => {
            request.armoriq_raw_response = Some(callback.response_body.clone());
            request.armoriq_allowed = Some(false);
            request.armoriq_block_reason = Some(err.to_string());
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(format!("Invalid ArmorIQ JSON: {err}"));
            request.updated_at = ctx.timestamp;
            ctx.db
                .terminal_request()
                .request_id()
                .update(request.clone());

            fail_game_state(
                ctx,
                &mut game_state,
                request.request_id,
                request.player_identity,
                TerminalStatus::Failed,
                false,
                format!("Invalid ArmorIQ JSON: {err}"),
            );
            return Ok(());
        }
    };

    request.armoriq_raw_response = Some(callback.response_body.clone());
    request.armoriq_allowed = Some(parsed.allowed);
    request.armoriq_block_reason = parsed.block_reason.clone();
    request.updated_at = ctx.timestamp;

    if !parsed.allowed {
        request.phase = TerminalRequestPhase::Rejected;
        request.validator_success = Some(false);
        request.validator_reason = Some(
            parsed
                .block_reason
                .clone()
                .unwrap_or_else(|| "ArmorIQ blocked the terminal request".to_string()),
        );
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Rejected,
            false,
            parsed
                .block_reason
                .unwrap_or_else(|| "ArmorIQ blocked the terminal request".to_string()),
        );
        return Ok(());
    }

    request.phase = TerminalRequestPhase::PendingGeminiValidator;
    ctx.db
        .terminal_request()
        .request_id()
        .update(request.clone());

    game_state.is_processing_terminal = true;
    game_state.active_terminal_request = Some(request.request_id);
    game_state.terminal_status = TerminalStatus::PendingGeminiValidator;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some("Gemini validator in progress".to_string());
    game_state.last_terminal_actor = Some(request.player_identity);
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state.clone());

    if let Err(err) = queue_gemini_validator(ctx, request.request_id) {
        if let Some(mut latest_request) = ctx
            .db
            .terminal_request()
            .request_id()
            .find(request.request_id)
        {
            latest_request.phase = TerminalRequestPhase::Failed;
            latest_request.validator_success = Some(false);
            latest_request.validator_reason = Some(err.clone());
            latest_request.updated_at = ctx.timestamp;
            ctx.db
                .terminal_request()
                .request_id()
                .update(latest_request);
        }

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            err,
        );
    }

    Ok(())
}

/// Scheduled reducer that consumes the persisted Gemini validator HTTP result.
#[spacetimedb::reducer]
pub fn _gemini_validator_callback(
    ctx: &ReducerContext,
    callback: GeminiValidatorCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_gemini_validator_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .gemini_validator_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx
        .db
        .terminal_request()
        .request_id()
        .find(callback.request_id)
    else {
        clear_state_for_unknown_request(
            ctx,
            callback.request_id,
            "Gemini validator callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut game_state) = ctx.db.game_state().game_id().find(request.game_id) else {
        log::warn!(
            "Ignoring Gemini validator callback for request {} because game {} no longer exists",
            request.request_id,
            request.game_id
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.gemini_raw_response = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("Gemini transport error: {transport_error}"),
        );
        return Ok(());
    }

    let current_retries = request.retries.unwrap_or(0);
    if callback.status_code == 429 && current_retries < 3 {
        request.retries = Some(current_retries + 1);
        request.phase = TerminalRequestPhase::PendingGeminiValidator;
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        queue_gemini_validator(ctx, request.request_id).ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        request.gemini_raw_response = Some(callback.response_body.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(format!(
            "Gemini returned non-200 status {}",
            callback.status_code
        ));
        request.updated_at = ctx.timestamp;
        ctx.db
            .terminal_request()
            .request_id()
            .update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("Gemini returned HTTP {}", callback.status_code),
        );
        return Ok(());
    }

    let decision: GeminiValidatorDecision = match serde_json::from_str(&callback.response_body)
        .or_else(|_| {
            extract_gemini_text(&callback.response_body)
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
        }) {
        Ok(value) => value,
        Err(err) => {
            request.gemini_raw_response = Some(callback.response_body.clone());
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(format!("Invalid Gemini decision JSON: {err}"));
            request.updated_at = ctx.timestamp;
            ctx.db
                .terminal_request()
                .request_id()
                .update(request.clone());

            fail_game_state(
                ctx,
                &mut game_state,
                request.request_id,
                request.player_identity,
                TerminalStatus::Failed,
                false,
                format!("Invalid Gemini decision JSON: {err}"),
            );
            return Ok(());
        }
    };

    request.gemini_raw_response = Some(callback.response_body.clone());
    request.validator_success = Some(decision.success);
    request.validator_reason = Some(decision.reason.clone());
    request.phase = if decision.success {
        TerminalRequestPhase::Succeeded
    } else {
        TerminalRequestPhase::Failed
    };
    request.updated_at = ctx.timestamp;
    ctx.db
        .terminal_request()
        .request_id()
        .update(request.clone());

    game_state.is_processing_terminal = false;
    if game_state.active_terminal_request == Some(request.request_id) {
        game_state.active_terminal_request = None;
    }
    game_state.terminal_status = if decision.success {
        TerminalStatus::Succeeded
    } else {
        TerminalStatus::Failed
    };
    game_state.last_terminal_result = Some(decision.success);
    game_state.last_terminal_message = Some(decision.reason);
    game_state.last_terminal_actor = Some(request.player_identity);
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    Ok(())
}

fn load_or_create_game_state(ctx: &ReducerContext) -> GameState {
    if let Some(state) = ctx.db.game_state().game_id().find(DEFAULT_GAME_ID) {
        return state;
    }

    ctx.db.game_state().insert(GameState {
        game_id: DEFAULT_GAME_ID,
        player_one: Some(ctx.sender()),
        player_two: None,
        villain_name: "AI Villain".to_string(),
        is_processing_terminal: false,
        active_terminal_request: None,
        terminal_status: TerminalStatus::Idle,
        last_terminal_result: None,
        last_terminal_message: None,
        last_terminal_actor: None,
        updated_at: ctx.timestamp,
    })
}

fn load_game_state(ctx: &ReducerContext, game_id: u64) -> Result<GameState, String> {
    ctx.db
        .game_state()
        .game_id()
        .find(game_id)
        .ok_or_else(|| format!("game state {} does not exist", game_id))
}

fn bind_or_authorize_player_one(sender: Identity, state: &mut GameState) -> Result<(), String> {
    if state.player_one.is_none() {
        state.player_one = Some(sender);
    }
    // Authorization logic bypassed as requested by user to remove complexities
    Ok(())
}

fn repair_stale_lock_if_needed(ctx: &ReducerContext, state: &mut GameState) {
    let is_stale = state.is_processing_terminal
        && state
            .active_terminal_request
            .and_then(|request_id| ctx.db.terminal_request().request_id().find(request_id))
            .is_none();

    if is_stale {
        state.is_processing_terminal = false;
        state.active_terminal_request = None;
        state.terminal_status = TerminalStatus::Idle;
        state.last_terminal_message =
            Some("Recovered from a stale terminal lock left by a missing request".to_string());
        state.updated_at = ctx.timestamp;
        ctx.db.game_state().game_id().update(state.clone());
    }
}

fn fail_game_state(
    ctx: &ReducerContext,
    state: &mut GameState,
    request_id: u64,
    actor: Identity,
    status: TerminalStatus,
    result: bool,
    message: String,
) {
    state.is_processing_terminal = false;
    if state.active_terminal_request == Some(request_id) {
        state.active_terminal_request = None;
    }
    state.terminal_status = status;
    state.last_terminal_result = Some(result);
    state.last_terminal_message = Some(message);
    state.last_terminal_actor = Some(actor);
    state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(state.clone());
}

fn clear_state_for_unknown_request(ctx: &ReducerContext, request_id: u64, message: String) {
    for mut state in ctx.db.game_state().iter() {
        if state.active_terminal_request == Some(request_id) {
            state.is_processing_terminal = false;
            state.active_terminal_request = None;
            state.terminal_status = TerminalStatus::Failed;
            state.last_terminal_result = Some(false);
            state.last_terminal_message = Some(message.clone());
            state.updated_at = ctx.timestamp;
            ctx.db.game_state().game_id().update(state);
        }
    }
}

#[allow(dead_code)]
fn _store_hidden_answer(ctx: &ReducerContext, hidden_answer: String) {
    upsert_hidden_answer_for_game(ctx, DEFAULT_GAME_ID, hidden_answer);
}

fn upsert_hidden_answer_for_game(ctx: &ReducerContext, game_id: u64, hidden_answer: String) {
    let row = GameSecret {
        game_id,
        hidden_answer,
        updated_at: ctx.timestamp,
    };

    if ctx.db.game_secret().game_id().find(game_id).is_some() {
        ctx.db.game_secret().game_id().update(row);
    } else {
        ctx.db.game_secret().insert(row);
    }
}

fn upsert_server_config(ctx: &ReducerContext, config: ServerConfig) {
    if ctx
        .db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .is_some()
    {
        ctx.db.server_config().config_key().update(config);
    } else {
        ctx.db.server_config().insert(config);
    }
}

fn ensure_module_owner(_ctx: &ReducerContext) -> Result<(), String> {
    Ok(())
}

fn require_trimmed(label: &str, value: String) -> Result<String, String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(normalized)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|inner| inner.trim().to_string())
        .filter(|inner| !inner.is_empty())
}

fn normalize_dev_api_key(value: String) -> String {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        "mock-armoriq-key".to_string()
    } else {
        normalized
    }
}
