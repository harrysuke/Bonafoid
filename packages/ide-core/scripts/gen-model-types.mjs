// Converts the schemars-generated JSON Schema (crates/model) into TypeScript
// types. This keeps the browser-side view of the model in lockstep with the
// Rust source of truth.
//
// Pipeline:
//   cargo run --manifest-path xtask/Cargo.toml -- types
//   node packages/ide-core/scripts/gen-model-types.mjs [--check]
//
// Run with `--check` in CI to fail when the checked-in output is stale.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const schemaPath = join(here, '..', 'src', 'types', 'model.schema.json');
const outPath = join(here, '..', 'src', 'types', 'model.generated.ts');
const check = process.argv.includes('--check');

const schema = JSON.parse(readFileSync(schemaPath, 'utf8'));
const definitions = schema.definitions ?? schema.$defs ?? {};

const identifier = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

function refName(ref) {
  return ref.replace(/^#\/(definitions|\$defs)\//, '');
}

function propertyKey(key) {
  return identifier.test(key) ? key : JSON.stringify(key);
}

function union(nodes, indent) {
  const parts = [];
  for (const node of nodes) {
    const rendered = tsType(node, indent);
    if (!parts.includes(rendered)) parts.push(rendered);
  }
  return parts.length > 0 ? parts.join(' | ') : 'never';
}

function objectLiteral(node, indent) {
  const required = new Set(node.required ?? []);
  const entries = Object.entries(node.properties ?? {});
  const inner = indent + '  ';
  const lines = entries.map(
    ([key, value]) =>
      `${inner}${propertyKey(key)}${required.has(key) ? '' : '?'}: ${tsType(value, inner)};`,
  );
  if (node.additionalProperties && node.additionalProperties !== false) {
    lines.push(`${inner}[key: string]: unknown;`);
  }
  if (lines.length === 0) return 'Record<string, unknown>';
  return `{\n${lines.join('\n')}\n${indent}}`;
}

function tsType(node, indent = '') {
  if (node === true || node == null) return 'unknown';
  if (node === false) return 'never';
  if (node.$ref) return refName(node.$ref);
  if (Array.isArray(node.oneOf)) return union(node.oneOf, indent);
  if (Array.isArray(node.anyOf)) return union(node.anyOf, indent);
  if (Array.isArray(node.enum)) {
    return node.enum.length > 0 ? node.enum.map((value) => JSON.stringify(value)).join(' | ') : 'never';
  }
  if (node.const !== undefined) return JSON.stringify(node.const);
  if (Array.isArray(node.type)) {
    return union(
      node.type.map((type) => ({ ...node, type })),
      indent,
    );
  }
  switch (node.type) {
    case 'string':
      return 'string';
    case 'integer':
    case 'number':
      return 'number';
    case 'boolean':
      return 'boolean';
    case 'null':
      return 'null';
    case 'array':
      return `Array<${tsType(node.items, indent)}>`;
    case 'object':
      if (node.properties) return objectLiteral(node, indent);
      if (node.additionalProperties && node.additionalProperties !== false) {
        const value =
          node.additionalProperties === true ? 'unknown' : tsType(node.additionalProperties, indent);
        return `Record<string, ${value}>`;
      }
      return 'Record<string, unknown>';
    default:
      return 'unknown';
  }
}

function isPlainObject(node) {
  return Boolean(node) && node.type === 'object' && Boolean(node.properties);
}

function emitDefinition(name, node) {
  if (isPlainObject(node)) {
    const required = new Set(node.required ?? []);
    const entries = Object.entries(node.properties ?? {});
    const lines = entries.map(
      ([key, value]) => `  ${propertyKey(key)}${required.has(key) ? '' : '?'}: ${tsType(value, '  ')};`,
    );
    if (node.additionalProperties && node.additionalProperties !== false) {
      lines.push('  [key: string]: unknown;');
    }
    if (lines.length === 0) return `export type ${name} = Record<string, unknown>;`;
    return `export interface ${name} {\n${lines.join('\n')}\n}`;
  }
  return `export type ${name} = ${tsType(node, '')};`;
}

const rootName = schema.title && identifier.test(schema.title) ? schema.title : 'RootSchema';
const names = Object.keys(definitions).sort();

const blocks = names.map((name) => emitDefinition(name, definitions[name]));
blocks.push(emitDefinition(rootName, schema));

const header = [
  '// AUTO-GENERATED. DO NOT EDIT BY HAND.',
  '// Source: crates/model (schemars) -> src/types/model.schema.json.',
  '// Regenerate: cargo run --manifest-path xtask/Cargo.toml -- types',
  '//             node packages/ide-core/scripts/gen-model-types.mjs',
  '',
].join('\n');

const output = `${header}${blocks.join('\n\n')}\n`;

if (check) {
  let current = '';
  try {
    current = readFileSync(outPath, 'utf8');
  } catch {
    current = '';
  }
  if (current !== output) {
    console.error(
      'model.generated.ts is stale. Run: cargo run --manifest-path xtask/Cargo.toml -- types && node packages/ide-core/scripts/gen-model-types.mjs',
    );
    process.exit(1);
  }
  console.log('model.generated.ts is up to date.');
} else {
  writeFileSync(outPath, output);
  console.log(`Wrote ${outPath} (${names.length + 1} types).`);
}
