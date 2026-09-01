#!/usr/bin/env node

import { execFileSync } from 'node:child_process';

function bd(args, options = {}) {
  try {
    return execFileSync('bd', args, {
      encoding: 'utf8',
      maxBuffer: 10 * 1024 * 1024,
      ...options,
    });
  } catch (error) {
    if (error.stderr) {
      process.stderr.write(error.stderr);
    }

    process.exit(error.status ?? 1);
  }
}

function readyIssues() {
  const output = bd(['ready', '--json']);
  const issues = JSON.parse(output);

  return issues
    .filter((issue) => issue.issue_type !== 'epic')
    .sort((a, b) => {
      const priority = (a.priority ?? 999) - (b.priority ?? 999);

      if (priority !== 0) {
        return priority;
      }

      return String(a.created_at ?? '').localeCompare(
        String(b.created_at ?? ''),
      );
    });
}

function nextIssue() {
  const issue = readyIssues()[0];

  if (!issue) {
    console.error('bd-next: no ready non-epic issues');
    process.exit(2);
  }

  return issue;
}

function compact(issue) {
  return {
    id: issue.id,
    priority: issue.priority,
    type: issue.issue_type,
    title: issue.title,
  };
}

function printCompact(issue) {
  console.log(
    `${issue.id}\tP${issue.priority}\t${issue.issue_type}\t${issue.title}`,
  );
}

function claim(issue) {
  const output = bd(['update', issue.id, '--claim', '--json']);

  const result = JSON.parse(output);
  const claimed = Array.isArray(result) ? result[0] : result;

  console.log(`${claimed.id}\t${claimed.status}`);

  return claimed.id;
}

function show(id) {
  execFileSync('bd', ['show', id], {
    stdio: 'inherit',
  });
}

function usage() {
  console.log(`
Usage:
  bd-next                     Show next ready non-epic issue
  bd-next --id                Print only its ID
  bd-next --json              Print compact JSON
  bd-next --list              List all ready non-epic issues
  bd-next --claim             Claim next issue
  bd-next --show              Show full next issue
  bd-next --claim --show      Claim it, then show that exact issue

Selection:
  - excludes epics
  - lowest numeric priority wins
  - created_at breaks priority ties
`);
}

const args = new Set(process.argv.slice(2));

if (args.has('--help') || args.has('-h')) {
  usage();
  process.exit(0);
}

if (args.has('--list')) {
  const issues = readyIssues();

  if (args.has('--json')) {
    console.log(JSON.stringify(issues.map(compact), null, 2));
  } else {
    for (const issue of issues) {
      printCompact(issue);
    }
  }

  process.exit(0);
}

const issue = nextIssue();

if (args.has('--claim')) {
  const id = claim(issue);

  // Important: show the issue we actually claimed.
  // Do not call bd ready again after claiming.
  if (args.has('--show')) {
    show(id);
  }

  process.exit(0);
}

if (args.has('--show')) {
  show(issue.id);
  process.exit(0);
}

if (args.has('--id')) {
  console.log(issue.id);
  process.exit(0);
}

if (args.has('--json')) {
  console.log(JSON.stringify(compact(issue)));
  process.exit(0);
}

printCompact(issue);
