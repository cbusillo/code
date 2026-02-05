<script setup lang="ts">
import { computed, ref, watch } from "vue";

import type {
  HistoryLine,
  SessionSummary,
  TimelineItem,
  TimelineKind,
} from "../api/types";
import { parseHistoryItems } from "../api/rollout";
import { useGatewayToken } from "../hooks/useGatewayToken";
import { useTheme } from "../hooks/useTheme";
import ComposerDock from "../features/composer/ComposerDock.vue";
import FiltersPanel from "../features/filters/FiltersPanel.vue";
import SessionsPanel from "../features/sessions/SessionsPanel.vue";
import TimelinePanel from "../features/timeline/TimelinePanel.vue";

import styles from "./App.module.css";

const FILTERS_KEY = "codeGatewayFiltersV3";
const SESSIONS_CACHE_PREFIX = "codeGatewaySessionsCacheV1";
type HistoryWsMode = "initial" | "stream";

type FilterState = {
  kinds: Record<TimelineKind, boolean>;
  subkinds: Record<string, boolean>;
};

type HistoryRequestPayload = {
  before?: number;
  after?: number;
  anchor?: "head" | "tail";
  limit?: number;
};

const defaultFilters: FilterState = {
  kinds: {
    assistant: true,
    user: true,
    system: true,
    tool: true,
    exec: true,
    diff: true,
    review: true,
  },
  subkinds: {
    reasoning: true,
    image: true,
    exec: true,
    "exec-begin": true,
    "exec-output": true,
    "exec-end": true,
    diff: true,
    "tool-mcp": true,
    "tool-custom": true,
    "tool-web": true,
    "tool-browser": true,
    "tool-call": true,
    "tool-output": true,
  },
};

const { theme, toggle } = useTheme();
const { token, setToken, label } = useGatewayToken();

const isBlankSession = (session: SessionSummary) => {
  const title =
    session.nickname || session.summary || session.last_user_snippet || "";
  const count = session.message_count ?? 0;
  return count === 0 && title.trim().length === 0;
};

const isHiddenSession = (session: SessionSummary, active: string | null) =>
  isBlankSession(session) && session.conversation_id !== active;

const sessions = ref<SessionSummary[]>([]);
const sessionsPulse = ref(0);
const sessionsLoading = ref(false);
const activeId = ref<string | null>(null);
const items = ref<TimelineItem[]>([]);
const status = ref("Idle");
const error = ref<string | null>(null);
const loading = ref(false);
const search = ref("");
const hasMoreBefore = ref(false);
const hasMoreAfter = ref(false);
const historyStart = ref<number | null>(null);
const historyEnd = ref<number | null>(null);
const composerText = ref("");
const sending = ref(false);
const filtersCollapsed = ref(false);
const historyMode = ref<"tail" | "head">("tail");
const runtimeId = ref<string | null>(null);
const sessionLoading = ref(false);
const wsRef = ref<WebSocket | null>(null);
const wsRetryRef = ref<number | null>(null);
const historyWsRef = ref<WebSocket | null>(null);
const historyHandshakeRef = ref<
  { id: string; token: string; at: number } | null
>(null);
const historyRetryRef = ref<number | null>(null);
const pendingHistoryRequests = ref<HistoryRequestPayload[]>([]);
const historyWsDisabledRef = ref(false);
const historyWsAttemptRef = ref(0);
const historyWsUrlRef = ref<string | null>(null);
const historyConnectTimerRef = ref<number | null>(null);
const historyWsModeRef = ref<HistoryWsMode>("initial");
const historyInitialSessionRef = ref<string | null>(null);
const historyActiveTokenRef = ref<string | null>(null);
const knownSessionIdsRef = ref<Set<string>>(new Set());
const lastRequestedSessionRef = ref<string | null>(null);
const historyWsBackoffRef = ref(0);
const lastIndexRef = ref<number | null>(null);
const seenLineIndexRef = ref<Set<number>>(new Set());
const liveIndexRef = ref<number | null>(null);
const seenEventRef = ref<Set<string>>(new Set());
const reasoningIdRef = ref<string | null>(null);
const controlWsRef = ref<WebSocket | null>(null);
const controlRetryRef = ref<number | null>(null);
const pendingSessionsRef = ref(false);
const pendingControlMessages = ref<object[]>([]);
const pendingResumeRef = ref<string | null>(null);
const sessionLoadSeq = ref(0);
const mobilePanel = ref<"none" | "sessions" | "filters">("none");
const lazyHistory = ref(true);
const lazyHistoryReady = ref(false);
const historyPrimed = ref(false);
const primeHistoryTimerRef = ref<number | null>(null);
const primeHistoryInFlightRef = ref(false);
const primeFillTimerRef = ref<number | null>(null);
const primeFillInFlightRef = ref(false);
const primeFillExpectedBeforeRef = ref<number | null>(null);
const taskActiveRef = ref(false);
const activityLabelRef = ref<string | null>(null);
const activeExecCountRef = ref(0);
const activeToolCountRef = ref(0);

const HISTORY_PRIME_LIMIT = 40;
const HISTORY_TARGET_LIMIT = 200;
const HISTORY_LARGE_SESSION_THRESHOLD = 10000;
const HISTORY_LARGE_PRIME_LIMIT = 20;
const HISTORY_AUTO_FILL_THRESHOLD = 5000;

const messageSignatureForItem = (item: TimelineItem) =>
  [
    item.kind,
    item.subkind ?? "",
    item.title ?? "",
    item.text ?? "",
    item.imageUrl ?? "",
  ].join("::");

const extractEntry = (line: HistoryLine) =>
  (line.value as Record<string, unknown> | undefined) || line;

const eventKeyFromPayload = (payload: Record<string, unknown> | null | undefined) => {
  if (!payload) return null;
  const id = typeof payload.id === "string" ? payload.id : "";
  const seq = (payload as { event_seq?: number | string | null }).event_seq;
  if (!id && (seq === null || seq === undefined)) {
    return null;
  }
  return `${id}:${seq === null || seq === undefined ? "" : String(seq)}`;
};

const resetHistoryTracking = () => {
  seenLineIndexRef.value = new Set();
  seenEventRef.value = new Set();
};

const registerHistoryLines = (
  lines: HistoryLine[],
  skipAgentMessages: boolean,
) => {
  const fresh: HistoryLine[] = [];
  lines.forEach((line) => {
    const index = line.index;
    if (index !== undefined && seenLineIndexRef.value.has(index)) {
      return;
    }
    const entry = extractEntry(line);
    if (entry.type === "event") {
      const payload = entry.payload as Record<string, unknown> | null | undefined;
      const msg =
        (payload as any)?.msg ||
        (payload as any)?.message ||
        (payload as any)?.event;
      const msgType = (msg as any)?.type;
      const key = eventKeyFromPayload(payload);
      if (key) {
        seenEventRef.value.add(key);
      }
      if (skipAgentMessages && msgType === "agent_message") {
        if (index !== undefined) {
          seenLineIndexRef.value.add(index);
        }
        return;
      }
    }
    if (index !== undefined) {
      seenLineIndexRef.value.add(index);
    }
    fresh.push(line);
  });
  return fresh;
};

