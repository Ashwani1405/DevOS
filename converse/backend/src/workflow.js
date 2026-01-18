import fetch from "node-fetch";

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

export async function executeWorkflow(message) {
  const apiKey = process.env.ONDEMAND_API_KEY;
  const workflowId = process.env.WORKFLOW_ID;

  if (!apiKey || !workflowId) {
    throw new Error("Workflow execution not configured");
  }

  const res = await fetchWithRetries(
    `https://api.on-demand.io/automation/api/workflow/${workflowId}/execute`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        apikey: apiKey,
      },
      body: JSON.stringify({
        input: { message },
      }),
    }
  );

  let json;
  try {
    json = await res.json();
  } catch (err) {
    throw new Error("Invalid JSON from workflow execute API");
  }

  if (!res.ok) {
    throw new Error(
      `Workflow execute failed (${res.status}): ${JSON.stringify(json)}`
    );
  }

  const executionId = json.id || json.executionId || json.executionID;

  if (!executionId) {
    throw new Error(
      `Workflow did not return executionId: ${JSON.stringify(json)}`
    );
  }

  return executionId;
}
