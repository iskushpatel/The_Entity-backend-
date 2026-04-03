const http = require("node:http");
const { loadLocalEnv } = require("./env");
const {
  buildClueGeneratorPrompt,
  buildTerminalValidatorPrompt,
  buildVillainSpeechPrompt,
  clueGeneratorSchema,
  terminalValidatorSchema,
  villainSpeechSchema
} = require("./prompts");
const {
  buildMockArmorIqResponse,
  buildMockClueResponse,
  buildMockValidatorResponse,
  buildMockVillainResponse,
  callArmorIqVerify,
  callGeminiJson,
  synthesizeWithElevenLabs
} = require("./clients");

loadLocalEnv();

function createConfigFromEnv(overrides = {}) {
  return {
    host: overrides.host ?? process.env.RELAY_HOST ?? "127.0.0.1",
    port: Number(overrides.port ?? process.env.RELAY_PORT ?? 8787),
    mockMode: coerceBoolean(overrides.mockMode ?? process.env.MOCK_MODE ?? false),
    armorIqVerifyUrl: overrides.armorIqVerifyUrl ?? process.env.ARMORIQ_VERIFY_URL ?? "",
    armorIqUpstreamVerifyUrl:
      overrides.armorIqUpstreamVerifyUrl ?? process.env.ARMORIQ_UPSTREAM_VERIFY_URL ?? "",
    armorIqTokenIssueUrl:
      overrides.armorIqTokenIssueUrl ?? process.env.ARMORIQ_TOKEN_ISSUE_URL ?? "",
    armorIqApiKeyHeader:
      overrides.armorIqApiKeyHeader ?? process.env.ARMORIQ_API_KEY_HEADER ?? "x-api-key",
    armorIqApiKey: overrides.armorIqApiKey ?? process.env.ARMORIQ_API_KEY ?? "",
    armorIqUserId: overrides.armorIqUserId ?? process.env.ARMORIQ_USER_ID ?? "",
    armorIqAgentId: overrides.armorIqAgentId ?? process.env.ARMORIQ_AGENT_ID ?? "",
    geminiApiKey: overrides.geminiApiKey ?? process.env.GEMINI_API_KEY ?? "",
    geminiClueModel: overrides.geminiClueModel || process.env.GEMINI_CLUE_MODEL || "gemini-2.5-flash",
    geminiValidatorModel:
      overrides.geminiValidatorModel || process.env.GEMINI_VALIDATOR_MODEL || "gemini-2.5-flash",
    geminiVillainModel:
      overrides.geminiVillainModel || process.env.GEMINI_VILLAIN_MODEL || "gemini-2.5-flash",
    elevenLabsApiKey: overrides.elevenLabsApiKey ?? process.env.ELEVENLABS_API_KEY ?? "",
    elevenLabsVoiceId: overrides.elevenLabsVoiceId ?? process.env.ELEVENLABS_VOICE_ID ?? "",
    elevenLabsModelId:
      overrides.elevenLabsModelId || process.env.ELEVENLABS_MODEL_ID || "eleven_multilingual_v2"
  };
}

function createServer(config = createConfigFromEnv()) {
  return http.createServer(async (req, res) => {
    try {
      if (req.method === "GET" && req.url === "/health") {
        return sendJson(res, 200, {
          ok: true,
          mock_mode: config.mockMode,
          routes: [
            "POST /api/armoriq/verify",
            "POST /api/gemini/clue-generator",
            "POST /api/gemini/terminal-validator",
            "POST /api/villain/speech"
          ]
        });
      }

      if (req.method === "POST" && req.url === "/api/armoriq/verify") {
        const body = await readJsonBody(req);
        requireString(body.player_input, "player_input");
        requireString(body.action, "action");
        requireString(body?.context?.hidden_answer, "context.hidden_answer");

        const result = await callArmorIqVerify({
          apiKey: config.armorIqApiKey,
          apiKeyHeader: config.armorIqApiKeyHeader,
          verifyUrl: resolveArmorIqUpstreamUrl(config, req),
          tokenIssueUrl: resolveArmorIqTokenIssueUrl(config, req),
          userId: config.armorIqUserId,
          agentId: config.armorIqAgentId,
          payload: body,
          mockMode: config.mockMode,
          mockValue: buildMockArmorIqResponse(body)
        });

        return sendJson(res, 200, result);
      }

      if (req.method === "POST" && req.url === "/api/gemini/clue-generator") {
        const body = await readJsonBody(req);
        const result = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiClueModel,
          prompt: buildClueGeneratorPrompt(body),
          responseJsonSchema: clueGeneratorSchema,
          mockMode: config.mockMode,
          mockValue: buildMockClueResponse(body),
          maxOutputTokens: 4096,
          temperature: 0.4
        });
        return sendJson(res, 200, result);
      }

      if (req.method === "POST" && req.url === "/api/gemini/terminal-validator") {
        const body = await readJsonBody(req);
        requireString(body.player_input, "player_input");
        requireString(body.hidden_answer, "hidden_answer");

        const result = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiValidatorModel,
          prompt: buildTerminalValidatorPrompt(body),
          responseJsonSchema: terminalValidatorSchema,
          mockMode: config.mockMode,
          mockValue: buildMockValidatorResponse(body),
          maxOutputTokens: 256,
          temperature: 0.0
        });
        return sendJson(res, 200, result);
      }

      if (req.method === "POST" && req.url === "/api/villain/speech") {
        const body = await readJsonBody(req);
        const textResult = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiVillainModel,
          prompt: buildVillainSpeechPrompt(body),
          responseJsonSchema: villainSpeechSchema,
          mockMode: config.mockMode,
          mockValue: buildMockVillainResponse(body),
          maxOutputTokens: 2048,
          temperature: 0.6
        });

        const selectedCue =
          pickSpeechCue(textResult.speech_cues, body.selected_cue_id) || textResult.speech_cues[0];

        const audio = await synthesizeWithElevenLabs({
          apiKey: config.elevenLabsApiKey,
          voiceId: body.voice_id || config.elevenLabsVoiceId,
          text: selectedCue.speech_text,
          modelId: body.voice_model_id || config.elevenLabsModelId,
          mockMode: config.mockMode
        });

        return sendJson(res, 200, {
          speech_cues: textResult.speech_cues,
          selected_cue_id: selectedCue.cue_id,
          audio_base64: audio.audio_base64,
          mime_type: audio.mime_type,
          tts_provider: audio.provider
        });
      }

      return sendJson(res, 404, { error: "Route not found" });
    } catch (error) {
      return sendJson(res, error.statusCode || 500, {
        error: error.message || "Internal server error"
      });
    }
  });
}

