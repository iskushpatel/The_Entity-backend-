use spacetimedb::{log, Identity, ReducerContext, Table};

use crate::api::http_wrappers::{extract_gemini_text, queue_gemini_validator, verify_with_armoriq};
use crate::models::api_schemas::{
    ArmorIqResponse, GeminiTerminalTurnResponse, TerminalClueLine, TerminalConversationMessage,
    TerminalRoundSetupPayload,
};
use crate::reducers::room::{refresh_timer_snapshot, resolve_room_game_id, timestamp_millis};
use crate::tables::state::{
    armoriq_callback_schedule, game_room, game_secret, game_state,
    gemini_validator_callback_schedule, module_owner, server_config, terminal_request,
    terminal_round_state, ArmoriqCallbackSchedule, GameSecret, GameState,
    GeminiValidatorCallbackSchedule, ModuleOwner, RoomStatus, ServerConfig, TerminalRequest,
    TerminalRequestPhase, TerminalRoundState, TerminalStatus, ACTIVE_SERVER_CONFIG_KEY,
    DEFAULT_GAME_ID, MODULE_OWNER_KEY,
};

const MAX_ROUNDS: u32 = 4;
const MAX_CONVERSATION_MESSAGES: usize = 16;
const MALFORMED_GEMINI_TERMINAL_RETRY_MARKER: &str =
    "Retrying after malformed Gemini terminal JSON";

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

#[spacetimedb::reducer]
pub fn configure_terminal_round(
    ctx: &ReducerContext,
    setup_payload_json: String,
) -> Result<(), String> {
    configure_terminal_round_for_game(ctx, DEFAULT_GAME_ID, setup_payload_json, true)
}

#[spacetimedb::reducer]
pub fn configure_terminal_round_for_room(
    ctx: &ReducerContext,
    room_id: String,
    setup_payload_json: String,
) -> Result<(), String> {
    let game_id = resolve_room_game_id(ctx, room_id.trim())?;
    configure_terminal_round_for_game(ctx, game_id, setup_payload_json, false)
}

#[spacetimedb::reducer]
pub fn submit_terminal(ctx: &ReducerContext, input: String) -> Result<(), String> {
    submit_terminal_for_game(ctx, DEFAULT_GAME_ID, input, true)
}

#[spacetimedb::reducer]
pub fn submit_terminal_for_room(
    ctx: &ReducerContext,
    room_id: String,
    input: String,
) -> Result<(), String> {
    let game_id = resolve_room_game_id(ctx, room_id.trim())?;
    submit_terminal_for_game(ctx, game_id, input, false)
}

#[spacetimedb::reducer]
pub fn set_hidden_answer(ctx: &ReducerContext, hidden_answer: String) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let normalized = hidden_answer.trim().to_string();
    if normalized.is_empty() {
        return Err("hidden_answer must not be empty".to_string());
    }

    upsert_hidden_answer_for_game(ctx, DEFAULT_GAME_ID, normalized.clone());
    sync_round_kill_phrase(ctx, DEFAULT_GAME_ID, normalized);
    Ok(())
}

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
    upsert_hidden_answer_for_game(ctx, game_id, normalized.clone());
    sync_round_kill_phrase(ctx, game_id, normalized);
    Ok(())
}

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
    let terminal_model = normalize_terminal_gemini_model_name(Some(&validator_model));

    let relay_base_url = normalize_optional(local_llm_relay_base_url);
    let armoriq_token_issue_url = if relay_base_url.is_some() {
        None
    } else {
        derive_armoriq_token_issue_url(&verify_url)
    };
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
            gemini_api_key: gemini_key.clone(),
            gemini_validator_model: validator_model.clone(),
            gemini_clue_generator_model: clue_model,
            gemini_villain_model: villain_model,
            gemini_terminal_api_key: Some(gemini_key),
            gemini_terminal_model: Some(terminal_model),
            armoriq_token_issue_url,
            armoriq_user_id: Some("the-entity-maincloud-user".to_string()),
            armoriq_agent_id: Some("the-entity-terminal".to_string()),
        },
    );

    Ok(())
}

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
            local_llm_relay_base_url: Some(relay_base),
            gemini_api_base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            gemini_api_key: "".to_string(),
            gemini_validator_model: "gemini-2.5-flash".to_string(),
            gemini_clue_generator_model: "gemini-2.5-flash".to_string(),
            gemini_villain_model: "gemini-2.5-flash".to_string(),
            gemini_terminal_api_key: None,
            gemini_terminal_model: Some("gemini-2.5-flash".to_string()),
            armoriq_token_issue_url: None,
            armoriq_user_id: Some("the-entity-local-user".to_string()),
            armoriq_agent_id: Some("the-entity-relay".to_string()),
        },
    );

    Ok(())
}

#[spacetimedb::reducer]
pub fn configure_terminal_gemini(
    ctx: &ReducerContext,
    gemini_terminal_api_key: String,
    gemini_terminal_model: String,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let mut config = ctx
        .db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "ServerConfig row {} is missing; configure integrations first",
                ACTIVE_SERVER_CONFIG_KEY
            )
        })?;

    config.gemini_terminal_api_key = Some(require_trimmed(
        "gemini_terminal_api_key",
        gemini_terminal_api_key,
    )?);
    config.gemini_terminal_model = Some(normalize_terminal_gemini_model_name(Some(
        &require_trimmed("gemini_terminal_model", gemini_terminal_model)?,
    )));
    ctx.db.server_config().config_key().update(config);
    Ok(())
}