const hasAssistantResponseItem = (lines: HistoryLine[]) =>
  lines.some((line) => {
    const entry = extractEntry(line);
    if (entry.type !== "response_item") {
      return false;
    }
    const payload = entry.payload as { type?: string; role?: string } | null | undefined;
    return payload?.type === "message" && payload?.role === "assistant";
  });

const dropEventDuplicates = (historyItems: TimelineItem[]) => {
  if (!items.value.length || !historyItems.length) {
    return;
  }
  const historyEventKeys = new Set(
    historyItems
      .map((item) => item.eventKey)
      .filter((key): key is string => Boolean(key)),
  );
  const historyMessageSignatures = new Set(
    historyItems
      .filter((item) => item.subkind === "message")
      .map((item) => messageSignatureForItem(item)),
  );
  items.value = items.value.filter((item) => {
    if (item.origin !== "event") {
      return true;
    }
    if (item.eventKey && historyEventKeys.has(item.eventKey)) {
      return false;
    }
    if (
      item.subkind === "message" &&
      historyMessageSignatures.has(messageSignatureForItem(item))
    ) {
      return false;
    }
    return true;
  });
};

const sessionsCacheKey = (tokenValue: string) =>
  `${SESSIONS_CACHE_PREFIX}::${tokenValue.slice(0, 8)}`;

const readSessionsCache = (tokenValue: string): SessionSummary[] | null => {
  try {
    const raw = localStorage.getItem(sessionsCacheKey(tokenValue));
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw) as { items?: unknown };
    if (!Array.isArray(parsed.items)) {
      return null;
    }
    return parsed.items as SessionSummary[];
  } catch {
    return null;
  }
};

const writeSessionsCache = (tokenValue: string, itemsList: SessionSummary[]) => {
  try {
    localStorage.setItem(
      sessionsCacheKey(tokenValue),
      JSON.stringify({
        savedAt: new Date().toISOString(),
        items: itemsList,
      }),
    );
  } catch {
    // Ignore cache failures (quota, privacy mode, etc.).
  }
};

const loadFilters = (): FilterState => {
  try {
    const raw = localStorage.getItem(FILTERS_KEY);
    if (!raw) {
      return defaultFilters;
    }
    const parsed = JSON.parse(raw) as Partial<FilterState>;
    return {
      kinds: { ...defaultFilters.kinds, ...(parsed.kinds || {}) },
      subkinds: { ...defaultFilters.subkinds, ...(parsed.subkinds || {}) },
    };
  } catch {
    return defaultFilters;
  }
};

const filters = ref<FilterState>(loadFilters());

const filteredItems = computed(() =>
  items.value.filter((item: TimelineItem) => {
    const hasText = Boolean(item.text && item.text.trim().length > 0);
    const hasImage = Boolean(item.imageUrl);
    if (!hasText && !hasImage) {
      return false;
    }
    if (item.subkind === "image") {
      return filters.value.subkinds.image !== false;
    }
    if (!filters.value.kinds[item.kind]) {
      return false;
    }
    if (item.subkind && filters.value.subkinds[item.subkind] === false) {
      return false;
    }
    return true;
  }),
);

const timelineLoading = computed(() => {
  if (loading.value) {
    return true;
  }
  if (sessionLoading.value && activeId.value && items.value.length === 0) {
    return true;
  }
  if (sessionsLoading.value && !activeId.value) {
    return true;
  }
  return false;
});

const composerStatus = computed(() => {
  if (sending.value) return "Sending";
  if (sessionLoading.value && activeId.value) return "Resuming session";
  if (loading.value && activeId.value) return "Loading history";
  if (activeExecCountRef.value > 0) return "Running command";
  if (activeToolCountRef.value > 0) {
    return activityLabelRef.value ?? "Using tools";
  }
  if (taskActiveRef.value) {
    return activityLabelRef.value ?? "Thinking";
  }
  if (status.value && !["Live", "Ready"].includes(status.value)) {
    return status.value;
  }
  return null;
});

const counts = computed(() => {
  const next: Record<TimelineKind, number> = {
    assistant: 0,
    user: 0,
    system: 0,
    tool: 0,
    exec: 0,
    diff: 0,
    review: 0,
  };
  items.value.forEach((item: TimelineItem) => {
    next[item.kind] += 1;
  });
  return next;
});

const isDev = import.meta.env.DEV;
const devBanner = computed(() => {
  if (!isDev) {
    return null;
  }
  const sessionId = activeId.value ?? runtimeId.value ?? "none";
  const sessionSource = sessions.value.find(
    (session) => session.conversation_id === activeId.value,
  )?.source;
  return {
    sessionId,
    sessionSource: sessionSource ?? "unknown",
    lastIndex: lastIndexRef.value ?? "-",
    streamOpen: wsRef.value?.readyState === WebSocket.OPEN,
    historyOpen: historyWsRef.value?.readyState === WebSocket.OPEN,
  };
});

const subCounts = computed(() => {
  const next: Record<string, number> = {};
  Object.keys(defaultFilters.subkinds).forEach((key) => {
    next[key] = 0;
  });
  items.value.forEach((item: TimelineItem) => {
    if (item.subkind) {
      next[item.subkind] = (next[item.subkind] || 0) + 1;
    }
  });
  return next;
});

const compactLabel = (value: string, maxLength: number) => {
  const trimmed = value.trim();
  if (trimmed.length <= maxLength) {
    return trimmed;
  }
  if (maxLength <= 3) {
    return trimmed.slice(0, maxLength);
  }
  return `${trimmed.slice(0, Math.max(0, maxLength - 3))}...`;
};

const activeSession = computed(() =>
  sessions.value.find((session) => session.conversation_id === activeId.value),
);

const mobileSessionLabel = computed(() => {
  const session = activeSession.value;
  if (!session) {
    return activeId.value ? activeId.value.slice(0, 8) : "Select session";
  }
  const candidate =
    session.nickname ||
    session.summary ||
    session.last_user_snippet ||
    session.cwd ||
    session.conversation_id;
  return compactLabel(candidate, 32);
});

const mobileFiltersCount = computed(() => {
  let total = 0;
  Object.values(filters.value.kinds).forEach((value) => {
    if (!value) {
      total += 1;
    }
  });
  Object.values(filters.value.subkinds).forEach((value) => {
    if (!value) {
      total += 1;
    }
  });
  return total;
});

const messageCountFor = (id: string | null) => {
  if (!id) return null;
  const match = sessions.value.find(
    (session) => session.conversation_id === id,
  );
  if (!match || typeof match.message_count !== "number") {
    return null;
  }
  return match.message_count;
};

const historyPrimeLimitFor = (id: string | null) => {
  const count = messageCountFor(id);
  if (count !== null && count >= HISTORY_LARGE_SESSION_THRESHOLD) {
    return HISTORY_LARGE_PRIME_LIMIT;
  }
  return HISTORY_PRIME_LIMIT;
};

