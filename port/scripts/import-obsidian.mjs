#!/usr/bin/env node
/**
 * Obsidian vault -> committed site snapshot.
 *
 * This is the build-time import from the CANONICAL source of truth: the real
 * Obsidian vault (configured via OBSIDIAN_VAULT_PATH, defaulting to
 * ~/Documents/Obsidian Vault). It regenerates the committed, deployable
 * snapshot under `content/` (the site vault). Run `npm run sync` afterwards to
 * copy the snapshot into Astro's build-time `src/content`.
 *
 * Directory mapping (from the canonical vault into the site snapshot):
 *   Writing/ .. content/writing/
 *   Projects/ . content/projects/
 *   Shelf/ ... content/shelf/
 *   Work/ .... content/work/
 *   Yash/ .... content/content/
 *   Config/ .. content/config/
 *
 * The canonical vault is the one, hand-authored source. `content/` is a
 * GENERATED snapshot used for deployment and CI — never the canonical vault.
 *
 * Files are copied verbatim. Relationship — explicit frontmatter `links` and
 * Obsidian `[[wikilinks]]` in the body — are resolved by the graph builder at
 * build time, not rewritten here.
 *
 * If the vault path is missing, the script reports a notice and exits 0; the
 * site still builds from the already-committed snapshot.
 */

import {
	readdirSync,
	copyFileSync,
	mkdirSync,
	existsSync,
	rmSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { homedir } from "node:os";
import { readFileSync } from "node:fs";

// ---------------------------------------------------------------------------
// .env loading (no dependencies). Already-set process.env wins.
// ---------------------------------------------------------------------------
function loadEnvFile() {
	const envPath = join(process.cwd(), ".env");
	if (!existsSync(envPath)) return;
	for (const line of readFileSync(envPath, "utf8").split("\n")) {
		const trimmed = line.trim();
		if (!trimmed || trimmed.startsWith("#")) continue;
		const eq = trimmed.indexOf("=");
		if (eq === -1) continue;
		const key = trimmed.slice(0, eq).trim();
		const value = trimmed.slice(eq + 1).trim().replace(/^["']|["']$/g, "");
		if (!(key in process.env)) process.env[key] = value;
	}
}

function walk(dir, acc = []) {
	if (!existsSync(dir)) return acc;
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (entry.name.startsWith(".")) continue;
		const full = join(dir, entry.name);
		if (entry.isDirectory()) walk(full, acc);
		else if (entry.isFile() && entry.name.endsWith(".md")) acc.push(full);
	}
	return acc;
}

function copyMarkdown(srcDir, destDir) {
	if (!existsSync(srcDir)) return 0;
	// Destination is a generated snapshot — clear it so it exactly mirrors the
	// vault (removed vault notes disappear from the snapshot too).
	rmSync(destDir, { recursive: true, force: true });
	mkdirSync(destDir, { recursive: true });
	let n = 0;
	for (const file of walk(srcDir)) {
		const rel = file.slice(srcDir.length + 1);
		const dest = join(destDir, rel);
		mkdirSync(dirname(dest), { recursive: true });
		copyFileSync(file, dest);
		n += 1;
	}
	return n;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
loadEnvFile();

const vault =
	process.env.OBSIDIAN_VAULT_PATH || join(homedir(), "Documents", "Obsidian Vault");
const ROOT = process.cwd();
const SNAPSHOT = join(ROOT, "content");

const MAPPING = {
	Writing: "writing",
	Projects: "projects",
	Shelf: "shelf",
	Work: "work",
	Yash: "content",
};

const total = {};
for (const [from, to] of Object.entries(MAPPING)) {
	const srcDir = join(vault, from);
	const destDir = join(SNAPSHOT, to);
	if (!existsSync(srcDir)) {
		console.log(`[import-obsidian] missing "${from}/" in vault — skipped.`);
		continue;
	}
	const n = copyMarkdown(srcDir, destDir);
	total[to] = n;
	console.log(`[import-obsidian] ${n} file(s) "${from}" -> content/${to}`);
}

// Config: copy the vault's Config/* files verbatim (e.g. site.json).
const cfgSrc = join(vault, "Config");
const cfgDest = join(SNAPSHOT, "config");
if (existsSync(cfgSrc)) {
	rmSync(cfgDest, { recursive: true, force: true });
	mkdirSync(cfgDest, { recursive: true });
	let n = 0;
	for (const entry of readdirSync(cfgSrc, { withFileTypes: true })) {
		if (entry.name.startsWith(".")) continue;
		const full = join(cfgSrc, entry.name);
		if (entry.isFile()) {
			copyFileSync(full, join(cfgDest, entry.name));
			n += 1;
		}
	}
	total["config"] = n;
	console.log(`[import-obsidian] ${n} file(s) "Config" -> content/config`);
}

if (Object.keys(total).length === 0) {
	console.log(
		`[import-obsidian] no content imported from "${vault}". Set OBSIDIAN_VAULT_PATH in .env to your canonical vault.`,
	);
	process.exit(0);
}

console.log(
	`[import-obsidian] imported from "${vault}" into content/. Run "npm run sync" to copy into Astro build-time content.`,
);