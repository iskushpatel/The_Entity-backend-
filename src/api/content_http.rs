use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use spacetimedb::{log, ProcedureContext, ReducerContext, Table};

use crate::api::http_wrappers::extract_gemini_text;
use crate::models::api_schemas::{
    GeminiContent, GeminiGenerateContentRequest, GeminiGenerationConfig, GeminiPart,
    VillainSpeechGenerationPayload,
};
use crate::tables::state::{
    round_generation_callback_schedule, round_generation_request,
    round_generation_request_schedule, server_config, villain_speech_callback_schedule,
    villain_speech_request, villain_speech_request_schedule, villain_tts_callback_schedule,
    villain_tts_request_schedule, voice_config, RoundGenerationCallbackSchedule,
    RoundGenerationPhase, RoundGenerationRequest, RoundGenerationRequestSchedule, ServerConfig,
    VillainSpeechCallbackSchedule, VillainSpeechPhase, VillainSpeechRequest,
    VillainSpeechRequestSchedule, VillainTtsCallbackSchedule, VillainTtsRequestSchedule,
    VoiceConfig, ACTIVE_SERVER_CONFIG_KEY, ACTIVE_VOICE_CONFIG_KEY,
};

pub fn queue_round_generation_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .round_generation_request_schedule()
        .insert(RoundGenerationRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn queue_villain_speech_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .villain_speech_request_schedule()
        .insert(VillainSpeechRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn queue_villain_tts_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .villain_tts_request_schedule()
        .insert(VillainTtsRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn normalize_round_key(round_key: &str) -> Result<String, String> {
    let normalized = round_key.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "round_1" | "round_2" | "round_3" | "round_4" => Ok(normalized),
        _ => Err(format!(
            "round_key must be one of round_1, round_2, round_3, or round_4; received {}",
            round_key
        )),
    }
}

pub fn default_round_response_schema(round_key: &str) -> Result<Value, String> {
    match normalize_round_key(round_key)?.as_str() {
        "round_1" => Ok(round_one_final_response_schema()),
        "round_2" | "round_3" | "round_4" => Ok(json!({
            "type": "object",
            "additionalProperties": true
        })),
        _ => unreachable!(),
    }
}

fn round_one_clue_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "clue_id": { "type": "string" },
            "clue_type": { "type": "string" },
            "clue_text": { "type": "string", "minLength": 80 },
            "required_manual_refs": {
                "type": "array",
                "minItems": 2,
                "items": { "type": "string" }
            },
            "expected_inference": { "type": "string", "minLength": 60 },
            "difficulty": { "type": "string" }
        },
        "required": [
            "clue_id",
            "clue_type",
            "clue_text",
            "required_manual_refs",
            "expected_inference",
            "difficulty"
        ]
    })
}

fn round_one_walkthrough_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "step_id": { "type": "string" },
            "clue_id": { "type": "string" },
            "manual_refs_used": {
                "type": "array",
                "minItems": 2,
                "items": { "type": "string" }
            },
            "deduction": { "type": "string", "minLength": 140 },
            "phrase_progression": { "type": "string", "minLength": 90 }
        },
        "required": ["step_id", "clue_id", "manual_refs_used", "deduction", "phrase_progression"]
    })
}

fn round_one_solution_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "final_identity_guess": { "type": "string" },
            "final_target_word_inference": { "type": "string" },
            "why_target_word_fits": { "type": "string", "minLength": 120 },
            "how_manual_reveals_word": { "type": "string", "minLength": 140 }
        },
        "required": [
            "final_identity_guess",
            "final_target_word_inference",
            "why_target_word_fits",
            "how_manual_reveals_word"
        ]
    })
}

fn round_one_manual_section_blueprint_schema(id_key: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            id_key: { "type": "string" },
            "title": { "type": "string" },
            "purpose": { "type": "string", "minLength": 30 },
            "linked_clue_ids": {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string" }
            },
            "hidden_phrase_function": { "type": "string", "minLength": 40 }
        },
        "required": [id_key, "title", "purpose", "linked_clue_ids", "hidden_phrase_function"]
    })
}

