use serde_json::{json, Value};
use spacetimedb::{log, ProcedureContext, ReducerContext, Table};

use crate::models::api_schemas::{
    ArmorIqContext, ArmorIqRequest, GeminiContent, GeminiGenerateContentRequest,
    GeminiGenerateContentResponse, GeminiGenerationConfig, GeminiPart,
    RelayTerminalValidatorRequest,
};
use crate::tables::state::{
    armoriq_callback_schedule, armoriq_request_schedule, game_state,
    gemini_validator_callback_schedule, gemini_validator_request_schedule, server_config,
    terminal_request, ArmoriqCallbackSchedule, ArmoriqRequestSchedule,
    GeminiValidatorCallbackSchedule, GeminiValidatorRequestSchedule, ServerConfig,
    TerminalRequest, TerminalRequestPhase, TerminalStatus, ACTIVE_SERVER_CONFIG_KEY,
};

/// Reducer-side helper that enqueues the non-blocking ArmorIQ verification hop.
///
/// Reducers in current SpacetimeDB releases cannot perform HTTP I/O directly, so this helper
/// persists a schedule row for the outbound procedure that will execute the request.
pub fn verify_with_armoriq(
    ctx: &ReducerContext,
    request_id: u64,
    _input: String,
    _hidden_answer: String,
) -> Result<(), String> {
    ctx.db
        .armoriq_request_schedule()
        .insert(ArmoriqRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

/// Reducer-side helper that enqueues the non-blocking Gemini validator hop.
pub fn queue_gemini_validator(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .gemini_validator_request_schedule()
        .insert(GeminiValidatorRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

/// Scheduled procedure that performs the outbound ArmorIQ POST request.
#[spacetimedb::procedure]
pub fn process_armoriq_request(ctx: &mut ProcedureContext, job: ArmoriqRequestSchedule) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_armoriq_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .armoriq_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_armoriq_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            fail_request_without_callback(ctx, job.request_id, err);
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_armoriq_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_armoriq_callback(ctx, request_id, 0, String::new(), Some(err.to_string()));
        }
    }
}

/// Scheduled procedure that performs the outbound Gemini validator POST request.
#[spacetimedb::procedure]
pub fn process_gemini_validator_request(
    ctx: &mut ProcedureContext,
    job: GeminiValidatorRequestSchedule,
) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_gemini_validator_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .gemini_validator_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_gemini_validator_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            fail_request_without_callback(ctx, job.request_id, err);
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_gemini_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_gemini_callback(ctx, request_id, 0, String::new(), Some(err.to_string()));
        }
    }
}

/// Extracts the first text part from a Gemini `generateContent` response envelope.
pub fn extract_gemini_text(body: &str) -> Result<String, String> {
    let envelope: GeminiGenerateContentResponse =
        serde_json::from_str(body).map_err(|err| format!("failed to parse Gemini envelope: {err}"))?;

    envelope
        .candidates
        .into_iter()
        .flat_map(|candidate| candidate.content.parts.into_iter())
        .find_map(|part| part.text)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "Gemini response did not contain a text candidate".to_string())
}

fn prepare_armoriq_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .terminal_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("terminal request {request_id} no longer exists"))?;

    let config = load_server_config(tx)?;
    let http_request = build_armoriq_http_request(
        &config,
        request.player_input.clone(),
        request.hidden_answer_snapshot.clone(),
    )?;

    Ok((request.request_id, http_request))
}

fn prepare_gemini_validator_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .terminal_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("terminal request {request_id} no longer exists"))?;

    if request.phase != TerminalRequestPhase::PendingGeminiValidator {
        return Err(format!(
            "terminal request {request_id} is not ready for Gemini validation"
        ));
    }

    let config = load_server_config(tx)?;
    let http_request = build_gemini_validator_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn load_server_config(tx: &spacetimedb::TxContext) -> Result<ServerConfig, String> {
    tx.db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "ServerConfig row {} is missing; seed integration settings before using terminal validation",
                ACTIVE_SERVER_CONFIG_KEY
            )
        })
}

fn build_armoriq_http_request(
    config: &ServerConfig,
    input: String,
    hidden_answer: String,
) -> Result<spacetimedb::http::Request<String>, String> {
    require_non_empty("ServerConfig.armoriq_verify_url", &config.armoriq_verify_url)?;
    require_non_empty(
        "ServerConfig.armoriq_api_key_header",
        &config.armoriq_api_key_header,
    )?;
    require_non_empty("ServerConfig.armoriq_api_key", &config.armoriq_api_key)?;

    let payload = ArmorIqRequest {
        player_input: input,
        action: "terminal_override".to_string(),
        context: ArmorIqContext { hidden_answer },
    };

    let body = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize ArmorIQ payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(config.armoriq_verify_url.as_str())
        .header("Content-Type", "application/json")
        .header(
            config.armoriq_api_key_header.as_str(),
            config.armoriq_api_key.as_str(),
        )
        .body(body)
        .map_err(|err| format!("failed to build ArmorIQ request: {err}"))
}