const shouldAutoFillHistory = (id: string | null) => {
  const count = messageCountFor(id);
  if (count === null) {
    return true;
  }
  return count <= HISTORY_AUTO_FILL_THRESHOLD;
};

watch(
  filters,
  (value) => {
    localStorage.setItem(FILTERS_KEY, JSON.stringify(value));
  },
  { deep: true },
);

const scheduleResume = (id: string, delay: number) => {
  pendingResumeRef.value = id;
  sessionLoading.value = true;
  status.value = "Resuming session";
  const attempt = () => {
    if (pendingResumeRef.value != id) {
      return;
    }
    const ws = controlWsRef.value;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      window.setTimeout(attempt, 500);
      return;
    }
    sendControlMessage(
      { type: "resume_conversation", conversation_id: id },
      false,
    );
  };
  window.setTimeout(attempt, delay);
};

const closeHistorySocket = () => {
  const ws = historyWsRef.value;
  historyWsRef.value = null;
  historyHandshakeRef.value = null;
  if (!ws) return;
  historyWsBackoffRef.value = 0;
  if (ws.readyState === WebSocket.CONNECTING) {
    ws.onopen = () => ws.close();
    return;
  }
  ws.close();
};

const setHistoryWsModeForSession = (nextId: string | null) => {
  historyWsModeRef.value = "stream";
  historyWsDisabledRef.value = true;
  historyInitialSessionRef.value = null;
  historyWsBackoffRef.value = 0;
  if (nextId) {
    closeHistorySocket();
  }
};

const requestSessions = () => {
  sessionsLoading.value = true;
  pendingSessionsRef.value = true;
  const ws = controlWsRef.value;
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(
      JSON.stringify({
        type: "list_conversations",
        limit: 200,
      }),
    );
    pendingSessionsRef.value = false;
  }
};

const applySessionsList = (itemsList: SessionSummary[]) => {
  const active = activeId.value;
  const visible = (itemsList || []).filter(
    (item) => !isHiddenSession(item, active),
  );
  sessions.value = visible;
  knownSessionIdsRef.value = new Set(
    visible.map((item) => item.conversation_id),
  );
  sessionsPulse.value += 1;
  sessionsLoading.value = false;
  error.value = null;
  if (token.value) {
    writeSessionsCache(token.value, sessions.value);
  }
  if (!visible.length) {
    activateSession(null);
    status.value = "Ready";
    return;
  }
  if (active && visible.some((item) => item.conversation_id === active)) {
    return;
  }
  const last = localStorage.getItem("codeGatewayLastConversation");
  if (last && visible.some((item) => item.conversation_id === last)) {
    activateSession(last);
    return;
  }
  const fallback = visible[0].conversation_id;
  activateSession(fallback);
};

const sendControlMessage = (payload: object, allowQueue = true) => {
  const ws = controlWsRef.value;
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    if (allowQueue) {
      pendingControlMessages.value = [...pendingControlMessages.value, payload];
    }
    return true;
  }
  try {
    ws.send(JSON.stringify(payload));
  } catch (err) {
    error.value = (err as Error).message;
    status.value = "Error";
    return false;
  }
  return true;
};

const handleHistoryChunk = (message: {
  items: HistoryLine[];
  start_index?: number | null;
  end_index?: number | null;
  truncated?: boolean;
  before?: number | null;
  after?: number | null;
  anchor?: string | null;
}) => {
  primeHistoryInFlightRef.value = false;
  const startIndex = message.start_index;
  const endIndex = message.end_index;
  if (message.anchor === "head" || message.anchor === "tail") {
    resetHistoryTracking();
  }
  const rawLines = message.items || [];
  const skipAgentMessages = hasAssistantResponseItem(rawLines);
  const freshLines = registerHistoryLines(rawLines, skipAgentMessages);
  const parsed = parseHistoryItems(freshLines);
  const historyItems = parsed.map((item) => ({ ...item, origin: "history" }));
  if (message.anchor === "head") {
    cancelPrimeFill();
    reasoningIdRef.value = null;
    items.value = historyItems;
    historyMode.value = "head";
    hasMoreAfter.value = Boolean(message.truncated);
    hasMoreBefore.value = false;
    historyStart.value = startIndex ?? null;
    historyEnd.value = endIndex ?? null;
  } else if (message.anchor === "tail") {
    cancelPrimeFill();
    reasoningIdRef.value = null;
    items.value = historyItems;
    historyMode.value = "tail";
    hasMoreBefore.value = Boolean(message.truncated);
    hasMoreAfter.value = false;
    historyStart.value = startIndex ?? null;
    historyEnd.value = endIndex ?? null;
    cancelPrimeHistory();
    const canAutoFill = shouldAutoFillHistory(activeId.value);
    if (
      !primeFillInFlightRef.value &&
      canAutoFill &&
      message.truncated &&
      historyItems.length < HISTORY_TARGET_LIMIT &&
      startIndex !== undefined &&
      startIndex !== null
    ) {
      primeFillInFlightRef.value = true;
      primeFillExpectedBeforeRef.value = startIndex;
      const remaining = HISTORY_TARGET_LIMIT - historyItems.length;
      if (primeFillTimerRef.value !== null) {
        window.clearTimeout(primeFillTimerRef.value);
      }
      primeFillTimerRef.value = window.setTimeout(() => {
        primeFillTimerRef.value = null;
        sendHistoryRequest({ before: startIndex, limit: remaining });
      }, 200);
    }
  } else if (message.before !== undefined && message.before !== null) {
    if (primeFillExpectedBeforeRef.value === message.before) {
      primeFillInFlightRef.value = false;
      primeFillExpectedBeforeRef.value = null;
    }
    dropEventDuplicates(historyItems);
    items.value = [...historyItems, ...items.value];
    historyMode.value = "tail";
    hasMoreBefore.value = Boolean(message.truncated);
  } else if (message.after !== undefined && message.after !== null) {
    dropEventDuplicates(historyItems);
    const liveStart = items.value.findIndex((item) => item.origin !== "history");
    if (liveStart === -1) {
      items.value = [...items.value, ...historyItems];
    } else {
      items.value = [
        ...items.value.slice(0, liveStart),
        ...historyItems,
        ...items.value.slice(liveStart),
      ];
    }
    historyMode.value = "head";
    hasMoreAfter.value = Boolean(message.truncated);
  }
  if (
    message.anchor !== "head" &&
    message.anchor !== "tail" &&
    startIndex !== undefined &&
    startIndex !== null
  ) {
    historyStart.value =
      historyStart.value === null
        ? startIndex
        : Math.min(historyStart.value, startIndex);
  }
  if (
    message.anchor !== "head" &&
    message.anchor !== "tail" &&
    endIndex !== undefined &&
    endIndex !== null
  ) {
    historyEnd.value =
      historyEnd.value === null
        ? endIndex
        : Math.max(historyEnd.value, endIndex);
  }
  if (endIndex !== undefined && endIndex !== null) {
    if (liveIndexRef.value === null || liveIndexRef.value <= endIndex) {
      liveIndexRef.value = endIndex + 1;
    }
    lastIndexRef.value =
      lastIndexRef.value === null
        ? endIndex
        : Math.max(lastIndexRef.value, endIndex);
  }
  loading.value = false;
  status.value = "Live";
  sessionLoading.value = false;
};

