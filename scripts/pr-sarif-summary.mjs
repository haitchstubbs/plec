import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const markers = {
  opengrep: '<!-- plec-security-comment:opengrep -->',
  trivy: '<!-- plec-security-comment:trivy -->',
};
const maxDetails = 20;
const maxTextLength = 240;

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
        results.push({
          ruleId: result.ruleId ?? rule?.id ?? 'unknown-rule',
          message:
            result.message?.text ??
            result.message?.markdown ??
            'Finding',
          level: severityFor(result, rule),
          path: uri
            ? decodeURIComponent(uri).replace(/^file:\/\//, '')
            : 'unknown path',
          line: location?.region?.startLine,
        });
      }
    }
    return { available: true, results };
  } catch {
    return { available: false, results: [] };
  }
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

export async function buildTrivySummary(
  securityFile = 'trivy-results.sarif',
  licenseFile = 'trivy-license-results.sarif',
) {
  const marker = markers.trivy;
  const security = await readSarif(securityFile);
  const licenses = await readSarif(licenseFile);
  const securityCount = security.results.length;
  const highLicense = licenses.results.filter(({ level }) =>
    ['high', 'critical', 'error'].includes(level),
  );
  const licenseCount = licenses.results.length;
  const licenseStatus = !licenses.available
    ? '⚠️'
    : highLicense.length
      ? '❌'
      : licenseCount
        ? '⚠️'
        : '✅';
  let body = `${marker}\n## 🛡️ Trivy\n\n| Scan | Result | Findings |\n| --- | --- | ---: |`;
  body += `\n| Vulnerabilities / secrets / misconfigurations | ${security.available ? (securityCount ? '❌' : '✅') : '⚠️'} | ${security.available ? securityCount : 'Unavailable'} |`;
  body += `\n| Licenses | ${licenseStatus} | ${licenses.available ? licenseCount : 'Unavailable'} |`;
  if (!security.available) {
    body += '\n\n⚠️ Security SARIF report unavailable.';
  } else if (!securityCount) {
    body += '\n\nNo HIGH/CRITICAL security findings.';
  } else {
    body += `\n\n❌ ${securityCount} HIGH/CRITICAL security finding${securityCount === 1 ? '' : 's'}.`;
    body += detailsList(security.results, 'Security findings');
  }
  if (!licenses.available) {
    body += '\n\n⚠️ License SARIF report unavailable.';
  } else if (licenseCount) {
    const counts = new Map();
    for (const result of licenses.results) {
      const severity =
        result.level === 'error'
          ? 'high/critical (SARIF error)'
          : result.level;
      counts.set(severity, (counts.get(severity) ?? 0) + 1);
    }
    body += '\n\nLicense findings by severity: ';
    body += [...counts]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([severity, count]) => `${plainText(severity)} ${count}`)
      .join(', ');
    if (highLicense.length) {
      body += `\n\n❌ ${highLicense.length} HIGH/CRITICAL license finding${highLicense.length === 1 ? '' : 's'}; the license gate should fail.`;
    } else {
      body +=
        '\n\nLicense findings are informational and do not fail the HIGH/CRITICAL gate.';
    }
    body += detailsList(licenses.results, 'License findings');
  }
  body += `\n\n[View workflow run](${runUrl()})`;
  return body;
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

async function publishComment(body, marker) {
  const token = process.env.GITHUB_TOKEN;
  const repository = process.env.GITHUB_REPOSITORY;
  const pullNumber = process.env.PR_NUMBER;
  if (!token || !repository || !pullNumber) {
    console.warn(
      'PR summary not published: GitHub PR context or token is unavailable.',
    );
    return;
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
      await githubRequest(`${api}/${existing.id}`, token, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
      });
    } else {
      await githubRequest(api, token, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
      });
    }
  } catch (error) {
    console.warn(`PR summary could not be published: ${error.message}`);
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