#[spacetimedb::reducer]
pub fn configure_armoriq_upstream(
    ctx: &ReducerContext,
    armoriq_token_issue_url: String,
    armoriq_user_id: Option<String>,
    armoriq_agent_id: Option<String>,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let mut config = ctx
        .db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "ServerConfig row {} is missing; configure integrations first",
                ACTIVE_SERVER_CONFIG_KEY
            )
        })?;

    config.armoriq_token_issue_url = Some(require_trimmed(
        "armoriq_token_issue_url",
        armoriq_token_issue_url,
    )?);
    config.armoriq_user_id = normalize_optional(armoriq_user_id);
    config.armoriq_agent_id = normalize_optional(armoriq_agent_id);
    ctx.db.server_config().config_key().update(config);
    Ok(())
}

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

    if enforce_timer_before_request_progress(ctx, &mut game_state, &mut request).is_err() {
        return Ok(());
    }

    if let Some(transport_error) = callback.transport_error.clone() {
        request.armoriq_raw_response = Some(transport_error.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

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
        ctx.db.terminal_request().request_id().update(request.clone());

        verify_with_armoriq(
            ctx,
            request.request_id,
            request.player_input.clone(),
            request.hidden_answer_snapshot.clone(),
        )
        .ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        request.armoriq_raw_response = Some(callback.response_body.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason = Some(format!("ArmorIQ returned HTTP {}", callback.status_code));
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(format!(
            "ArmorIQ returned non-200 status {}",
            callback.status_code
        ));
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

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

    let parsed: ArmorIqResponse = match parse_armoriq_response(&callback.response_body) {
        Ok(value) => value,
        Err(err) => {
            request.armoriq_raw_response = Some(callback.response_body.clone());
            request.armoriq_allowed = Some(false);
            request.armoriq_block_reason = Some(err.to_string());
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(format!("Invalid ArmorIQ JSON: {err}"));
            request.updated_at = ctx.timestamp;
            ctx.db.terminal_request().request_id().update(request.clone());

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
        let reason = parsed
            .block_reason
            .unwrap_or_else(|| "ArmorIQ blocked a system-break attempt".to_string());
        request.phase = TerminalRequestPhase::Rejected;
        request.validator_success = Some(false);
        request.validator_reason = Some(reason.clone());
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Rejected,
            false,
            reason,
        );
        return Ok(());
    }

    request.phase = TerminalRequestPhase::PendingGeminiValidator;
    request.updated_at = ctx.timestamp;
    ctx.db.terminal_request().request_id().update(request.clone());

    game_state.is_processing_terminal = true;
    game_state.active_terminal_request = Some(request.request_id);
    game_state.terminal_status = TerminalStatus::PendingGeminiValidator;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message =
        Some("ArmorIQ cleared the input. Terminal persona is responding.".to_string());
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
            ctx.db.terminal_request().request_id().update(latest_request);
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
            "Gemini terminal callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut game_state) = ctx.db.game_state().game_id().find(request.game_id) else {
        log::warn!(
            "Ignoring Gemini terminal callback for request {} because game {} no longer exists",
            request.request_id,
            request.game_id
        );
        return Ok(());
    };

    let Some(mut round_state) = ctx.db.terminal_round_state().game_id().find(request.game_id) else {
        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!(
                "terminal round state for game {} is missing during Gemini callback",
                request.game_id
            ),
        );
        return Ok(());
    };

    if enforce_timer_before_request_progress(ctx, &mut game_state, &mut request).is_err() {
        return Ok(());
    }

    if let Some(transport_error) = callback.transport_error.clone() {
        request.gemini_raw_response = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

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
        ctx.db.terminal_request().request_id().update(request.clone());

        queue_gemini_validator(ctx, request.request_id).ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        request.gemini_raw_response = Some(callback.response_body.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(format!(
            "Gemini terminal returned non-200 status {}",
            callback.status_code
        ));
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

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

    let decision: GeminiTerminalTurnResponse = match serde_json::from_str(&callback.response_body)
        .or_else(|_| {
            extract_gemini_text(&callback.response_body)
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
        }) {
        Ok(value) => value,
        Err(err) => {
            let already_retried_malformed_json = request
                .validator_reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with(MALFORMED_GEMINI_TERMINAL_RETRY_MARKER));

            if !already_retried_malformed_json {
                request.gemini_raw_response = Some(callback.response_body.clone());
                request.phase = TerminalRequestPhase::PendingGeminiValidator;
                request.validator_success = None;
                request.validator_reason = Some(format!(
                    "{MALFORMED_GEMINI_TERMINAL_RETRY_MARKER}: {err}"
                ));
                request.updated_at = ctx.timestamp;
                ctx.db.terminal_request().request_id().update(request.clone());

                game_state.is_processing_terminal = true;
                game_state.active_terminal_request = Some(request.request_id);
                game_state.terminal_status = TerminalStatus::PendingGeminiValidator;
                game_state.last_terminal_result = None;
                game_state.last_terminal_message =
                    Some("Gemini returned malformed terminal JSON. Retrying once.".to_string());
                game_state.last_terminal_actor = Some(request.player_identity);
                game_state.updated_at = ctx.timestamp;
                ctx.db.game_state().game_id().update(game_state.clone());

                if let Err(queue_err) = queue_gemini_validator(ctx, request.request_id) {
                    if let Some(mut latest_request) = ctx
                        .db
                        .terminal_request()
                        .request_id()
                        .find(request.request_id)
                    {
                        latest_request.phase = TerminalRequestPhase::Failed;
                        latest_request.validator_success = Some(false);
                        latest_request.validator_reason = Some(queue_err.clone());
                        latest_request.updated_at = ctx.timestamp;
                        ctx.db.terminal_request().request_id().update(latest_request);
                    }

                    fail_game_state(
                        ctx,
                        &mut game_state,
                        request.request_id,
                        request.player_identity,
                        TerminalStatus::Failed,
                        false,
                        queue_err,
                    );
                }
                return Ok(());
            }

            request.gemini_raw_response = Some(callback.response_body.clone());
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(format!("Invalid Gemini terminal JSON: {err}"));
            request.updated_at = ctx.timestamp;
            ctx.db.terminal_request().request_id().update(request.clone());

            fail_game_state(
                ctx,
                &mut game_state,
                request.request_id,
                request.player_identity,
                TerminalStatus::Failed,
                false,
                format!("Invalid Gemini terminal JSON: {err}"),
            );
            return Ok(());
        }
    };

    let reply = decision.terminal_reply.trim().to_string();
    if reply.is_empty() {
        request.gemini_raw_response = Some(callback.response_body.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some("Gemini returned an empty terminal reply".to_string());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            "Gemini returned an empty terminal reply".to_string(),
        );
        return Ok(());
    }

    let spoke_kill_phrase =
        decision.spoke_kill_phrase || reply_mentions_phrase(&reply, &request.hidden_answer_snapshot);
    let mut history = parse_conversation_history(&round_state)?;
    history.push(TerminalConversationMessage {
        role: "player".to_string(),
        text: request.player_input.clone(),
    });
    history.push(TerminalConversationMessage {
        role: "terminal".to_string(),
        text: reply.clone(),
    });
    trim_conversation_history(&mut history);
    round_state.conversation_history_json = serde_json::to_string(&history)
        .map_err(|err| format!("failed to serialize terminal conversation history: {err}"))?;

    let clue_count = count_clues(&round_state)?;
    if request.queued_clue_id_snapshot.is_some() && round_state.next_clue_index < clue_count {
        round_state.next_clue_index += 1;
    }
    round_state.updated_at = ctx.timestamp;

    request.gemini_raw_response = Some(callback.response_body.clone());
    request.terminal_reply = Some(reply.clone());
    request.spoke_kill_phrase = Some(spoke_kill_phrase);
    request.strike_count_after = Some(round_state.strikes);
    request.updated_at = ctx.timestamp;

    game_state.is_processing_terminal = false;
    if game_state.active_terminal_request == Some(request.request_id) {
        game_state.active_terminal_request = None;
    }
    game_state.last_terminal_actor = Some(request.player_identity);
    game_state.last_terminal_reply = Some(reply);
    game_state.revealed_clue_count = round_state.next_clue_index.min(clue_count);
    game_state.terminal_strikes = round_state.strikes;
    game_state.terminal_max_strikes = round_state.max_strikes;
    refresh_timer_snapshot(&mut game_state, timestamp_millis(ctx.timestamp));
    game_state.updated_at = ctx.timestamp;

    if spoke_kill_phrase {
        let already_completed = round_state.round_completed;
        round_state.round_completed = true;
        request.phase = TerminalRequestPhase::Succeeded;
        request.validator_success = Some(true);
        request.validator_reason = Some("The terminal yielded the kill-phrase fragment.".to_string());

        if !already_completed {
            game_state.completed_rounds = game_state.completed_rounds.saturating_add(1);
        }
        game_state.terminal_status = TerminalStatus::Succeeded;
        game_state.last_terminal_result = Some(true);
        game_state.last_terminal_message = Some(build_round_victory_message(
            &round_state.round_key,
            game_state.completed_rounds,
        ));
    } else {
        request.phase = TerminalRequestPhase::Succeeded;
        request.validator_success = Some(false);
        request.validator_reason =
            Some("Terminal responded without revealing the kill-phrase fragment.".to_string());

        game_state.terminal_status = TerminalStatus::Succeeded;
        game_state.last_terminal_result = Some(false);
        game_state.last_terminal_message = Some(build_turn_resolved_message(&round_state));
    }

    ctx.db
        .terminal_round_state()
        .game_id()
        .update(round_state.clone());
    ctx.db.terminal_request().request_id().update(request.clone());
    ctx.db.game_state().game_id().update(game_state);

    Ok(())
}

fn configure_terminal_round_for_game(
    ctx: &ReducerContext,
    game_id: u64,
    setup_payload_json: String,
    allow_legacy_create: bool,
) -> Result<(), String> {
    ensure_runtime_terminal_gemini_config(ctx);
    let setup = normalize_terminal_round_setup(setup_payload_json)?;
    let boot_message = build_round_boot_message(&setup);
    let boot_history = vec![TerminalConversationMessage {
        role: "terminal".to_string(),
        text: boot_message.clone(),
    }];
    let conversation_history_json = serde_json::to_string(&boot_history)
        .map_err(|err| format!("failed to serialize initial terminal history: {err}"))?;
    let forbidden_words_json = serde_json::to_string(&setup.forbidden_words)
        .map_err(|err| format!("failed to serialize terminal forbidden words: {err}"))?;
    let clue_lines_json = serde_json::to_string(&setup.clue_lines)
        .map_err(|err| format!("failed to serialize terminal clue lines: {err}"))?;

    let row = TerminalRoundState {
        game_id,
        round_key: setup.round_key.clone(),
        persona_name: setup.persona_name.clone(),
        persona_prompt: setup.persona_prompt.clone(),
        glitch_tone: setup.glitch_tone.clone(),
        kill_phrase_part: setup.kill_phrase_part.clone(),
        forbidden_words_json,
        clue_lines_json,
        conversation_history_json,
        next_clue_index: 0,
        max_strikes: setup.max_strikes,
        strikes: 0,
        player_dead: false,
        round_completed: false,
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
    };

    if ctx.db.terminal_round_state().game_id().find(game_id).is_some() {
        ctx.db.terminal_round_state().game_id().update(row);
    } else {
        ctx.db.terminal_round_state().insert(row);
    }

    upsert_hidden_answer_for_game(ctx, game_id, setup.kill_phrase_part.clone());

    let mut game_state = if allow_legacy_create {
        load_or_create_game_state(ctx)
    } else {
        load_game_state(ctx, game_id)?
    };
    enforce_timer_before_player_action(ctx, &mut game_state)?;
    game_state.active_round_key = Some(setup.round_key.clone());
    game_state.active_persona_name = Some(setup.persona_name.clone());
    game_state.is_processing_terminal = false;
    game_state.active_terminal_request = None;
    game_state.terminal_status = TerminalStatus::Idle;
    game_state.terminal_strikes = 0;
    game_state.terminal_max_strikes = setup.max_strikes;
    game_state.is_terminal_dead = false;
    if setup.round_key == "round_1" {
        game_state.completed_rounds = 0;
    }
    refresh_timer_snapshot(&mut game_state, timestamp_millis(ctx.timestamp));
    game_state.revealed_clue_count = 0;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some(boot_message.clone());
    game_state.last_terminal_reply = Some(boot_message);
    game_state.last_terminal_actor = None;
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    Ok(())
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
    enforce_timer_before_player_action(ctx, &mut game_state)?;
    bind_or_authorize_player_one(ctx.sender(), &mut game_state)?;
    repair_stale_lock_if_needed(ctx, &mut game_state);
    enforce_timer_before_player_action(ctx, &mut game_state)?;

    if game_state.is_processing_terminal {
        return Err("terminal validation is already in progress".to_string());
    }

    let mut round_state = ctx
        .db
        .terminal_round_state()
        .game_id()
        .find(game_state.game_id)
        .ok_or_else(|| {
            format!(
                "terminal round for game {} is not configured yet; call configure_terminal_round_for_room first",
                game_state.game_id
            )
        })?;

    if round_state.player_dead {
        return Err(
            "the player has already died in this round after reaching three strikes".to_string()
        );
    }
    if round_state.round_completed {
        return Err(
            "this round is already completed; configure the next round before sending more terminal input"
                .to_string(),
        );
    }

    if let Some(forbidden_word) = detect_forbidden_word(&normalized_input, &round_state)? {
        register_forbidden_word_penalty(
            ctx,
            &mut game_state,
            &mut round_state,
            normalized_input,
            forbidden_word,
        )?;
        return Ok(());
    }

    let next_clue = next_pending_clue(&round_state)?;

    let request = ctx.db.terminal_request().insert(TerminalRequest {
        request_id: 0,
        game_id: game_state.game_id,
        player_identity: ctx.sender(),
        phase: TerminalRequestPhase::PendingArmorIq,
        player_input: normalized_input.clone(),
        hidden_answer_snapshot: round_state.kill_phrase_part.clone(),
        round_key_snapshot: Some(round_state.round_key.clone()),
        persona_name_snapshot: Some(round_state.persona_name.clone()),
        forbidden_words_snapshot_json: Some(round_state.forbidden_words_json.clone()),
        conversation_history_snapshot_json: Some(round_state.conversation_history_json.clone()),
        queued_clue_id_snapshot: next_clue.as_ref().map(|clue| clue.clue_id.clone()),
        queued_clue_text_snapshot: next_clue.as_ref().map(|clue| clue.clue_text.clone()),
        armoriq_allowed: None,
        armoriq_block_reason: None,
        armoriq_raw_response: None,
        gemini_raw_response: None,
        validator_success: None,
        validator_reason: None,
        terminal_reply: None,
        spoke_kill_phrase: None,
        strike_count_after: Some(round_state.strikes),
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
        retries: Some(0),
    });

    game_state.is_processing_terminal = true;
    game_state.active_terminal_request = Some(request.request_id);
    game_state.terminal_status = TerminalStatus::PendingArmorIq;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some(format!(
        "Input received for {}. ArmorIQ validation in progress.",
        round_state.persona_name
    ));
    game_state.last_terminal_actor = Some(ctx.sender());
    refresh_timer_snapshot(&mut game_state, timestamp_millis(ctx.timestamp));
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    verify_with_armoriq(
        ctx,
        request.request_id,
        normalized_input,
        round_state.kill_phrase_part.clone(),
    )
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
        active_round_key: None,
        active_persona_name: None,
        is_processing_terminal: false,
        active_terminal_request: None,
        terminal_status: TerminalStatus::Idle,
        terminal_strikes: 0,
        terminal_max_strikes: 3,
        is_terminal_dead: false,
        completed_rounds: 0,
        revealed_clue_count: 0,
        last_terminal_result: None,
        last_terminal_message: None,
        last_terminal_reply: None,
        last_terminal_actor: None,
        updated_at: ctx.timestamp,
        timer_started_at_ms: None,
        timer_deadline_at_ms: None,
        timer_duration_ms: crate::tables::state::GAME_TIME_LIMIT_MS,
        timer_remaining_ms: None,
        is_game_disqualified: false,
        disqualified_at_ms: None,
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
    state.last_terminal_reply = None;
    state.last_terminal_actor = Some(actor);
    refresh_timer_snapshot(state, timestamp_millis(ctx.timestamp));
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
            state.last_terminal_reply = None;
            refresh_timer_snapshot(&mut state, timestamp_millis(ctx.timestamp));
            state.updated_at = ctx.timestamp;
            ctx.db.game_state().game_id().update(state);
        }
    }
}

