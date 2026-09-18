#!/usr/bin/env node

import { spawnSync } from "node:child_process";

function run(command, args) {
    const result = spawnSync(command, args, {
        encoding: "utf8",
        stdio: ["inherit", "pipe", "inherit"],
    });

    if (result.error) {
        throw result.error;
    }

    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }

    return result.stdout.trim();
}

const issueNumber = process.argv[2];

if (!issueNumber || !/^\d+$/.test(issueNumber)) {
    console.error("Usage: node scripts/issue.mjs <issue-number>");
    process.exit(1);
}

const repository = JSON.parse(
    run("gh", ["repo", "view", "--json", "nameWithOwner"]),
).nameWithOwner;

const issue = JSON.parse(
    run("gh", [
        "api",
        `repos/${repository}/issues/${issueNumber}`,
    ]),
);

console.log(`#${issue.number} ${issue.title}`);
console.log();

if (issue.state !== "open") {
    console.log(`State: ${issue.state}`);
    console.log();
}

if (issue.labels?.length) {
    console.log(`Labels: ${issue.labels.map((label) => label.name).join(", ")}`);
    console.log();
}

console.log(issue.body ?? "");