import type { HistoryLine, RolloutEntry, TimelineItem } from "./types";

const extractEntry = (line: HistoryLine): RolloutEntry =>
  (line.value as RolloutEntry) || line;

const asText = (value: unknown) => {
  if (value === undefined || value === null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

const makeId = (index: number | undefined, suffix: string) =>
  `${index ?? ""}-${suffix}-${Math.random().toString(36).slice(2, 6)}`;

const roleToKind = (role: string | undefined) => {
  switch (role) {
    case "assistant":
      return "assistant";
    case "user":
      return "user";
    case "system":
    case "developer":
      return "system";
    case "tool":
      return "tool";
    default:
      return "assistant";
  }
};

const summarizePayload = (payload: Record<string, unknown>) => {
  const summary = payload.summary as Array<{ text?: string }> | undefined;
  if (Array.isArray(summary)) {
    const lines = summary
      .map((item) => (typeof item?.text === "string" ? item.text : ""))
      .filter(Boolean);
    if (lines.length) {
      return lines.join("\n\n");
    }
  }
  if (typeof payload.content === "string") {
    return payload.content;
  }
  return "";
};

export const parseHistoryItems = (lines: HistoryLine[]) => {
  const items: TimelineItem[] = [];

  lines.forEach((line) => {
    const entry = extractEntry(line);
    const index = line.index;

    if (entry.type === "event") {
      const payload = entry.payload || {};
      const msg =
        (payload as any).msg ||
        (payload as any).message ||
        (payload as any).event ||
        payload;
      if (!msg || typeof msg !== "object") {
        return;
      }
      const msgType = (msg as any).type;
      if (!msgType) {
        return;
      }
      if (msgType === "user_message") {
        items.push({
          id: makeId(index, "user"),
          kind: "user",
          subkind: "message",
          title: "You",
          text: asText((msg as any).message),
          index,
        });
        return;
      }
      if (msgType === "agent_message") {
        items.push({
          id: makeId(index, "assistant"),
          kind: "assistant",
          subkind: "message",
          title: "Assistant",
          text: asText((msg as any).message),
          index,
        });
        return;
      }
      if (
        msgType === "task_started" ||
        msgType === "task_complete" ||
        msgType === "turn_aborted"
      ) {
        items.push({
          id: makeId(index, "task"),
          kind: "system",
          subkind: "message",
          title: msgType.replace(/_/g, " "),
          text: asText((msg as any).message || (msg as any).reason || msg),
          index,
        });
        return;
      }
      if (
        msgType === "agent_reasoning" ||
        msgType === "agent_reasoning_delta"
      ) {
        const nextText = asText((msg as any).text || (msg as any).delta);
        if (!nextText) {
          return;
        }
        const last = items[items.length - 1];
        if (
          last &&
          last.kind === "assistant" &&
          last.subkind === "reasoning" &&
          last.index === index
        ) {
          last.text = `${last.text ?? ""}${nextText}`;
        } else {
          items.push({
            id: makeId(index, "reasoning"),
            kind: "assistant",
            subkind: "reasoning",
            title: "Reasoning",
            text: nextText,
            index,
          });
        }
        return;
      }
      if (msgType === "exec_command_begin") {
        items.push({
          id: makeId(index, "exec-start"),
          kind: "exec",
          subkind: "exec",
          title: "Exec",
          text: Array.isArray((msg as any).command)
            ? (msg as any).command.join(" ")
            : asText((msg as any).command),
          meta: "running",
          index,
        });
        return;
      }
      if (msgType === "exec_command_end") {
        items.push({
          id: makeId(index, "exec-end"),
          kind: "exec",
          subkind: "exec",
          title: "Exec finished",
          text: asText((msg as any).stderr || (msg as any).stdout),
          meta: `exit ${(msg as any).exit_code ?? "?"}`,
          index,
        });
        return;
      }
      if (msgType === "turn_diff") {
        items.push({
          id: makeId(index, "diff"),
          kind: "diff",
          subkind: "diff",
          title: "Diff",
          text: asText((msg as any).unified_diff),
          index,
        });
        return;
      }
      if (msgType === "patch_apply_begin" || msgType === "patch_apply_end") {
        items.push({
          id: makeId(index, "patch"),
          kind: "diff",
          subkind: "diff",
          title:
            msgType === "patch_apply_begin" ? "Apply patch" : "Patch applied",
          text: asText((msg as any).path || (msg as any).message || msg),
          index,
        });
        return;
      }
      if (msgType === "background_event") {
        items.push({
          id: makeId(index, "background"),
          kind: "system",
          subkind: "message",
          title: "Background event",
          text: asText((msg as any).message || (msg as any).event || msg),
          index,
        });
        return;
      }
      if (msgType === "web_search_begin" || msgType === "web_search_complete") {
        items.push({
          id: makeId(index, "web"),
          kind: "tool",
          subkind: "tool-web",
          title:
            msgType === "web_search_begin"
              ? "Web search"
              : "Web search complete",
          text: asText((msg as any).query),
          index,
        });
        return;
      }
      if (msgType === "browser_screenshot_update") {
        items.push({
          id: makeId(index, "browser"),
          kind: "tool",
          subkind: "tool-browser",
          title: "Browser screenshot",
          text: asText((msg as any).url || (msg as any).screenshot_path),
          index,
        });
        return;
      }
      if (
        msgType === "mcp_tool_call_begin" ||
        msgType === "mcp_tool_call_end"
      ) {
        const invocation = (msg as any).invocation || {};
        items.push({
          id: makeId(index, "mcp"),
          kind: "tool",
          subkind: "tool-mcp",
          title:
            msgType === "mcp_tool_call_begin"
              ? "MCP tool"
              : "MCP tool finished",
          text: `${invocation.server || ""}.${invocation.tool || ""}`,
          index,
        });
        return;
      }
      if (
        msgType === "custom_tool_call_begin" ||
        msgType === "custom_tool_call_update" ||
        msgType === "custom_tool_call_end"
      ) {
        items.push({
          id: makeId(index, "tool"),
          kind: "tool",
          subkind: "tool-custom",
          title: msgType.replace(/_/g, " "),
          text: asText((msg as any).tool_name),
          index,
        });
        return;
      }
    }

    if (entry.type === "response_item") {
      const payload = entry.payload || {};
      const itemType = (payload as any).type;
      if (itemType === "reasoning") {
        items.push({
          id: makeId(index, "reasoning"),
          kind: "assistant",
          subkind: "reasoning",
          title: "Reasoning",
          text: summarizePayload(payload) || asText(payload),
          index,
        });
        return;
      }
      if (itemType !== "message") {
        const label = (payload as any).name || (payload as any).type || "Tool";
        items.push({
          id: makeId(index, "tool"),
          kind: "tool",
          subkind: itemType === "function_call" ? "tool-call" : "tool-output",
          title: label,
          text: asText((payload as any).arguments || (payload as any).output),
          index,
        });
        return;
      }
      const role = (payload as any).role;
      const kind = roleToKind(role);
      const content = Array.isArray((payload as any).content)
        ? (payload as any).content
        : [];
      content.forEach((segment: any, idx: number) => {
        if (segment.type === "output_text" || segment.type === "input_text") {
          items.push({
            id: makeId(index, `msg-${idx}`),
            kind,
            subkind: "message",
            title:
              kind === "assistant"
                ? "Assistant"
                : kind === "user"
                  ? "You"
                  : "System",
            text: asText(segment.text || segment.refusal),
            index,
          });
        }
        if (segment.type === "output_image" || segment.type === "input_image") {
          items.push({
            id: makeId(index, `img-${idx}`),
            kind,
            subkind: "image",
            title: kind === "assistant" ? "Assistant image" : "Image",
            imageUrl: segment.image_url || segment.url,
            index,
          });
        }
      });
    }
  });

  return items;
};
