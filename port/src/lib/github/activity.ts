/**
 * General activity data: issues authored by the user and recently-updated
 * repositories. Kept separate from contributions and pull requests so the
 * page can compose them as needed.
 */

import { graphql, type GitHubConfig } from "./client.js";
import type { Issue, RecentRepository } from "./types.js";

const ISSUES_QUERY = /* GraphQL */ `
  query GetIssues($first: Int!) {
    search(query: "author:LOGIN is:issue is:public", type: ISSUE, first: $first) {
      edges {
        node {
          ... on Issue {
            repository {
              nameWithOwner
            }
            number
            title
            state
            createdAt
            url
          }
        }
      }
    }
  }
`;

const SEARCH_QUERY_PLACEHOLDER = "author:LOGIN";

const REPOSITORIES_QUERY = /* GraphQL */ `
  query GetRepositories($login: String!, $first: Int!) {
    user(login: $login) {
      repositories(
        first: $first
        orderBy: { field: UPDATED_AT, direction: DESC }
        ownerAffiliations: OWNER
      ) {
        nodes {
          nameWithOwner
          description
          primaryLanguage {
            name
          }
          url
          updatedAt
        }
      }
    }
  }
`;

	interface IssuesResponse {
	search: {
		edges: Array<{
			node: {
				repository: { nameWithOwner: string };
				number: number;
				title: string;
				state: "OPEN" | "CLOSED";
				createdAt: string;
				url: string;
			};
		}>;
	};
}

interface RepositoriesResponse {
	user: {
		repositories: {
			nodes: Array<{
				nameWithOwner: string;
				description: string | null;
				primaryLanguage?: { name: string } | null;
				url: string;
				updatedAt: string;
			}>;
		};
	};
}

export async function getIssues(config: GitHubConfig, limit = 20): Promise<Issue[]> {
	const query = ISSUES_QUERY.replace(
		SEARCH_QUERY_PLACEHOLDER,
		`author:${config.username}`,
	);
	const data = await graphql<IssuesResponse>(
		query,
		{ first: limit },
		config,
	);

	return data.search.edges.map((edge) => {
		const node = edge.node;
		return {
			repository: node.repository.nameWithOwner,
			number: node.number,
			title: node.title,
			state: node.state === "OPEN" ? "open" : "closed",
			createdAt: node.createdAt,
			url: node.url,
		};
	});
}

export async function getRecentRepositories(
	config: GitHubConfig,
	limit = 10,
): Promise<RecentRepository[]> {
	const data = await graphql<RepositoriesResponse>(
		REPOSITORIES_QUERY,
		{ login: config.username, first: limit },
		config,
	);

	const nodes = data.user?.repositories?.nodes ?? [];
	return nodes.map((repo) => {
		return {
			owner: repo.nameWithOwner.split("/")[0],
			name: repo.nameWithOwner.split("/").slice(1).join("/"),
			description: repo.description ?? undefined,
			language: repo.primaryLanguage?.name,
			url: repo.url,
			updatedAt: repo.updatedAt,
		};
	});
}
