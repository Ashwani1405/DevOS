/**
 * Domain types for GitHub activity.
 *
 * These are the shapes surfaced to the UI. They are intentionally decoupled
 * from the raw GitHub API/GraphQL response payloads so the data-fetching layer
 * can change without touching component/UI code.
 */

export interface ContributionDay {
	/** ISO date, e.g. "2024-06-01". */
	date: string;
	/** Number of contributions that day (0..N). */
	count: number;
	/** GitHub's own level 0..4 for the cell shade. */
	level: number;
}

export interface ContributionWeek {
	/** Days in the week; short weeks are shorter arrays (padded at build time). */
	days: ContributionDay[];
}

export type PullRequestState = "open" | "merged" | "closed";

export interface PullRequest {
	/** e.g. "owner/name". */
	repository: string;
	number: number;
	title: string;
	state: PullRequestState;
	createdAt: string;
	/** ISO timestamp, present when available (merged/updated). */
	updatedAt?: string;
	mergedAt?: string;
	/** Last activity timestamp (created/updated/merged) for sorting. */
	updatedDate: string;
	url: string;
	/** Primary language of the head repository, when available. */
	language?: string;
	/** Number of comments, when available. */
	reviewsCount?: number;
}

export interface Issue {
	repository: string;
	number: number;
	title: string;
	state: "open" | "closed";
	createdAt: string;
	url: string;
}

export interface RecentRepository {
	owner: string;
	name: string;
	description?: string;
	language?: string;
	url: string;
	/** Last updated ISO timestamp from GitHub. */
	updatedAt: string;
}

export interface GitHubActivityData {
	username: string;
	/** Total contributions in the current contribution year. */
	totalContributions: number;
	/** Full contribution calendar, week-by-week, over the last year. */
	contributions: ContributionWeek[];
	pullRequests: PullRequest[];
	/** Aggregate PR counts. */
	prCounts: {
		open: number;
		merged: number;
		closed: number;
		total: number;
	};
	issues: Issue[];
	recentRepositories: RecentRepository[];
	fetchedAt: string;
}

export interface GitHubConfig {
	username: string;
	token: string;
}
