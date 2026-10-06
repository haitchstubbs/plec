export type RouteClass = 'api' | 'action' | 'document' | 'static';

export interface CanonicalRequestTarget {
  path: string;
  query: string;
}

/**
 * Extracts the validated, encoded path from an HTTP request target. It keeps
 * percent escapes, dot segments, repeated slashes, and path spelling intact;
 * URL.pathname must not be used here because WHATWG URL parsing normalizes
 * some of those forms.
 */
export function canonicalRequestTarget(
  target: string,
): CanonicalRequestTarget {
  if (
    target.length === 0 ||
    /[\u0000-\u0020\u007f#\\]/u.test(target) ||
    /%(?![0-9A-Fa-f]{2})/u.test(target)
  ) {
    throw new TypeError('malformed HTTP request target');
  }

  let pathAndQuery: string;
  if (target.startsWith('/')) {
    pathAndQuery = target;
  } else {
    const absolute =
      /^(https?):\/\/([^/?#]+)(\/[^?#]*)?(\?[^#]*)?$/iu.exec(target);
    if (!absolute || absolute[2]!.includes('@')) {
      throw new TypeError('unsupported HTTP request-target form');
    }
    // Parse only to validate the authority. Never take pathname from URL.
    try {
      const parsed = new URL(`${absolute[1]}://${absolute[2]}`);
      if (!parsed.hostname) throw new Error('missing host');
    } catch {
      throw new TypeError('malformed HTTP request-target authority');
    }
    pathAndQuery = `${absolute[3] ?? '/'}${absolute[4] ?? ''}`;
  }

  const queryIndex = pathAndQuery.indexOf('?');
  const path =
    queryIndex < 0 ? pathAndQuery : pathAndQuery.slice(0, queryIndex);
  const query =
    queryIndex < 0 ? '' : pathAndQuery.slice(queryIndex + 1);
  if (!path.startsWith('/')) {
    throw new TypeError('request target must contain an absolute path');
  }
  return { path, query };
}

export function classifyPlecPath(path: string): RouteClass {
  if (path === '/api' || path.startsWith('/api/')) return 'api';
  if (path.startsWith('/_plec/actions/')) return 'action';
  return isDocumentPath(path) ? 'document' : 'static';
}

function isDocumentPath(path: string): boolean {
  if (path === '/') return true;
  const segment = path.slice(path.lastIndexOf('/') + 1);
  return !segment.includes('.');
}