const activateSession = (id: string | null) => {
  if (id === activeId.value) {
    return;
  }
  if (
    id &&
    knownSessionIdsRef.value.size > 0 &&
    !knownSessionIdsRef.value.has(id) &&
    runtimeId.value !== id
  ) {
    requestSessions();
    return;
  }
  setHistoryWsModeForSession(id);
  lastRequestedSessionRef.value = id;
  reasoningIdRef.value = null;
  primeHistoryInFlightRef.value = false;
  historyPrimed.value = false;
  lazyHistoryReady.value = false;
  pendingHistoryRequests.value = [];
  cancelPrimeFill();
  resetActivity();
  resetHistoryTracking();
  lastIndexRef.value = null;
  if (id) {
    localStorage.setItem("codeGatewayLastConversation", id);
  }
  sessionLoading.value = Boolean(id);
  runtimeId.value = null;
  items.value = [];
  historyStart.value = null;
  historyEnd.value = null;
  hasMoreBefore.value = false;
  hasMoreAfter.value = false;
  historyMode.value = "tail";
  liveIndexRef.value = null;
  activeId.value = id;
  if (id) {
    scheduleResume(id, 200);
    schedulePrimeHistory(true);
  }
};

const loadSession = (id: string, seq: number) => {
  if (!token.value) return;
  reasoningIdRef.value = null;
  primeHistoryInFlightRef.value = false;
  historyPrimed.value = false;
  lazyHistoryReady.value = false;
  pendingHistoryRequests.value = [];
  cancelPrimeFill();
  resetActivity();
  sessionLoading.value = true;
  items.value = [];
  historyStart.value = null;
  historyEnd.value = null;
  hasMoreBefore.value = false;
  hasMoreAfter.value = false;
  liveIndexRef.value = null;
  runtimeId.value = null;
  localStorage.setItem("codeGatewayLastConversation", id);
  loading.value = true;
  status.value = "Resuming session";
  scheduleResume(id, 400);
};

const enqueueHistoryRequest = (request: HistoryRequestPayload) => {
  pendingHistoryRequests.value = [...pendingHistoryRequests.value, request];
};

const buildHistoryPayload = (request: HistoryRequestPayload) => {
  const payload: Record<string, unknown> = {
    type: "history_request",
    ...request,
  };
  if (
    runtimeId.value &&
    activeId.value &&
    runtimeId.value !== activeId.value
  ) {
    payload.rollout = activeId.value;
  }
  return payload;
};

const flushPendingHistoryRequests = () => {
  if (!pendingHistoryRequests.value.length) {
    return;
  }
  const historyWs = historyWsRef.value;
  const streamWs = wsRef.value;
  const activeWs = historyWsModeRef.value === "initial"
    ? historyWs && historyWs.readyState === WebSocket.OPEN
      ? historyWs
      : streamWs && streamWs.readyState === WebSocket.OPEN
        ? streamWs
        : null
    : streamWs && streamWs.readyState === WebSocket.OPEN
      ? streamWs
      : null;
  if (!activeWs) {
    return;
  }
  const queued = pendingHistoryRequests.value;
  pendingHistoryRequests.value = [];
  queued.forEach((request) => {
    activeWs.send(JSON.stringify(buildHistoryPayload(request)));
  });
};

const sendHistoryRequest = (request: HistoryRequestPayload) => {
  const historyWs = historyWsRef.value;
  const streamWs = wsRef.value;
  if (
    historyWsModeRef.value === "initial" &&
    historyWs &&
    historyWs.readyState === WebSocket.OPEN
  ) {
    historyWs.send(JSON.stringify(buildHistoryPayload(request)));
    return;
  }
  if (streamWs && streamWs.readyState === WebSocket.OPEN) {
    streamWs.send(JSON.stringify(buildHistoryPayload(request)));
    return;
  }
  enqueueHistoryRequest(request);
};

const handleHistoryHandshakeFailure = () => {
  const active = activeId.value;
  const tokenValue = token.value;
  if (!active || !tokenValue) {
    return;
  }
  const state = historyHandshakeRef.value;
  if (!state || state.id != active || state.token !== tokenValue) {
    return;
  }
  historyHandshakeRef.value = null;
  if (!pendingResumeRef.value) {
    scheduleResume(active, 0);
  }
};

const primeHistory = async () => {
  if (historyPrimed.value || primeHistoryInFlightRef.value) return;
  primeHistoryInFlightRef.value = true;
  loading.value = true;
  status.value = "Loading recent history";
  const limit = historyPrimeLimitFor(activeId.value);
  sendHistoryRequest({ anchor: "tail", limit });
};

const schedulePrimeHistory = (force = false) => {
  if (!lazyHistory.value || historyPrimed.value) return;
  if (!force && historyWsModeRef.value !== "initial") return;
  if (!force && !lazyHistoryReady.value) return;
  if (primeHistoryTimerRef.value !== null) return;
  primeHistoryTimerRef.value = window.setTimeout(() => {
    primeHistoryTimerRef.value = null;
    void primeHistory();
  }, 50);
};

const cancelPrimeHistory = () => {
  if (primeHistoryTimerRef.value !== null) {
    window.clearTimeout(primeHistoryTimerRef.value);
    primeHistoryTimerRef.value = null;
  }
};

const cancelPrimeFill = () => {
  if (primeFillTimerRef.value !== null) {
    window.clearTimeout(primeFillTimerRef.value);
    primeFillTimerRef.value = null;
  }
  primeFillInFlightRef.value = false;
  primeFillExpectedBeforeRef.value = null;
};

const resetActivity = () => {
  taskActiveRef.value = false;
  activityLabelRef.value = null;
  activeExecCountRef.value = 0;
  activeToolCountRef.value = 0;
};

const noteActivity = (label: string) => {
  activityLabelRef.value = label;
  taskActiveRef.value = true;
};

const settleActivity = () => {
  if (activeExecCountRef.value > 0) {
    activityLabelRef.value = "Running command";
    return;
  }
  if (activeToolCountRef.value > 0) {
    activityLabelRef.value = activityLabelRef.value ?? "Using tools";
    return;
  }
  if (!taskActiveRef.value) {
    activityLabelRef.value = null;
  }
};