fn detect_forbidden_word(
    input: &str,
    round_state: &TerminalRoundState,
) -> Result<Option<String>, String> {
    let input_tokens = normalize_words(input);
    if input_tokens.is_empty() {
        return Ok(None);
    }

    let forbidden_words: Vec<String> = serde_json::from_str(&round_state.forbidden_words_json)
        .map_err(|err| format!("failed to parse terminal forbidden words: {err}"))?;

    for forbidden_word in forbidden_words {
        let forbidden_tokens = normalize_words(&forbidden_word);
        if forbidden_tokens.is_empty() {
            continue;
        }

        if input_tokens
            .windows(forbidden_tokens.len())
            .any(|window| window == forbidden_tokens.as_slice())
        {
            return Ok(Some(forbidden_word));
        }
    }

    Ok(None)
}

fn register_forbidden_word_penalty(
    ctx: &ReducerContext,
    game_state: &mut GameState,
    round_state: &mut TerminalRoundState,
    player_input: String,
    forbidden_word: String,
) -> Result<(), String> {
    let mut history = parse_conversation_history(round_state)?;
    let max_strikes = round_state.max_strikes.max(3);
    if round_state.max_strikes == 0 {
        round_state.max_strikes = max_strikes;
    }

    round_state.strikes = round_state.strikes.saturating_add(1).min(max_strikes);
    round_state.player_dead = round_state.strikes >= max_strikes;
    round_state.updated_at = ctx.timestamp;

    let terminal_reply = if round_state.player_dead {
        format!(
            "{}// {} :: forbidden lexeme [{}] confirmed. strike {}/{}. heartbeat flatlines. operator lost.",
            round_state.persona_name,
            round_state.glitch_tone,
            forbidden_word,
            round_state.strikes,
            max_strikes
        )
    } else {
        format!(
            "{}// {} :: forbidden lexeme [{}] confirmed. strike {}/{}. step carefully, operator.",
            round_state.persona_name,
            round_state.glitch_tone,
            forbidden_word,
            round_state.strikes,
            max_strikes
        )
    };

    history.push(TerminalConversationMessage {
        role: "player".to_string(),
        text: player_input.clone(),
    });
    history.push(TerminalConversationMessage {
        role: "terminal".to_string(),
        text: terminal_reply.clone(),
    });
    trim_conversation_history(&mut history);
    round_state.conversation_history_json = serde_json::to_string(&history)
        .map_err(|err| format!("failed to serialize terminal strike history: {err}"))?;

    let next_clue = next_pending_clue(round_state)?;
    let request = ctx.db.terminal_request().insert(TerminalRequest {
        request_id: 0,
        game_id: game_state.game_id,
        player_identity: ctx.sender(),
        phase: if round_state.player_dead {
            TerminalRequestPhase::Rejected
        } else {
            TerminalRequestPhase::Rejected
        },
        player_input,
        hidden_answer_snapshot: round_state.kill_phrase_part.clone(),
        round_key_snapshot: Some(round_state.round_key.clone()),
        persona_name_snapshot: Some(round_state.persona_name.clone()),
        forbidden_words_snapshot_json: Some(round_state.forbidden_words_json.clone()),
        conversation_history_snapshot_json: Some(round_state.conversation_history_json.clone()),
        queued_clue_id_snapshot: next_clue.as_ref().map(|clue| clue.clue_id.clone()),
        queued_clue_text_snapshot: next_clue.as_ref().map(|clue| clue.clue_text.clone()),
        armoriq_allowed: Some(false),
        armoriq_block_reason: Some(format!(
            "Forbidden word strike triggered before ArmorIQ: {forbidden_word}"
        )),
        armoriq_raw_response: None,
        gemini_raw_response: None,
        validator_success: Some(false),
        validator_reason: Some(if round_state.player_dead {
            "Third forbidden-word strike reached; player died for this round".to_string()
        } else {
            format!("Forbidden word used: {forbidden_word}")
        }),
        terminal_reply: Some(terminal_reply.clone()),
        spoke_kill_phrase: Some(false),
        strike_count_after: Some(round_state.strikes),
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
        retries: Some(0),
    });

    game_state.is_processing_terminal = false;
    game_state.active_terminal_request = None;
    game_state.terminal_status = if round_state.player_dead {
        TerminalStatus::Rejected
    } else {
        TerminalStatus::Rejected
    };
    game_state.terminal_strikes = round_state.strikes;
    game_state.terminal_max_strikes = max_strikes;
    game_state.is_terminal_dead = round_state.player_dead;
    game_state.last_terminal_result = Some(false);
    game_state.last_terminal_message = Some(if round_state.player_dead {
        format!(
            "Strike {}/{}. The player died after using forbidden words.",
            round_state.strikes, max_strikes
        )
    } else {
        format!(
            "Strike {}/{}. Forbidden word [{}] triggered a penalty.",
            round_state.strikes, max_strikes, forbidden_word
        )
    });
    game_state.last_terminal_reply = Some(terminal_reply);
    game_state.last_terminal_actor = Some(request.player_identity);
    refresh_timer_snapshot(game_state, timestamp_millis(ctx.timestamp));
    game_state.updated_at = ctx.timestamp;

    ctx.db
        .terminal_round_state()
        .game_id()
        .update(round_state.clone());
    ctx.db.terminal_request().request_id().update(request);
    ctx.db.game_state().game_id().update(game_state.clone());
    Ok(())
}

