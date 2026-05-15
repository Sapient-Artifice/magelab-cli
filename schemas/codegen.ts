#!/usr/bin/env npx tsx
/**
 * Codegen: reads schemas/websocket/protocol.json and generates:
 *   - src/client/messages.rs  (Rust serde types)
 *   - extension/src/protocol.ts  (TypeScript interfaces)
 *   - ../../mage-lab/backend/generated/ws_types.py  (Python Pydantic models)
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

// --- Python codegen ---

// Track enum names to handle collisions (Phase, Phase1, etc.)
const seenEnumNames = new Map<string, number>();

function enumClassName(fieldName: string): string {
  const base = pascalCase(fieldName);
  const count = seenEnumNames.get(base) || 0;
  seenEnumNames.set(base, count + 1);
  return count === 0 ? base : `${base}${count}`;
}

// Collect enum definitions to emit before the classes that use them
const enumDefs: string[] = [];

function jsonTypeToPython(prop: any, fieldName: string, optional: boolean): string {
  let t: string;
  if (prop.const) {
    t = `Literal['${prop.const}']`;
  } else if (prop.enum && prop.type === "string") {
    const cls = enumClassName(fieldName);
    const members = (prop.enum as string[]).map((v: string) => `    ${v} = '${v}'`).join("\n");
    enumDefs.push(`\nclass ${cls}(Enum):\n${members}\n`);
    t = cls;
  } else if (prop.type === "string") {
    t = "str";
  } else if (prop.type === "integer") {
    t = "int";
  } else if (prop.type === "number") {
    t = "float";
  } else if (prop.type === "boolean") {
    t = "bool";
  } else if (prop.type === "array") {
    const items = prop.items;
    if (!items) {
      t = "List[Any]";
    } else {
      const inner = jsonTypeToPython(items, fieldName, false);
      t = `List[${inner}]`;
    }
  } else if (prop.type === "object" && prop.additionalProperties) {
    t = "Dict[str, Any]";
  } else if (prop.type === "object") {
    t = "Dict[str, Any]";
  } else if (Array.isArray(prop.type)) {
    const nonNull = (prop.type as string[]).filter((x: string) => x !== "null");
    if (nonNull.length === 1) {
      t = jsonTypeToPython({ type: nonNull[0] }, fieldName, false);
      optional = true;
    } else {
      t = "Any";
    }
  } else if (!prop.type && Object.keys(prop).length === 0) {
    t = "Any";
  } else {
    t = "Any";
  }
  return optional ? `Optional[${t}]` : t;
}

function pyExtraConfig(def: any): string {
  if (def.additionalProperties === true) {
    return "    model_config = ConfigDict(\n        extra='allow',\n    )";
  }
  return "    model_config = ConfigDict(\n        extra='forbid',\n    )";
}

function genPyClass(name: string, def: any): string {
  const props = def.properties || {};
  const required = new Set(def.required || []);
  const lines: string[] = [];

  lines.push(`class ${name}(BaseModel):`);
  lines.push(pyExtraConfig(def));

  const requiredFields: string[] = [];
  const optionalFields: string[] = [];

  for (const [key, prop] of Object.entries(props) as [string, any][]) {
    const opt = !required.has(key);
    const pyType = jsonTypeToPython(prop, key, opt);

    if (prop.description && !prop.const) {
      if (opt) {
        optionalFields.push(`    ${key}: ${pyType} = None`);
      } else {
        const desc = prop.description.replace(/'/g, "\\'");
        requiredFields.push(`    ${key}: ${pyType} = Field(\n        ..., description='${desc}'\n    )`);
      }
    } else if (opt) {
      optionalFields.push(`    ${key}: ${pyType} = None`);
    } else {
      requiredFields.push(`    ${key}: ${pyType}`);
    }
  }

  lines.push(...requiredFields);
  lines.push(...optionalFields);

  if (requiredFields.length === 0 && optionalFields.length === 0) {
    lines.push("    pass");
  }

  return lines.join("\n");
}

function genPython(): string {
  seenEnumNames.clear();
  enumDefs.length = 0;

  const clientDef = defs.ClientMessage;
  const serverDef = defs.ServerMessage;

  const clientNames: string[] = clientDef.oneOf.map((r: any) => r.$ref.split("/").pop());
  const serverNames: string[] = serverDef.oneOf.map((r: any) => r.$ref.split("/").pop());

  const allNames = [...new Set([...clientNames, ...serverNames])];

  // Generate ToolsList nested types
  const toolsListDef = defs.ToolsList;
  const toolsItems = toolsListDef?.properties?.tools?.items;
  let nestedTypes = "";
  if (toolsItems?.properties?.function?.properties) {
    nestedTypes += "\n\nclass Function(BaseModel):";
    nestedTypes += "\n    name: str";
    nestedTypes += "\n    description: Optional[str] = None";
    nestedTypes += "\n    parameters: Optional[Dict[str, Any]] = None";
    nestedTypes += "\n\n\nclass Tool(BaseModel):";
    nestedTypes += "\n    type: Literal['function'] = 'function'";
    nestedTypes += "\n    function: Optional[Function] = None";
    nestedTypes += "\n";
  }

  // Generate classes, collecting enums along the way
  const orderedOutput: string[] = [];

  for (const name of allNames) {
    const prevEnumCount = enumDefs.length;

    let classCode: string;
    if (name === "ToolsList" && nestedTypes) {
      classCode = "class ToolsList(BaseModel):\n";
      classCode += "    model_config = ConfigDict(\n        extra='forbid',\n    )\n";
      classCode += "    type: Literal['tools_list']\n";
      classCode += "    tools: List[Tool]";
    } else {
      classCode = genPyClass(name, defs[name]);
    }

    // Emit any new enums created during this class generation
    for (let i = prevEnumCount; i < enumDefs.length; i++) {
      orderedOutput.push(enumDefs[i]);
    }

    orderedOutput.push(`\n${classCode}\n`);
  }

  // Build RootModel unions
  const clientUnion = clientNames.join(",\n            ");
  const serverUnion = serverNames.join(",\n            ");

  const rootModels = `

class ClientMessage(
    RootModel[
        Union[
            ${clientUnion},
        ]
    ]
):
    root: Union[
        ${clientUnion},
    ] = Field(..., description='Messages sent from client to backend')


class ServerMessage(
    RootModel[
        Union[
            ${serverUnion},
        ]
    ]
):
    root: Union[
        ${serverUnion},
    ] = Field(..., description='Messages sent from backend to client')
`;

  return `# AUTO-GENERATED from schemas/websocket/protocol.json
# Do not edit manually. Run: npx tsx schemas/codegen.ts

from __future__ import annotations

from enum import Enum
from typing import Any, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field, RootModel
${nestedTypes}
${orderedOutput.join("")}${rootModels}`;
}

// --- Main ---

const rustCode = genRust();
const tsCode = genTypeScript();
const pyCode = genPython();

const rustCliPath = resolve(__dirname, "../src/client/messages.rs");
const rustCorePath = resolve(__dirname, "../../crates/magelab-core/src/protocol/generated.rs");
const tsPath = resolve(__dirname, "../extension/src/protocol.ts");
const pyPath = resolve(__dirname, "../../mage-lab/backend/generated/ws_types.py");

writeFileSync(rustCliPath, rustCode);
writeFileSync(rustCorePath, rustCode);
writeFileSync(tsPath, tsCode);
writeFileSync(pyPath, pyCode);

console.log(`Generated:`);
console.log(`  ${rustCliPath}`);
console.log(`  ${rustCorePath}`);
console.log(`  ${tsPath}`);
console.log(`  ${pyPath}`);