const updateActivityFromEvent = (msgType: string, msg: any) => {
  switch (msgType) {
    case "task_started":
      noteActivity("Thinking");
      return;
    case "task_complete":
    case "turn_aborted":
      taskActiveRef.value = false;
      settleActivity();
      return;
    case "agent_reasoning_delta":
    case "agent_reasoning":
      noteActivity("Thinking");
      return;
    case "agent_message_delta":
      noteActivity("Responding");
      return;
    case "agent_message":
      if (taskActiveRef.value) {
        activityLabelRef.value = "Responding";
      }
      return;
    case "exec_command_begin":
      activeExecCountRef.value += 1;
      activityLabelRef.value = "Running command";
      return;
    case "exec_command_end":
      activeExecCountRef.value = Math.max(0, activeExecCountRef.value - 1);
      settleActivity();
      return;
    case "custom_tool_call_begin":
    case "mcp_tool_call_begin":
    case "web_search_begin":
      activeToolCountRef.value += 1;
      activityLabelRef.value = "Using tools";
      return;
    case "custom_tool_call_end":
    case "mcp_tool_call_end":
    case "web_search_complete":
      activeToolCountRef.value = Math.max(0, activeToolCountRef.value - 1);
      settleActivity();
      return;
    case "browser_screenshot_update":
      if (!activityLabelRef.value) {
        activityLabelRef.value = "Using browser";
      }
      return;
    default:
      return;
  }
};

const loadOlder = async () => {
  if (historyStart.value === null) return;
  loading.value = true;
  status.value = "Loading earlier history";
  sendHistoryRequest({ before: historyStart.value, limit: 200 });
};

const loadNewer = async () => {
  if (historyEnd.value === null) return;
  loading.value = true;
  status.value = "Loading newer history";
  sendHistoryRequest({ after: historyEnd.value, limit: 200 });
};

const jumpToStart = async () => {
  loading.value = true;
  status.value = "Loading earliest history";
  sendHistoryRequest({ anchor: "head", limit: 200 });
};

const jumpToEnd = async () => {
  loading.value = true;
  status.value = "Loading latest history";
  sendHistoryRequest({ anchor: "tail", limit: 200 });
};

const createSession = () => {
  if (!token.value) return;
  status.value = "Creating session";
  sendControlMessage({ type: "new_conversation" });
};

const renameSession = (id: string) => {
  const session = sessions.value.find((item) => item.conversation_id === id);
  const suggested = session?.nickname || session?.summary || "";
  const nextName = window.prompt("Rename session", suggested);
  if (nextName === null) {
    return;
  }
  const trimmed = nextName.trim();
  sendControlMessage({
    type: "rename_conversation",
    conversation_id: id,
    nickname: trimmed ? trimmed : null,
  });
  requestSessions();
};

const deleteSession = (id: string) => {
  const session = sessions.value.find((item) => item.conversation_id === id);
  const label = session?.nickname || session?.summary || id.slice(0, 8);
  const confirmed = window.confirm(
    `Hide session "${label}"? This keeps the files on disk.`,
  );
  if (!confirmed) {
    return;
  }
  sendControlMessage({ type: "delete_conversation", conversation_id: id });
  if (activeId.value === id) {
    activateSession(null);
  }
  requestSessions();
};

const sendMessage = () => {
  const targetId = runtimeId.value;
  if (!token.value || !targetId || !composerText.value.trim()) return;
  const ws = wsRef.value;
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    error.value = "Stream not connected";
    status.value = "Error";
    return;
  }
  const text = composerText.value.trim();
  composerText.value = "";
  items.value = [
    ...items.value,
    {
      id: `local-${Date.now()}`,
      kind: "user",
      subkind: "message",
      title: "You",
      text,
    },
  ];
  try {
    sending.value = true;
    ws.send(
      JSON.stringify({
        type: "send_user_message",
        items: [{ type: "text", data: { text } }],
      }),
    );
  } catch (err) {
    error.value = (err as Error).message;
  } finally {
    sending.value = false;
  }
};

const updateSessionFromEvent = (msgType?: string, payload?: any) => {
  if (!activeId.value) return;
  const now = new Date().toISOString();
  const snippet =
    msgType === "user_message"
      ? String(payload?.message || "").trim().slice(0, 140)
      : undefined;
  sessions.value = sessions.value.map((session) => {
    if (session.conversation_id !== activeId.value) {
      return session;
    }
    return {
      ...session,
      updated_at: now,
      last_user_snippet: snippet ?? session.last_user_snippet,
    };
  });
};