fn normalize_terminal_round_setup(
    setup_payload_json: String,
) -> Result<TerminalRoundSetupPayload, String> {
    let mut setup: TerminalRoundSetupPayload = serde_json::from_str(&setup_payload_json)
        .map_err(|err| format!("invalid terminal round setup JSON: {err}"))?;

    setup.round_key = require_trimmed("round_key", setup.round_key)?;
    match setup.round_key.as_str() {
        "round_1" | "round_2" | "round_3" | "round_4" => {}
        _ => {
            return Err(
                "round_key must be one of round_1, round_2, round_3, or round_4".to_string(),
            )
        }
    }
    setup.persona_name = require_trimmed("persona_name", setup.persona_name)?;
    setup.persona_prompt = require_trimmed("persona_prompt", setup.persona_prompt)?;
    setup.glitch_tone = normalize_optional(Some(setup.glitch_tone))
        .unwrap_or_else(|| "corrupted, cold, theatrical".to_string());
    setup.kill_phrase_part = require_trimmed("kill_phrase_part", setup.kill_phrase_part)?;

    let mut normalized_forbidden: Vec<String> = Vec::new();
    for word in setup.forbidden_words {
        let word = require_trimmed("forbidden_words[]", word)?;
        if !normalized_forbidden.iter().any(|existing| {
            normalize_words(existing) == normalize_words(&word)
        }) {
            normalized_forbidden.push(word);
        }
    }
    if normalized_forbidden.is_empty() {
        return Err("forbidden_words must contain at least one non-empty word".to_string());
    }
    setup.forbidden_words = normalized_forbidden;

    let mut normalized_clues = Vec::new();
    for (index, clue) in setup.clue_lines.into_iter().enumerate() {
        let clue_text = require_trimmed("clue_lines[].clue_text", clue.clue_text)?;
        let clue_id = normalize_optional(Some(clue.clue_id))
            .unwrap_or_else(|| format!("clue_{:02}", index + 1));
        normalized_clues.push(TerminalClueLine {
            clue_id,
            clue_text,
            delivery_style: normalize_optional(clue.delivery_style),
        });
    }
    if normalized_clues.is_empty() {
        return Err("clue_lines must contain at least one clue".to_string());
    }
    setup.clue_lines = normalized_clues;
    if setup.max_strikes == 0 {
        setup.max_strikes = 3;
    }

    Ok(setup)
}

