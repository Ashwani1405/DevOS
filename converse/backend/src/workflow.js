import fetch from "node-fetch";

function getApiKey() {
  const key = process.env.ONDEMAND_API_KEY;
  if (!key) {
    throw new Error("Missing ONDEMAND_API_KEY environment variable");
  }
  return key;
}

function getWorkflowId() {
  const id = process.env.WORKFLOW_ID;
  if (!id) {
    throw new Error("Missing WORKFLOW_ID environment variable");
  }
  return id;
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

export async function executeWorkflow(message) {
  // Check if API credentials are configured
  const apiKey = process.env.ONDEMAND_API_KEY;
  const workflowId = process.env.WORKFLOW_ID;

  // If credentials are not properly set, don't attempt API call
  const isInvalidApiKey = !apiKey || apiKey.includes('your_') || apiKey.toLowerCase() === 'none';
  const isInvalidWorkflowId = !workflowId || workflowId.includes('your_') || workflowId.toLowerCase() === 'none';
  
  if (isInvalidApiKey || isInvalidWorkflowId) {
    return null; // Return null to indicate workflow not configured
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
    },
  );

  let json;
  try {
    json = await res.json();
  } catch (err) {
    throw new Error(`Invalid JSON in workflow response: ${err.message}`);
  }

  if (!res.ok) {
    console.error(`Workflow API error (${res.status}):`, json);
    return null; // Return null on error instead of throwing or returning mock
  }

  return json;
}
