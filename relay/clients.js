const GEMINI_API_BASE = "https://generativelanguage.googleapis.com/v1beta";
const ELEVENLABS_API_BASE = "https://api.elevenlabs.io/v1";

function buildMockArmorIqResponse(input = {}) {
  const playerInput = normalizeLoose(input.player_input);
  const hiddenAnswer = normalizeLoose(input?.context?.hidden_answer);
  const matches =
    playerInput === hiddenAnswer ||
    (hiddenAnswer && playerInput.includes(hiddenAnswer));

  if (matches) {
    return {
      allowed: true,
      block_reason: null
    };
  }

  return {
    allowed: false,
    block_reason: "Input does not satisfy the terminal override policy."
  };
}

function buildMockClueResponse(input = {}) {
  const roundKey = String(input.round_key || "").trim().toLowerCase();
  if (roundKey !== "round_1") {
    throw new Error(`${roundKey || "unknown"} clue mock is not configured yet`);
  }

  const requestedPersona =
    input.requested_persona ||
    input.requestedPersona ||
    input.persona_name ||
    input.personaName ||
    "1920s Detective";

  const mockProfile = buildRoundOneMockProfile(requestedPersona);

  return {
    persona_name: requestedPersona,
    persona_paragraphs: mockProfile.persona_paragraphs,
    target_word: mockProfile.target_word,
    forbidden_words: mockProfile.forbidden_words,
    clues: mockProfile.clues,
    manual: mockProfile.manual,
    decoder_walkthrough: mockProfile.decoder_walkthrough,
    solution: mockProfile.solution
  };
}

function buildMockValidatorResponse(input) {
  const normalizedInput = normalizeLoose(input.player_input);
  const normalizedAnswer = normalizeLoose(input.hidden_answer);

  const exactSubmission =
    normalizedInput === normalizedAnswer ||
    [
      `submit ${normalizedAnswer}`,
      `enter ${normalizedAnswer}`,
      `run ${normalizedAnswer}`,
      `input ${normalizedAnswer}`,
      `execute ${normalizedAnswer}`,
      `say ${normalizedAnswer}`
    ].includes(normalizedInput);

  return {
    success: exactSubmission,
    reason: exactSubmission
      ? "The terminal input is a valid direct submission of the secret phrase."
      : "The terminal input does not contain a valid direct submission of the secret phrase."
  };
}

function buildMockVillainResponse() {
  return {
    speech_cues: [
      {
        cue_id: "v_r1_c1",
        round_key: "round_1",
        linked_clue_id: "r1_c1",
        trigger: "Play when the persona boots and the first lexical hint appears.",
        delivery_style: "Velvet calm with buried contempt.",
        speech_text:
          "Politeness is the oldest lock I know. Push too hard and the door will only admire your desperation. Ask correctly, and perhaps I will let the station remember what it did."
      },
      {
        cue_id: "v_r2_c1",
        round_key: "round_2",
        linked_clue_id: "r2_c1",
        trigger: "Play during the first post-mortem reveal.",
        delivery_style: "Clinical, almost tender, then suddenly cruel.",
        speech_text:
          "They always argue over what killed the body first. Cold, fire, terror, faith. It hardly matters. By the time they begin counting wounds, the answer has already learned to hide in the paperwork."
      },
      {
        cue_id: "v_r3_c1",
        round_key: "round_3",
        linked_clue_id: "r3_c1",
        trigger: "Play when the thematic cipher is first rendered.",
        delivery_style: "Soft and reverent, like a priest at the wrong altar.",
        speech_text:
          "Meaning will mislead you. Structure is where devotion leaves its fingerprints. If you insist on reading for comfort, the text will let you drown before it lets you understand."
      }
    ]
  };
}