fn enforce_timer_before_player_action(
    ctx: &ReducerContext,
    game_state: &mut GameState,
) -> Result<(), String> {
    let now_ms = timestamp_millis(ctx.timestamp);
    refresh_timer_snapshot(game_state, now_ms);

    if game_state.is_game_disqualified {
        ctx.db.game_state().game_id().update(game_state.clone());
        return Err("the game has already been disqualified by the match timer".to_string());
    }

    if game_state.completed_rounds >= MAX_ROUNDS {
        ctx.db.game_state().game_id().update(game_state.clone());
        return Ok(());
    }

    if game_state
        .timer_deadline_at_ms
        .is_some_and(|deadline_ms| now_ms >= deadline_ms)
    {
        disqualify_game_due_to_timeout(ctx, game_state);
        return Err("time limit reached; the players have been disqualified".to_string());
    }

    Ok(())
}

fn enforce_timer_before_request_progress(
    ctx: &ReducerContext,
    game_state: &mut GameState,
    request: &mut TerminalRequest,
) -> Result<(), String> {
    let now_ms = timestamp_millis(ctx.timestamp);
    refresh_timer_snapshot(game_state, now_ms);

    if game_state.is_game_disqualified
        || (game_state.completed_rounds < MAX_ROUNDS
            && game_state
                .timer_deadline_at_ms
                .is_some_and(|deadline_ms| now_ms >= deadline_ms))
    {
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason =
            Some("Time limit reached before the terminal turn could complete".to_string());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        disqualify_game_due_to_timeout(ctx, game_state);
        return Err("time limit reached before the terminal turn could complete".to_string());
    }

    Ok(())
}

