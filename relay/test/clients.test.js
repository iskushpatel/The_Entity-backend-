const test = require("node:test");
const assert = require("node:assert/strict");
const { callGeminiJson } = require("../clients");

function makeResponse({ status = 200, body = "{}", headers = {} } = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: {
      get(name) {
        return headers[String(name || "").toLowerCase()] ?? null;
      }
    },
    async text() {
      return body;
    }
  };
}

test("callGeminiJson retries on HTTP 429 and succeeds", async () => {
  const originalFetch = global.fetch;
  let callCount = 0;

  global.fetch = async () => {
    callCount += 1;

    if (callCount === 1) {
      return makeResponse({
        status: 429,
        body: "rate limited",
        headers: { "retry-after": "0" }
      });
    }

    return makeResponse({
      status: 200,
      body: JSON.stringify({
        candidates: [
          {
            finishReason: "STOP",
            content: {
              parts: [{ text: JSON.stringify({ ok: true }) }]
            }
          }
        ]
      })
    });
  };

  try {
    const result = await callGeminiJson({
      apiKey: "test-key",
      model: "gemini-2.5-flash",
      prompt: "test prompt",
      rateLimitBackoffBaseMs: 0,
      timeoutMs: 1000
    });

    assert.equal(result.ok, true);
    assert.equal(callCount, 2);
  } finally {
    global.fetch = originalFetch;
  }
});

test("callGeminiJson fails after exhausting HTTP 429 retries", async () => {
  const originalFetch = global.fetch;
  let callCount = 0;

  global.fetch = async () => {
    callCount += 1;
    return makeResponse({
      status: 429,
      body: "still rate limited",
      headers: { "retry-after": "0" }
    });
  };

  try {
    await assert.rejects(
      () =>
        callGeminiJson({
          apiKey: "test-key",
          model: "gemini-2.5-flash",
          prompt: "test prompt",
          rateLimitBackoffBaseMs: 0,
          timeoutMs: 1000
        }),
      /Gemini returned HTTP 429/
    );

    assert.equal(callCount, 4);
  } finally {
    global.fetch = originalFetch;
  }
});
