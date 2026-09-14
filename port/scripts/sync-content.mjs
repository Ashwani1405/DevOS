#!/usr/bin/env node
/**
 * Site content sync.
 *
 * Copies the committed site snapshot `content/` (the deployable vault generated
 * from the canonical Obsidian vault by `npm run sync:obsidian`) into Astro's
 * build-time content:
 *
 *   content/{writing,projects,shelf,work,content} -> src/content/{same}
 *   content/config/site.json                      -> src/data/site.json
 *
 * The committed `src/content` and `src/data/site.json` are what the site
 * actually builds from, so a deployment (which cannot read your local
 * filesystem) stays fully standalone. Run `npm run sync` after changing the
 * snapshot, or `npm run sync:obsidian` to import from the canonical vault first.
 */

import {
	readdirSync,
	copyFileSync,
	mkdirSync,
	existsSync,
	rmSync,
} from "node:fs";
import { join, dirname } from "node:path";

const ROOT = process.cwd();
const VAULT = join(ROOT, "content");
const SRC = join(ROOT, "src");

const COLLECTIONS = ["writing", "projects", "shelf", "work", "content"];

function walk(dir) {
	const out = [];
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (entry.name.startsWith(".")) continue;
		const full = join(dir, entry.name);
		if (entry.isDirectory()) out.push(...walk(full));
		else out.push(full);
	}
	return out;
}

function copyTree(srcDir, destDir) {
	if (!existsSync(srcDir)) return 0;
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

const total = {};
for (const collection of COLLECTIONS) {
	const n = copyTree(join(VAULT, collection), join(SRC, "content", collection));
	total[collection] = n;
	console.log(`[sync] ${collection}: ${n} file(s) -> src/content/${collection}`);
}

// Site configuration (identity, nav, traces, homepage sections).
const siteConfigSrc = join(VAULT, "config", "site.json");
if (existsSync(siteConfigSrc)) {
	mkdirSync(join(SRC, "data"), { recursive: true });
	copyFileSync(siteConfigSrc, join(SRC, "data", "site.json"));
	console.log(`[sync] site config -> src/data/site.json`);
}