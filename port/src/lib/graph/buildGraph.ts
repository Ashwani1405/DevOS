/**
 * Relationship / graph builder.
 *
 * Derives the YASH graph from the site's content structure at build time. It
 * knows nothing about how content is rendered — it only produces a normalized
 * `{ nodes, edges }` payload for the YASH visualization.
 *
 * Two relationship types (per the content-driven spec):
 *
 *   A. STRUCTURAL — by content type. Every published writing item belongs to
 *      the WRITE trace, every published project to BUILD, every published
 *      shelf item to READ. Zero manual maintenance.
 *
 *   B. EXPLICIT — via each note's frontmatter `links` and its Obsidian
 *      [[wikilinks]] in the body, resolved against known content slug/title.
 *
 * The graph is regenerated on every build from the live content, so adding a
 * vault note automatically adds a node, and removing/unpublishing it removes it.
 */

import { getWriting, getProjects, getShelf } from "../content/index";
import type { TraceConfig } from "../../data/site";

export interface GraphNode {
	id: string;
	/** Human label shown in the UI. */
	label: string;
	/** life | trace | item */
	kind: "life" | "trace" | "item";
	/** Which trace this belongs to (for trace/item nodes). */
	trace?: string;
	/** Route to navigate to if clicked. Items link to their real page. */
	href?: string;
	/** Brief description, if available. */
	description?: string;
	/** Comma-joined metadata shown on hover (e.g. year / author). */
	meta?: string;
}

export interface GraphEdge {
	source: string;
	target: string;
}

export interface GraphData {
	nodes: GraphNode[];
	edges: GraphEdge[];
}

function extractWikilinks(body: string): string[] {
	const links = new Set<string>();
	const cleaned = body.replace(/```[\s\S]*?```/g, "");
	for (const m of cleaned.matchAll(/\[\[([^\]|]+)(?:\|[^\]]+)?\]\]/g)) {
		const target = m[1].trim().replace(/#.*$/, "").trim();
		if (target) links.add(target);
	}
	return [...links];
}

function slugify(value: string): string {
	return value
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-+|-+$/g, "")
		.replace(/-{2,}/g, "-");
}

export function normalizeLinks(frontmatterLinks: string[], body: string): string[] {
	const set = new Set<string>((frontmatterLinks || []).map((l) => l.trim()).filter(Boolean));
	for (const l of extractWikilinks(body)) set.add(l);
	return [...set];
}

export async function buildGraph(traces: TraceConfig[]): Promise<GraphData> {
	const nodes: GraphNode[] = [];
	const edges: GraphEdge[] = [];

	const lifeId = "yash";
	nodes.push({ id: lifeId, label: "YASH", kind: "life" });

	const byId = new Map<string, GraphNode>();
	const byTitle = new Map<string, string>();
	const byTitleLower = new Map<string, string>();

	const visibleTraces = traces.filter((t) => t.visible !== false);
	for (const t of visibleTraces) {
		const node: GraphNode = {
			id: `trace:${t.id}`,
			label: t.label,
			kind: "trace",
			trace: t.id,
		};
		nodes.push(node);
		byId.set(node.id, node);
		edges.push({ source: lifeId, target: node.id });
	}

	/** Queue of { nodeId, targetLabel[] } for deferred link resolution. */
	const pendingLinks: Array<{ nodeId: string; targets: string[] }> = [];

	function addTraceItems(
		traceId: string,
		items: Array<{
			slug: string;
			title: string;
			href: string;
			description?: string;
			meta?: string;
			frontmatterLinks?: string[];
			body?: string;
		}>,
	) {
		const t = visibleTraces.find((v) => v.id === traceId);
		if (!t) return;
		const traceNodeId = `trace:${traceId}`;
		for (const item of items) {
			const nodeId = slugify(item.slug || item.title);
			const node: GraphNode = {
				id: nodeId,
				label: item.title,
				kind: "item",
				trace: traceId,
				href: item.href,
				description: item.description,
				meta: item.meta,
			};
			nodes.push(node);
			// Keep the FIRST node for a given id (dedupe across collections).
			if (!byId.has(nodeId)) byId.set(nodeId, node);
			byTitle.set(item.title, nodeId);
			byTitleLower.set(item.title.toLowerCase(), nodeId);
			edges.push({ source: traceNodeId, target: nodeId });

			const targets = normalizeLinks(item.frontmatterLinks ?? [], item.body ?? "");
			if (targets.length) pendingLinks.push({ nodeId, targets });
		}
	}

	addTraceItems(
		"write",
		(await getWriting()).map((w) => ({
			slug: w.slug,
			title: w.title,
			href: `/writing/${w.slug}`,
			description: w.description,
			meta: w.readingTime,
			frontmatterLinks: w.links,
			body: w.body,
		})),
	);
	addTraceItems(
		"build",
		(await getProjects()).map((p) => ({
			slug: p.slug,
			title: p.title,
			href: `/projects/${p.slug}`,
			description: p.description,
			meta: String(p.year),
			frontmatterLinks: p.links,
			body: p.body,
		})),
	);
	addTraceItems(
		"read",
		(await getShelf()).map((s) => ({
			slug: s.slug,
			title: s.title,
			href: `/shelf/${s.slug}`,
			description: s.author,
			meta: s.status,
			frontmatterLinks: s.links,
			body: s.body,
		})),
	);

	// Deferred resolution — order-independent across all traces.
	for (const { nodeId, targets } of pendingLinks) {
		// Also fold body wikilinks extracted at the container level.
		for (const raw of targets) {
			const target = raw.trim();
			if (!target) continue;
			const slug = slugify(target);
			const resolved =
				byId.get(slug)?.id ||
				byTitle.get(target) ||
				byTitleLower.get(target.toLowerCase()) ||
				byTitleLower.get(slug);
			if (resolved && resolved !== nodeId) {
				edges.push({ source: nodeId, target: resolved });
			}
		}
	}

	return { nodes, edges };
}