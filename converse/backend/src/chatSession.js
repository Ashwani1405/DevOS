import fetch from "node-fetch";

const BASE = "https://api.on-demand.io/chat/v1";

function getApiKey() {
  const key = process.env.ONDEMAND_API_KEY;
  if (!key) {
    throw new Error("Missing ONDEMAND_API_KEY environment variable");
  }
  return key;
}

async function fetchWithRetries(url, options = {}, retries = 2, backoffMs = 300) {
  let lastErr;
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const res = await fetch(url, options);
      return res;
    } catch (err) {
      lastErr = err;
      if (attempt < retries) {
        await new Promise((r) => setTimeout(r, backoffMs * Math.pow(2, attempt)));
      }
    }
  }
  throw new Error(`Network request failed after ${retries + 1} attempts: ${lastErr?.message}`);
}

export async function createSession(userId) {
  const apiKey = process.env.ONDEMAND_API_KEY;

  // Fallback: return mock session if credentials not set
  if (!apiKey) {
    console.warn("Chat API credentials not configured. Returning mock session.");
    return `mock-session-${userId}-${Date.now()}`;
  }

  const res = await fetchWithRetries(`${BASE}/sessions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      apikey: apiKey
    },
    body: JSON.stringify({
      externalUserId: userId
    })
  });

  let json;
  try {
    json = await res.json();
  } catch (err) {
    throw new Error(`Invalid JSON in createSession response: ${err.message}`);
  }

  console.log("Create session response:", json);

  if (!res.ok) {
    throw new Error(
      `Chat session creation failed (${res.status}): ${JSON.stringify(json)}`
    );
  }

  if (!json?.data?.id) {
    throw new Error(
      `Unexpected createSession response: ${JSON.stringify(json)}`
    );
  }

  return json.data.id;
}

export async function sendMessage(sessionId, message) {
  const apiKey = process.env.ONDEMAND_API_KEY;

  // Fallback: return mock response if credentials not set
  if (!apiKey) {
    console.warn("Chat API credentials not configured. Returning mock response.");
    return {
      success: true,
      data: {
        content: `Mock response to: "${message}". To enable real chat, configure ONDEMAND_API_KEY environment variable.`,
        timestamp: new Date().toISOString()
      }
    };
  }

  const res = await fetchWithRetries(`${BASE}/sessions/${sessionId}/query`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      apikey: apiKey
    },
    body: JSON.stringify({
      query: message,
      endpointId: "predefined-openai-gpt4o",
      responseMode: "sync"
    })
  });

  let json;
  try {
    json = await res.json();
  } catch (err) {
    throw new Error(`Invalid JSON in sendMessage response: ${err.message}`);
  }

  console.log("Chat query response:", json);

  if (!res.ok) {
    throw new Error(
      `Chat query failed (${res.status}): ${JSON.stringify(json)}`
    );
  }

  return json;
}
