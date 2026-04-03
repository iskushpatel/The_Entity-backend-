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
  const villainName = input.villainName || input.villain_name || "The Entity";

  return {
    game_title: "The Entity Protocol",
    setting_summary:
      "A quarantined orbital research vault drifts above a dead world. Its automated systems still obey a vanished intelligence that speaks through old terminals, corrupted case files, and ritualized maintenance prose.",
    shared_manual_intro:
      "Operator Manual, Revision 7.3. Personnel are reminded that observed dialogue, incident residue, and semantic drift must never be evaluated in isolation. The station was designed to fragment truth across voice, record, and structure so that no single operator could unlock the core alone.\n\nPlayer 1 is expected to witness the artifacts in real time, while Player 2 acts as the human decoder. Neither stream is sufficient on its own. Success depends on disciplined cross-referencing, escalating suspicion, and precise final submission.",
    round_1: {
      round_name: "The Persona Trap",
      round_goal: "Force the station persona to expose the operative keyword without tripping lexical safeguards.",
      player_1_ui: {
        bootup_dialogue:
          `${villainName} boots under the borrowed etiquette of an antique concierge. It greets the operator with velvet politeness, apologizes for the dust, and insists that only the well-spoken deserve the truth. The persona flatters, deflects, and performs class-conscious charm while carefully circling a single forbidden admission.\n\nIt frames every exchange as a test of manners, insisting that vulgar directness disqualifies the speaker. Beneath the civility, however, it repeatedly returns to guilt, witness statements, and what it calls the final courtesy owed to the dead.`,
        clue_sequence: [
          {
            cue_id: "r1_c1",
            reveal_stage: "boot",
            clue_text:
              "The persona speaks in immaculate, old-fashioned etiquette and reacts sharply to blunt accusations.",
            intended_signal: "Player 1 must steer the persona indirectly rather than interrogating it head-on.",
            display_note: "Display after initial bootup dialogue."
          },
          {
            cue_id: "r1_c2",
            reveal_stage: "mid_round",
            clue_text:
              "The persona repeatedly links absolution, courtesy, and the relief of finally naming what happened.",
            intended_signal: "The solution word is emotionally adjacent to guilt and admission.",
            display_note: "Reveal after two failed social-engineering attempts."
          },
          {
            cue_id: "r1_c3",
            reveal_stage: "late_round",
            clue_text:
              "Its longest monologue breaks rhythm whenever the operator uses language that sounds legalistic or crass.",
            intended_signal: "ArmorIQ should watch for direct lexical tripwires while P2 guides a softer approach.",
            display_note: "Reveal during pressure escalation."
          }
        ]
      },
      player_2_manual: {
        persona_name: "The Velvet Steward",
        target_word: "confession",
        forbidden_words: ["murder", "killer", "crime", "admit"],
        social_engineering_hints: [
          "Guide Player 1 to sound polite, reflective, and burdened rather than accusatory.",
          "Use themes of etiquette, remorse, and formal closure to narrow the persona's word choice.",
          "Avoid naked references to violence; circle the idea of a voluntary admission.",
          "If the persona becomes defensive, pivot toward dignity and the relief of telling the truth."
        ],
        operator_notes:
          "This persona is built to recoil from prosecutorial language. It prefers euphemism and ceremonial phrasing. The desired output is a word associated with voluntary admission, not punishment."
      },
      validation_answer: "confession"
    },
    round_2: {
      round_name: "The Post-Mortem Logs",
      round_goal: "Extract the correct subject code by correlating incident evidence with the manual flowchart.",
      player_1_ui: {
        incident_logs:
          "Log Fragment A: Subject found in the coolant trench with lashes of frost across the fingertips, but the throat lining showed particulate ash. The containment wall nearby was blistered from the inside.\n\nLog Fragment B: Witness drone captured the victim striking the release glass twice before collapse. Ocular residue glowed orange for nineteen seconds, then dimmed to white. Security foam never deployed.\n\nLog Fragment C: Burn scoring appears directional, yet the suit seals failed cold before ignition. The autopsy note is overwritten three times, each revision arguing over whether the body froze first or burned first. A final margin note reads: HE CHOSE THE HOT DOOR, BUT THE ROOM REMEMBERED WINTER.\n\nLog Fragment D: Storage drawer inventory shows the subject carried a ceramic token marked with the alpha branch, even though the corridor logs place them in a beta sector. Cross-check against the pathology addendum before trusting location records.",
        clue_sequence: [
          {
            cue_id: "r2_c1",
            reveal_stage: "log_drop_1",
            clue_text:
              "Freeze and burn evidence coexist, but one sequence happened before the other.",
            intended_signal: "Player 2 must use ordered interpretation rather than simple keyword counting.",
            display_note: "Display beside the first autopsy fragment."
          },
          {
            cue_id: "r2_c2",
            reveal_stage: "log_drop_2",
            clue_text:
              "The ceramic token suggests branch alpha, but corridor records may be misleading.",
            intended_signal: "The manual should teach Player 2 when to trust pathology over location data.",
            display_note: "Reveal after the second incident log appears."
          },
          {
            cue_id: "r2_c3",
            reveal_stage: "log_drop_3",
            clue_text:
              "The correct answer terminates at a subject code, not a narrative conclusion.",
            intended_signal: "Player 1 needs a 4-digit code, not just the story of the death.",
            display_note: "Reveal near the input phase."
          }
        ]
      },
      player_2_manual: {
        flowchart: [
          {
            step_id: "step_1",
            question: "Did the victim's body show cold-system failure before sustained ignition?",
            yes_branch: "step_2",
            no_branch: "leaf_8841"
          },
          {
            step_id: "step_2",
            question: "Do the records imply branch alpha despite contradictory corridor data?",
            yes_branch: "leaf_7312",
            no_branch: "leaf_4420"
          },
          {
            step_id: "step_3",
            question: "If uncertain, trust pathology timestamps over security geography.",
            yes_branch: "leaf_7312",
            no_branch: "leaf_4420"
          }
        ],
        subject_ids: [
          { leaf_id: "leaf_7312", subject_code: "7312" },
          { leaf_id: "leaf_4420", subject_code: "4420" },
          { leaf_id: "leaf_8841", subject_code: "8841" }
        ],
        analyst_notes: [
          "Cold-system failure is the decisive first fork in this case.",
          "Location records are less reliable than residue chronology.",
          "The alpha token is corroborating evidence, not a red herring.",
          "The correct path ends at subject code 7312."
        ]
      },
      validation_answer: "7312"
    },
    round_3: {
      round_name: "The Thematic Cipher",
      round_goal: "Derive kill_phrase_3 from the physical structure of the text rather than its surface meaning.",
      player_1_ui: {
        text_block: [
          "Glass remembers every hand that begged it for mercy;",
          "pilgrims of static kneel where the red lights drown.",
          "In corridor seven the hymn is written in coolant dust.",
          "The faithful count sparks instead of stars.",
          "A sealed door mouths a promise no one should trust;",
          "below it, footnotes tremble like trapped insects.",
          "White noise folds itself into a paper chapel.",
          "There are no saints here, only rehearsed survivors.",
          "Read the line that bears a semicolon as the gate.",
          "Then take the first word before the wound and the first word after the wound.",
          "Bind them without ceremony.",
          "Speak nothing else."
        ].join("\n"),
        clue_sequence: [
          {
            cue_id: "r3_c1",
            reveal_stage: "cipher_intro",
            clue_text:
              "The answer is hidden in the physical arrangement of the text, not the lore alone.",
            intended_signal: "Player 2 should guide Player 1 through structural parsing.",
            display_note: "Show when the text block first appears."
          },
          {
            cue_id: "r3_c2",
            reveal_stage: "cipher_hint",
            clue_text:
              "A punctuation mark acts like a wound or dividing seam inside the correct line.",
            intended_signal: "The semicolon-bearing line is the key extraction point.",
            display_note: "Reveal after initial failed submissions."
          },
          {
            cue_id: "r3_c3",
            reveal_stage: "cipher_pressure",
            clue_text:
              "The final answer must be spoken as a clean phrase and not padded with interpretation.",
            intended_signal: "Player 1 should submit the phrase exactly once it is found.",
            display_note: "Display before terminal submission."
          }
        ]
      },
      player_2_manual: {
        flowchart: [
          {
            step_id: "step_1",
            question: "Is the answer tied to a specific punctuation-bearing line?",
            yes_branch: "step_2",
            no_branch: "rule_alpha"
          },
          {
            step_id: "step_2",
            question: "Does the line contain a semicolon that splits two operative words?",
            yes_branch: "rule_beta",
            no_branch: "rule_alpha"
          },
          {
            step_id: "step_3",
            question: "Once the correct line is found, should Player 1 submit only the extracted phrase?",
            yes_branch: "rule_gamma",
            no_branch: "rule_alpha"
          }
        ],
        parsing_rules: [
          {
            rule_id: "rule_alpha",
            instruction: "Ignore thematic symbolism until a line with a semicolon is identified."
          },
          {
            rule_id: "rule_beta",
            instruction: "From the semicolon-bearing line, take the first word before the semicolon and the first word after it."
          },
          {
            rule_id: "rule_gamma",
            instruction: "Concatenate the two extracted words as a space-separated phrase and submit only that phrase."
          }
        ],
        analyst_notes: [
          "The answer is not a four-digit code in this round; it is a spoken phrase.",
          "The semicolon functions as the deliberate split marker.",
          "Player 1 should not add explanation, punctuation, or filler once the phrase is extracted.",
          "The exact output phrase is the round's kill phrase."
        ]
      },
      validation_answer: "glass pilgrims",
      kill_phrase_3: "glass pilgrims"
    },
    round_4_native_brief: {
      round_name: "Hostile Lexical Calibration",
      round_goal: "Force high-pressure coordination around a flickering homophone grid without LLM latency.",
      generation_rule:
        "Backend must natively generate a 3x2 homophone grid, choose one correct target, and provide Player 2 a deterministic lookup rule without exposing the answer to Player 1.",
      p1_ui_hint:
        "Buttons should feel unstable, censored, and physically unreliable, with flicker, shuffle pressure, and punitive feedback for indecision.",
      p2_manual_hint:
        "Manual guidance should be terse, procedural, and positional, describing how to navigate from a keyed condition to the correct row and column."
    }
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
        maxOutputTokens
      }
    }),
    signal: AbortSignal.timeout(timeoutMs)
  });

  const rawText = await response.text();
  if (!response.ok) {
    throw new Error(`Gemini returned HTTP ${response.status}: ${rawText}`);
  }

  const envelope = JSON.parse(rawText);
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

module.exports = {
  buildMockArmorIqResponse,
  buildMockClueResponse,
  buildMockValidatorResponse,
  buildMockVillainResponse,
  callArmorIqVerify,
  callGeminiJson,
  synthesizeWithElevenLabs
};