function startServer(configOverrides = {}) {
  const config = createConfigFromEnv(configOverrides);
  const server = createServer(config);

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(config.port, config.host, () => {
      resolve({ server, config });
    });
  });
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let data = "";
    req.setEncoding("utf8");

    req.on("data", (chunk) => {
      data += chunk;
      if (data.length > 128 * 1024) {
        reject(withStatus(new Error("Request body is too large"), 413));
        req.destroy();
      }
    });

    req.on("end", () => {
      if (!data) {
        resolve({});
        return;
      }

      try {
        resolve(JSON.parse(data));
      } catch {
        reject(withStatus(new Error("Invalid JSON body"), 400));
      }
    });

    req.on("error", reject);
  });
}

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, { "Content-Type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(payload));
}

function requireString(value, fieldName) {
  if (typeof value !== "string" || !value.trim()) {
    throw withStatus(new Error(`${fieldName} must be a non-empty string`), 400);
  }
}

function withStatus(error, statusCode) {
  error.statusCode = statusCode;
  return error;
}

function coerceBoolean(value) {
  return value === true || value === "true" || value === "1";
}

function pickSpeechCue(speechCues, selectedCueId) {
  if (!Array.isArray(speechCues) || speechCues.length === 0) {
    throw withStatus(new Error("No speech cues were generated"), 500);
  }

  if (!selectedCueId) {
    return speechCues[0];
  }

  return speechCues.find((cue) => cue.cue_id === selectedCueId) || speechCues[0];
}

function resolveArmorIqUpstreamUrl(config, req) {
  const upstreamUrl = config.armorIqUpstreamVerifyUrl || config.armorIqVerifyUrl;
  if (!upstreamUrl) {
    return upstreamUrl;
  }

  const localUrl = `http://${req.headers.host}${req.url}`;
  if (normalizeUrl(upstreamUrl) === normalizeUrl(localUrl)) {
    throw withStatus(
      new Error(
        "ARMORIQ_UPSTREAM_VERIFY_URL must point to the real ArmorIQ service, not this relay endpoint"
      ),
      500
    );
  }

  return upstreamUrl;
}

function resolveArmorIqTokenIssueUrl(config, req) {
  const tokenIssueUrl = config.armorIqTokenIssueUrl;
  if (!tokenIssueUrl) {
    return "";
  }

  const localUrl = `http://${req.headers.host}${req.url}`;
  if (normalizeUrl(tokenIssueUrl) === normalizeUrl(localUrl)) {
    throw withStatus(
      new Error("ARMORIQ_TOKEN_ISSUE_URL must not point to this relay endpoint"),
      500
    );
  }

  return tokenIssueUrl;
}

function normalizeUrl(value) {
  return String(value || "").trim().replace(/\/+$/, "").toLowerCase();
}

if (require.main === module) {
  startServer()
    .then(({ config }) => {
      console.log(
        `Local relay listening on http://${config.host}:${config.port} (mock_mode=${config.mockMode})`
      );
    })
    .catch((error) => {
      console.error(error);
      process.exitCode = 1;
    });
}

module.exports = {
  createConfigFromEnv,
  createServer,
  startServer
};
