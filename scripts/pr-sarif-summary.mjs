import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const markers = {
  opengrep: '<!-- plec-security-comment:opengrep -->',
  trivy: '<!-- plec-security-comment:trivy -->',
};
const maxDetails = 20;
const maxTextLength = 240;
const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

function plainText(value, limit = maxTextLength) {
  return String(value ?? '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, limit)
    .replace(
      /[&<>]/g,
      (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[char],
    )
    .replace(/([\\`*_{}[\]()#+\-.!|>])/g, '\\$1');
}

function inlineCode(value) {
  return `\`${String(value ?? '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, maxTextLength)
    .replace(/[`\\]/g, '')
    .replace(/[<>]/g, '')}\``;
}

function severityFor(result, rule) {
  const attributes = [
    result?.properties?.severity,
    rule?.properties?.severity,
    rule?.properties?.tags,
    result?.properties?.tags,
  ]
    .flat(Infinity)
    .filter((value) => typeof value === 'string')
    .map((value) => value.toLowerCase());

  const score = Number(
    result?.properties?.['security-severity'] ??
      rule?.properties?.['security-severity'],
  );
  if (Number.isFinite(score)) {
    if (score >= 9) return 'critical';
    if (score >= 7) return 'high';
    if (score >= 4) return 'medium';
    if (score > 0) return 'low';
  }

  for (const severity of ['critical', 'high', 'medium', 'low']) {
    if (
      attributes.some(
        (candidate) =>
          candidate === severity || candidate.includes(severity),
      )
    ) {
      return severity;
    }
  }

  const level = [result?.level, rule?.defaultConfiguration?.level]
    .find((value) => typeof value === 'string')
    ?.toLowerCase();
  if (['error', 'warning', 'note', 'info'].includes(level)) {
    return level;
  }
  return 'unknown';
}

function licenseDetails(message, ruleId) {
  const field = (name) =>
    new RegExp(`(?:^|\\n)\\s*${name}:?\\s*([^\\n]+)`, 'i')
      .exec(message)?.[1]
      ?.trim();
  const license = /(?:^|\n)\s*License\s+([^\s]+)/i.exec(message)?.[1];
  const packageName = field('PkgName');
  const classification = field('Classification')?.toLowerCase();
  return {
    packageName:
      packageName ?? ruleId?.split(':')[0] ?? 'unknown package',
    license:
      license ??
      ruleId?.split(':').slice(1).join(':') ??
      'unknown license',
    classification: classification ?? 'unclassified',
    artifact: field('Artifact'),
    path: field('Path'),
  };
}

async function readSarif(file) {
  try {
    const sarif = JSON.parse(await readFile(file, 'utf8'));
    const results = [];
    for (const run of sarif.runs ?? []) {
      const rules = run.tool?.driver?.rules ?? [];
      for (const result of run.results ?? []) {
        const rule =
          rules[result.ruleIndex] ??
          rules.find(({ id }) => id === result.ruleId);
        const location = result.locations?.[0]?.physicalLocation;
        const uri = location?.artifactLocation?.uri;
        const message =
          result.message?.text ?? result.message?.markdown ?? 'Finding';
        results.push({
          ruleId: result.ruleId ?? rule?.id ?? 'unknown-rule',
          message,
          level: severityFor(result, rule),
          path: uri
            ? decodeURIComponent(uri).replace(/^file:\/\//, '')
            : 'unknown path',
          line: location?.region?.startLine,
          license: licenseDetails(message, result.ruleId ?? rule?.id),
        });
      }
    }
    return { available: true, results };
  } catch {
    return { available: false, results: [] };
  }
}

function validPackageName(name) {
  return /^(?:@[a-z0-9._~-]+\/)?[a-z0-9._~-]+$/i.test(name);
}

async function resolvePackageManifest(name, fromDirectory) {
  if (!validPackageName(name)) return undefined;
  const segments = name.split('/');
  let directory = fromDirectory;
  while (
    directory === repoRoot ||
    directory.startsWith(`${repoRoot}${path.sep}`)
  ) {
    const candidate = path.join(
      directory,
      'node_modules',
      ...segments,
      'package.json',
    );
    try {
      return await realpath(candidate);
    } catch {
      directory = path.dirname(directory);
    }
  }
  return undefined;
}

export async function fullstackRuntimePackages() {
  const appManifestPath = path.join(
    repoRoot,
    'apps/fullstack/package.json',
  );
  const app = JSON.parse(await readFile(appManifestPath, 'utf8'));
  const direct = new Set([
    ...Object.keys(app.dependencies ?? {}),
    ...Object.keys(app.optionalDependencies ?? {}),
  ]);
  const packages = new Set(direct);
  const queue = [];
  for (const name of direct) {
    const manifest = await resolvePackageManifest(
      name,
      path.dirname(appManifestPath),
    );
    if (manifest) queue.push(manifest);
  }

  const visited = new Set();
  while (queue.length) {
    const manifestPath = queue.pop();
    if (visited.has(manifestPath)) continue;
    visited.add(manifestPath);
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
    const dependencies = {
      ...manifest.dependencies,
      ...manifest.optionalDependencies,
    };
    for (const name of Object.keys(dependencies)) {
      packages.add(name);
      const resolved = await resolvePackageManifest(
        name,
        path.dirname(manifestPath),
      );
      if (resolved && !visited.has(resolved)) queue.push(resolved);
    }
  }
  return { direct, packages };
}

function detailsList(results, title) {
  if (!results.length) return '';
  const shown = results.slice(0, maxDetails);
  const lines = shown.map(({ ruleId, message, path, line }) => {
    const location = line ? `${path}:${line}` : path;
    return `- **${inlineCode(ruleId)}** — ${plainText(message)} — ${inlineCode(location)}`;
  });
  const omitted = results.length - shown.length;
  if (omitted > 0)
    lines.push(`- ${omitted} additional findings omitted.`);
  return `\n<details>\n<summary>${title} (${results.length})</summary>\n\n${lines.join('\n')}\n\n</details>`;
}

function runUrl() {
  return `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`;
}

export async function buildOpenGrepSummary(
  file = 'opengrep-results.sarif',
) {
  const marker = markers.opengrep;
  const report = await readSarif(file);
  let body = `${marker}\n## 🔎 OpenGrep SAST\n\n`;
  if (!report.available) {
    body +=
      '⚠️ Report unavailable (SARIF file was not produced or could not be read).';
  } else if (!report.results.length) {
    body += '✅ No findings';
  } else {
    body += `❌ ${report.results.length} findings\n\n| Severity | Count |\n| --- | ---: |`;
    const counts = new Map();
    for (const result of report.results) {
      counts.set(result.level, (counts.get(result.level) ?? 0) + 1);
    }
    for (const [severity, count] of [...counts].sort(([a], [b]) =>
      a.localeCompare(b),
    )) {
      body += `\n| ${plainText(severity[0].toUpperCase() + severity.slice(1))} | ${count} |`;
    }
    body += detailsList(report.results, 'Findings');
  }
  body +=
    '\n\nScanned Rust, TypeScript, JavaScript, JSX/TSX and CSS using `p/security-audit`.';
  body += `\n\n[View workflow run](${runUrl()})`;
  return body;
}

export function renderTrivySummary(
  security,
  licenses,
  {
    securityOutcome = process.env.SECURITY_SCAN_OUTCOME,
    licenseGateOutcome = process.env.LICENSE_GATE_OUTCOME,
    runtimeScope,
  } = {},
) {
  const marker = markers.trivy;
  const securityCount = security.results.length;
  const highLicense = licenses.results.filter(({ level }) =>
    ['high', 'critical'].includes(level),
  );
  const licenseCount = licenses.results.length;
  const runtimeLicenses = runtimeScope
    ? licenses.results.filter((result) =>
        runtimeScope.packages.has(result.license.packageName),
      )
    : [];
  const otherLicenses = runtimeScope
    ? licenses.results.filter(
        (result) =>
          !runtimeScope.packages.has(result.license.packageName),
      )
    : licenses.results;
  const runtimeClassifications = classificationCounts(runtimeLicenses);
  const otherClassifications = classificationCounts(otherLicenses);
  const runtimeReviewCount = reviewCount(runtimeClassifications);
  const runtimeStatus =
    !licenses.available || !runtimeScope
      ? '⚠️ Unavailable'
      : runtimeReviewCount
        ? '⚠️ Review'
        : '✅';
  const otherStatus = 'ℹ️ Inventory';
  const licenseGateStatus = !licenses.available
    ? '⚠️'
    : licenseGateOutcome === 'skipped'
      ? '⏭️'
      : licenseGateOutcome === 'failure' || highLicense.length
        ? '❌'
        : '✅';
  const securityStatus = !security.available
    ? '⚠️'
    : securityOutcome === 'failure' || securityCount
      ? '❌'
      : securityOutcome === 'skipped'
        ? '⏭️'
        : '✅';
  let body = `${marker}\n## 🛡️ Trivy\n\n| Scan | Result | Findings |\n| --- | --- | ---: |`;
  body += `\n| Vulnerabilities / secrets / misconfigurations | ${securityStatus} | ${security.available ? (securityOutcome === 'failure' && !securityCount ? 'Scan failed' : securityCount) : 'Unavailable'} |`;
  body += `\n| License gate (HIGH/CRITICAL, repository-wide) | ${licenseGateStatus} | ${!licenses.available ? 'Unavailable' : licenseGateOutcome === 'skipped' ? 'Not run' : licenseGateOutcome === 'failure' && !highLicense.length ? 'Gate failed' : highLicense.length} |`;
  body += `\n| Fullstack production dependencies | ${runtimeStatus} | ${licenses.available && runtimeScope ? runtimeLicenses.length : 'Unavailable'} |`;
  body += `\n| Other workspace records (outside fullstack runtime; includes tooling) | ${otherStatus} | ${licenses.available ? otherLicenses.length : 'Unavailable'} |`;
  if (!security.available) {
    body += '\n\n⚠️ Security SARIF report unavailable.';
  } else if (!securityCount) {
    body +=
      securityOutcome === 'failure'
        ? '\n\n⚠️ Security scan failed without SARIF findings; inspect the workflow run.'
        : '\n\nNo HIGH/CRITICAL security findings.';
  } else {
    body += `\n\n❌ ${securityCount} HIGH/CRITICAL security finding${securityCount === 1 ? '' : 's'}.`;
    body += detailsList(security.results, 'Security findings');
  }
  if (!licenses.available) {
    body += '\n\n⚠️ License SARIF report unavailable.';
  } else if (!licenseCount) {
    body += '\n\nNo license records.';
  } else {
    if (runtimeScope) {
      body += `\n\nFullstack production dependency classifications: ${formatClassifications(runtimeClassifications)}.`;
      body += `\nOther workspace classifications: ${formatClassifications(otherClassifications)}.`;
      body +=
        '\n“Unknown” is Trivy’s classification; it does not mean the license identifier is missing.';
    } else {
      body += '\n\nRepository-wide license classifications: ';
      body += `${formatClassifications(otherClassifications)}.`;
      body += '\n⚠️ Fullstack runtime scope could not be determined.';
    }
    if (licenseGateOutcome === 'failure' && !highLicense.length) {
      body +=
        '\n\n❌ License gate failed; SARIF did not expose a severity count.';
    } else if (highLicense.length) {
      body += `\n\n❌ ${highLicense.length} HIGH/CRITICAL license finding${highLicense.length === 1 ? '' : 's'}.`;
    } else if (licenseGateOutcome === 'skipped') {
      body += '\n\n⚠️ The license gate did not run.';
    } else {
      body += '\n\nNo HIGH/CRITICAL license gate findings.';
    }
    if (runtimeReviewCount) {
      body += `\n\n${runtimeReviewCount} fullstack runtime record${runtimeReviewCount === 1 ? '' : 's'} has a reciprocal or unclassified Trivy classification. Notice records are attribution obligations, not gate failures.`;
    }
    const orderedLicenses = prioritizeLicenseRecords(
      licenses.results,
      runtimeScope,
    );
    body += licenseDetailsList(orderedLicenses, runtimeScope);
  }
  body += `\n\n[View workflow run](${runUrl()})`;
  return body;
}

export async function buildTrivySummary(
  securityFile = 'trivy-results.sarif',
  licenseFile = 'trivy-license-results.sarif',
) {
  let runtimeScope;
  try {
    runtimeScope = await fullstackRuntimePackages();
  } catch (error) {
    console.warn(
      `Fullstack runtime dependency scope unavailable: ${error.message}`,
    );
  }
  return renderTrivySummary(
    await readSarif(securityFile),
    await readSarif(licenseFile),
    { runtimeScope },
  );
}

function classificationCounts(results) {
  const counts = new Map();
  for (const result of results) {
    const classification = result.license.classification;
    counts.set(classification, (counts.get(classification) ?? 0) + 1);
  }
  return counts;
}

function reviewCount(classifications) {
  return (
    (classifications.get('reciprocal') ?? 0) +
    (classifications.get('unclassified') ?? 0)
  );
}

function formatClassifications(classifications) {
  if (!classifications.size) return 'none';
  return [...classifications]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(
      ([classification, count]) =>
        `${plainText(classification)} ${count}`,
    )
    .join(', ');
}

function prioritizeLicenseRecords(results, runtimeScope) {
  const rank = (result) => {
    if (!runtimeScope) return 0;
    const name = result.license.packageName;
    const classification = result.license.classification;
    const direct = runtimeScope.direct.has(name);
    const runtime = runtimeScope.packages.has(name);
    const review = ['reciprocal', 'unknown', 'unclassified'].includes(
      classification,
    );
    if (direct) return 0;
    if (runtime && review) return 1;
    if (!runtime && review) return 2;
    if (runtime) return 3;
    return 4;
  };
  return [...results].sort(
    (a, b) =>
      rank(a) - rank(b) ||
      a.license.packageName.localeCompare(b.license.packageName) ||
      a.license.license.localeCompare(b.license.license),
  );
}

function licenseDetailsList(results, runtimeScope) {
  if (!results.length) return '';
  const shown = results.slice(0, maxDetails);
  const lines = shown.map(({ license, level }) => {
    const location =
      license.path ?? license.artifact ?? 'unknown source';
    const suffix = level === 'error' ? ' — SARIF error' : '';
    const scope = runtimeScope
      ? runtimeScope.direct.has(license.packageName)
        ? 'fullstack direct'
        : runtimeScope.packages.has(license.packageName)
          ? 'fullstack runtime'
          : 'outside fullstack runtime'
      : 'scope unavailable';
    return `- **${inlineCode(license.packageName)}** — ${inlineCode(license.license)} (Trivy: ${plainText(license.classification)}; ${scope}) — ${inlineCode(location)}${suffix}`;
  });
  const omitted = results.length - shown.length;
  if (omitted > 0)
    lines.push(`- ${omitted} additional records omitted.`);
  return `\n<details>\n<summary>License records (${results.length})</summary>\n\n${lines.join('\n')}\n\n</details>`;
}

async function githubRequest(url, token, options = {}) {
  const response = await fetch(url, {
    ...options,
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${token}`,
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'plec-security-summary',
      ...options.headers,
    },
  });
  if (!response.ok) {
    throw new Error(
      `GitHub API ${response.status}: ${(await response.text()).slice(0, 500)}`,
    );
  }
  return response.status === 204 ? undefined : response.json();
}

export async function publishComment(body, marker) {
  const token = process.env.GITHUB_TOKEN;
  const repository = process.env.GITHUB_REPOSITORY;
  const pullNumber = process.env.PR_NUMBER;
  if (!token || !repository || !pullNumber) {
    console.warn(
      'PR summary not published: GitHub PR context or token is unavailable.',
    );
    return { action: 'skipped' };
  }

  try {
    const api = `${process.env.GITHUB_API_URL ?? 'https://api.github.com'}/repos/${repository}/issues/${pullNumber}/comments`;
    let page = `${api}?per_page=100`;
    const comments = [];
    while (page) {
      const response = await fetch(page, {
        headers: {
          Accept: 'application/vnd.github+json',
          Authorization: `Bearer ${token}`,
          'X-GitHub-Api-Version': '2022-11-28',
          'User-Agent': 'plec-security-summary',
        },
      });
      if (!response.ok) {
        throw new Error(
          `GitHub API ${response.status}: ${(await response.text()).slice(0, 500)}`,
        );
      }
      comments.push(...(await response.json()));
      const next = response.headers
        .get('link')
        ?.split(',')
        .find((part) => part.includes('rel="next"'));
      page = next ? /<([^>]+)>/.exec(next)?.[1] : undefined;
    }

    const existing = comments.find(
      (comment) =>
        comment.user?.login === 'github-actions[bot]' &&
        comment.body?.includes(marker),
    );
    if (existing) {
      const updated = await githubRequest(
        `${process.env.GITHUB_API_URL ?? 'https://api.github.com'}/repos/${repository}/issues/comments/${existing.id}`,
        token,
        {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ body }),
        },
      );
      if (updated?.id !== existing.id || updated?.body !== body) {
        throw new Error(
          `GitHub did not confirm update of PR comment ${existing.id}.`,
        );
      }
      console.log(`Updated PR summary comment ${existing.id}.`);
      return { action: 'updated', commentId: existing.id };
    } else {
      const created = await githubRequest(api, token, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
      });
      if (!created?.id || created.body !== body) {
        throw new Error(
          'GitHub did not confirm creation of the PR summary comment.',
        );
      }
      console.log(`Created PR summary comment ${created.id}.`);
      return { action: 'created', commentId: created.id };
    }
  } catch (error) {
    console.warn(`PR summary could not be published: ${error.message}`);
    return { action: 'failed' };
  }
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const kind = process.argv[2];
  if (kind === 'opengrep') {
    await publishComment(
      await buildOpenGrepSummary(),
      markers.opengrep,
    );
  } else if (kind === 'trivy') {
    await publishComment(await buildTrivySummary(), markers.trivy);
  } else {
    console.error(
      'Usage: node scripts/pr-sarif-summary.mjs <opengrep|trivy>',
    );
    process.exitCode = 2;
  }
}
