import assert from 'node:assert/strict';
import test from 'node:test';
import {
  fullstackRuntimePackages,
  publishComment,
  renderTrivySummary,
} from './pr-sarif-summary.mjs';

const marker = '<!-- plec-security-comment:trivy -->';

test('updates the marker-owned bot comment through the issue-comments endpoint', async (context) => {
  const env = {
    GITHUB_TOKEN: process.env.GITHUB_TOKEN,
    GITHUB_REPOSITORY: process.env.GITHUB_REPOSITORY,
    GITHUB_API_URL: process.env.GITHUB_API_URL,
    PR_NUMBER: process.env.PR_NUMBER,
  };
  Object.assign(process.env, {
    GITHUB_TOKEN: 'test-token',
    GITHUB_REPOSITORY: 'owner/repo',
    GITHUB_API_URL: 'https://api.github.com',
    PR_NUMBER: '55',
  });
  context.after(() => {
    for (const [key, value] of Object.entries(env)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  const botComment = {
    id: 42,
    user: { login: 'github-actions[bot]' },
    body: `${marker}\nold body`,
  };
  const otherScannerComment = {
    id: 41,
    user: { login: 'github-actions[bot]' },
    body: '<!-- plec-security-comment:opengrep -->\nOpenGrep body',
  };
  let comments = [otherScannerComment, botComment];
  const requests = [];
  context.mock.method(
    globalThis,
    'fetch',
    async (input, options = {}) => {
      const url = String(input);
      const method = options.method ?? 'GET';
      requests.push({ url, method });

      if (method === 'GET') {
        return Response.json(comments);
      }
      if (method === 'PATCH') {
        assert.equal(
          url,
          'https://api.github.com/repos/owner/repo/issues/comments/42',
        );
        botComment.body = JSON.parse(options.body).body;
        return Response.json(botComment);
      }
      if (method === 'POST') {
        const created = {
          id: 43,
          user: { login: 'github-actions[bot]' },
          body: JSON.parse(options.body).body,
        };
        comments.push(created);
        return Response.json(created, { status: 201 });
      }
      throw new Error(`Unexpected request: ${method} ${url}`);
    },
  );
  context.mock.method(console, 'log', () => {});

  const first = `${marker}\nsummary for run 1`;
  const second = `${marker}\nsummary for run 2`;
  assert.deepEqual(await publishComment(first, marker), {
    action: 'updated',
    commentId: 42,
  });
  assert.deepEqual(await publishComment(second, marker), {
    action: 'updated',
    commentId: 42,
  });

  assert.equal(comments.length, 2);
  assert.equal(botComment.body, second);
  assert.deepEqual(
    requests.filter(({ method }) => method !== 'GET'),
    [
      {
        url: 'https://api.github.com/repos/owner/repo/issues/comments/42',
        method: 'PATCH',
      },
      {
        url: 'https://api.github.com/repos/owner/repo/issues/comments/42',
        method: 'PATCH',
      },
    ],
  );
});

test('separates fullstack production licenses from other workspace records', async () => {
  const runtimeScope = await fullstackRuntimePackages();
  assert.ok(runtimeScope.direct.has('lucide'));
  assert.ok(runtimeScope.packages.has('@fontsource-variable/outfit'));
  assert.ok(runtimeScope.packages.has('@fontsource-variable/raleway'));
  assert.ok(!runtimeScope.packages.has('lightningcss'));

  const security = { available: true, results: [] };
  const licenses = {
    available: true,
    results: [
      {
        level: 'note',
        license: {
          packageName: '@fontsource-variable/outfit',
          license: 'OFL-1.1',
          classification: 'unknown',
          path: 'yarn.lock',
        },
      },
      {
        level: 'note',
        license: {
          packageName: 'lucide',
          license: 'ISC',
          classification: 'notice',
          path: 'yarn.lock',
        },
      },
      {
        level: 'warning',
        license: {
          packageName: 'lightningcss',
          license: 'MPL-2.0',
          classification: 'reciprocal',
          path: 'yarn.lock',
        },
      },
    ],
  };
  const body = renderTrivySummary(security, licenses, {
    securityOutcome: 'success',
    licenseGateOutcome: 'success',
    runtimeScope,
  });

  assert.match(
    body,
    /Fullstack production dependencies \| ⚠️ Review \| 2/,
  );
  assert.match(
    body,
    /Other workspace records \(outside fullstack runtime; includes tooling\) \| ⚠️ Review \| 1/,
  );
  assert.match(body, /“Unknown” is Trivy’s classification/);
  assert.ok(
    body.indexOf('@fontsource-variable/outfit') <
      body.indexOf('lightningcss'),
  );
  assert.ok(body.indexOf('lucide') < body.indexOf('lightningcss'));
  assert.match(body, /fullstack direct/);
  assert.match(body, /outside fullstack runtime/);
});