fn round_one_signal_thread_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "thread_id": { "type": "string" },
            "linked_clue_ids": {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string" }
            },
            "manual_refs": {
                "type": "array",
                "minItems": 2,
                "items": { "type": "string" }
            },
            "hidden_phrase_signal": { "type": "string", "minLength": 90 },
            "narrative_bridge": { "type": "string", "minLength": 140 }
        },
        "required": [
            "thread_id",
            "linked_clue_ids",
            "manual_refs",
            "hidden_phrase_signal",
            "narrative_bridge"
        ]
    })
}

fn round_one_skeleton_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "persona_name": { "type": "string" },
            "persona_paragraphs": {
                "type": "array",
                "minItems": 2,
                "items": { "type": "string" }
            },
            "target_word": { "type": "string" },
            "forbidden_words": {
                "type": "array",
                "minItems": 5,
                "items": { "type": "string" }
            },
            "clues": {
                "type": "array",
                "minItems": 4,
                "items": round_one_clue_schema()
            },
            "manual_blueprint": {
                "type": "object",
                "properties": {
                    "overview": { "type": "string", "minLength": 120 },
                    "hidden_phrase_bridge": { "type": "string", "minLength": 90 },
                    "codex_entries": {
                        "type": "array",
                        "minItems": 6,
                        "items": round_one_manual_section_blueprint_schema("entry_id")
                    },
                    "timeline_fragments": {
                        "type": "array",
                        "minItems": 4,
                        "items": round_one_manual_section_blueprint_schema("fragment_id")
                    },
                    "cipher_legend": {
                        "type": "array",
                        "minItems": 4,
                        "items": round_one_manual_section_blueprint_schema("cipher_id")
                    },
                    "protocol_matrix": {
                        "type": "array",
                        "minItems": 5,
                        "items": round_one_manual_section_blueprint_schema("protocol_id")
                    },
                    "false_leads": {
                        "type": "array",
                        "minItems": 3,
                        "items": round_one_manual_section_blueprint_schema("lead_id")
                    },
                    "signal_threads": {
                        "type": "array",
                        "minItems": 4,
                        "items": round_one_manual_section_blueprint_schema("thread_id")
                    }
                },
                "required": [
                    "overview",
                    "hidden_phrase_bridge",
                    "codex_entries",
                    "timeline_fragments",
                    "cipher_legend",
                    "protocol_matrix",
                    "false_leads",
                    "signal_threads"
                ]
            },
            "solution": round_one_solution_schema()
        },
        "required": [
            "persona_name",
            "persona_paragraphs",
            "target_word",
            "forbidden_words",
            "clues",
            "manual_blueprint",
            "solution"
        ]
    })
}

