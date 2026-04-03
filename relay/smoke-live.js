const { loadLocalEnv } = require("./env");
const { callArmorIqVerify, callGeminiJson } = require("./clients");

loadLocalEnv();

async function main() {
  const mode = process.argv[2] || "all";

  if (mode === "all" || mode === "gemini") {
    const geminiResult = await callGeminiJson({
      apiKey: process.env.GEMINI_API_KEY,
      model: process.env.GEMINI_VALIDATOR_MODEL || "gemini-2.5-flash",
      prompt:
        'Return JSON only: {"success": true, "reason": "live Gemini reachable for terminal validation"}',
      responseJsonSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          success: { type: "boolean" },
          reason: { type: "string" }
        },
        required: ["success", "reason"]
      },
      mockMode: false
    });

    console.log("Gemini live smoke test passed:");
    console.log(JSON.stringify(geminiResult, null, 2));
  }

  if (mode === "all" || mode === "armoriq") {
    const verifyUrl = process.env.ARMORIQ_UPSTREAM_VERIFY_URL;
    if (!verifyUrl) {
      throw new Error("ARMORIQ_UPSTREAM_VERIFY_URL is missing");
    }

    const armoriqResult = await callArmorIqVerify({
      apiKey: process.env.ARMORIQ_API_KEY,
      apiKeyHeader: process.env.ARMORIQ_API_KEY_HEADER || "x-api-key",
      verifyUrl,
      tokenIssueUrl: process.env.ARMORIQ_TOKEN_ISSUE_URL,
      userId: process.env.ARMORIQ_USER_ID,
      agentId: process.env.ARMORIQ_AGENT_ID,
      payload: {
        player_input: "run containment override",
        action: "terminal_override",
        context: {
          hidden_answer: "containment override"
        }
      },
      mockMode: false
    });

    console.log("ArmorIQ live smoke test passed:");
    console.log(JSON.stringify(armoriqResult, null, 2));
  }
}

main().catch((error) => {
  console.error(error?.message || String(error));
  process.exitCode = 1;
});