const extractReasoningText = (payload: any) => {
  const value = payload?.text ?? payload?.delta ?? payload?.message ?? "";
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

const appendReasoningDelta = (
  delta: string,
  index: number,
  key: string | null,
) => {
  if (!delta) return;
  const next = [...items.value];
  let targetIndex = -1;
  if (reasoningIdRef.value) {
    targetIndex = next.findIndex((item) => item.id === reasoningIdRef.value);
  }
  if (targetIndex === -1) {
    for (let i = next.length - 1; i >= 0; i -= 1) {
      const candidate = next[i];
      if (candidate.kind === "assistant" && candidate.subkind === "reasoning") {
        targetIndex = i;
        break;
      }
      if (candidate.kind === "assistant" && candidate.subkind === "message") {
        break;
      }
    }
  }
  if (targetIndex !== -1) {
    const current = next[targetIndex];
    next[targetIndex] = {
      ...current,
      text: `${current.text ?? ""}${delta}`,
      eventKey: current.eventKey ?? (key ?? undefined),
      origin: current.origin ?? "event",
    };
    reasoningIdRef.value = next[targetIndex].id;
    items.value = next;
    return;
  }
  const id = `reasoning-${index}-${Math.random().toString(36).slice(2, 6)}`;
  reasoningIdRef.value = id;
  items.value = [
    ...next,
    {
      id,
      kind: "assistant",
      subkind: "reasoning",
      title: "Reasoning",
      text: delta,
      index,
      eventKey: key ?? undefined,
      origin: "event",
    },
  ];
};

const appendHistoryLines = (lines: HistoryLine[]) => {
  const parsed = parseHistoryItems(lines).map((item) => ({
    ...item,
    origin: "event",
  }));
  if (parsed.length) {
    items.value = [...items.value, ...parsed];
  }
};

const handleSnapshot = (message: {
  rollout: HistoryLine[];
  start_index?: number | null;
  end_index?: number | null;
  truncated?: boolean;
}) => {
  const hasRollout = Boolean(message.rollout && message.rollout.length);
  const endIndex = message.end_index;
  reasoningIdRef.value = null;
  resetHistoryTracking();
  const rawLines = message.rollout || [];
  const skipAgentMessages = hasAssistantResponseItem(rawLines);
  const freshLines = registerHistoryLines(rawLines, skipAgentMessages);
  const parsed = parseHistoryItems(freshLines).map((item) => ({
    ...item,
    origin: "history",
  }));
  items.value = parsed;
  historyPrimed.value = hasRollout;
  historyStart.value = message.start_index ?? null;
  historyEnd.value = message.end_index ?? null;
  lastIndexRef.value = endIndex ?? null;
  hasMoreBefore.value = Boolean(message.truncated);
  hasMoreAfter.value = false;
  historyMode.value = "tail";
  if (!hasRollout && lazyHistory.value) {
    loading.value = true;
    status.value = "Loading recent history";
    void primeHistory();
  } else {
    loading.value = false;
    status.value = "Live";
  }
  if (
    endIndex !== undefined &&
    endIndex !== null &&
    (liveIndexRef.value === null || liveIndexRef.value <= endIndex)
  ) {
    liveIndexRef.value = endIndex + 1;
  }
};

const handleEvent = (message: { event: any }) => {
  const event = message.event;
  if (!event) return;
  const key = eventKeyFromPayload(event as Record<string, unknown>);
  if (key && seenEventRef.value.has(key)) {
    return;
  }
  if (key) {
    seenEventRef.value.add(key);
  }
  const msgType = event?.msg?.type;
  if (msgType && typeof msgType === "string") {
    updateActivityFromEvent(msgType, event.msg);
  }
  if (msgType === "user_message" || msgType === "agent_message") {
    updateSessionFromEvent(msgType, event.msg);
    reasoningIdRef.value = null;
  }
  const nextIndex =
    liveIndexRef.value ??
    (historyEnd.value === null ? 0 : historyEnd.value + 1);
  const assignedIndex = Math.max(0, Math.trunc(nextIndex));
  liveIndexRef.value = assignedIndex + 1;
  if (msgType === "agent_reasoning" || msgType === "agent_reasoning_delta") {
    const delta = extractReasoningText(event.msg);
    if (delta) {
      appendReasoningDelta(delta, assignedIndex, key);
    }
    return;
  }
  appendHistoryLines([
    {
      index: assignedIndex,
      type: "event",
      payload: event,
    } as HistoryLine,
  ]);
};

watch(
  token,
  (value, _prev, onCleanup) => {
    if (!value) {
      sessions.value = [];
      sessionsLoading.value = false;
      historyActiveTokenRef.value = null;
      knownSessionIdsRef.value = new Set();
      resetActivity();
      activateSession(null);
      return;
    }
    historyWsModeRef.value = "stream";
    historyWsDisabledRef.value = true;
    historyInitialSessionRef.value = null;
    historyActiveTokenRef.value = value;
    const lastSessionId = localStorage.getItem("codeGatewayLastConversation");

    const cachedSessions = readSessionsCache(value);
    if (cachedSessions?.length) {
      const active = activeId.value;
      const visible = cachedSessions.filter(
        (session) => !isHiddenSession(session, active),
      );
      sessions.value = visible;
      knownSessionIdsRef.value = new Set(
        visible.map((session) => session.conversation_id),
      );
      sessionsPulse.value += 1;
      if (!activeId.value) {
        const preferred =
          (lastSessionId &&
            visible.some(
              (session) => session.conversation_id === lastSessionId,
            ) &&
            lastSessionId) ||
          visible[0]?.conversation_id ||
          null;
        activateSession(preferred);
      }
    } else if (lastSessionId && !activeId.value) {
      activateSession(lastSessionId);
    }

    pendingSessionsRef.value = true;
    sessionsLoading.value = true;
    status.value = activeId.value ? "Resuming session" : "Loading sessions";
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    const host = window.location.host;
    const url = `${protocol}://${host}/ws/control?token=${encodeURIComponent(
      value,
    )}`;
    let closed = false;
    let retries = 0;

    const clearRetry = () => {
      if (controlRetryRef.value !== null) {
        window.clearTimeout(controlRetryRef.value);
        controlRetryRef.value = null;
      }
    };

    const connect = () => {
      clearRetry();
      const ws = new WebSocket(url);
      controlWsRef.value = ws;
      ws.onopen = () => {
        requestSessions();
        if (pendingControlMessages.value.length) {
          const queued = pendingControlMessages.value;
          pendingControlMessages.value = [];
          queued.forEach((payload) => {
            try {
              ws.send(JSON.stringify(payload));
            } catch (err) {
              error.value = (err as Error).message;
              status.value = "Error";
            }
          });
        }
      };
      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data as string);
          if (message.type === "conversations_list") {
            applySessionsList(message.items || []);
          } else if (message.type === "conversation_created") {
            if (message.conversation_id) {
              runtimeId.value = message.conversation_id;
              activateSession(message.conversation_id);
              requestSessions();
            }
            status.value = "Ready";
          } else if (message.type === "conversation_resumed") {
            if (
              message.requested_id &&
              message.requested_id !== activeId.value
            ) {
              return;
            }
            if (
              lastRequestedSessionRef.value &&
              message.requested_id &&
              message.requested_id !== lastRequestedSessionRef.value
            ) {
              return;
            }
            pendingResumeRef.value = null;
            if (message.conversation_id) {
              runtimeId.value = message.conversation_id;
            }
            error.value = null;
            status.value = "Streaming";
          } else if (message.type === "error") {
            const errorMessage = message.message || "Websocket error";
            error.value = errorMessage;
            status.value = "Error";
            if (errorMessage.includes("conversation not found")) {
              activateSession(null);
              requestSessions();
            }
            if (pendingResumeRef.value) {
              pendingResumeRef.value = null;
              loading.value = false;
              sessionLoading.value = false;
            }
          }
        } catch (err) {
          error.value = (err as Error).message;
        }
      };
      ws.onclose = () => {
        controlWsRef.value = null;
        if (closed) return;
        retries += 1;
        const delay = Math.min(10000, 500 * 2 ** retries);
        controlRetryRef.value = window.setTimeout(connect, delay);
      };
      ws.onerror = () => {
        ws.close();
      };
    };

    connect();
    onCleanup(() => {
      closed = true;
      clearRetry();
      sessionsLoading.value = false;
      controlWsRef.value?.close();
      controlWsRef.value = null;
    });
  },
  { immediate: true },
);

watch(activeId, (value) => {
  if (value) {
    const seq = sessionLoadSeq.value + 1;
    sessionLoadSeq.value = seq;
    loadSession(value, seq);
  }
});

type StreamTarget = {
  token: string;
  id: string;
  rollout: string | null;
};

const streamTarget = computed<StreamTarget | null>(() => {
  const tokenValue = token.value;
  if (!tokenValue) return null;
  if (pendingResumeRef.value) {
    return null;
  }
  const runtime = runtimeId.value;
  const active = activeId.value;
  if (runtime) {
    return {
      token: tokenValue,
      id: runtime,
      rollout: active && runtime !== active ? active : null,
    };
  }
  if (sessionLoading.value || !active) {
    return null;
  }
  return {
    token: tokenValue,
    id: active,
    rollout: null,
  };
});

const historyTarget = computed(() => {
  const tokenValue = token.value;
  const active = activeId.value;
  if (!tokenValue || !active) return null;
  return { token: tokenValue, id: active };
});

const historyKey = computed(() => {
  const target = historyTarget.value;
  if (!target) return null;
  if (historyActiveTokenRef.value !== target.token) {
    return null;
  }
  if (historyWsDisabledRef.value) {
    return null;
  }
  if (historyWsModeRef.value !== "initial") {
    return null;
  }
  if (
    historyInitialSessionRef.value &&
    target.id !== historyInitialSessionRef.value
  ) {
    return null;
  }
  return `${target.token}::${target.id}`;
});