fn round_one_manual_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "overview": { "type": "string", "minLength": 260 },
            "hidden_phrase_bridge": { "type": "string", "minLength": 180 },
            "section_usage_notes": { "type": "string", "minLength": 160 },
            "codex_entries": {
                "type": "array",
                "minItems": 6,
                "items": {
                    "type": "object",
                    "properties": {
                        "entry_id": { "type": "string" },
                        "domain": { "type": "string" },
                        "term": { "type": "string" },
                        "content": { "type": "string", "minLength": 320 },
                        "relevance_tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "linked_clue_ids": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        },
                        "hidden_phrase_signal": { "type": "string", "minLength": 90 }
                    },
                    "required": [
                        "entry_id",
                        "domain",
                        "term",
                        "content",
                        "relevance_tags",
                        "linked_clue_ids",
                        "hidden_phrase_signal"
                    ]
                }
            },
            "timeline_fragments": {
                "type": "array",
                "minItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "fragment_id": { "type": "string" },
                        "timestamp_hint": { "type": "string" },
                        "event_summary": { "type": "string" },
                        "detail_text": { "type": "string", "minLength": 240 },
                        "linked_entities": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "linked_clue_ids": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        },
                        "hidden_phrase_signal": { "type": "string", "minLength": 90 }
                    },
                    "required": [
                        "fragment_id",
                        "timestamp_hint",
                        "event_summary",
                        "detail_text",
                        "linked_entities",
                        "linked_clue_ids",
                        "hidden_phrase_signal"
                    ]
                }
            },
            "cipher_legend": {
                "type": "array",
                "minItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "cipher_id": { "type": "string" },
                        "symbol_or_pattern": { "type": "string" },
                        "decoding_rule": { "type": "string" },
                        "expanded_note": { "type": "string", "minLength": 180 },
                        "example": { "type": "string", "minLength": 80 },
                        "linked_clue_ids": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        },
                        "hidden_phrase_signal": { "type": "string", "minLength": 90 }
                    },
                    "required": [
                        "cipher_id",
                        "symbol_or_pattern",
                        "decoding_rule",
                        "expanded_note",
                        "example",
                        "linked_clue_ids",
                        "hidden_phrase_signal"
                    ]
                }
            },
            "protocol_matrix": {
                "type": "array",
                "minItems": 5,
                "items": {
                    "type": "object",
                    "properties": {
                        "protocol_id": { "type": "string" },
                        "trigger_condition": { "type": "string" },
                        "prescribed_action": { "type": "string" },
                        "hidden_implication": { "type": "string" },
                        "detail_text": { "type": "string", "minLength": 220 },
                        "linked_clue_ids": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        },
                        "hidden_phrase_signal": { "type": "string", "minLength": 90 }
                    },
                    "required": [
                        "protocol_id",
                        "trigger_condition",
                        "prescribed_action",
                        "hidden_implication",
                        "detail_text",
                        "linked_clue_ids",
                        "hidden_phrase_signal"
                    ]
                }
            },
            "false_leads": {
                "type": "array",
                "minItems": 3,
                "items": {
                    "type": "object",
                    "properties": {
                        "lead_id": { "type": "string" },
                        "misleading_claim": { "type": "string" },
                        "why_it_looks_valid": { "type": "string" },
                        "why_it_is_wrong": { "type": "string" },
                        "misdirects_clue_ids": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        },
                        "discarded_by_manual_refs": {
                            "type": "array",
                            "minItems": 1,
                            "items": { "type": "string" }
                        }
                    },
                    "required": [
                        "lead_id",
                        "misleading_claim",
                        "why_it_looks_valid",
                        "why_it_is_wrong",
                        "misdirects_clue_ids",
                        "discarded_by_manual_refs"
                    ]
                }
            },
            "signal_threads": {
                "type": "array",
                "minItems": 4,
                "items": round_one_signal_thread_schema()
            }
        },
        "required": [
            "overview",
            "hidden_phrase_bridge",
            "section_usage_notes",
            "codex_entries",
            "timeline_fragments",
            "cipher_legend",
            "protocol_matrix",
            "false_leads",
            "signal_threads"
        ]
    })
}

fn round_one_expansion_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "manual": round_one_manual_schema(),
            "decoder_walkthrough": {
                "type": "array",
                "minItems": 4,
                "items": round_one_walkthrough_schema()
            }
        },
        "required": ["manual", "decoder_walkthrough"]
    })
}

fn round_one_final_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "persona_name": { "type": "string" },
            "persona_paragraphs": {
                "type": "array",
                "minItems": 2,
                "items": { "type": "string" }
            },
            "target_word": { "type": "string" },
            "forbidden_words": {
                "type": "array",
                "minItems": 5,
                "items": { "type": "string" }
            },
            "clues": {
                "type": "array",
                "minItems": 4,
                "items": round_one_clue_schema()
            },
            "manual": round_one_manual_schema(),
            "decoder_walkthrough": {
                "type": "array",
                "minItems": 4,
                "items": round_one_walkthrough_schema()
            },
            "solution": round_one_solution_schema()
        },
        "required": [
            "persona_name",
            "persona_paragraphs",
            "target_word",
            "forbidden_words",
            "clues",
            "manual",
            "decoder_walkthrough",
            "solution"
        ]
    })
}

#[spacetimedb::procedure]
pub fn process_round_content_request(
    ctx: &mut ProcedureContext,
    job: RoundGenerationRequestSchedule,
) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_round_content_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .round_generation_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_round_content_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_round_content_callback(ctx, job.request_id, 0, String::new(), Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_round_content_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_round_content_callback(
                ctx,
                request_id,
                0,
                String::new(),
                Some(err.to_string()),
            );
        }
    }
}

#[spacetimedb::procedure]
pub fn process_villain_speech_request(
    ctx: &mut ProcedureContext,
    job: VillainSpeechRequestSchedule,
) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_villain_speech_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .villain_speech_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_villain_speech_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_villain_speech_callback(ctx, job.request_id, 0, String::new(), Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_villain_speech_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_villain_speech_callback(
                ctx,
                request_id,
                0,
                String::new(),
                Some(err.to_string()),
            );
        }
    }
}

