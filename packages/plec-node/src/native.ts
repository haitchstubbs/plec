import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

export interface NativeHeader {
  name: string;
  value: string;
}

export interface NativeRequest {
  method: string;
  url: string;
  headers: NativeHeader[];
}

export interface NativeApplicationOptions {
  artifactPath: string;
  clientScript?: string;
  clientStyles?: string[];
  stylesHref?: string;
  preloads?: string[];
  customElements?: string[];
  title?: string;
  description?: string;
  development?: boolean;
}

export interface NativeDocumentResponse {
  readonly status: number;
  readonly headers: NativeHeader[];
  body(): ReadableStream<Uint8Array>;
}

export interface PlecApplication {
  handleDocument(request: NativeRequest): Promise<NativeDocumentResponse>;
  close(): Promise<void>;
}

interface NativeModule {
  loadApplication(options: NativeApplicationOptions): Promise<PlecApplication>;
}

function nativeFilename(): string {
  const platform = process.platform;
  const arch = process.arch;
  const report = process.report?.getReport() as
    | { header?: { glibcVersionRuntime?: string } }
    | undefined;
  if (
    platform === 'linux' &&
    arch === 'x64' &&
    report?.header?.glibcVersionRuntime
  ) return 'index.linux-x64-gnu.node';
  throw new Error(`@plec/node native addon is unavailable for ${platform}-${arch}`);
}

const require = createRequire(import.meta.url);
const native = require(fileURLToPath(new URL(`../native/${nativeFilename()}`, import.meta.url))) as NativeModule;

export const loadApplication = native.loadApplication;