fn build_gemini_validator_http_request(
    config: &ServerConfig,
    request: &TerminalRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    if let Some(relay_base_url) = config
        .local_llm_relay_base_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return build_local_relay_validator_request(relay_base_url, request);
    }

    require_non_empty("ServerConfig.gemini_api_base_url", &config.gemini_api_base_url)?;
    require_non_empty("ServerConfig.gemini_api_key", &config.gemini_api_key)?;
    require_non_empty(
        "ServerConfig.gemini_validator_model",
        &config.gemini_validator_model,
    )?;

    let prompt = format!(
        concat!(
            "You are the terminal validation agent for an asymmetric multiplayer game.\n",
            "Evaluate whether the player's terminal input should count as a successful resolution ",
            "against the hidden answer.\n",
            "Return JSON only with the schema {{\"success\": boolean, \"reason\": string}}.\n\n",
            "Player input:\n{}\n\n",
            "Hidden answer:\n{}\n"
        ),
        request.player_input,
        request.hidden_answer_snapshot
    );

    let payload = GeminiGenerateContentRequest {
        contents: vec![GeminiContent {
            role: Some("user".to_string()),
            parts: vec![GeminiPart {
                text: Some(prompt),
            }],
        }],
        generation_config: GeminiGenerationConfig {
            response_mime_type: "application/json".to_string(),
            response_json_schema: gemini_validator_response_schema(),
            candidate_count: 1,
            max_output_tokens: 256,
            temperature: 0.0,
        },
    };

    let url = format!(
        "{}/models/{}:generateContent?key={}",
        config.gemini_api_base_url.trim_end_matches('/'),
        config.gemini_validator_model,
        config.gemini_api_key,
    );

    let body = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize Gemini validator payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|err| format!("failed to build Gemini validator request: {err}"))
}

fn build_local_relay_validator_request(
    relay_base_url: &str,
    request: &TerminalRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    let payload = RelayTerminalValidatorRequest {
        player_input: request.player_input.clone(),
        hidden_answer: request.hidden_answer_snapshot.clone(),
    };

    let url = format!(
        "{}/api/gemini/terminal-validator",
        relay_base_url.trim_end_matches('/')
    );

    let body = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize relay validator payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|err| format!("failed to build relay validator request: {err}"))
}

fn gemini_validator_response_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "success": { "type": "boolean" },
            "reason": { "type": "string" }
        },
        "required": ["success", "reason"]
    })
}

fn enqueue_armoriq_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .armoriq_callback_schedule()
            .insert(ArmoriqCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn enqueue_gemini_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .gemini_validator_callback_schedule()
            .insert(GeminiValidatorCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn fail_request_without_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    message: String,
) {
    log::error!("terminal request {} failed before callback dispatch: {}", request_id, message);

    ctx.with_tx(|tx| {
        if let Some(mut request) = tx.db.terminal_request().request_id().find(request_id) {
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(message.clone());
            request.updated_at = tx.timestamp;
            tx.db.terminal_request().request_id().update(request.clone());

            if let Some(mut game_state) = tx.db.game_state().game_id().find(request.game_id) {
                if game_state.active_terminal_request == Some(request_id) {
                    game_state.is_processing_terminal = false;
                    game_state.active_terminal_request = None;
                    game_state.terminal_status = TerminalStatus::Failed;
                    game_state.last_terminal_result = Some(false);
                    game_state.last_terminal_message = Some(message.clone());
                    game_state.last_terminal_actor = Some(request.player_identity);
                    game_state.updated_at = tx.timestamp;
                    tx.db.game_state().game_id().update(game_state);
                }
            }
            return;
        }

        for mut game_state in tx.db.game_state().iter() {
            if game_state.active_terminal_request == Some(request_id) {
                game_state.is_processing_terminal = false;
                game_state.active_terminal_request = None;
                game_state.terminal_status = TerminalStatus::Failed;
                game_state.last_terminal_result = Some(false);
                game_state.last_terminal_message = Some(message.clone());
                game_state.updated_at = tx.timestamp;
                tx.db.game_state().game_id().update(game_state);
            }
        }
    });
}

fn require_non_empty(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}
