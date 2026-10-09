export function parseServeOptions(args, environment = process.env) {
  const options = {
    dir: 'dist',
    development: environment.NODE_ENV !== 'production',
  };
  let hasDirectory = false;
  let positionalOnly = false;

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (positionalOnly) {
      if (hasDirectory) throw new Error('serve accepts one directory');
      options.dir = argument;
      hasDirectory = true;
      continue;
    }
    if (argument === '--') {
      positionalOnly = true;
      continue;
    }
    if (argument === '-h' || argument === '--help')
      return { help: true };
    if (argument === '--development') {
      options.development = true;
      continue;
    }
    if (argument === '--trust-proxy') {
      options.trustProxy = true;
      continue;
    }
    if (argument === '--host' || argument === '--port') {
      const value = args[++index];
      if (value === undefined || value.startsWith('-'))
        throw new Error(`${argument} requires a value`);
      if (argument === '--host') options.host = value;
      else options.port = parsePort(value);
      continue;
    }
    if (argument.startsWith('--host=')) {
      options.host = argument.slice('--host='.length);
      if (!options.host) throw new Error('--host requires a value');
      continue;
    }
    if (argument.startsWith('--port=')) {
      options.port = parsePort(argument.slice('--port='.length));
      continue;
    }
    if (argument.startsWith('-'))
      throw new Error(`unknown serve option: ${argument}`);
    if (hasDirectory) throw new Error('serve accepts one directory');
    options.dir = argument;
    hasDirectory = true;
  }
  return { options };
}

export const SERVE_HELP = `Serve a built Plec application with the Node host

Usage: plec serve [OPTIONS] [DIR]

Arguments:
  [DIR]  Build output directory [default: dist]

Options:
      --host <HOST>  Bind address [default: 127.0.0.1]
      --port <PORT>  Port [default: PORT environment variable, then 3000]
      --development  Enable development diagnostics
      --trust-proxy  Trust forwarding headers from a trusted proxy
  -h, --help         Print help`;

function parsePort(value) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error('port must be an integer from 1 to 65535');
  return port;
}