#[spacetimedb::procedure]
pub fn process_villain_tts_request(ctx: &mut ProcedureContext, job: VillainTtsRequestSchedule) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_villain_tts_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .villain_tts_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_villain_tts_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_villain_tts_callback(ctx, job.request_id, 0, String::new(), None, Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            let mime_type = parts
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let body_base64 = BASE64_STANDARD.encode(body.into_bytes());
            enqueue_villain_tts_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body_base64,
                mime_type,
                None,
            );
        }
        Err(err) => {
            enqueue_villain_tts_callback(
                ctx,
                request_id,
                0,
                String::new(),
                None,
                Some(err.to_string()),
            );
        }
    }
}

fn prepare_round_content_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .round_generation_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("round generation request {} no longer exists", request_id))?;

    let config = load_server_config(tx)?;
    let http_request = build_round_content_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn prepare_villain_speech_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .villain_speech_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("villain speech request {} no longer exists", request_id))?;

    let config = load_server_config(tx)?;
    let http_request = build_villain_speech_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn prepare_villain_tts_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .villain_speech_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("villain speech request {} no longer exists", request_id))?;

    if request.phase != VillainSpeechPhase::PendingTts {
        return Err(format!(
            "villain speech request {} is not ready for TTS",
            request_id
        ));
    }

    let config = load_voice_config(tx)?;
    let http_request = build_villain_tts_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn load_server_config(tx: &spacetimedb::TxContext) -> Result<ServerConfig, String> {
    tx.db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "ServerConfig row {} is missing; configure Gemini integrations before generating content",
                ACTIVE_SERVER_CONFIG_KEY
            )
        })
}

fn load_voice_config(tx: &spacetimedb::TxContext) -> Result<VoiceConfig, String> {
    tx.db
        .voice_config()
        .config_key()
        .find(ACTIVE_VOICE_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "VoiceConfig row {} is missing; configure ElevenLabs before synthesizing villain audio",
                ACTIVE_VOICE_CONFIG_KEY
            )
        })
}

fn build_round_content_http_request(
    config: &ServerConfig,
    request: &RoundGenerationRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    require_non_empty(
        "ServerConfig.gemini_api_base_url",
        &config.gemini_api_base_url,
    )?;
    require_non_empty("ServerConfig.gemini_api_key", &config.gemini_api_key)?;
    require_non_empty(
        "ServerConfig.gemini_clue_generator_model",
        &config.gemini_clue_generator_model,
    )?;

    let prompt = build_round_generation_prompt(request)?;
    let max_tokens = round_generation_max_output_tokens(request);
    let response_schema = round_generation_response_schema(request)?;

    build_gemini_request(
        &config.gemini_api_base_url,
        &config.gemini_api_key,
        &config.gemini_clue_generator_model,
        prompt,
        response_schema,
        round_generation_temperature(request),
        max_tokens,
    )
}

fn build_villain_speech_http_request(
    config: &ServerConfig,
    request: &VillainSpeechRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    require_non_empty(
        "ServerConfig.gemini_api_base_url",
        &config.gemini_api_base_url,
    )?;
    require_non_empty("ServerConfig.gemini_api_key", &config.gemini_api_key)?;
    require_non_empty(
        "ServerConfig.gemini_villain_model",
        &config.gemini_villain_model,
    )?;

    let payload: VillainSpeechGenerationPayload =
        serde_json::from_str(&request.request_payload_json)
            .map_err(|err| format!("invalid villain speech payload JSON: {err}"))?;

    let prompt = build_villain_speech_prompt(&payload, &request.request_payload_json);

    build_gemini_request(
        &config.gemini_api_base_url,
        &config.gemini_api_key,
        &config.gemini_villain_model,
        prompt,
        Some(villain_speech_response_schema()),
        0.6,
        2048,
    )
}

