/**
 * Contribution calendar data.
 *
 * Fetches the real GitHub contribution calendar via GraphQL and maps it onto
 * the contribution-week structure used by the UI. No approximation is made
 * from commit counts.
 */

import { graphql, type GitHubConfig } from "./client.js";
import type { ContributionDay, ContributionWeek } from "./types.js";

const CONTRIBUTIONS_QUERY = /* GraphQL */ `
  query GetContributions($login: String!) {
    user(login: $login) {
      contributionsCollection {
        contributionCalendar {
          totalContributions
          weeks {
            contributionDays {
              date
              contributionCount
              color
            }
          }
        }
      }
    }
  }
`;

interface ContributionsResponse {
	user: {
		contributionsCollection: {
			contributionCalendar: {
				totalContributions: number;
				weeks: Array<{
					contributionDays: Array<{
						date: string;
						contributionCount: number;
						color: string;
					}>;
				}>;
			};
		};
	};
}

interface ContributionsResult {
	totalContributions: number;
	weeks: ContributionWeek[];
}

export async function getContributionCalendar(
	config: GitHubConfig,
): Promise<ContributionsResult> {
	const data = await graphql<ContributionsResponse>(
		CONTRIBUTIONS_QUERY,
		{ login: config.username },
		config,
	);

	const weeks = data.user.contributionsCollection.contributionCalendar.weeks.map(
		(week) => ({
			days: week.contributionDays.map((day) => ({
				date: day.date,
				count: day.contributionCount,
				// Derive a 0..4 level from GitHub's color so the graph can be
				// re-styled later without touching the data layer.
				level: toLevel(day.color, day.contributionCount),
			})),
		}),
	);

	return {
		totalContributions:
			data.user.contributionsCollection.contributionCalendar.totalContributions,
		weeks,
	};
}

/**
 * Maps a GitHub contribution cell color to its 0..4 activity level, matching
 * GitHub's standard palette. Falls back to the contribution count when the
 * color is not recognized.
 */
function toLevel(color: string, count: number): number {
	const palette: Record<string, number> = {
		"#ebedf0": 0,
		"#9be9a8": 1,
		"#40c463": 2,
		"#30a14e": 3,
		"#216e39": 4,
		// GitLab-oriented / other themes occasionally return different shades.
		"#c6e48b": 1,
		"#7bc96f": 2,
		"#239a3b": 3,
		"#196127": 4,
	};
	const normalized = color.toLowerCase();
	if (palette[normalized] !== undefined) {
		return palette[normalized];
	}
	// Fallback based on the raw count.
	if (count <= 0) return 0;
	if (count < 3) return 1;
	if (count < 6) return 2;
	if (count < 10) return 3;
	return 4;
}