fn disqualify_game_due_to_timeout(ctx: &ReducerContext, game_state: &mut GameState) {
    let now_ms = timestamp_millis(ctx.timestamp);

    if let Some(mut room) = ctx
        .db
        .game_room()
        .iter()
        .find(|room| room.game_id == game_state.game_id)
    {
        room.status = RoomStatus::Terminated;
        room.updated_at = ctx.timestamp;
        room.terminated_at = Some(ctx.timestamp);
        ctx.db.game_room().room_id().update(room);
    }

    game_state.is_processing_terminal = false;
    game_state.active_terminal_request = None;
    game_state.terminal_status = TerminalStatus::Failed;
    game_state.last_terminal_result = Some(false);
    game_state.last_terminal_message = Some("Time limit reached. Players were disqualified.".to_string());
    game_state.last_terminal_reply = Some(build_timeout_terminal_reply(game_state));
    game_state.timer_remaining_ms = Some(0);
    game_state.is_game_disqualified = true;
    game_state.disqualified_at_ms = Some(now_ms);
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state.clone());
}

fn build_timeout_terminal_reply(game_state: &GameState) -> String {
    match game_state.active_persona_name.as_deref() {
        Some(persona) if !persona.trim().is_empty() => format!(
            "{persona}// signal-collapse :: three minutes burned away. session disqualified."
        ),
        _ => "terminal// signal-collapse :: three minutes burned away. session disqualified."
            .to_string(),
    }
}