watch(
  historyKey,
  (key) => {
    if (key) {
      lazyHistoryReady.value = true;
      schedulePrimeHistory(true);
    }
  },
  { immediate: true },
);

const streamKey = computed(() => {
  const target = streamTarget.value;
  if (!target) return null;
  return `${target.token}::${target.id}::${target.rollout ?? ""}::${
    lazyHistory.value ? "lazy" : "full"
  }`;
});


watch(
  streamKey,
  (_key, _prev, onCleanup) => {
    const target = streamTarget.value;
    if (!target) {
      return;
    }
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    const host = window.location.host;
    const params = new URLSearchParams();
    params.set("token", target.token);
    params.set("snapshot_limit", "0");
    if (target.rollout) {
      params.set("rollout", target.rollout);
    }
    const url = `${protocol}://${host}/ws/${target.id}?${params.toString()}`;
    let closed = false;
    let retries = 0;

    const clearRetry = () => {
      if (wsRetryRef.value !== null) {
        window.clearTimeout(wsRetryRef.value);
        wsRetryRef.value = null;
      }
    };

    const connect = () => {
      clearRetry();
      const ws = new WebSocket(url);
      wsRef.value = ws;
      const loadingId = activeId.value;
      if (loadingId) {
        status.value = "Syncing";
      }
      ws.onopen = () => {
        if (sessionLoading.value) {
          status.value = "Syncing";
        }
        flushPendingHistoryRequests();
        if (!lazyHistory.value) {
          sendHistoryRequest({ anchor: "tail", limit: HISTORY_TARGET_LIMIT });
        } else if (
          items.value.length === 0 &&
          (!historyWsRef.value || historyWsRef.value.readyState !== WebSocket.OPEN)
        ) {
          primeHistory();
        }
      };
      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data as string);
          if (message.type === "snapshot") {
            if (!historyPrimed.value) {
              handleSnapshot(message);
              if (loadingId === activeId.value) {
                sessionLoading.value = false;
              }
            }
          } else if (message.type === "history_chunk") {
            if (
              !historyWsRef.value ||
              historyWsRef.value.readyState !== WebSocket.OPEN
            ) {
              handleHistoryChunk(message);
              historyPrimed.value = true;
            }
          } else if (message.type === "event") {
            handleEvent(message);
          } else if (message.type === "error") {
            const errorMessage = message.message || "Websocket error";
            error.value = errorMessage;
            primeHistoryInFlightRef.value = false;
            cancelPrimeFill();
            if (errorMessage.includes("conversation not found")) {
              activateSession(null);
              requestSessions();
              loading.value = false;
              sessionLoading.value = false;
            }
          }
        } catch (err) {
          error.value = (err as Error).message;
        }
      };
      ws.onclose = () => {
        wsRef.value = null;
        if (closed) return;
        retries += 1;
        const delay = Math.min(10000, 500 * 2 ** retries);
        wsRetryRef.value = window.setTimeout(connect, delay);
      };
      ws.onerror = () => {
        ws.close();
      };
    };

    connect();
    onCleanup(() => {
      closed = true;
      clearRetry();
      cancelPrimeHistory();
      cancelPrimeFill();
      primeHistoryInFlightRef.value = false;
      wsRef.value?.close();
      wsRef.value = null;
    });
  },
  { immediate: true },
);

watch(
  historyKey,
  (key, _prev, onCleanup) => {
    if (!key) {
      closeHistorySocket();
      return;
    }
    const target = historyTarget.value;
    if (!target) return;
    historyWsDisabledRef.value = false;
    historyWsAttemptRef.value = 0;
    const historyTargetId = target.id;
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    const host = window.location.host;
    const params = new URLSearchParams();
    params.set("token", target.token);
    params.set("snapshot_limit", "0");
    const url = `${protocol}://${host}/ws/history/${target.id}?${params.toString()}`;
    historyWsUrlRef.value = url;
    let closed = false;
    let retries = 0;

    const clearRetry = () => {
      if (historyRetryRef.value !== null) {
        window.clearTimeout(historyRetryRef.value);
        historyRetryRef.value = null;
      }
    };

    const connect = () => {
      if (historyWsDisabledRef.value) {
        return;
      }
      clearRetry();
      if (historyConnectTimerRef.value !== null) {
        window.clearTimeout(historyConnectTimerRef.value);
      }
      historyConnectTimerRef.value = window.setTimeout(() => {
        historyConnectTimerRef.value = null;
        if (historyWsDisabledRef.value) {
          return;
        }
        if (historyTargetId !== activeId.value) {
          return;
        }
        if (historyWsBackoffRef.value > 0) {
          const delay = Math.min(5000, 200 * 2 ** historyWsBackoffRef.value);
          historyConnectTimerRef.value = window.setTimeout(() => {
            historyConnectTimerRef.value = null;
            connect();
          }, delay);
          return;
        }
        const ws = new WebSocket(url);
        historyWsRef.value = ws;
        let opened = false;
        historyHandshakeRef.value = {
          id: historyTargetId,
          token: target.token,
          at: Date.now(),
        };
        ws.onopen = () => {
          opened = true;
          historyHandshakeRef.value = null;
          historyWsAttemptRef.value = 0;
          historyWsBackoffRef.value = 0;
          flushPendingHistoryRequests();
          if (!lazyHistory.value) {
            sendHistoryRequest({ anchor: "tail", limit: HISTORY_TARGET_LIMIT });
          } else if (items.value.length === 0) {
            primeHistory();
          }
        };
        ws.onclose = () => {
          historyWsRef.value = null;
          if (!opened) {
            handleHistoryHandshakeFailure();
          }
          const shouldRetry = !closed && historyWsUrlRef.value === url;
          if (!shouldRetry) return;
          if (!opened) {
            historyWsAttemptRef.value += 1;
            if (historyWsAttemptRef.value >= 2) {
              historyWsDisabledRef.value = true;
              flushPendingHistoryRequests();
              return;
            }
          }
          historyWsBackoffRef.value = Math.min(
            6,
            historyWsBackoffRef.value + 1,
          );
          retries += 1;
          const maxDelay = historyPrimed.value ? 10000 : 2000;
          const baseDelay = historyPrimed.value ? 500 : 200;
          const delay = Math.min(maxDelay, baseDelay * 2 ** retries);
          historyRetryRef.value = window.setTimeout(connect, delay);
        };
        ws.onmessage = (event) => {
          try {
            const message = JSON.parse(event.data as string);
            if (message.type === "snapshot") {
              handleSnapshot(message);
              if (historyTargetId === activeId.value) {
                sessionLoading.value = false;
              }
            } else if (message.type === "history_chunk") {
              handleHistoryChunk(message);
              historyPrimed.value = true;
            } else if (message.type === "error") {
              error.value = message.message || "Websocket error";
              primeHistoryInFlightRef.value = false;
              cancelPrimeFill();
            }
          } catch (err) {
            error.value = (err as Error).message;
          }
        };
        ws.onerror = () => {
          ws.close();
        };
      }, 600);
    };

    connect();
    onCleanup(() => {
      closed = true;
      clearRetry();
      if (historyConnectTimerRef.value !== null) {
        window.clearTimeout(historyConnectTimerRef.value);
        historyConnectTimerRef.value = null;
      }
      cancelPrimeHistory();
      cancelPrimeFill();
      primeHistoryInFlightRef.value = false;
      pendingHistoryRequests.value = [];
      historyWsUrlRef.value = null;
      const ws = historyWsRef.value;
      historyWsRef.value = null;
      if (ws) {
        if (ws.readyState === WebSocket.CONNECTING) {
          ws.onopen = () => ws.close();
        } else {
          ws.close();
        }
      }
    });
  },
  { immediate: true },
);