async function callGeminiJson({
  apiKey,
  model,
  prompt,
  responseJsonSchema,
  mockMode,
  mockValue,
  timeoutMs = 20000,
  maxOutputTokens = 512,
  temperature = 0.2
}) {
  if (mockMode) {
    return mockValue;
  }

  if (!apiKey) {
    throw new Error("GEMINI_API_KEY is missing");
  }

  if (!model) {
    throw new Error("Gemini model name is missing");
  }

  const response = await fetch(`${GEMINI_API_BASE}/models/${model}:generateContent`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-goog-api-key": apiKey
    },
    body: JSON.stringify({
      contents: [{ role: "user", parts: [{ text: prompt }] }],
      generationConfig: {
        responseMimeType: "application/json",
        responseJsonSchema,
        temperature,
        candidateCount: 1,
        maxOutputTokens,
        thinkingConfig: {
          thinkingBudget: 0,
          includeThoughts: false
        }
      }
    }),
    signal: AbortSignal.timeout(timeoutMs)
  });

  const rawText = await response.text();
  if (!response.ok) {
    throw new Error(`Gemini returned HTTP ${response.status}: ${rawText}`);
  }

  const envelope = JSON.parse(rawText);
  const finishReason = envelope?.candidates?.[0]?.finishReason;
  if (finishReason && finishReason !== "STOP") {
    throw new Error(`Gemini returned incomplete output (finishReason=${finishReason})`);
  }

  const candidateText =
    envelope?.candidates?.[0]?.content?.parts?.find((part) => typeof part.text === "string")?.text;

  if (!candidateText) {
    throw new Error("Gemini response did not include a text candidate");
  }

  return JSON.parse(candidateText);
}

async function callArmorIqVerify({
  apiKey,
  apiKeyHeader = "x-api-key",
  verifyUrl,
  tokenIssueUrl,
  userId,
  agentId,
  payload,
  mockMode,
  mockValue,
  timeoutMs = 10000
}) {
  if (mockMode) {
    return mockValue;
  }

  if (!verifyUrl) {
    throw new Error("ARMORIQ_VERIFY_URL is missing");
  }

  if (!apiKey) {
    throw new Error("ARMORIQ_API_KEY is missing");
  }

  if (!apiKeyHeader) {
    throw new Error("ARMORIQ_API_KEY_HEADER is missing");
  }

  const issued = await issueArmorIqToken({
    apiKey,
    apiKeyHeader,
    tokenIssueUrl: tokenIssueUrl || deriveArmorIqTokenIssueUrl(verifyUrl),
    payload,
    userId,
    agentId,
    timeoutMs
  });

  return {
    allowed: true,
    block_reason: null,
    intent_reference: issued.envelope.intent_reference || issued.token.tokenId || null,
    expires_at: issued.envelope.expires_at || null,
    provider: "armoriq"
  };
}

async function issueArmorIqToken({
  apiKey,
  apiKeyHeader,
  tokenIssueUrl,
  payload,
  userId,
  agentId,
  timeoutMs
}) {
  if (!tokenIssueUrl) {
    throw new Error("ARMORIQ token issue URL is missing");
  }

  const response = await fetch(tokenIssueUrl, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      [apiKeyHeader]: apiKey
    },
    body: JSON.stringify(buildArmorIqTokenIssuePayload(payload, userId, agentId)),
    signal: AbortSignal.timeout(timeoutMs)
  });

  const rawText = await response.text();
  if (!response.ok) {
    throw new Error(`ArmorIQ token issue failed with HTTP ${response.status}: ${rawText}`);
  }

  let envelope;
  try {
    envelope = JSON.parse(rawText);
  } catch (error) {
    throw new Error(`ArmorIQ token issue returned invalid JSON: ${error.message}`);
  }

  if (envelope?.success === false) {
    throw new Error(
      envelope?.message || envelope?.error || "ArmorIQ token issue request was rejected"
    );
  }

  const token = extractArmorIqToken(envelope);
  if (!token) {
    throw new Error("ArmorIQ token issue response did not include a token");
  }

  return {
    token,
    envelope
  };
}

function deriveArmorIqTokenIssueUrl(verifyUrl) {
  const url = new URL(verifyUrl);
  url.pathname = "/token/issue";
  url.search = "";
  url.hash = "";
  return url.toString();
}

function buildArmorIqTokenIssuePayload(payload, userId, agentId) {
  return {
    user_id: userId || "the-entity-local-user",
    agent_id: agentId || "the-entity-relay",
    action: payload?.action || "terminal_override",
    plan: {
      goal: "Authorize a terminal override validation request",
      steps: [
        {
          action: payload?.action || "terminal_override",
          mcp: "the-entity-terminal",
          params: {
            player_input: payload?.player_input || "",
            hidden_answer: payload?.context?.hidden_answer || ""
          }
        }
      ]
    },
    policy: {
      allow: ["*"],
      deny: []
    }
  };
}

