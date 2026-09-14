import { defineCollection, z } from "astro:content";
import { glob } from "astro/loaders";
import type { SchemaContext } from "astro:content";

// Astro 7 uses the YAML frontmatter of each content file directly. The loader
// base points at the *committed build input* in `src/content/{collection}`,
// which `npm run sync` regenerates from the canonical Obsidian vault.
// The committed files are a deployable snapshot — never edited by hand.

// Shared publication gate + stable slug so any content item can be shown or
// hidden and referenced from the YASH graph without touching component code.
const published = (slugExample: string) =>
	z.object({
		title: z.string(),
		slug: z.string().default(slugExample),
		published: z.boolean().default(true),
		// Space-separated Obsidian wikilinks / explicit relationship targets
		// resolved by the graph builder into edges (optional).
		links: z.array(z.string()).default([]),
	});

export const writing = defineCollection({
	loader: glob({ pattern: "**/*.md", base: "./src/content/writing" }),
	schema: ({ image }: SchemaContext) =>
		published("writing").extend({
			date: z.coerce.date(),
			description: z.string(),
			tags: z.array(z.string()).default([]),
			readingTime: z.string(),
			// Back-compat with older `status: published|draft`.
			status: z.enum(["published", "draft"]).optional(),
		}),
});

export const projects = defineCollection({
	loader: glob({ pattern: "**/*.md", base: "./src/content/projects" }),
	schema: ({ image }: SchemaContext) =>
		published("project").extend({
			description: z.string(),
			longDescription: z.string().optional().default(""),
			year: z.union([z.number(), z.string()]),
			status: z.enum([
				"active",
				"maintaining",
				"archived",
				"in progress",
				"planned",
			]),
			featured: z.boolean().optional().default(false),
			technologies: z.array(z.string()).default([]),
			github: z.string().optional(),
			live: z.string().optional(),
			image: z.string().optional(),
		}),
});

export const shelf = defineCollection({
	loader: glob({ pattern: "**/*.md", base: "./src/content/shelf" }),
	schema: ({ image }: SchemaContext) =>
		published("shelf-item").extend({
			author: z.string(),
			type: z.enum(["book", "paper", "article", "film", "other"]).default("book"),
			status: z
				.enum(["currently reading", "recently read", "want to read", "read"])
				.default("currently reading"),
			category: z.string().default("Other"),
			notes: z.string().optional(),
			rating: z.union([z.string(), z.number()]).optional(),
			link: z.string().optional(),
			pages: z.number().optional(),
			isbn: z.string().optional(),
		}),
});

export const work = defineCollection({
	loader: glob({ pattern: "**/*.md", base: "./src/content/work" }),
	schema: ({ image }: SchemaContext) =>
		z.object({
			title: z.string(), // Role.
			slug: z.string(),
			published: z.boolean().default(true),
			company: z.string(),
			startDate: z.string(),
			endDate: z.string().nullable().default(null),
			location: z.string().optional(),
			remote: z.boolean().optional().default(false),
			description: z.string(),
			technologies: z.array(z.string()).default([]),
			bullets: z.array(z.string()).default([]),
			links: z.array(z.string()).default([]),
		}),
});

export const content = defineCollection({
	loader: glob({ pattern: "**/*.md", base: "./src/content/content" }),
	schema: ({ image }: SchemaContext) =>
		z.object({
			title: z.string(),
			slug: z.string(),
			published: z.boolean().default(true),
			// Currently used for the personal (Yash) pages: about, now.
			kind: z
				.enum(["about", "now", "elsewhere", "bio"])
				.optional()
				.default("about"),
			links: z.array(z.string()).default([]),
			body: z.string().optional(),
		}),
});

export const collections = { writing, projects, shelf, work, content };