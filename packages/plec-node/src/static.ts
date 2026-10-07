import { realpath, stat } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import path from 'node:path';

enum HttpMethod {
  Get = 'GET',
  Head = 'HEAD',
}

enum ContentEncoding {
  Brotli = 'br',
  Gzip = 'gzip',
  Identity = 'identity',
}

enum AssetPath {
  ClientRoot = '/_plec',
  ClientPrefix = '/_plec/',
}

enum HeaderName {
  AcceptEncoding = 'accept-encoding',
  AcceptRanges = 'accept-ranges',
  Allow = 'allow',
  CacheControl = 'cache-control',
  ContentEncoding = 'content-encoding',
  ContentLength = 'content-length',
  ContentRange = 'content-range',
  ContentType = 'content-type',
  Range = 'range',
  Vary = 'vary',
}

enum HttpStatus {
  PartialContent = 206,
  NotFound = 404,
  NotAcceptable = 406,
  RangeNotSatisfiable = 416,
}

enum StaticMessage {
  AllowedMethods = 'GET, HEAD',
  AssetNotFound = 'asset not found',
  InvalidAssetPath = 'invalid asset path',
  AcceptEncoding = 'Accept-Encoding',
  NoCache = 'no-cache',
  Bytes = 'bytes',
  JsonUtf8 = 'application/json; charset=utf-8',
}

const MIME_TYPES: Record<string, string> = {
  '.avif': 'image/avif',
  '.css': 'text/css; charset=utf-8',
  '.csv': 'text/csv; charset=utf-8',
  '.gif': 'image/gif',
  '.htm': 'text/html; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.ico': 'image/x-icon',
  '.jpeg': 'image/jpeg',
  '.jpg': 'image/jpeg',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.mp3': 'audio/mpeg',
  '.mp4': 'video/mp4',
  '.otf': 'font/otf',
  '.pdf': 'application/pdf',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.wasm': 'application/wasm',
  '.webmanifest': 'application/manifest+json',
  '.webp': 'image/webp',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.xml': 'application/xml; charset=utf-8',
};

type CompressionFormat = {
  readonly encoding: ContentEncoding;
  readonly extension: string;
};
const COMPRESSION_FORMATS: readonly CompressionFormat[] = [
  { encoding: ContentEncoding.Brotli, extension: '.br' },
  { encoding: ContentEncoding.Gzip, extension: '.gz' },
];
type CompressibleEncoding =
  ContentEncoding.Brotli | ContentEncoding.Gzip;
type AvailableEncoding =
  CompressibleEncoding | ContentEncoding.Identity;
type AssetVariants = Partial<Record<CompressibleEncoding, string>>;

export async function serveStatic(
  request: Request,
  pathname: string,
  publicRoot: string,
  clientRoot: string,
): Promise<Response> {
  if (
    request.method !== HttpMethod.Get &&
    request.method !== HttpMethod.Head
  )
    return new Response(null, {
      status: 405,
      headers: { [HeaderName.Allow]: StaticMessage.AllowedMethods },
    });

  if (pathname === AssetPath.ClientRoot) return assetNotFound();
  const clientAsset = pathname.startsWith(AssetPath.ClientPrefix);
  const root = clientAsset ? clientRoot : publicRoot;
  const relativeEncoded = clientAsset
    ? pathname.slice(AssetPath.ClientPrefix.length)
    : pathname.slice(1);
  let segments: string[];
  try {
    segments = relativeEncoded
      .split('/')
      .map((part) => decodeURIComponent(part));
  } catch {
    return invalidAssetPath();
  }
  if (
    segments.some(
      (part) =>
        part === '' ||
        part === '.' ||
        part === '..' ||
        part.includes('/') ||
        part.includes('\\'),
    )
  )
    return invalidAssetPath();
  const relative = path.join(...segments);
  try {
    // The host canonicalizes publicRoot and clientRoot during startup.
    const rootReal = root;
    let candidate = await realpath(path.join(rootReal, relative));
    if (!inside(rootReal, candidate)) return invalidAssetPath();
    const mediaType = contentType(candidate);
    const variants: AssetVariants = {};
    for (const { encoding, extension } of COMPRESSION_FORMATS) {
      try {
        const sidecar = await realpath(`${candidate}${extension}`);
        if (inside(rootReal, sidecar) && (await stat(sidecar)).isFile())
          variants[encoding as CompressibleEncoding] = sidecar;
      } catch {
        /* absent sidecars are not available representations */
      }
    }
    const qualities = parseAcceptEncoding(
      request.headers.get(HeaderName.AcceptEncoding) ?? '',
    );
    const available: AvailableEncoding[] = [ContentEncoding.Identity];
    for (const { encoding } of COMPRESSION_FORMATS) {
      const compressibleEncoding = encoding as CompressibleEncoding;
      if (variants[compressibleEncoding])
        available.push(compressibleEncoding);
    }
    const selected = available
      .filter((coding) => encodingQuality(qualities, coding) > 0)
      .sort(
        (a, b) =>
          encodingQuality(qualities, b) -
            encodingQuality(qualities, a) ||
          (a === 'br'
            ? -1
            : b === 'br'
              ? 1
              : a === 'gzip'
                ? -1
                : b === 'gzip'
                  ? 1
                  : 0),
      )[0];
    if (!selected)
      return new Response(null, {
        status: HttpStatus.NotAcceptable,
        headers: { [HeaderName.Vary]: StaticMessage.AcceptEncoding },
      });
    const encoding: CompressibleEncoding | undefined =
      selected === ContentEncoding.Identity
        ? undefined
        : (selected as CompressibleEncoding);
    if (encoding) candidate = variants[encoding]!;
    const info = await stat(candidate);
    if (!info.isFile()) return assetNotFound();
    const headers = new Headers({
      [HeaderName.ContentType]: mediaType,
      [HeaderName.CacheControl]: StaticMessage.NoCache,
    });
    if (Object.keys(variants).length > 0)
      headers.set(HeaderName.Vary, StaticMessage.AcceptEncoding);
    if (encoding) headers.set(HeaderName.ContentEncoding, encoding);
    const range = request.headers.get(HeaderName.Range);
    const size = info.size;
    let start = 0;
    let end = size - 1;
    let status = 200;
    if (range && !encoding) {
      const parsedRange = parseSingleByteRange(range, size);
      if (parsedRange?.kind === 'satisfiable') {
        ({ start, end } = parsedRange);
        status = HttpStatus.PartialContent;
        headers.set(HeaderName.AcceptRanges, StaticMessage.Bytes);
        headers.set(
          HeaderName.ContentRange,
          `${StaticMessage.Bytes} ${start}-${end}/${size}`,
        );
      } else if (parsedRange?.kind === 'unsatisfiable') {
        headers.set(HeaderName.AcceptRanges, StaticMessage.Bytes);
        headers.set(
          HeaderName.ContentRange,
          `${StaticMessage.Bytes} */${size}`,
        );
        return new Response(null, {
          status: HttpStatus.RangeNotSatisfiable,
          headers,
        });
      }
    }
    headers.set(
      HeaderName.ContentLength,
      String(
        status === HttpStatus.PartialContent ? end - start + 1 : size,
      ),
    );
    if (request.method === HttpMethod.Head)
      return new Response(null, { status, headers });
    const stream = createReadStream(
      candidate,
      status === 206 ? { start, end } : undefined,
    );
    return new Response(
      Readable.toWeb(stream) as ReadableStream<Uint8Array>,
      { status, headers },
    );
  } catch (error) {
    if (
      (error as NodeJS.ErrnoException).code === 'ENOENT' ||
      (error as NodeJS.ErrnoException).code === 'ENOTDIR'
    )
      return assetNotFound();
    return invalidAssetPath();
  }
}

