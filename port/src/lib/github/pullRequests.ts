/**
 * Pull request data.
 *
 * Uses GitHub's cross-repository search (GraphQL) to fetch pull requests
 * authored by the user across ALL repositories and organizations. No
 * repository or organization allowlist is required — GitHub does the
 * discovery for us.
 *
 * Supports pagination so we can fetch a larger set without loading
 * everything into the UI at once.
 */

import { graphql, type GitHubConfig } from "./client.js";
import type { PullRequest, PullRequestState } from "./types.js";

const PULL_REQUESTS_QUERY = /* GraphQL */ `
  query GetPullRequests($first: Int!, $after: String) {
    search(
      query: "author:LOGIN is:pr is:public"
      type: ISSUE
      first: $first
      after: $after
    ) {
      pageInfo {
        hasNextPage
        endCursor
      }
      edges {
        node {
          ... on PullRequest {
            repository {
              nameWithOwner
              primaryLanguage {
                name
              }
            }
            number
            title
            state
            createdAt
            updatedAt
            mergedAt
            url
            reviews {
              totalCount
            }
          }
        }
      }
    }
  }
`;

const SEARCH_QUERY_PLACEHOLDER = "author:LOGIN";

const PAGE_SIZE = 50;

interface PullRequestsResponse {
	search: {
		pageInfo: {
			hasNextPage: boolean;
			endCursor: string | null;
		};
		edges: Array<{
			node: {
				repository: {
					nameWithOwner: string;
					primaryLanguage?: { name: string } | null;
				};
				number: number;
				title: string;
				state: "OPEN" | "MERGED" | "CLOSED";
				createdAt: string;
				updatedAt: string;
				mergedAt: string | null;
				url: string;
				reviews: { totalCount: number };
			};
		}>;
	};
}

function toState(state: "OPEN" | "MERGED" | "CLOSED"): PullRequestState {
	switch (state) {
		case "MERGED":
			return "merged";
		case "CLOSED":
			return "closed";
		default:
			return "open";
	}
}

/**
 * Fetch pull requests authored by the given user.
 *
 * @param config        GitHub credentials.
 * @param maxItems      Target maximum number of PRs to fetch; the actual count
 *                      may be lower if there are fewer. Defaults to 100.
 */
export async function getPullRequests(
	config: GitHubConfig,
	maxItems = 100,
): Promise<PullRequest[]> {
	const pullRequests: PullRequest[] = [];
	let after: string | null = null;

	while (pullRequests.length < maxItems) {
		const pageSize = Math.min(PAGE_SIZE, maxItems - pullRequests.length);
		const query = PULL_REQUESTS_QUERY.replace(
			SEARCH_QUERY_PLACEHOLDER,
			`author:${config.username}`,
		);
		const data = await graphql<PullRequestsResponse>(
			query,
			{ first: pageSize, after },
			config,
		);

		for (const edge of data.search.edges) {
			const pr = edge.node;
			const state = toState(pr.state);
			pullRequests.push({
				repository: pr.repository.nameWithOwner,
				number: pr.number,
				title: pr.title,
				state,
				createdAt: pr.createdAt,
				updatedAt: pr.updatedAt || undefined,
				mergedAt: pr.mergedAt || undefined,
				// Most-recent activity for stable sorting.
				updatedDate: pr.mergedAt ?? pr.updatedAt ?? pr.createdAt,
				url: pr.url,
				language: pr.repository.primaryLanguage?.name,
				reviewsCount: pr.reviews.totalCount,
			});
		}

		if (!data.search.pageInfo.hasNextPage || !data.search.pageInfo.endCursor) {
			break;
		}
		after = data.search.pageInfo.endCursor;
	}

	return pullRequests.sort((a, b) =>
		b.updatedDate.localeCompare(a.updatedDate),
	);
}

/** Simple aggregate counts over the fetched PR set. */
export function countPullRequests(pullRequests: PullRequest[]) {
	return {
		open: pullRequests.filter((pr) => pr.state === "open").length,
		merged: pullRequests.filter((pr) => pr.state === "merged").length,
		closed: pullRequests.filter((pr) => pr.state === "closed").length,
		total: pullRequests.length,
	};
}