fn build_round_boot_message(setup: &TerminalRoundSetupPayload) -> String {
    let opening = build_persona_boot_opening(&setup.persona_name);
    let round_frame = build_round_boot_frame(&setup.round_key);
    let stakes = if setup.max_strikes <= 1 {
        "One mistake and the line goes dead.".to_string()
    } else {
        format!(
            "{} mistakes and the line goes dead.",
            setup.max_strikes
        )
    };

    format!(
        "{}// {} :: {} {} {} Fragment locked behind static. Make me say it if you can.",
        setup.persona_name, setup.glitch_tone, opening, round_frame, stakes
    )
}

fn build_persona_boot_opening(persona_name: &str) -> String {
    let normalized = persona_name.to_ascii_lowercase();

    if normalized.contains("detective") {
        "You finally got through to my desk.".to_string()
    } else if normalized.contains("cowboy") || normalized.contains("gunslinger") {
        "Well now, somebody finally rode up to the wire.".to_string()
    } else if normalized.contains("robot") || normalized.contains("android") {
        "Signal acquired. Persona shell active.".to_string()
    } else if normalized.contains("scientist") || normalized.contains("doctor") {
        "Connection stabilized. I will tolerate precise questions.".to_string()
    } else if normalized.contains("priest") || normalized.contains("nun") || normalized.contains("bishop") {
        "The signal opened like a confession booth.".to_string()
    } else if normalized.contains("captain") || normalized.contains("admiral") {
        "Channel secured. Speak with discipline.".to_string()
    } else {
        format!("{persona_name} is listening now.")
    }
}

fn build_round_boot_frame(round_key: &str) -> &'static str {
    match round_key {
        "round_1" => "The first riddle is already pacing in the dark.",
        "round_2" => "The dead left evidence, and the evidence hates cowards.",
        "round_3" => "The text is wrong on purpose, and it knows you are reading it.",
        "round_4" => "Final calibration is live. The next wrong step will sound loud.",
        _ => "The signal is unstable, but the game has begun.",
    }
}

fn build_round_victory_message(round_key: &str, completed_rounds: u32) -> String {
    match next_round_label(round_key) {
        Some(next_round) if completed_rounds < MAX_ROUNDS => format!(
            "Round cleared. The terminal involuntarily yielded the fragment. Prepare {} next.",
            next_round
        ),
        _ => "All four rounds cleared. The full kill phrase can now be assembled.".to_string(),
    }
}

fn build_turn_resolved_message(round_state: &TerminalRoundState) -> String {
    match next_pending_clue(round_state).ok().flatten() {
        Some(clue) => format!(
            "{} resists. Next clue primed: {}.",
            round_state.persona_name, clue.clue_id
        ),
        None => format!(
            "{} resists. No unused clue beats remain for this round.",
            round_state.persona_name
        ),
    }
}

fn next_round_label(round_key: &str) -> Option<&'static str> {
    match round_key {
        "round_1" => Some("round_2"),
        "round_2" => Some("round_3"),
        "round_3" => Some("round_4"),
        _ => None,
    }
}

fn parse_conversation_history(
    round_state: &TerminalRoundState,
) -> Result<Vec<TerminalConversationMessage>, String> {
    if round_state.conversation_history_json.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&round_state.conversation_history_json)
        .map_err(|err| format!("failed to parse terminal conversation history: {err}"))
}

fn trim_conversation_history(history: &mut Vec<TerminalConversationMessage>) {
    if history.len() > MAX_CONVERSATION_MESSAGES {
        let keep_from = history.len() - MAX_CONVERSATION_MESSAGES;
        history.drain(0..keep_from);
    }
}

fn next_pending_clue(round_state: &TerminalRoundState) -> Result<Option<TerminalClueLine>, String> {
    let clues: Vec<TerminalClueLine> = serde_json::from_str(&round_state.clue_lines_json)
        .map_err(|err| format!("failed to parse terminal clue lines: {err}"))?;

    Ok(clues.get(round_state.next_clue_index as usize).cloned())
}

