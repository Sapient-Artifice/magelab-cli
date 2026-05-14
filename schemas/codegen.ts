#!/usr/bin/env npx tsx
/**
 * Codegen: reads schemas/websocket/protocol.json and generates:
 *   - src/client/messages.rs  (Rust serde types)
 *   - extension/src/protocol.ts  (TypeScript interfaces)
 *
 * Usage: npx tsx schemas/codegen.ts
 */

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const schema = JSON.parse(
  readFileSync(resolve(__dirname, "websocket/protocol.json"), "utf-8")
);
const defs = schema.$defs;

// --- Helpers ---

function pascalCase(s: string): string {
  return s.replace(/(^|_|-)([a-z])/g, (_, __, c) => c.toUpperCase());
}

function snakeCase(s: string): string {
  return s.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();
}

function typeValue(def: any): string {
  return def.properties?.type?.const || "unknown";
}

function isOptional(name: string, def: any): boolean {
  return !(def.required || []).includes(name);
}

// --- Rust codegen ---

function jsonTypeToRust(prop: any, optional: boolean): string {
  let t: string;
  if (prop.const) {
    t = "String";
  } else if (prop.type === "string") {
    t = "String";
  } else if (prop.type === "integer") {
    t = "i64";
  } else if (prop.type === "boolean") {
    t = "bool";
  } else if (prop.type === "array") {
    const items = prop.items;
    const inner = items ? jsonTypeToRust(items, false) : "Value";
    t = `Vec<${inner}>`;
  } else if (prop.type === "object" && prop.additionalProperties) {
    t = "HashMap<String, Value>";
  } else if (prop.type === "object") {
    t = "Value";
  } else {
    t = "Value"; // catch-all
  }
  return optional ? `Option<${t}>` : t;
}

function genRustVariant(name: string, def: any): string {
  const tv = typeValue(def);
  const props = def.properties || {};
  const fields: string[] = [];

  for (const [key, prop] of Object.entries(props) as [string, any][]) {
    if (key === "type") continue;
    const opt = isOptional(key, def);
    const rustType = jsonTypeToRust(prop, opt);
    if (opt) {
      fields.push(`        #[serde(default)]\n        ${snakeCase(key)}: ${rustType},`);
    } else {
      fields.push(`        ${snakeCase(key)}: ${rustType},`);
    }
  }

  const rename = `    #[serde(rename = "${tv}")]`;
  if (fields.length === 0) {
    return `${rename}\n    ${name} {},`;
  }
  return `${rename}\n    ${name} {\n${fields.join("\n")}\n    },`;
}

function genRust(): string {
  const clientDef = defs.ClientMessage;
  const serverDef = defs.ServerMessage;

  const clientNames = clientDef.oneOf.map((r: any) => r.$ref.split("/").pop());
  const serverNames = serverDef.oneOf.map((r: any) => r.$ref.split("/").pop());

  const clientVariants = clientNames
    .map((n: string) => genRustVariant(n, defs[n]))
    .join("\n\n");

  const serverVariants = serverNames
    .map((n: string) => genRustVariant(n, defs[n]))
    .join("\n\n");

  return `// AUTO-GENERATED from schemas/websocket/protocol.json
// Do not edit manually. Run: npx tsx schemas/codegen.ts

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Messages sent from client to backend via WebSocket
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum OutgoingMessage {
${clientVariants}
}

/// Messages received from backend via WebSocket
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum IncomingMessage {
${serverVariants}

    #[serde(other)]
    Unknown,
}
`;
}

// --- TypeScript codegen ---

function jsonTypeToTs(prop: any): string {
  if (prop.const) return `"${prop.const}"`;
  if (prop.type === "string") return "string";
  if (prop.type === "integer" || prop.type === "number") return "number";
  if (prop.type === "boolean") return "boolean";
  if (prop.type === "array") {
    const inner = prop.items ? jsonTypeToTs(prop.items) : "unknown";
    return `${inner}[]`;
  }
  if (prop.type === "object") return "Record<string, unknown>";
  if (!prop.type) return "unknown"; // untyped (e.g. result: {})
  return "unknown";
}

function genTsInterface(name: string, def: any): string {
  const props = def.properties || {};
  const lines: string[] = [];

  for (const [key, prop] of Object.entries(props) as [string, any][]) {
    const opt = isOptional(key, def) ? "?" : "";
    const tsType = jsonTypeToTs(prop);
    lines.push(`  ${key}${opt}: ${tsType};`);
  }

  return `export interface ${name} {\n${lines.join("\n")}\n}`;
}

function genTypeScript(): string {
  const clientDef = defs.ClientMessage;
  const serverDef = defs.ServerMessage;

  const clientNames: string[] = clientDef.oneOf.map((r: any) => r.$ref.split("/").pop());
  const serverNames: string[] = serverDef.oneOf.map((r: any) => r.$ref.split("/").pop());

  const allNames = [...new Set([...clientNames, ...serverNames])];
  const interfaces = allNames
    .map((n) => genTsInterface(n, defs[n]))
    .join("\n\n");

  const clientUnion = clientNames.join("\n  | ");
  const serverUnion = serverNames.join("\n  | ");

  return `// AUTO-GENERATED from schemas/websocket/protocol.json
// Do not edit manually. Run: npx tsx schemas/codegen.ts

${interfaces}

export type ClientMessage =
  | ${clientUnion};

export type ServerMessage =
  | ${serverUnion};
`;
}

// --- Main ---

const rustCode = genRust();
const tsCode = genTypeScript();

const rustCliPath = resolve(__dirname, "../src/client/messages.rs");
const rustCorePath = resolve(__dirname, "../../crates/magelab-core/src/protocol/generated.rs");
const tsPath = resolve(__dirname, "../extension/src/protocol.ts");

writeFileSync(rustCliPath, rustCode);
writeFileSync(rustCorePath, rustCode);
writeFileSync(tsPath, tsCode);

console.log(`Generated:`);
console.log(`  ${rustCliPath}`);
console.log(`  ${rustCorePath}`);
console.log(`  ${tsPath}`);
