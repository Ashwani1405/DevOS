import fetch from "node-fetch";

const BASE = "https://api.on-demand.io/chat/v1";

async function fetchWithRetries(url, options = {}, retries = 2, backoffMs = 300) {
  let lastErr;
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const res = await fetch(url, options);
      return res;
    } catch (err) {
      lastErr = err;
      if (attempt < retries) {
        await new Promise((r) =>
          setTimeout(r, backoffMs * Math.pow(2, attempt))
        );
      }
    }
  }
  throw new Error(
    `Network request failed after ${retries + 1} attempts: ${lastErr?.message}`
  );
}

export async function createSession(userId) {
  const apiKey = process.env.ONDEMAND_API_KEY;

  // Safe fallback: chat-only mock session
  if (!apiKey) {
    console.warn("Chat API not configured. Using mock chat session.");
    return `mock-session-${userId}-${Date.now()}`;
  }

  const res = await fetchWithRetries(`${BASE}/sessions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      apikey: apiKey,
    },
    body: JSON.stringify({
      externalUserId: userId,
    }),
  });

  const json = await res.json();

  if (!res.ok || !json?.data?.id) {
    throw new Error(
      `Chat session creation failed: ${JSON.stringify(json)}`
    );
  }

  return json.data.id;
}

export async function sendMessage(sessionId, message) {
  const apiKey = process.env.ONDEMAND_API_KEY;

  // Safe fallback: pure conversational response
  if (!apiKey) {
    return {
      success: true,
      data: {
        content: `Chat response (mock): ${message}`,
        timestamp: new Date().toISOString(),
      },
    };
  }

  const res = await fetchWithRetries(
    `${BASE}/sessions/${sessionId}/query`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        apikey: apiKey,
      },
      body: JSON.stringify({
        query: message,
        endpointId: "predefined-openai-gpt4o",
        responseMode: "sync",
      }),
    }
  );

  const json = await res.json();

  if (!res.ok) {
    throw new Error(`Chat query failed: ${JSON.stringify(json)}`);
  }

  return json;
}
