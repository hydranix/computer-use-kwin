#!/usr/bin/env node

import { spawnSync } from "node:child_process";

const requiredFiles = [
	"pi/extension/index.ts",
	"pi/extension/generated-tools.ts",
	"pi/extension/mcp-client.bundle.cjs",
	"pi/extension/THIRD_PARTY_NOTICES.txt",
];
const maxUnpackedBytes = 750_000;

const result = spawnSync(
	"npm",
	["pack", "--dry-run", "--json"],
	{ encoding: "utf8" },
);
if (result.status !== 0) {
	process.stderr.write(result.stderr);
	process.exit(result.status ?? 1);
}

const metadata = JSON.parse(result.stdout);
const pack = Array.isArray(metadata) ? metadata[0] : Object.values(metadata)[0];
if (!pack || !Array.isArray(pack.files) || typeof pack.unpackedSize !== "number") {
	throw new Error("npm pack returned unexpected package metadata");
}
const paths = new Set(pack.files.map((file) => file.path));
const missing = requiredFiles.filter((path) => !paths.has(path));
if (missing.length > 0) {
	throw new Error(`npm package is missing required Pi files: ${missing.join(", ")}`);
}
if (pack.unpackedSize > maxUnpackedBytes) {
	throw new Error(
		`npm package unpacked size ${pack.unpackedSize} exceeds ${maxUnpackedBytes} bytes`,
	);
}

console.log(
	`npm package OK: ${pack.files.length} files, ${pack.size} bytes packed, ` +
		`${pack.unpackedSize} bytes unpacked`,
);
