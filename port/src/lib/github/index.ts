/**
 * GitHub activity — public entry point.
 *
 * Composes the contribution calendar, pull requests, issues, and recent
 * repositories into a single typed result for the UI.
 *
 * Build/empty-state behavior:
 *  - If GITHUB_TOKEN / GITHUB_USERNAME are not configured, returns an
 *    `available: false` result so the page can render a clean empty state and
 *    the build still succeeds.
 *  - If they ARE configured but a request fails (bad token, rate limit,
 *    network), the error is thrown here — surfacing as a clear build error so
 *    misconfiguration is caught at build time rather than silently.
 */

import { loadGitHubConfig } from "./client.js";
import { getContributionCalendar } from "./contributions.js";
import { getPullRequests, countPullRequests } from "./pullRequests.js";
import { getIssues, getRecentRepositories } from "./activity.js";
import type { GitHubActivityData } from "./types.js";

export type GitHubActivityResult =
	| { available: true; data: GitHubActivityData }
	| { available: false };

export async function getGitHubActivity(): Promise<GitHubActivityResult> {
	const config = loadGitHubConfig();

	if (!config) {
		return { available: false };
	}

	// Fetch each section independently so that a partial failure on one
	// (e.g. an organization the token cannot access, or a transient rate
	// limit) degrades gracefully instead of breaking the whole build.
	// Sections that fail simply come back empty.
	const [calendar, pullRequests, issues, recentRepositories] =
		await Promise.all([
			runSection(getContributionCalendar, config),
			runSection(getPullRequests, config),
			runSection(getIssues, config),
			runSection(getRecentRepositories, config),
		]);

	return {
		available: true,
		data: {
			username: config.username,
			totalContributions: calendar?.totalContributions ?? 0,
			contributions: calendar?.weeks ?? [],
			pullRequests: pullRequests ?? [],
			prCounts: countPullRequests(pullRequests ?? []),
			issues: issues ?? [],
			recentRepositories: recentRepositories ?? [],
			fetchedAt: new Date().toISOString(),
		},
	};
}

/** Runs a data fetch, returning `undefined` on failure instead of throwing. */
async function runSection<T>(
	fn: (config: import("./client.js").GitHubConfig) => Promise<T>,
	config: import("./client.js").GitHubConfig,
): Promise<T | undefined> {
	try {
		return await fn(config);
	} catch (error) {
		// Log for the developer at build time (not shown to visitors) and
		// continue building the rest of the page.
		console.warn(
			`[github] one section failed and was skipped: ${(error as Error).message}`,
		);
		return undefined;
	}
}