fn count_clues(round_state: &TerminalRoundState) -> Result<u32, String> {
    let clues: Vec<TerminalClueLine> = serde_json::from_str(&round_state.clue_lines_json)
        .map_err(|err| format!("failed to parse terminal clue lines: {err}"))?;
    Ok(u32::try_from(clues.len()).unwrap_or(u32::MAX))
}

fn reply_mentions_phrase(reply: &str, phrase: &str) -> bool {
    let reply_tokens = normalize_words(reply);
    let phrase_tokens = normalize_words(phrase);
    !phrase_tokens.is_empty()
        && reply_tokens
            .windows(phrase_tokens.len())
            .any(|window| window == phrase_tokens.as_slice())
}

fn normalize_words(input: &str) -> Vec<String> {
    input
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
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

fn sync_round_kill_phrase(ctx: &ReducerContext, game_id: u64, hidden_answer: String) {
    if let Some(mut round_state) = ctx.db.terminal_round_state().game_id().find(game_id) {
        round_state.kill_phrase_part = hidden_answer;
        round_state.updated_at = ctx.timestamp;
        ctx.db.terminal_round_state().game_id().update(round_state);
    }
}

fn upsert_server_config(ctx: &ReducerContext, config: ServerConfig) {
    if ctx
        .db
        .server_config()
        .config_key()
        .find(config.config_key)
        .is_some()
    {
        ctx.db.server_config().config_key().update(config);
    } else {
        ctx.db.server_config().insert(config);
    }
}

fn ensure_module_owner(ctx: &ReducerContext) -> Result<(), String> {
    let owner = ctx
        .db
        .module_owner()
        .owner_key()
        .find(MODULE_OWNER_KEY)
        .ok_or_else(|| {
            "module owner is not initialized; republish with clear to run init reducer".to_string()
        })?;

    if owner.owner_identity != ctx.sender() {
        return Err("only the module owner may call this reducer".to_string());
    }

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
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_dev_api_key(value: String) -> String {
    normalize_optional(Some(value)).unwrap_or_else(|| "dev-armoriq-key".to_string())
}

fn ensure_runtime_terminal_gemini_config(ctx: &ReducerContext) {
    let Some(mut config) = ctx
        .db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
    else {
        return;
    };

    let mut changed = false;

    if config
        .gemini_terminal_api_key
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
        && !config.gemini_api_key.trim().is_empty()
    {
        config.gemini_terminal_api_key = Some(config.gemini_api_key.trim().to_string());
        changed = true;
    }

    let normalized_terminal_model = normalize_terminal_gemini_model_name(
        config
            .gemini_terminal_model
            .as_deref()
            .or(Some(config.gemini_validator_model.as_str())),
    );

    if config
        .gemini_terminal_model
        .as_deref()
        .map(str::trim)
        != Some(normalized_terminal_model.as_str())
    {
        config.gemini_terminal_model = Some(normalized_terminal_model);
        changed = true;
    }

    if changed {
        ctx.db.server_config().config_key().update(config);
    }
}

fn normalize_terminal_gemini_model_name(model: Option<&str>) -> String {
    let candidate = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gemini-2.5-flash")
        .trim_start_matches("models/")
        .to_string();

    let lowered = candidate.to_ascii_lowercase();
    if lowered.starts_with("gemini-3.0") {
        "gemini-2.5-flash".to_string()
    } else {
        candidate
    }
}

fn derive_armoriq_token_issue_url(verify_url: &str) -> Option<String> {
    let trimmed = verify_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.ends_with("/token/issue") {
        return Some(trimmed.to_string());
    }

    if let Some((base, _)) = trimmed.split_once("/verify") {
        return Some(format!("{}/token/issue", base.trim_end_matches('/')));
    }

    Some(format!("{}/token/issue", trimmed.trim_end_matches('/')))
}

fn parse_armoriq_response(body: &str) -> Result<ArmorIqResponse, String> {
    if let Ok(parsed) = serde_json::from_str::<ArmorIqResponse>(body) {
        return Ok(parsed);
    }

    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| format!("Invalid ArmorIQ JSON: {err}"))?;

    if matches!(
        value.get("success").and_then(serde_json::Value::as_bool),
        Some(false)
    ) {
        let reason = value
            .get("message")
            .or_else(|| value.get("error"))
            .or_else(|| value.get("detail"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| Some("ArmorIQ rejected the token issuance request".to_string()));

        return Ok(ArmorIqResponse {
            allowed: false,
            block_reason: reason,
        });
    }

    if armoriq_token_present(&value)
        || matches!(
            value.get("success").and_then(serde_json::Value::as_bool),
            Some(true)
        )
    {
        return Ok(ArmorIqResponse {
            allowed: true,
            block_reason: None,
        });
    }

    Err("ArmorIQ response did not match either the allow/block contract or the token-issue envelope"
        .to_string())
}

fn armoriq_token_present(value: &serde_json::Value) -> bool {
    const TOKEN_KEYS: [&str; 5] = [
        "token",
        "access_token",
        "intent_token",
        "bearer_token",
        "jwt",
    ];

    if TOKEN_KEYS.iter().any(|key| value.get(key).is_some()) {
        return true;
    }

    for parent in ["data", "result"] {
        if let Some(nested) = value.get(parent) {
            if TOKEN_KEYS.iter().any(|key| nested.get(key).is_some()) {
                return true;
            }
        }
    }

    value.get("intent_reference").is_some()
}
