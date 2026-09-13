import { readFileSync } from 'node:fs';

function parseString(source, offset) {
  const start = offset;
  offset += 1;

  while (offset < source.length) {
    if (source[offset] === '\\') {
      offset += 2;
    } else if (source[offset] === '"') {
      offset += 1;
      return [JSON.parse(source.slice(start, offset)), offset];
    } else {
      offset += 1;
    }
  }

  throw new Error('Unterminated JSON string.');
}

function skipWhitespace(source, offset) {
  while (/\s/.test(source[offset])) {
    offset += 1;
  }
  return offset;
}

function parseValue(source, offset) {
  offset = skipWhitespace(source, offset);

  if (source[offset] === '{') {
    return parseObject(source, offset);
  }
  if (source[offset] === '[') {
    offset += 1;
    offset = skipWhitespace(source, offset);
    while (source[offset] !== ']') {
      offset = parseValue(source, offset);
      offset = skipWhitespace(source, offset);
      if (source[offset] === ',') {
        offset = skipWhitespace(source, offset + 1);
      } else if (source[offset] !== ']') {
        throw new Error(`Expected ',' or ']' at offset ${offset}.`);
      }
    }
    return offset + 1;
  }
  if (source[offset] === '"') {
    return parseString(source, offset)[1];
  }

  while (offset < source.length && !/[,}\]\s]/.test(source[offset])) {
    offset += 1;
  }
  return offset;
}

function parseObject(source, offset) {
  const keys = new Set();
  offset = skipWhitespace(source, offset + 1);

  while (source[offset] !== '}') {
    if (source[offset] !== '"') {
      throw new Error(`Expected object key at offset ${offset}.`);
    }
    const [key, afterKey] = parseString(source, offset);
    if (keys.has(key)) {
      throw new Error(`Duplicate JSON key: ${key}`);
    }
    keys.add(key);

    offset = skipWhitespace(source, afterKey);
    if (source[offset] !== ':') {
      throw new Error(`Expected ':' at offset ${offset}.`);
    }
    offset = skipWhitespace(source, parseValue(source, offset + 1));
    if (source[offset] === ',') {
      offset = skipWhitespace(source, offset + 1);
    } else if (source[offset] !== '}') {
      throw new Error(`Expected ',' or '}' at offset ${offset}.`);
    }
  }

  return offset + 1;
}

for (const file of process.argv.slice(2)) {
  const source = readFileSync(file, 'utf8');
  JSON.parse(source);
  const offset = skipWhitespace(source, parseValue(source, 0));
  if (offset !== source.length) {
    throw new Error(`${file}: unexpected content at offset ${offset}.`);
  }
}