fn build_villain_tts_http_request(
    config: &VoiceConfig,
    request: &VillainSpeechRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    let payload: VillainSpeechGenerationPayload =
        serde_json::from_str(&request.request_payload_json)
            .map_err(|err| format!("invalid villain speech payload JSON: {err}"))?;

    require_non_empty(
        "VoiceConfig.elevenlabs_api_base_url",
        &config.elevenlabs_api_base_url,
    )?;
    require_non_empty("VoiceConfig.elevenlabs_api_key", &config.elevenlabs_api_key)?;

    let voice_id = payload
        .voice_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.elevenlabs_default_voice_id.clone());
    require_non_empty("Voice voice_id", &voice_id)?;

    let model_id = payload
        .voice_model_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.elevenlabs_default_model_id.clone());
    require_non_empty("Voice model_id", &model_id)?;

    let text = request
        .selected_speech_text
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "selected villain speech text is missing for TTS".to_string())?;

    let url = format!(
        "{}/text-to-speech/{}",
        config.elevenlabs_api_base_url.trim_end_matches('/'),
        voice_id
    );
    let body = serde_json::to_string(&json!({
        "text": text,
        "model_id": model_id,
    }))
    .map_err(|err| format!("failed to serialize ElevenLabs payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .header("Accept", "audio/mpeg")
        .header("xi-api-key", config.elevenlabs_api_key.as_str())
        .body(body)
        .map_err(|err| format!("failed to build villain TTS request: {err}"))
}

fn build_gemini_request(
    base_url: &str,
    api_key: &str,
    model: &str,
    prompt: String,
    response_schema: Option<Value>,
    temperature: f32,
    max_output_tokens: u32,
) -> Result<spacetimedb::http::Request<String>, String> {
    let payload = GeminiGenerateContentRequest {
        contents: vec![GeminiContent {
            role: Some("user".to_string()),
            parts: vec![GeminiPart { text: Some(prompt) }],
        }],
        generation_config: GeminiGenerationConfig {
            response_mime_type: "application/json".to_string(),
            response_json_schema: response_schema,
            candidate_count: 1,
            max_output_tokens,
            temperature,
            thinking_config: None,
        },
    };

    let url = format!(
        "{}/models/{}:generateContent?key={}",
        base_url.trim_end_matches('/'),
        model,
        api_key,
    );

    let body = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize Gemini request payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|err| format!("failed to build Gemini HTTP request: {err}"))
}

fn round_generation_response_schema(request: &RoundGenerationRequest) -> Result<Option<Value>, String> {
    match request.round_key.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => Ok(Some(round_one_skeleton_response_schema())),
            RoundGenerationPhase::PendingGeminiExpansion => Ok(Some(round_one_expansion_response_schema())),
            RoundGenerationPhase::Failed | RoundGenerationPhase::Succeeded => {
                Ok(Some(load_request_response_schema(request)?))
            }
        },
        "round_2" | "round_3" | "round_4" => Ok(Some(load_request_response_schema(request)?)),
        _ => Ok(None),
    }
}

fn load_request_response_schema(request: &RoundGenerationRequest) -> Result<Value, String> {
    serde_json::from_str(&request.response_schema_json).map_err(|err| {
        format!(
            "failed to parse stored response schema for round {}: {err}",
            request.round_key
        )
    })
}

fn round_generation_temperature(request: &RoundGenerationRequest) -> f32 {
    match request.round_key.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => 0.65,
            RoundGenerationPhase::PendingGeminiExpansion => 0.78,
            _ => 0.78,
        },
        "round_2" => 0.8,
        "round_3" => 0.75,
        "round_4" => 0.55,
        _ => 0.7,
    }
}

fn round_generation_max_output_tokens(request: &RoundGenerationRequest) -> u32 {
    match request.round_key.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => 5600,
            RoundGenerationPhase::PendingGeminiExpansion => 8192,
            _ => 8192,
        },
        "round_2" => 2200,
        "round_3" => 2200,
        "round_4" => 1600,
        _ => 1800,
    }
}

