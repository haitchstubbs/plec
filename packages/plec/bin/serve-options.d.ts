export interface ServeCliOptions {
  dir: string;
  host?: string;
  port?: number;
  development: boolean;
  trustProxy?: boolean;
}

export function parseServeOptions(
  args: string[],
  environment?: NodeJS.ProcessEnv,
): { help: true } | { options: ServeCliOptions };

export const SERVE_HELP: string;