const workbenchClass = computed(() => [
  styles.workbench,
  filtersCollapsed.value ? styles.workbenchCollapsed : "",
].join(" "));
const rightDockClass = computed(() => [
  styles.rightDock,
  filtersCollapsed.value ? styles.rightDockCollapsed : "",
].join(" "));
const hasMore = computed(() =>
  historyMode.value === "tail" ? hasMoreBefore.value : hasMoreAfter.value,
);
const loadMore = async () => {
  if (historyMode.value === "tail") {
    await loadOlder();
  } else {
    await loadNewer();
  }
};
const loadMorePlacement = computed(() =>
  historyMode.value === "tail" ? "prepend" : "append",
);

const toggleKind = (kind: TimelineKind) => {
  filters.value = {
    ...filters.value,
    kinds: { ...filters.value.kinds, [kind]: !filters.value.kinds[kind] },
  };
};

const toggleSubkind = (subkind: string) => {
  filters.value = {
    ...filters.value,
    subkinds: {
      ...filters.value.subkinds,
      [subkind]: !filters.value.subkinds[subkind],
    },
  };
};

const handleTokenInput = (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  setToken(target?.value ?? "");
};
</script>

<template>
  <div :class="styles.app">
    <header :class="styles.titleBar">
      <div :class="styles.brand">Every Code</div>
      <div :class="styles.status">{{ status }}</div>
      <div :class="styles.toolbar">
        <input
          :class="styles.tokenInput"
          :value="token"
          placeholder="Gateway token"
          @input="handleTokenInput"
        />
        <span :class="styles.tokenLabel">{{ label }}</span>
        <button :class="styles.ghostButton" type="button" @click="toggle">
          {{ theme === "dark" ? "Light" : "Dark" }}
        </button>
      </div>
    </header>

    <main :class="workbenchClass">
      <div :class="styles.leftDock">
        <SessionsPanel
          :sessions="sessions"
          :active-id="activeId"
          :search="search"
          :loading="sessionsLoading"
          :pulse-key="sessionsPulse"
          @search-change="search = $event"
          @select="activateSession"
          @new="createSession"
          @rename="renameSession"
          @delete="deleteSession"
        />
      </div>

      <section :class="styles.centerColumn">
        <div v-if="error" :class="styles.errorBanner">{{ error }}</div>
        <div v-if="devBanner" :class="styles.devBanner">
          <span :class="styles.devBadge">Dev</span>
          <span>Session: {{ devBanner.sessionId }}</span>
          <span>Source: {{ devBanner.sessionSource }}</span>
          <span>Last index: {{ devBanner.lastIndex }}</span>
          <span>Stream: {{ devBanner.streamOpen ? "open" : "closed" }}</span>
          <span>History: {{ devBanner.historyOpen ? "open" : "closed" }}</span>
        </div>
        <div :class="styles.mobileBar">
          <button
            :class="styles.mobileSessionButton"
            type="button"
            @click="mobilePanel = 'sessions'"
          >
            <span :class="styles.mobileSessionLabel">{{ mobileSessionLabel }}</span>
            <span :class="styles.mobileCaret">v</span>
          </button>
          <button
            :class="styles.mobileFiltersButton"
            type="button"
            @click="mobilePanel = 'filters'"
          >
            Filters
            <span
              v-if="mobileFiltersCount > 0"
              :class="styles.mobileFiltersBadge"
            >
              {{ mobileFiltersCount }}
            </span>
          </button>
        </div>
        <TimelinePanel
          :items="filteredItems"
          :loading="timelineLoading"
          :has-more="hasMore"
          :on-load-more="loadMore"
          :load-more-placement="loadMorePlacement"
          :reset-key="activeId"
          :on-jump-to-start="jumpToStart"
          :on-jump-to-end="jumpToEnd"
          :status-text="timelineLoading ? status : undefined"
          :on-user-scroll="lazyHistory && !historyPrimed ? primeHistory : undefined"
        />
    <ComposerDock
      :value="composerText"
      :disabled="!runtimeId || sending || sessionLoading"
      :status="composerStatus"
      @change="composerText = $event"
      @send="sendMessage"
    />
      </section>

      <div :class="rightDockClass">
        <button
          v-if="filtersCollapsed"
          :class="styles.dockTab"
          type="button"
          @click="filtersCollapsed = false"
        >
          Filters
        </button>
        <FiltersPanel
          v-else
          :filters="filters"
          :counts="counts"
          :sub-counts="subCounts"
          :collapsible="true"
          @toggle-kind="toggleKind"
          @toggle-subkind="toggleSubkind"
          @collapse="filtersCollapsed = true"
        />
      </div>
    </main>

    <div
      v-if="mobilePanel !== 'none'"
      :class="styles.mobileOverlay"
      @click.self="mobilePanel = 'none'"
    >
      <div :class="styles.mobileSheet">
        <div :class="styles.mobileHeader">
          <div :class="styles.mobileTabs">
            <button
              :class="mobilePanel === 'sessions' ? styles.mobileActive : ''"
              type="button"
              @click="mobilePanel = 'sessions'"
            >
              Sessions
            </button>
            <button
              :class="mobilePanel === 'filters' ? styles.mobileActive : ''"
              type="button"
              @click="mobilePanel = 'filters'"
            >
              Filters
            </button>
          </div>
          <button
            :class="styles.ghostButton"
            type="button"
            @click="mobilePanel = 'none'"
          >
            Close
          </button>
        </div>
        <div :class="styles.mobileBody">
          <SessionsPanel
            v-if="mobilePanel === 'sessions'"
            :class-name="styles.mobilePanel"
            :sessions="sessions"
            :active-id="activeId"
            :search="search"
            :loading="sessionsLoading"
            :pulse-key="sessionsPulse"
            @search-change="search = $event"
            @select="(id) => { activateSession(id); mobilePanel = 'none'; }"
            @new="createSession"
            @rename="renameSession"
            @delete="deleteSession"
          />
          <FiltersPanel
            v-else
            :class-name="styles.mobilePanel"
            :filters="filters"
            :counts="counts"
            :sub-counts="subCounts"
            @toggle-kind="toggleKind"
            @toggle-subkind="toggleSubkind"
          />
        </div>
      </div>
    </div>
  </div>
</template>