fn build_round_generation_prompt(request: &RoundGenerationRequest) -> Result<String, String> {
    let payload_value: Value = serde_json::from_str(&request.request_payload_json)
        .map_err(|err| format!("invalid request_payload_json: {err}"))?;

    match normalize_round_key(&request.round_key)?.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => build_round_one_skeleton_prompt(&payload_value),
            RoundGenerationPhase::PendingGeminiExpansion => build_round_one_expansion_prompt(&payload_value, request.skeleton_payload_json.as_deref()),
            _ => Err("Invalid phase for round 1 generation".to_string()),
        },
        "round_2" => Ok([
            "You are an expert game designer creating Round 2 content for an asymmetrical deduction game.",
            "Generate a complete clue and manual package for the requested Round 2 structure.",
            "Round 2 should center on corrupted incident logs, post-mortem evidence, branching logic, and a hidden answer that can only be solved by combining the Player 1 material with the Player 2 manual.",
            "The clue material must be atmospheric but internally consistent.",
            "The manual must be explicit, usable under pressure, and sufficient to derive the validation answer from the clue material.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        "round_3" => Ok([
            "You are an expert game designer creating Round 3 content for an asymmetrical deduction game.",
            "Generate a thematic cipher round where Player 1 receives a dense structural text block and Player 2 receives a manual containing parsing rules, branching logic, or procedural extraction rules.",
            "The final answer must be solvable from the physical structure of the Player 1 text, not from vague interpretation alone.",
            "The manual must be rigorous enough for two players to collaborate under time pressure.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        "round_4" => Ok([
            "You are an expert game designer creating Round 4 content for an asymmetrical deduction game.",
            "Generate a high-pressure final calibration round built around short, hostile, mechanically precise clue-and-manual logic.",
            "If the requested structure resembles a native or deterministic final round, produce the rule package, player-facing logic, and manual mapping rather than implementation notes.",
            "The result must be concise, tense, and easy to operationalize in code and UI.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        _ => unreachable!(),
    }
}

fn get_requested_persona(payload_value: &Value) -> Result<&str, String> {
    payload_value
        .get("requested_persona")
        .or_else(|| payload_value.get("requestedPersona"))
        .or_else(|| payload_value.get("persona_name"))
        .or_else(|| payload_value.get("personaName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "round_1 generation requires requested_persona in request_payload_json".to_string()
        })
}

fn build_round_one_skeleton_prompt(payload_value: &Value) -> Result<String, String> {
    let requested_persona = get_requested_persona(payload_value)?;

    Ok([
        "Role & Objective:",
        "You are the skeleton extraction agent for Round 1 of an asymmetrical deduction game.",
        "Your job is to freeze the puzzle structure so a later expansion agent can spend almost all of its output budget on the manual itself.",
        "Return JSON only.",
        "",
        "Requested Persona:",
        requested_persona,
        "",
        "Skeleton tasks:",
        "",
        "persona_name: Repeat the exact requested persona.",
        "",
        "persona_paragraphs: Write exactly 2 evocative paragraphs in the persona voice so Player 1 can identify who is speaking.",
        "Keep each paragraph tight and atmospheric rather than long.",
        "",
        "target_word: Select a highly specific noun associated with this persona's lore or methods.",
        "",
        "forbidden_words: List exactly 5 words that are the most obvious clues or synonyms for the target_word.",
        "",
        "clues: Generate exactly 4 compact final clue objects now. Each clue must already contain clue_id, clue_type, clue_text, required_manual_refs, expected_inference, and difficulty.",
        "Each clue_text should be concrete, atmospheric, and rich enough to feel like a real clue beat rather than a placeholder.",
        "Use stable manual reference ids with these prefixes only: cx_ for codex_entries, tl_ for timeline_fragments, lg_ for cipher_legend, pm_ for protocol_matrix, fl_ for false_leads.",
        "required_manual_refs must point to ids that will also exist in manual_blueprint, and every clue should depend on at least 2 manual refs.",
        "",
        "manual_blueprint: Plan the entire manual, but do not write the long descriptive content yet.",
        "manual_blueprint.overview should be a short paragraph explaining the document theme and how distractors obscure the real solution.",
        "manual_blueprint.hidden_phrase_bridge should explain, without saying the answer verbatim, what kind of idea the full manual is slowly steering the players toward.",
        "manual_blueprint must contain concrete placeholder records for every section with ids, titles, and purposes:",
        "- codex_entries: exactly 6 records using entry_id",
        "- timeline_fragments: exactly 4 records using fragment_id",
        "- cipher_legend: exactly 4 records using cipher_id",
        "- protocol_matrix: exactly 5 records using protocol_id",
        "- false_leads: exactly 3 records using lead_id",
        "- signal_threads: exactly 4 records using thread_id",
        "Every blueprint record must include title, purpose, linked_clue_ids, and hidden_phrase_function.",
        "Keep each title to a few words and each purpose to one short sentence, but make the clue linkage and hidden_phrase_function specific.",
        "",
        "solution: Provide final_identity_guess, final_target_word_inference, why_target_word_fits, and how_manual_reveals_word.",
        "",
        "Absolute constraints:",
        "1. Do not use the persona_name, target_word, or forbidden_words inside persona_paragraphs.",
        "2. Keep the skeleton lean: no long manual prose yet.",
        "3. Keep all ids and clue references internally consistent.",
        "4. The later expansion agent will inherit this skeleton as source of truth, so do not leave placeholders vague.",
        "5. Favor compact phrasing over flourish in this phase; the expansion phase handles depth.",
        "6. The clue chain must make the players depend on the manual to infer the hidden phrase rather than guessing it from theme alone.",
        "",
        "Additional payload JSON:",
        &serde_json::to_string_pretty(payload_value).unwrap_or_else(|_| "{}".to_string()),
    ]
    .join("\n"))
}

