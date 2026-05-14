import { Type, type TSchema, type TObject, type TProperties } from "@sinclair/typebox";
import type { BackendSocket, ToolSchema, ToolsList } from "./websocket.js";

// Backend tool names that Pi handles natively — do not register these.
const SKIP_TOOLS = new Set([
  "read_file",
  "open_file",
  "write_file",
  "run_bash",
]);

/**
 * Fetch tool schemas from the backend and register non-native tools with Pi.
 * Returns the set of registered tool names so the caller can activate only
 * these tools (not every inactive tool in Pi).
 */
export async function registerBackendTools(
  pi: any, // ExtensionAPI — typed as any to avoid hard dep on Pi types
  socket: BackendSocket
): Promise<{ count: number; names: Set<string> }> {
  const response = await socket.requestByType<ToolsList>(
    { type: "get_tools" },
    "tools_list"
  );

  let registered = 0;
  const names = new Set<string>();

  for (const tool of response.tools as unknown as ToolSchema[]) {
    const fn = tool.function;
    if (!fn?.name || SKIP_TOOLS.has(fn.name)) continue;

    const name = fn.name;
    const description = fn.description || name;
    const parameters = jsonSchemaToTypebox(fn.parameters);

    pi.registerTool({
      name,
      label: name,
      description,
      promptSnippet: description,
      parameters,

      async execute(
        _toolCallId: string,
        params: Record<string, unknown>,
        signal: AbortSignal | undefined,
        _onUpdate: unknown,
        _ctx: unknown
      ) {
        if (socket.closed) {
          return {
            content: [{ type: "text", text: `Error: MageLab backend disconnected.` }],
            isError: true,
          };
        }

        // Check cancellation before starting the remote call.
        if (signal?.aborted) {
          return { content: [{ type: "text", text: "Cancelled" }], isError: true };
        }

        try {
          const result = await socket.callTool(name, params);

          // Check again after the (potentially long) remote call returns.
          if (signal?.aborted) {
            return {
              content: [{ type: "text", text: "Cancelled" }],
              isError: true,
            };
          }

          if (!result.success) {
            return {
              content: [
                { type: "text", text: `Error: ${result.error || "Unknown error"}` },
              ],
              isError: true,
            };
          }

          const text =
            typeof result.result === "string"
              ? result.result
              : JSON.stringify(result.result, null, 2);

          return {
            content: [{ type: "text", text: text ?? "(no output)" }],
          };
        } catch (err: any) {
          return {
            content: [{ type: "text", text: `Error: ${err.message}` }],
            isError: true,
          };
        }
      },
    });

    names.add(name);
    registered++;
  }

  return { count: registered, names };
}

/**
 * Convert a JSON Schema object definition to a TypeBox TObject.
 *
 * Handles the common types used by MageLab backend tool schemas:
 * string, number, integer, boolean, array, object. Anything unrecognized
 * falls back to Type.Unknown().
 */
function jsonSchemaToTypebox(
  schema?: { type?: string; properties?: Record<string, any>; required?: string[] }
): TObject {
  if (!schema?.properties) {
    return Type.Object({});
  }

  const required = new Set(schema.required || []);
  const props: TProperties = {};

  for (const [key, prop] of Object.entries(schema.properties)) {
    let field = convertType(prop);
    if (!required.has(key)) {
      field = Type.Optional(field);
    }
    props[key] = field;
  }

  return Type.Object(props);
}

function convertType(prop: any): TSchema {
  if (!prop || typeof prop !== "object") return Type.Unknown();

  const opts: Record<string, unknown> = {};
  if (prop.description) opts.description = prop.description;
  if (prop.default !== undefined) opts.default = prop.default;

  switch (prop.type) {
    case "string":
      if (prop.enum) {
        return Type.Union(
          prop.enum.map((v: string) => Type.Literal(v)),
          opts
        );
      }
      return Type.String(opts);

    case "number":
    case "integer":
      if (prop.minimum !== undefined) opts.minimum = prop.minimum;
      if (prop.maximum !== undefined) opts.maximum = prop.maximum;
      return prop.type === "integer" ? Type.Integer(opts) : Type.Number(opts);

    case "boolean":
      return Type.Boolean(opts);

    case "array":
      return Type.Array(prop.items ? convertType(prop.items) : Type.Unknown(), opts);

    case "object":
      if (prop.properties) {
        return jsonSchemaToTypebox(prop);
      }
      return Type.Record(Type.String(), Type.Unknown(), opts);

    default:
      return Type.Unknown();
  }
}
