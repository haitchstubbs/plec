import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const rootDir = resolve(__dirname, '..');
const workspaceRoot = resolve(rootDir, '../..');
const rustManifestPath = join(
  workspaceRoot,
  'crates',
  'query',
  'Cargo.toml',
);
const docsPath = join(rootDir, 'docs', 'dialect-matrix.md');
const docsDir = dirname(docsPath);
const tsDialectsPath = join(
  rootDir,
  'src',
  'core',
  'constants',
  'dialects.const.ts',
);

function runExporter() {
  const output = execFileSync(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      rustManifestPath,
      '-p',
      'query_core',
      '--bin',
      'dialect_matrix_exporter',
      '--',
      '--format',
      'json',
    ],
    {
      cwd: workspaceRoot,
      encoding: 'utf-8',
    },
  );
  return JSON.parse(output);
}

function parseTsDialects() {
  const source = readFileSync(tsDialectsPath, 'utf-8');
  const arrayMatch =
    source.match(/DIALECTS\s*=\s*\[([\s\S]*?)\]\s*as const/) ??
    source.match(/dialect_list\s*=\s*\[([\s\S]*?)\]\s*as const/);
  if (!arrayMatch) {
    throw new Error(
      'Unable to parse DIALECTS constant from TypeScript source.',
    );
  }
  const quoted = [...arrayMatch[1].matchAll(/"([^"]+)"/g)].map(
    (match) => match[1],
  );
  return [...new Set(quoted)];
}

function boolLabel(value) {
  return value ? 'Yes' : 'No';
}

function listOrDash(items) {
  return items.length > 0 ? items.join(', ') : 'n/a';
}

function render(data, tsDialects) {
  const canonicalNames = data.dialects.map((dialect) => dialect.name);
  const aliasNames = data.aliases.map((alias) => alias.alias);
  const contractNames = new Set([...canonicalNames, ...aliasNames]);
  const nonContractInputs = tsDialects
    .filter((name) => !contractNames.has(name))
    .sort((left, right) => left.localeCompare(right));

  const capabilityHeader =
    '| Dialect | Placeholder | RETURNING | Window | NULLS FIRST/LAST | DISTINCT ON | CTE | ILIKE | Insert conflict | Pagination | Offset requires ORDER BY | Offset-only allowed | LIMIT via TOP only | RIGHT JOIN | FULL OUTER JOIN | USING | LATERAL | Recursive CTE style | Recursive CTE aliases required | Lock strengths | Lock modifiers |';
  const capabilityDivider =
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |';
  const capabilityRows = data.dialects.map(
    (dialect) =>
      `| \`${dialect.name}\` | \`${dialect.placeholder_style}\` | ${boolLabel(dialect.returning)} | ${boolLabel(dialect.window_functions)} | ${boolLabel(dialect.null_ordering)} | ${boolLabel(dialect.distinct_on)} | ${boolLabel(dialect.supports_cte)} | ${boolLabel(dialect.ilike)} | \`${dialect.insert_conflict_style}\` | \`${dialect.pagination_family}\` | ${boolLabel(dialect.pagination_offset_requires_order_by)} | ${boolLabel(dialect.pagination_offset_only_allowed)} | ${boolLabel(dialect.pagination_limit_only_uses_top)} | ${boolLabel(dialect.join_right)} | ${boolLabel(dialect.join_full_outer)} | ${boolLabel(dialect.join_using)} | ${boolLabel(dialect.join_lateral)} | \`${dialect.recursive_cte_style}\` | ${boolLabel(dialect.recursive_cte_column_aliases_required)} | ${listOrDash(dialect.lock_strengths)} | ${listOrDash(dialect.lock_modifiers)} |`,
  );

  const policyHeader =
    '| Dialect | RETURNING | ILIKE | INTERSECT ALL | DISTINCT ON | LIMIT/OFFSET semantic family |';
  const policyDivider = '| --- | --- | --- | --- | --- | --- |';
  const policyRows = data.dialects.map(
    (dialect) =>
      `| \`${dialect.name}\` | \`${dialect.policy.returning}\` | \`${dialect.policy.ilike}\` | \`${dialect.policy.intersect_all}\` | \`${dialect.policy.distinct_on}\` | \`${dialect.policy.pagination_limit_offset}\` |`,
  );

  const aliasLines = data.aliases.map(
    (alias) => `- \`${alias.alias}\` -> \`${alias.canonical}\``,
  );
  const nonContractLine =
    nonContractInputs.length > 0
      ? nonContractInputs.map((name) => `\`${name}\``).join(', ')
      : 'None';

  return `# node-query dialect matrix

This file is generated from Rust capability and policy metadata.

## Capability guarantees

- \`DialectRender\`: feature is supported and rendered with dialect-specific SQL.
- \`FallbackRewrite\`: feature is rewritten to an equivalent form and may emit a warning.
- \`HardError\`: feature has no valid equivalent and must fail validation/rendering.

## Supported dialect names

Canonical dialect names:
- ${canonicalNames.map((name) => `\`${name}\``).join(', ')}

Dialect aliases accepted by parser:
${aliasLines.join('\n')}

TypeScript \`DIALECTS\` entries that are currently non-contract inputs (unsupported unless Rust capability support exists):
- ${nonContractLine}

## Capability matrix (Rust core source of truth)

${capabilityHeader}
${capabilityDivider}
${capabilityRows.join('\n')}

## Policy classification matrix (M2-3)

${policyHeader}
${policyDivider}
${policyRows.join('\n')}

## Node/WASM/core parity expectations

- Rust core defines runtime capability, validation, and policy semantics.
- Node native and WASM bindings must expose equivalent behavior for the same dialect and query shape.
- TypeScript remains a typed facade (compile-time inference and API ergonomics), not a second runtime semantics engine.

## Maintenance note

1. Add or change dialect/feature capability policy in Rust first.
2. Regenerate this matrix via \`yarn docs:generate-dialect-matrix\`.
3. Keep Node native and WASM bindings parity-aligned with Rust behavior.
`;
}

function main() {
  const checkMode = process.argv.includes('--check');
  const payload = runExporter();
  const tsDialects = parseTsDialects();
  const next = `${render(payload, tsDialects).trim()}\n`;

  if (checkMode) {
    const current = readFileSync(docsPath, 'utf-8');
    if (current !== next) {
      console.error(
        'dialect-matrix.md is out of date. Run: yarn docs:generate-dialect-matrix',
      );
      process.exit(1);
    }
    console.log('dialect-matrix.md is up to date.');
    return;
  }

  mkdirSync(docsDir, { recursive: true });
  writeFileSync(docsPath, next, 'utf-8');
  console.log('Wrote docs/dialect-matrix.md');
}

main();