fn build_round_one_expansion_prompt(payload_value: &Value, skeleton_json: Option<&str>) -> Result<String, String> {
    let requested_persona = get_requested_persona(payload_value)?;
    let skeleton = skeleton_json.ok_or_else(|| "Skeleton JSON missing for expansion phase".to_string())?;

    Ok([
        "Role & Objective:",
        "You are the manual expansion agent for Round 1 of an asymmetrical deduction game.",
        "A previous agent already froze the identity, the target word, the clue set, the record ids, and the solution path.",
        "Your job is only to write the large descriptive manual and the finished decoder walkthrough.",
        "Return JSON only.",
        "",
        "Requested Persona:",
        requested_persona,
        "",
        "Skeleton Context:",
        skeleton,
        "",
        "Expansion tasks:",
        "",
        "Output only these top-level keys: manual, decoder_walkthrough.",
        "Do not rewrite persona_name, persona_paragraphs, target_word, forbidden_words, clues, or solution. The server will merge those from the skeleton phase.",
        "",
        "The manual JSON must preserve these record fields exactly:",
        "- manual top-level: overview, hidden_phrase_bridge, section_usage_notes, codex_entries, timeline_fragments, cipher_legend, protocol_matrix, false_leads, signal_threads",
        "- codex_entries: entry_id, domain, term, content, relevance_tags, linked_clue_ids, hidden_phrase_signal",
        "- timeline_fragments: fragment_id, timestamp_hint, event_summary, detail_text, linked_entities, linked_clue_ids, hidden_phrase_signal",
        "- cipher_legend: cipher_id, symbol_or_pattern, decoding_rule, expanded_note, example, linked_clue_ids, hidden_phrase_signal",
        "- protocol_matrix: protocol_id, trigger_condition, prescribed_action, hidden_implication, detail_text, linked_clue_ids, hidden_phrase_signal",
        "- false_leads: lead_id, misleading_claim, why_it_looks_valid, why_it_is_wrong, misdirects_clue_ids, discarded_by_manual_refs",
        "- signal_threads: thread_id, linked_clue_ids, manual_refs, hidden_phrase_signal, narrative_bridge",
        "- decoder_walkthrough: step_id, clue_id, manual_refs_used, deduction, phrase_progression",
        "",
        "Manual writing rules:",
        "1. Expand every blueprint record into a full descriptive record using the exact ids from the skeleton.",
        "2. The manual must feel like a large, in-world reference dossier with layered detail, not a terse data table.",
        "3. Hide the useful signal inside plausible but readable distractor material, but never let the important trail become ambiguous.",
        "4. overview should be 2 to 3 dense paragraphs that explain the dossier tone, stakes, and how evidence has been obscured.",
        "5. hidden_phrase_bridge should be a dense paragraph that explains the conceptual path toward the hidden phrase without stating it verbatim.",
        "6. section_usage_notes should tell Player 2 how to use the document under pressure and which section families answer which clue styles.",
        "7. codex_entries.content should usually be 2 to 3 substantial paragraphs full of concrete details, historical texture, and clue-relevant signal.",
        "8. timeline_fragments.detail_text should add rich context beyond the summary in roughly 5 to 8 sentences.",
        "9. cipher_legend.expanded_note should clearly explain the symbol system in roughly 4 to 6 sentences while still fitting the fiction.",
        "10. protocol_matrix.detail_text should sound operational and specific in roughly 5 to 8 sentences.",
        "11. false_leads must be convincing enough to slow the players down before they discard them, and the explanation of why they fail should be clear.",
        "12. signal_threads must explicitly braid clues, manual refs, and hidden-phrase progression together so the final answer feels earned.",
        "13. decoder_walkthrough must map each clue_id to exact manual_refs_used, a precise deduction, and a phrase_progression note that shows how the hidden answer becomes unavoidable.",
        "14. Preserve all clue ids and manual ids exactly as provided by the skeleton.",
        "15. The hidden phrase must never be dropped casually into the manual body. Instead, each hidden_phrase_signal should explain which conceptual fragment of the answer the section supports.",
        "16. Aim for a genuinely large, descriptive manual. A 2,500 to 4,000 word result is appropriate if the schema supports it.",
        "",
        "Quality target:",
        "Spend the token budget on depth, specificity, cross-reference logic, and descriptive texture in the manual. Do not compress the manual into one-line records.",
        "",
        "Additional payload JSON:",
        &serde_json::to_string_pretty(payload_value).unwrap_or_else(|_| "{}".to_string()),
    ]
    .join("\n"))
}