function assetNotFound(): Response {
  return new Response(
    JSON.stringify({ error: StaticMessage.AssetNotFound }),
    {
      status: HttpStatus.NotFound,
      headers: { [HeaderName.ContentType]: StaticMessage.JsonUtf8 },
    },
  );
}
function invalidAssetPath(): Response {
  return new Response(
    JSON.stringify({ error: StaticMessage.InvalidAssetPath }),
    {
      status: 400,
      headers: { [HeaderName.ContentType]: StaticMessage.JsonUtf8 },
    },
  );
}
function contentType(file: string): string {
  const ext = path.extname(file).toLowerCase();
  return MIME_TYPES[ext] ?? 'application/octet-stream';
}

type EncodingQualities = Map<string, number>;
function parseAcceptEncoding(header: string): EncodingQualities {
  const result: EncodingQualities = new Map();
  for (const part of header.split(',')) {
    const [rawToken, ...parameters] = part
      .trim()
      .toLowerCase()
      .split(';');
    if (!rawToken || !/^[!#$%&'*+.^_`|~0-9a-z-]+$/u.test(rawToken))
      continue;
    let quality = 1;
    let valid = true;
    for (const parameter of parameters) {
      const match =
        /^\s*q\s*=\s*(0(?:\.\d{0,3})?|1(?:\.0{0,3})?)\s*$/u.exec(
          parameter,
        );
      if (!match) {
        valid = false;
        break;
      }
      quality = Number(match[1]);
    }
    if (valid) result.set(rawToken, quality);
  }
  return result;
}
function encodingQuality(
  qualities: EncodingQualities,
  coding: string,
): number {
  if (qualities.has(coding)) return qualities.get(coding)!;
  const wildcard = qualities.get('*');
  if (coding === ContentEncoding.Identity)
    return wildcard === 0 ? 0 : 1;
  return wildcard ?? 0;
}
type ParsedRange =
  | { kind: 'satisfiable'; start: number; end: number }
  | { kind: 'unsatisfiable' }
  | undefined;
function parseSingleByteRange(
  header: string,
  size: number,
): ParsedRange {
  const match = /^bytes=(\d*)-(\d*)$/iu.exec(header.trim());
  if (!match || (!match[1] && !match[2])) return undefined;
  const first = match[1] ? Number(match[1]) : undefined;
  const last = match[2] ? Number(match[2]) : undefined;
  if (
    (first !== undefined && !Number.isSafeInteger(first)) ||
    (last !== undefined && !Number.isSafeInteger(last))
  )
    return undefined;
  if (
    size === 0 ||
    (first === undefined && last === 0) ||
    (first !== undefined && first >= size) ||
    (first !== undefined && last !== undefined && last < first)
  )
    return { kind: 'unsatisfiable' };
  const start = first ?? Math.max(0, size - last!);
  const end =
    first === undefined
      ? size - 1
      : Math.min(last ?? size - 1, size - 1);
  return { kind: 'satisfiable', start, end };
}
function inside(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return (
    relative === '' ||
    (!relative.startsWith(`..${path.sep}`) &&
      relative !== '..' &&
      !path.isAbsolute(relative))
  );
}
