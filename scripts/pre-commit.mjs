import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";

function run(command, args, options = {}) {
    const result = spawnSync(command, args, {
        encoding: "utf8",
        ...options,
    });

    if (result.error) {
        throw result.error;
    }

    if (result.status !== 0) {
        if (result.stderr) {
            process.stderr.write(result.stderr);
        }

        process.exit(result.status ?? 1);
    }

    return result.stdout;
}

if (!existsSync("MILESTONES.md")) {
    console.error("MILESTONES.md does not exist.");
    process.exit(1);
}

const output = run("bd", ["export", "--all"]);

const issues = output
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));

const markdown = issues
    .map(
        (issue) => `# ${issue.id} ${issue.title}

${issue.description ?? ""}

${issue.design ?? ""}

${issue.acceptance_criteria ?? ""}

${issue.notes ?? ""}
`
    )
    .join("\n---\n\n");

writeFileSync("MILESTONES.md", markdown);

console.log(`Updated MILESTONES.md with ${issues.length} Beads issues.`);