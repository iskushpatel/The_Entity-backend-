const GEMINI_API_BASE = "https://generativelanguage.googleapis.com/v1beta";
const ELEVENLABS_API_BASE = "https://api.elevenlabs.io/v1";

function buildMockClueResponse(input = {}) {
  const villainName = input.villainName || input.villain_name || "The Entity";
  return {
    villain_clue_dialogue: `${villainName}: You already saw the answer. You just failed to name it.`,
    p2_manual_snippet:
      "Maintenance note: the emergency override is stored as a short phrase and never as a numeric code.",
    hidden_answer: "containment override"
  };
}

function buildMockValidatorResponse(input) {
  const normalizedInput = normalizeLoose(input.player_input);
  const normalizedAnswer = normalizeLoose(input.hidden_answer);
  const success =
    normalizedInput === normalizedAnswer || normalizedInput.includes(normalizedAnswer);

  return {
    success,
    reason: success
      ? "The terminal input matches the hidden answer."
      : "The terminal input does not semantically resolve to the hidden answer."
  };
}

function buildMockVillainResponse(input = {}) {
  const villainName = input.villainName || input.villain_name || "The Entity";
  return {
    speech_text: `${villainName}: You are not uncovering the truth. You are only walking deeper into my design. Every locked door you open was meant to guide you here.`
  };
}

async function callGeminiJson({
  apiKey,
  model,
  prompt,
  responseJsonSchema,
  mockMode,
  mockValue,
  timeoutMs = 20000
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
        temperature: 0.2,
        candidateCount: 1,
        maxOutputTokens: 512
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
  buildMockClueResponse,
  buildMockValidatorResponse,
  buildMockVillainResponse,
  callGeminiJson,
  synthesizeWithElevenLabs
};
