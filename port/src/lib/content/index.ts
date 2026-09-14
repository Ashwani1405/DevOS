/**
 * Content data-access layer.
 *
 * The single place pages and components read published content from. Everything
 * here is derived from the content collections (which `npm run sync` regenerates
 * from the canonical Obsidian vault). No content is hardcoded in components.
 *
 * Each helper filters out unpublished/`draft`/`status: draft` entries so that
 * adding/removing a note in the vault automatically appears or disappears here.
 */

import { getCollection, type CollectionEntry } from "astro:content";

type AnyEntry = CollectionEntry<"writing"> | CollectionEntry<"projects"> | CollectionEntry<"shelf"> | CollectionEntry<"work"> | CollectionEntry<"content">;

/** Detect the 'unpublish' signal for a content entry regardless of schema shape. */
function isPublished(data: Record<string, unknown>): boolean {
	if (data.published !== undefined) return data.published === true;
	if (data.status === "draft") return false;
	if ("draft" in data) return (data as { draft: boolean }).draft !== true;
	return true;
}

export interface LoadedWriting {
	id: string;
	slug: string;
	title: string;
	date: Date;
	description: string;
	tags: string[];
	readingTime: string;
	links: string[];
	body: string;
}

export interface LoadedProject {
	id: string;
	slug: string;
	title: string;
	description: string;
	longDescription: string;
	year: number | string;
	status: string;
	featured: boolean;
	technologies: string[];
	github?: string;
	live?: string;
	links: string[];
	body: string;
}

export interface LoadedShelf {
	id: string;
	slug: string;
	title: string;
	author: string;
	type: string;
	status: string;
	category: string;
	notes?: string;
	rating?: string | number;
	link?: string;
	links: string[];
	body: string;
}

export interface LoadedWork {
	id: string;
	slug: string;
	title: string;
	company: string;
	startDate: string;
	endDate: string | null;
	location?: string;
	remote: boolean;
	description: string;
	technologies: string[];
	bullets: string[];
	links: string[];
	body: string;
}

export interface LoadedPost {
	id: string;
	slug: string;
	title: string;
	kind: string;
	links: string[];
	body: string;
}

function toLinks(links: unknown): string[] {
	if (!Array.isArray(links)) return [];
	return links.map((l) => String(l).trim()).filter(Boolean);
}

function bodyOf(entry: AnyEntry): string {
	return entry.body?.trim() ?? "";
}

export async function getWriting(): Promise<LoadedWriting[]> {
	const entries = await getCollection("writing");
	return entries
		.filter((e) => isPublished(e.data as never))
		.map((e) => ({
			id: e.id,
			slug: (e.data.slug as string) || e.id,
			title: e.data.title,
			date: e.data.date,
			description: e.data.description,
			tags: e.data.tags ?? [],
			readingTime: e.data.readingTime,
			links: toLinks(e.data.links),
			body: bodyOf(e),
		}))
		.sort((a, b) => b.date.valueOf() - a.date.valueOf());
}

export async function getProjects(): Promise<LoadedProject[]> {
	const entries = await getCollection("projects");
	return entries
		.filter((e) => isPublished(e.data as never))
		.map((e) => ({
			id: e.id,
			slug: (e.data.slug as string) || e.id,
			title: e.data.title,
			description: e.data.description,
			longDescription: (e.data.longDescription as string) || "",
			year: e.data.year,
			status: e.data.status,
			featured: e.data.featured === true,
			technologies: e.data.technologies ?? [],
			github: e.data.github,
			live: e.data.live,
			links: toLinks(e.data.links),
			body: bodyOf(e),
		}))
		.sort((a, b) => String(b.year).localeCompare(String(a.year)));
}

export async function getShelf(): Promise<LoadedShelf[]> {
	const entries = await getCollection("shelf");
	return entries
		.filter((e) => isPublished(e.data as never))
		.map((e) => ({
			id: e.id,
			slug: (e.data.slug as string) || e.id,
			title: e.data.title,
			author: e.data.author,
			type: e.data.type,
			status: e.data.status,
			category: e.data.category,
			notes: e.data.notes,
			rating: e.data.rating,
			link: e.data.link,
			links: toLinks(e.data.links),
			body: bodyOf(e),
		}));
}

export async function getWork(): Promise<LoadedWork[]> {
	const entries = await getCollection("work");
	return entries
		.filter((e) => isPublished(e.data as never))
		.map((e) => ({
			id: e.id,
			slug: (e.data.slug as string) || e.id,
			title: e.data.title,
			company: e.data.company,
			startDate: e.data.startDate,
			endDate: e.data.endDate ?? null,
			location: e.data.location,
			remote: e.data.remote === true,
			description: e.data.description,
			technologies: e.data.technologies ?? [],
			bullets: e.data.bullets ?? [],
			links: toLinks(e.data.links),
			body: bodyOf(e),
		}))
		.sort(
			(a, b) =>
				(b.endDate ?? b.startDate).localeCompare(a.endDate ?? a.startDate),
		);
}

export async function getPosts(): Promise<LoadedPost[]> {
	const entries = await getCollection("content");
	return entries
		.filter((e) => isPublished(e.data as never))
		.map((e) => ({
			id: e.id,
			slug: (e.data.slug as string) || e.id,
			title: e.data.title,
			kind: (e.data.kind as string) || "about",
			links: toLinks(e.data.links),
			body: bodyOf(e),
		}));
}

export async function getPost(kind: string): Promise<LoadedPost | undefined> {
	const posts = await getPosts();
	return posts.find((p) => p.kind === kind);
}