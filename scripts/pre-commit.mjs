import { spawnSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
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

function formatDate(value) {
  if (!value) return null;
  return value.slice(0, 10);
}

function compact(values) {
  return values.filter(Boolean);
}

function renderSection(title, content) {
  if (!content?.trim()) return null;

  return `### ${title}

${content.trim()}`;
}

function renderIssue(issue) {
  const metadata = compact([
    `**Status:** ${issue.status}`,
    issue.priority != null ? `**Priority:** P${issue.priority}` : null,
    issue.issue_type ? `**Type:** ${issue.issue_type}` : null,
  ]).join(' · ');

  const people = compact([
    issue.owner ? `**Owner:** ${issue.owner}` : null,
    issue.assignee ? `**Assignee:** ${issue.assignee}` : null,
  ]).join(' · ');

  const dates = compact([
    formatDate(issue.created_at)
      ? `**Created:** ${formatDate(issue.created_at)}`
      : null,
    formatDate(issue.started_at)
      ? `**Started:** ${formatDate(issue.started_at)}`
      : null,
    formatDate(issue.updated_at)
      ? `**Updated:** ${formatDate(issue.updated_at)}`
      : null,
    formatDate(issue.closed_at)
      ? `**Closed:** ${formatDate(issue.closed_at)}`
      : null,
  ]).join(' · ');

  const labels =
    issue.labels?.length > 0
      ? `**Labels:** ${issue.labels.join(', ')}`
      : null;

  const sections = compact([
    issue.close_reason
      ? `**Close reason:** ${issue.close_reason.trim()}`
      : null,

    renderSection('Description', issue.description),
    renderSection('Design', issue.design),
    renderSection('Acceptance Criteria', issue.acceptance_criteria),
    renderSection('Notes', issue.notes),

    labels,
  ]);

  return `---

## ${issue.id} — ${issue.title}

${compact([metadata, people, dates, ...sections]).join('\n\n')}
`;
}

if (!existsSync('MILESTONES.md')) {
  console.error('MILESTONES.md does not exist.');
  process.exit(1);
}

const output = run('bd', ['export']);

const issues = output
  .split('\n')
  .filter(Boolean)
  .map((line) => JSON.parse(line))
  .filter((issue) => typeof issue.id === 'string')
  .sort((a, b) => a.id.localeCompare(b.id));

function cell(value) {
  if (value == null) return '';

  return String(value)
    .replace(/\|/g, '\\|')
    .replace(/\r?\n+/g, '<br>')
    .trim();
}

function getParentId(issue) {
  const parent = issue.dependencies?.find(
    (dependency) =>
      dependency.type === 'parent-child' ||
      dependency.dependency_type === 'parent-child',
  );

  return parent?.depends_on_id ?? parent?.id ?? null;
}

const issuesById = new Map(issues.map((issue) => [issue.id, issue]));

const childrenByParent = new Map();

for (const issue of issues) {
  const parentId = getParentId(issue);

  if (!parentId || !issuesById.has(parentId)) {
    continue;
  }

  const children = childrenByParent.get(parentId) ?? [];
  children.push(issue);
  childrenByParent.set(parentId, children);
}

for (const children of childrenByParent.values()) {
  children.sort((a, b) => a.id.localeCompare(b.id));
}

function renderRow(issue, depth = 0) {
  const indent = '&nbsp;&nbsp;'.repeat(depth);
  const prefix = depth > 0 ? '↳ ' : '';

  const isParent = childrenByParent.has(issue.id);

  const id = isParent
    ? `**${cell(issue.id)}**`
    : `${indent}${prefix}${cell(issue.id)}`;

  const title = isParent
    ? `**${cell(issue.title)}**`
    : cell(issue.title);

  return [
    id,
    title,
    cell(issue.status),
    issue.priority != null ? `P${issue.priority}` : '',
    cell(issue.issue_type),
    cell('haitchstubbs'),
    cell(issue.labels?.join(', ')),
  ];
}

function flattenIssue(issue, depth = 0) {
  const rows = [renderRow(issue, depth)];

  for (const child of childrenByParent.get(issue.id) ?? []) {
    rows.push(...flattenIssue(child, depth + 1));
  }

  return rows;
}

const roots = issues.filter((issue) => {
  const parentId = getParentId(issue);

  return !parentId || !issuesById.has(parentId);
});

const rows = roots.flatMap((issue) => flattenIssue(issue));
const markdown = [
  '# Milestones',
  '',
  'This project uses beads for issue tracking.',
  'While in stealth mode, issues are centralised locally and backed up remotely.',
  'A pre-commit hook ensures updates to the local issue tracker are captured here for visibility.',
  '',
  'Once this project opens to new contributors, issues will be tracked publically.',
  'In the meantime, issues can be subbmited via github and will be prioritised into the beads issue tracker.',
  '',
  '## Issue Tracker',
  '',
  '| ID | Title | Status | Priority | Type | Contributor | Labels |',
  '| --- | --- | --- | --- | --- | --- | --- |',
  ...rows.map((row) => `| ${row.join(' | ')} |`),
  '',
].join('\n');

writeFileSync('MILESTONES.md', markdown);

run('yarn', ['prettier', 'MILESTONES.md', '--write']);

run('git', ['add', '--', 'MILESTONES.md'], {
  stdio: 'inherit',
});
