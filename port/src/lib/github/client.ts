/**
 * GitHub GraphQL client.
 *
 * This module is only ever executed at build time (during `astro build` / page
 * rendering on the server). It reads the token from the environment and is
 * never imported by client-side code, so the token cannot leak into browser
 * bundles.
 *
 * Required environment variables:
 *   GITHUB_TOKEN     a GitHub Personal Access Token (read scope)
 *   GITHUB_USERNAME  the GitHub login to query
 */

import type { GitHubConfig } from "./types.js";

const GITHUB_GRAPHQL_ENDPOINT = "https://api.github.com/graphql";

export function loadGitHubConfig(): GitHubConfig | null {
	const token = process.env.GITHUB_TOKEN;
	const username = process.env.GITHUB_USERNAME;

	if (!token || !username) {
		return null;
	}

	return { token, username };
}

export async function graphql<T>(
	query: string,
	variables: Record<string, unknown>,
	config: GitHubConfig,
): Promise<T> {
	let response: Response;
	try {
		response = await fetch(GITHUB_GRAPHQL_ENDPOINT, {
			method: "POST",
			headers: {
				Authorization: `Bearer ${config.token}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({ query, variables }),
		});
	} catch (error) {
		throw new Error(
			`GitHub GraphQL request failed (network). Check your connection and token.`,
			{ cause: error },
		);
	}

	const body = (await response.json()) as {
		data?: unknown;
		errors?: Array<{ message: string }>;
	};

	if (!response.ok || body.errors?.length) {
		const detail = body.errors?.map((e) => e.message).join("; ");
		throw new Error(
			`GitHub GraphQL request failed (${response.status}): ${detail ?? "unknown error"}`,
		);
	}

	return body.data as T;
}