fn build_villain_speech_prompt(
    payload: &VillainSpeechGenerationPayload,
    raw_payload_json: &str,
) -> String {
    let villain_name = payload
        .villain_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("The Entity");
    let scene = payload
        .scene
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("the villain speaks as clues appear on screen");
    let tone = payload
        .tone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("cold, superior, predatory, and theatrical");

    [
        "You are the villain dialogue writer for an asymmetrical horror puzzle game.",
        "Return JSON only.",
        "Generate a set of spoken cues that should play alongside clue reveals using the supplied payload and clue context.",
        "",
        "Speech rules:",
        "1. Produce one speech cue for each supplied clue beat whenever clue ids are available.",
        "2. Preserve supplied clue ids exactly in linked_clue_id whenever possible.",
        "3. Each speech_text must be 2 to 4 sentences, voiced, performable, and suitable for TTS.",
        "4. The villain may taunt, misdirect, threaten, narrate, or frame the clue, but must not directly reveal the validation answer, hidden answer, or kill phrase.",
        "5. delivery_style must be a short performance note.",
        "6. trigger must briefly explain when the line should play in the UI.",
        "7. Escalate menace across rounds when the provided context suggests progression.",
        "",
        &format!("Villain name: {}", villain_name),
        &format!("Scene: {}", scene),
        &format!("Tone: {}", tone),
        "",
        "Payload and clue context JSON:",
        raw_payload_json,
    ]
    .join("\n")
}

fn villain_speech_response_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "speech_cues": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "cue_id": { "type": "string" },
                        "round_key": { "type": "string" },
                        "linked_clue_id": { "type": "string" },
                        "trigger": { "type": "string" },
                        "delivery_style": { "type": "string" },
                        "speech_text": { "type": "string" }
                    },
                    "required": ["cue_id", "round_key", "linked_clue_id", "trigger", "delivery_style", "speech_text"]
                }
            }
        },
        "required": ["speech_cues"]
    })
}

fn enqueue_round_content_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .round_generation_callback_schedule()
            .insert(RoundGenerationCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn enqueue_villain_speech_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .villain_speech_callback_schedule()
            .insert(VillainSpeechCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn enqueue_villain_tts_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body_base64: String,
    mime_type: Option<String>,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .villain_tts_callback_schedule()
            .insert(VillainTtsCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body_base64: response_body_base64.clone(),
                mime_type: mime_type.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

pub fn parse_round_generation_result(body: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;

    // If Gemini returned an envelope, extract and parse the text candidate.
    if parsed.get("candidates").is_some() {
        let finish_reason = parsed
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.get("finishReason"))
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");

        let text = extract_gemini_text(body)?;
        let candidate_value: Result<Value, _> = serde_json::from_str(&text);

        // Gemini can mark a response as MAX_TOKENS even when the JSON object is complete.
        // Accept complete candidate JSON regardless of finish reason and only fail when parse fails.
        if let Ok(value) = candidate_value {
            return Ok(value);
        }

        if finish_reason != "STOP" {
            return Err(format!(
                "Gemini candidate incomplete (finishReason={finish_reason}); retry generation with a smaller payload"
            ));
        }

        return serde_json::from_str(&text).map_err(|err| {
            format!(
                "failed to parse extracted Gemini candidate JSON: {err}; candidate_len={}",
                text.len()
            )
        });
    }

    Ok(parsed)
}

fn require_non_empty(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}
