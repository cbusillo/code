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
  `${index ?? "unknown"}-${suffix}`;

const eventKeyFromPayload = (
  payload: Record<string, unknown> | null | undefined,
) => {
  if (!payload) return null;
  const id = typeof payload.id === "string" ? payload.id : "";
  const seq = (payload as { event_seq?: number | string | null }).event_seq;
  if (!id && (seq === null || seq === undefined)) {
    return null;
  }
  return `${id}:${seq === null || seq === undefined ? "" : String(seq)}`;
};

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
  const seen = new Set<string>();

  const signatureFor = (item: TimelineItem, textOverride?: string) =>
    [
      item.index ?? "",
      item.kind,
      item.subkind ?? "",
      item.title ?? "",
      textOverride ?? item.text ?? "",
      item.imageUrl ?? "",
    ].join("::");

  const pushUnique = (item: TimelineItem) => {
    const signature = signatureFor(item);
    if (seen.has(signature)) {
      return;
    }
    seen.add(signature);
    items.push(item);
  };

  lines.forEach((line) => {
    const entry = extractEntry(line);
    const index = line.index;

    if (entry.type === "event") {
      const payload = entry.payload || {};
      const eventKey = eventKeyFromPayload(
        entry.payload as Record<string, unknown> | null | undefined,
      );
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
        pushUnique({
          id: makeId(index, "user"),
          kind: "user",
          subkind: "message",
          title: "You",
          text: asText((msg as any).message),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "agent_message") {
        pushUnique({
          id: makeId(index, "assistant"),
          kind: "assistant",
          subkind: "message",
          title: "Assistant",
          text: asText((msg as any).message),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (
        msgType === "task_started" ||
        msgType === "task_complete" ||
        msgType === "turn_aborted"
      ) {
        pushUnique({
          id: makeId(index, "task"),
          kind: "system",
          subkind: "message",
          title: msgType.replace(/_/g, " "),
          text: asText((msg as any).message || (msg as any).reason || msg),
          index,
          eventKey: eventKey ?? undefined,
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
          const prevText = last.text ?? "";
          const prevSignature = signatureFor(last, prevText);
          const nextValue = `${prevText}${nextText}`;
          last.text = nextValue;
          seen.delete(prevSignature);
          seen.add(signatureFor(last, nextValue));
        } else {
          pushUnique({
            id: makeId(index, "reasoning"),
            kind: "assistant",
            subkind: "reasoning",
            title: "Reasoning",
            text: nextText,
            index,
            eventKey: eventKey ?? undefined,
          });
        }
        return;
      }
      if (msgType === "exec_command_begin") {
        pushUnique({
          id: makeId(index, "exec-start"),
          kind: "exec",
          subkind: "exec-begin",
          title: "Exec",
          text: Array.isArray((msg as any).command)
            ? (msg as any).command.join(" ")
            : asText((msg as any).command),
          meta: "running",
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "exec_command_output_delta") {
        pushUnique({
          id: makeId(index, "exec-output"),
          kind: "exec",
          subkind: "exec-output",
          title: "Exec output",
          text: asText((msg as any).chunk),
          meta: asText((msg as any).stream),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "exec_command_end") {
        pushUnique({
          id: makeId(index, "exec-end"),
          kind: "exec",
          subkind: "exec-end",
          title: "Exec result",
          text: asText((msg as any).stderr || (msg as any).stdout),
          meta: `exit ${(msg as any).exit_code ?? "?"}`,
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "turn_diff") {
        pushUnique({
          id: makeId(index, "diff"),
          kind: "diff",
          subkind: "diff",
          title: "Diff",
          text: asText((msg as any).unified_diff),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "patch_apply_begin" || msgType === "patch_apply_end") {
        pushUnique({
          id: makeId(index, "patch"),
          kind: "diff",
          subkind: "diff",
          title:
            msgType === "patch_apply_begin" ? "Apply patch" : "Patch applied",
          text: asText((msg as any).path || (msg as any).message || msg),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "background_event") {
        pushUnique({
          id: makeId(index, "background"),
          kind: "system",
          subkind: "message",
          title: "Background event",
          text: asText((msg as any).message || (msg as any).event || msg),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "web_search_begin" || msgType === "web_search_complete") {
        pushUnique({
          id: makeId(index, "web"),
          kind: "tool",
          subkind: "tool-web",
          title:
            msgType === "web_search_begin"
              ? "Web search"
              : "Web search complete",
          text: asText((msg as any).query),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (msgType === "browser_screenshot_update") {
        pushUnique({
          id: makeId(index, "browser"),
          kind: "tool",
          subkind: "tool-browser",
          title: "Browser screenshot",
          text: asText((msg as any).url || (msg as any).screenshot_path),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (
        msgType === "mcp_tool_call_begin" ||
        msgType === "mcp_tool_call_end"
      ) {
        const invocation = (msg as any).invocation || {};
        pushUnique({
          id: makeId(index, "mcp"),
          kind: "tool",
          subkind: "tool-mcp",
          title:
            msgType === "mcp_tool_call_begin"
              ? "MCP tool"
              : "MCP tool finished",
          text: `${invocation.server || ""}.${invocation.tool || ""}`,
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
      if (
        msgType === "custom_tool_call_begin" ||
        msgType === "custom_tool_call_update" ||
        msgType === "custom_tool_call_end"
      ) {
        pushUnique({
          id: makeId(index, "tool"),
          kind: "tool",
          subkind: "tool-custom",
          title: msgType.replace(/_/g, " "),
          text: asText((msg as any).tool_name),
          index,
          eventKey: eventKey ?? undefined,
        });
        return;
      }
    }

    if (entry.type === "response_item") {
      const payload = entry.payload || {};
      const itemType = (payload as any).type;
      if (itemType === "reasoning") {
        pushUnique({
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
        pushUnique({
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
      const textSegments = content
        .filter(
          (segment: any) =>
            segment.type === "output_text" || segment.type === "input_text",
        )
        .map((segment: any) => asText(segment.text || segment.refusal))
        .filter(Boolean);
      if (textSegments.length) {
        pushUnique({
          id: makeId(index, "msg"),
          kind,
          subkind: "message",
          title:
            kind === "assistant"
              ? "Assistant"
              : kind === "user"
                ? "You"
                : "System",
          text: textSegments.join("\n\n"),
          index,
        });
      }
      content.forEach((segment: any, idx: number) => {
        if (segment.type === "output_image" || segment.type === "input_image") {
          pushUnique({
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