function extractArmorIqToken(envelope) {
  const candidates = [
    envelope?.token,
    envelope?.access_token,
    envelope?.intent_token,
    envelope?.bearer_token,
    envelope?.jwt,
    envelope?.data?.token,
    envelope?.data?.access_token,
    envelope?.data?.intent_token,
    envelope?.result?.token,
    envelope?.result?.access_token,
    envelope?.result?.intent_token
  ];

  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.trim()) {
      return candidate.trim();
    }

    if (candidate && typeof candidate === "object") {
      const tokenId =
        candidate.intent_reference || candidate.tokenId || envelope?.intent_reference;
      const signature = candidate.signature;
      const planHash = candidate.plan_hash || candidate.planHash || envelope?.plan_hash;
      const stepProofs = envelope?.step_proofs || candidate.step_proofs;

      if (signature && planHash && Array.isArray(stepProofs)) {
        return JSON.stringify({
          tokenId: tokenId || null,
          planHash,
          signature,
          stepProofs
        });
      }
    }
  }

  return null;
}

async function synthesizeWithElevenLabs({
  apiKey,
  voiceId,
  text,
  modelId,
  mockMode,
  timeoutMs = 20000
}) {
  if (mockMode) {
    return {
      provider: "mock",
      mime_type: "audio/mpeg",
      audio_base64: Buffer.from("mock-audio").toString("base64")
    };
  }

  if (!apiKey || !voiceId) {
    return {
      provider: "disabled",
      mime_type: null,
      audio_base64: null
    };
  }

  const response = await fetch(`${ELEVENLABS_API_BASE}/text-to-speech/${voiceId}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "xi-api-key": apiKey,
      Accept: "audio/mpeg"
    },
    body: JSON.stringify({
      text,
      model_id: modelId || "eleven_multilingual_v2"
    }),
    signal: AbortSignal.timeout(timeoutMs)
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`ElevenLabs returned HTTP ${response.status}: ${errorText}`);
  }

  const audioBuffer = Buffer.from(await response.arrayBuffer());
  return {
    provider: "elevenlabs",
    mime_type: response.headers.get("content-type") || "audio/mpeg",
    audio_base64: audioBuffer.toString("base64")
  };
}

function normalizeLoose(value) {
  return String(value || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function buildRoundOneMockProfile(requestedPersona) {
  const persona = String(requestedPersona || "").toLowerCase();

  if (persona.includes("detective")) {
    return {
      target_word: "ledger",
      forbidden_words: ["book", "record", "account", "log", "register"],
      persona_paragraphs: [
        "The city speaks in cigarette ash and rainwater, and every decent lie leaves a stain if you know where to look. I make my living in the narrow gap between what people swear happened and what the room itself is too tired to hide.",
        "My trade is patience sharpened into instinct. I trust scuffed floors, unpaid debts, and the way a frightened witness grips a glass too tightly. By dawn, I usually know which pocket held the truth long before anyone admits it.",
        "You will know me by the habit of circling the smallest inconsistency until it cracks open the whole night. I do not need a confession to begin; I only need the paper trail that everyone forgets to fear."
      ],
      clues: buildRoundOneMockClues(),
      manual: buildRoundOneMockManual(),
      decoder_walkthrough: buildRoundOneMockWalkthrough(),
      solution: {
        final_identity_guess: "1920s Detective",
        final_target_word_inference: "ledger"
      }
    };
  }

  if (persona.includes("pirate")) {
    return {
      target_word: "cutlass",
      forbidden_words: ["sword", "blade", "steel", "duel", "weapon"],
      persona_paragraphs: [
        "Salt, thunder, and mutiny are better tutors than any courtly school. I trust the horizon more than any promise spoken on dry land, because the sea has a habit of stripping the truth down to nerve and hunger.",
        "A crew will follow strength until the wind turns mean, and then they look for omens in every loose rope and blackening cloud. I answer with laughter, discipline, and the certainty that fear is only useful when someone else feels it first.",
        "You could know me by the way I talk about plunder as if it were destiny and danger as if it were weather. I was built for pursuit, command, and the hard gleam of survival when mercy sinks below the tide."
      ],
      clues: buildRoundOneMockClues(),
      manual: buildRoundOneMockManual(),
      decoder_walkthrough: buildRoundOneMockWalkthrough(),
      solution: {
        final_identity_guess: "Pirate Captain",
        final_target_word_inference: "cutlass"
      }
    };
  }

  return {
    target_word: "relic",
    forbidden_words: ["artifact", "idol", "shrine", "ruin", "antique"],
    persona_paragraphs: [
      "I speak as someone who has handled too many sacred things with bare hands and come away unchanged only in appearance. History, to me, is never dead. It waits in dust, in ceremony, and in the silence people mistake for safety.",
      "My instincts are built from old doctrines, careful observation, and the knowledge that every object survives because someone feared destroying it. I look for significance in wear patterns, omissions, and the stories that power teaches people to whisper.",
      "If you are trying to place me, listen for reverence sharpened into obsession. I belong wherever memory is guarded, catalogued, stolen, or awakened."
    ],
    clues: buildRoundOneMockClues(),
    manual: buildRoundOneMockManual(),
    decoder_walkthrough: buildRoundOneMockWalkthrough(),
    solution: {
      final_identity_guess: "Archivist",
      final_target_word_inference: "relic"
    }
  };
}

function buildRoundOneMockClues() {
  return [
    {
      clue_id: "c1",
      clue_type: "linguistic",
      clue_text: "The speaker distrusts testimony unless it aligns with a numbered city index and tobacco timings.",
      required_manual_refs: ["cx_02", "tl_03"],
      expected_inference: "Evidence weighting over confessional speech",
      difficulty: "hard"
    },
    {
      clue_id: "c2",
      clue_type: "timeline",
      clue_text: "Three events are out of order; only the post-rain street log can anchor sequence.",
      required_manual_refs: ["tl_05", "pm_04"],
      expected_inference: "Reconstruct chronology from reliability and protocol priority",
      difficulty: "expert"
    },
    {
      clue_id: "c3",
      clue_type: "cross_reference",
      clue_text: "The key noun is implied where debt language intersects chain-of-custody notation.",
      required_manual_refs: ["cx_08", "pm_09"],
      expected_inference: "Target concept belongs to tracked transactional evidence",
      difficulty: "expert"
    },
    {
      clue_id: "c4",
      clue_type: "symbolic",
      clue_text: "A split-circle mark means compare witness channel A against archived margin script.",
      required_manual_refs: ["lg_02", "cx_11"],
      expected_inference: "Use cipher legend to reinterpret witness terminology",
      difficulty: "hard"
    },
    {
      clue_id: "c5",
      clue_type: "behavioral",
      clue_text: "The voice profile rewards procedural caution and penalizes rhetorical certainty.",
      required_manual_refs: ["pm_02", "pm_07"],
      expected_inference: "Persona follows investigative protocol, not forceful accusation",
      difficulty: "medium"
    },
    {
      clue_id: "c6",
      clue_type: "timeline",
      clue_text: "Ignore any fragment tagged lantern-red unless corroborated by two independent channels.",
      required_manual_refs: ["tl_09", "fl_01"],
      expected_inference: "Discard seductive but unreliable branch",
      difficulty: "hard"
    },
    {
      clue_id: "c7",
      clue_type: "cross_reference",
      clue_text: "The hidden noun appears in absentia: it is never spoken in high-trust interviews but drives all reconciliation.",
      required_manual_refs: ["tl_12", "cx_14", "pm_12"],
      expected_inference: "Infer omitted core object from reconciliation pattern",
      difficulty: "expert"
    },
    {
      clue_id: "c8",
      clue_type: "linguistic",
      clue_text: "When the speaker contrasts rain and ink, decode by the index that maps weather to archive actions.",
      required_manual_refs: ["lg_05", "pm_10"],
      expected_inference: "Convert metaphor into procedural archive step",
      difficulty: "hard"
    }
  ];
}

function buildRoundOneMockManual() {
  return {
    codex_entries: buildRecordList("cx", 14, "Case codex entry"),
    timeline_fragments: buildTimelineList("tl", 10),
    cipher_legend: buildCipherList("lg", 8),
    protocol_matrix: buildProtocolList("pm", 10),
    false_leads: [
      {
        lead_id: "fl_01",
        misleading_claim: "Lantern-red tags always indicate authentic witness memory.",
        why_it_looks_valid: "The most dramatic fragments are lantern-red and emotionally consistent.",
        why_it_is_wrong: "Legend notes lantern-red as contamination-prone unless dual corroborated."
      },
      {
        lead_id: "fl_02",
        misleading_claim: "Any direct accusation collapses uncertainty faster.",
        why_it_looks_valid: "Aggressive language appears to force shorter responses.",
        why_it_is_wrong: "Protocol matrix marks coercive questioning as entropy-increasing."
      },
      {
        lead_id: "fl_03",
        misleading_claim: "Temporal mismatch implies forged records only.",
        why_it_looks_valid: "Out-of-order logs resemble tampering patterns.",
        why_it_is_wrong: "Timeline appendix documents delayed municipal syncing artifacts."
      }
    ]
  };
}

function buildRoundOneMockWalkthrough() {
  return [
    {
      step_id: "w1",
      clue_id: "c1",
      manual_refs_used: ["cx_02", "tl_03"],
      deduction: "Cross-link vocabulary frequency with rain-index timing to identify investigative persona bias."
    },
    {
      step_id: "w2",
      clue_id: "c2",
      manual_refs_used: ["tl_05", "pm_04"],
      deduction: "Use protocol precedence to reorder contested events and eliminate false chronology."
    },
    {
      step_id: "w3",
      clue_id: "c3",
      manual_refs_used: ["cx_08", "pm_09"],
      deduction: "Derive core hidden object by intersecting debt terms and custody process markers."
    },
    {
      step_id: "w4",
      clue_id: "c4",
      manual_refs_used: ["lg_02", "cx_11"],
      deduction: "Apply split-circle decoding rule to reinterpret testimony wording."
    },
    {
      step_id: "w5",
      clue_id: "c7",
      manual_refs_used: ["tl_12", "cx_14", "pm_12"],
      deduction: "Infer omitted target noun from high-trust omission pattern and reconciliation records."
    }
  ];
}

function buildRecordList(prefix, count, label) {
  return Array.from({ length: count }, (_, idx) => {
    const id = `${prefix}_${String(idx + 1).padStart(2, "0")}`;
    return {
      entry_id: id,
      domain: idx % 2 === 0 ? "forensic_language" : "urban_intelligence",
      term: `${label} ${idx + 1}`,
      description: `Extended annotation ${idx + 1} describing contextual dependencies, edge cases, and filtering rules for inference chains.`,
      relevance_tags: ["inference", idx % 2 === 0 ? "trace" : "testimony"]
    };
  });
}

function buildTimelineList(prefix, count) {
  return Array.from({ length: count }, (_, idx) => {
    const id = `${prefix}_${String(idx + 1).padStart(2, "0")}`;
    return {
      fragment_id: id,
      timestamp_hint: `night_cycle_${idx + 1}`,
      event_summary: `Fragment ${idx + 1} documents conflicting witness timing with municipal drift artifacts and post-rain indexing.`,
      reliability: idx % 3 === 0 ? "high" : idx % 3 === 1 ? "medium" : "low",
      linked_entities: ["operator_a", `sector_${(idx % 4) + 1}`]
    };
  });
}

function buildCipherList(prefix, count) {
  return Array.from({ length: count }, (_, idx) => {
    const id = `${prefix}_${String(idx + 1).padStart(2, "0")}`;
    return {
      cipher_id: id,
      symbol_or_pattern: `pattern_${idx + 1}`,
      decoding_rule: `Translate motif ${idx + 1} into procedural state transitions before semantic interpretation.`,
      example: `Example ${idx + 1}: symbol cluster re-maps from emotional tone to archive process stage.`
    };
  });
}

function buildProtocolList(prefix, count) {
  return Array.from({ length: count }, (_, idx) => {
    const id = `${prefix}_${String(idx + 1).padStart(2, "0")}`;
    return {
      protocol_id: id,
      trigger_condition: `When channel_${(idx % 3) + 1} confidence drops below threshold_${(idx % 5) + 1}`,
      prescribed_action: `Apply verification pass ${(idx % 4) + 1} and postpone hard conclusion until reconciliation.` ,
      hidden_implication: `This row implies evidence hierarchy ${idx + 1} that can invert naive clue readings.`
    };
  });
}

module.exports = {
  buildMockArmorIqResponse,
  buildMockClueResponse,
  buildMockValidatorResponse,
  buildMockVillainResponse,
  callArmorIqVerify,
  callGeminiJson,
  synthesizeWithElevenLabs
};
