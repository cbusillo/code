const STORAGE_KEYS = {
  wsUrl: "every-code-web-ui.ws-url",
  cwd: "every-code-web-ui.cwd",
  model: "every-code-web-ui.model",
};

const state = {
  socket: null,
  initialized: false,
  nextId: 1,
  pending: new Map(),
  threads: [],
  selectedThread: null,
  hydratedTurns: [],
  activeThreadId: null,
  subscriptions: new Map(),
  liveEvents: [],
};

const elements = {
  connectForm: document.querySelector("#connect-form"),
  disconnectButton: document.querySelector("#disconnect"),
  refreshButton: document.querySelector("#refresh-sessions"),
  wsUrl: document.querySelector("#ws-url"),
  connectionPill: document.querySelector("#connection-pill"),
  statusText: document.querySelector("#status-text"),
  sessionList: document.querySelector("#session-list"),
  sessionCount: document.querySelector("#session-count"),
  threadTitle: document.querySelector("#thread-title"),
  threadMeta: document.querySelector("#thread-meta"),
  threadTurns: document.querySelector("#thread-turns"),
  resumeButton: document.querySelector("#resume-thread"),
  reloadButton: document.querySelector("#reload-thread"),
  startForm: document.querySelector("#start-form"),
  startCwd: document.querySelector("#start-cwd"),
  startModel: document.querySelector("#start-model"),
  startPrompt: document.querySelector("#start-prompt"),
  promptForm: document.querySelector("#prompt-form"),
  turnPrompt: document.querySelector("#turn-prompt"),
  sendTurn: document.querySelector("#send-turn"),
  eventLog: document.querySelector("#event-log"),
  sessionCardTemplate: document.querySelector("#session-card-template"),
};

hydrateFormState();
bindEvents();
renderConnectionState("idle", "Waiting for a local app-server.");
renderSessions();
renderTranscript();
renderEventLog();

function bindEvents() {
  elements.connectForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await connect();
  });

  elements.disconnectButton.addEventListener("click", () => {
    disconnect("Disconnected.");
  });

  elements.refreshButton.addEventListener("click", async () => {
    await refreshSessions();
  });

  elements.resumeButton.addEventListener("click", async () => {
    await hydrateSelectedThread();
  });

  elements.reloadButton.addEventListener("click", async () => {
    if (state.selectedThread) {
      await hydrateSelectedThread();
    } else {
      await refreshSessions();
    }
  });

  elements.startForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await startThread();
  });

  elements.promptForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await sendPrompt();
  });
}

function hydrateFormState() {
  const savedUrl = window.localStorage.getItem(STORAGE_KEYS.wsUrl);
  const savedCwd = window.localStorage.getItem(STORAGE_KEYS.cwd);
  const savedModel = window.localStorage.getItem(STORAGE_KEYS.model);

  if (savedUrl) {
    elements.wsUrl.value = savedUrl;
  }
  if (savedCwd) {
    elements.startCwd.value = savedCwd;
  }
  if (savedModel) {
    elements.startModel.value = savedModel;
  }
}

function persistFormState() {
  window.localStorage.setItem(STORAGE_KEYS.wsUrl, elements.wsUrl.value.trim());
  window.localStorage.setItem(STORAGE_KEYS.cwd, elements.startCwd.value.trim());
  window.localStorage.setItem(
    STORAGE_KEYS.model,
    elements.startModel.value.trim(),
  );
}

async function connect() {
  const url = elements.wsUrl.value.trim();
  if (!url) {
    renderConnectionState("error", "Enter a WebSocket URL first.");
    return;
  }

  disconnect();
  persistFormState();
  renderConnectionState("idle", `Connecting to ${url}...`);

  await new Promise((resolve, reject) => {
    const socket = new WebSocket(url);

    socket.addEventListener("open", async () => {
      state.socket = socket;
      try {
        await rpc("initialize", {
          clientInfo: {
            name: "every-code-web-ui-spike",
            title: "Shared Session Web UI",
            version: "0.2.0",
          },
          capabilities: {
            experimentalApi: true,
          },
        });
        state.initialized = true;
        notify("initialized", {});
        renderConnectionState("connected", `Connected to ${url}.`);
        await refreshSessions();
        resolve();
      } catch (error) {
        reject(error);
      }
    });

    socket.addEventListener("message", (event) => {
      handleSocketMessage(event.data);
    });

    socket.addEventListener("close", () => {
      state.socket = null;
      state.initialized = false;
      rejectPending(new Error("WebSocket disconnected."));
      renderConnectionState("idle", "Socket closed.");
    });

    socket.addEventListener("error", () => {
      renderConnectionState("error", `Failed to connect to ${url}.`);
    });
  }).catch((error) => {
    handleError(error);
  });
}

function disconnect(message = "Offline") {
  if (state.socket) {
    state.socket.close();
  }
  state.socket = null;
  state.initialized = false;
  state.pending.clear();
  state.activeThreadId = null;
  renderConnectionState("idle", message);
}

function rejectPending(error) {
  for (const pending of state.pending.values()) {
    pending.reject(error);
  }
  state.pending.clear();
}

function rpc(method, params = {}) {
  if (!state.socket || state.socket.readyState !== WebSocket.OPEN) {
    throw new Error("Connect to the app-server first.");
  }

  const id = state.nextId;
  state.nextId += 1;
  state.socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));

  return new Promise((resolve, reject) => {
    state.pending.set(id, { resolve, reject });
  });
}

function notify(method, params = {}) {
  if (!state.socket || state.socket.readyState !== WebSocket.OPEN) {
    return;
  }
  state.socket.send(JSON.stringify({ jsonrpc: "2.0", method, params }));
}

function handleSocketMessage(raw) {
  const message = JSON.parse(raw);

  if (Object.prototype.hasOwnProperty.call(message, "id")) {
    const pending = state.pending.get(message.id);
    if (!pending) {
      return;
    }
    state.pending.delete(message.id);

    if (message.error) {
      pending.reject(new Error(message.error.message || "RPC error"));
    } else {
      pending.resolve(message.result || {});
    }
    return;
  }

  if (message.method) {
    handleNotification(message.method, message.params || {});
  }
}

function handleNotification(method, params) {
  const threadId =
    params.threadId || params.conversationId || state.activeThreadId;
  applyNotificationToThreadState(method, params);
  const summary = summarizeNotification(method, params);
  state.liveEvents.unshift({
    method,
    summary,
    conversationId: threadId,
    at: new Date(),
  });
  state.liveEvents = state.liveEvents.slice(0, 60);
  renderEventLog();
}

function applyNotificationToThreadState(method, params) {
  if (
    !params.threadId ||
    params.threadId !== state.activeThreadId ||
    !params.turn
  ) {
    return;
  }

  if (method !== "turn/started" && method !== "turn/completed") {
    return;
  }

  const nextTurns = [...state.hydratedTurns];
  const existingIndex = nextTurns.findIndex(
    (turn) => turn.id === params.turn.id,
  );
  if (existingIndex >= 0) {
    nextTurns[existingIndex] = {
      ...nextTurns[existingIndex],
      ...params.turn,
    };
  } else {
    nextTurns.push(params.turn);
  }
  state.hydratedTurns = nextTurns;
  renderTranscript();
}

function summarizeNotification(method, params) {
  if (method === "turn/started") {
    return `Turn ${params.turn?.id || "unknown"} started.`;
  }
  if (method === "turn/completed") {
    return `Turn ${params.turn?.id || "unknown"} ${params.turn?.status || "completed"}.`;
  }
  if (method.startsWith("codex/event/")) {
    const kind = method.replace("codex/event/", "");
    return params.msg ? `${kind}: ${JSON.stringify(params.msg)}` : kind;
  }
  return JSON.stringify(params);
}

async function refreshSessions() {
  if (!state.initialized) {
    renderConnectionState("idle", "Connect first, then refresh threads.");
    return;
  }

  try {
    renderConnectionState("connected", "Loading threads...");
    const response = await rpc("thread/list", { limit: 40 });
    state.threads = response.data || [];
    renderSessions();
    renderConnectionState(
      "connected",
      `Loaded ${state.threads.length} threads.`,
    );
  } catch (error) {
    handleError(error);
  }
}

function selectConversation(thread) {
  state.selectedThread = thread;
  state.activeThreadId = null;
  state.hydratedTurns = [];
  renderSessions();
  renderTranscript();
  renderConnectionState(
    "connected",
    `Selected ${thread.id}. Load it to hydrate turn history.`,
  );
}

async function hydrateSelectedThread() {
  if (!state.selectedThread) {
    return;
  }

  try {
    renderConnectionState(
      "connected",
      `Hydrating ${state.selectedThread.id}...`,
    );
    const response = await rpc("thread/read", {
      threadId: state.selectedThread.id,
      includeTurns: true,
    });
    state.activeThreadId = null;
    state.selectedThread = response.thread;
    state.hydratedTurns = response.thread.turns || [];
    renderSessions();
    renderTranscript();
    renderConnectionState(
      "connected",
      `Hydrated ${response.thread.id}. Resume it on the next prompt to stream live updates.`,
    );
  } catch (error) {
    handleError(error);
  }
}

async function resumeSelectedThreadIfNeeded() {
  if (!state.selectedThread) {
    return null;
  }

  if (state.activeThreadId === state.selectedThread.id) {
    await ensureListener(state.selectedThread.id);
    return state.selectedThread;
  }

  renderConnectionState(
    "connected",
    `Resuming ${state.selectedThread.id} for live prompts...`,
  );
  const response = await rpc("thread/resume", {
    threadId: state.selectedThread.id,
  });
  state.activeThreadId = response.thread.id;
  state.selectedThread = response.thread;
  state.hydratedTurns = response.thread.turns || state.hydratedTurns;
  await ensureListener(response.thread.id);
  renderSessions();
  renderTranscript();
  return response.thread;
}

async function ensureListener(threadId) {
  if (state.subscriptions.has(threadId)) {
    return;
  }

  const response = await rpc("addConversationListener", {
    conversationId: threadId,
    experimentalRawEvents: false,
  });
  state.subscriptions.set(threadId, response.subscriptionId);
}

async function startThread() {
  if (!state.initialized) {
    renderConnectionState("error", "Connect before starting a thread.");
    return;
  }

  persistFormState();
  const cwd = elements.startCwd.value.trim();
  const model = elements.startModel.value.trim();
  const prompt = elements.startPrompt.value.trim();
  const params = {};
  if (cwd) {
    params.cwd = cwd;
  }
  if (model) {
    params.model = model;
  }

  try {
    renderConnectionState("connected", "Starting a new thread...");
    const response = await rpc("thread/start", params);
    state.activeThreadId = response.thread.id;
    state.selectedThread = response.thread;
    state.hydratedTurns = [];
    await ensureListener(response.thread.id);

    if (prompt) {
      await rpc("turn/start", {
        threadId: response.thread.id,
        input: [
          {
            type: "text",
            text: prompt,
            textElements: [],
          },
        ],
      });
    }

    elements.startPrompt.value = "";
    renderSessions();
    renderTranscript();
    renderConnectionState("connected", `Started thread ${response.thread.id}.`);
  } catch (error) {
    handleError(error);
  }
}

async function sendPrompt() {
  if (!state.selectedThread) {
    renderConnectionState("error", "Select or start a thread first.");
    return;
  }

  const prompt = elements.turnPrompt.value.trim();
  if (!prompt) {
    renderConnectionState("error", "Enter a prompt before sending it.");
    return;
  }

  try {
    const thread = await resumeSelectedThreadIfNeeded();
    renderConnectionState("connected", `Sending prompt to ${thread.id}...`);
    await rpc("turn/start", {
      threadId: thread.id,
      input: [
        {
          type: "text",
          text: prompt,
          textElements: [],
        },
      ],
    });
    elements.turnPrompt.value = "";
    renderConnectionState("connected", `Prompt sent to ${thread.id}.`);
  } catch (error) {
    handleError(error);
  }
}

function renderConnectionState(kind, message) {
  elements.connectionPill.className = `pill ${kind}`;
  elements.connectionPill.textContent =
    kind === "connected" ? "Connected" : kind === "error" ? "Error" : "Offline";
  elements.statusText.textContent = message;

  const hasSelection = Boolean(state.selectedThread);
  const isReady = kind === "connected";
  elements.disconnectButton.disabled = !state.socket;
  elements.refreshButton.disabled = !isReady;
  elements.resumeButton.disabled = !isReady || !hasSelection;
  elements.reloadButton.disabled = !isReady;
  elements.turnPrompt.disabled = !isReady || !hasSelection;
  elements.sendTurn.disabled = !isReady || !hasSelection;
}

function renderSessions() {
  elements.sessionCount.textContent = `${state.threads.length} thread${state.threads.length === 1 ? "" : "s"}`;
  elements.sessionList.innerHTML = "";

  if (!state.threads.length) {
    elements.sessionList.className = "session-list empty-state";
    elements.sessionList.textContent =
      "No persisted threads were returned by the current app-server.";
    return;
  }

  elements.sessionList.className = "session-list";
  const fragment = document.createDocumentFragment();
  for (const thread of state.threads) {
    const card =
      elements.sessionCardTemplate.content.firstElementChild.cloneNode(true);
    card.querySelector(".source-badge").textContent = thread.source;
    card.querySelector(".updated-at").textContent = formatTimestamp(
      thread.updated_at || thread.created_at,
    );
    card.querySelector(".preview").textContent =
      thread.preview || "Untitled thread";
    card.querySelector(".cwd").textContent = thread.cwd;
    card.querySelector(".meta").textContent =
      `${thread.model_provider || "default"} • ${thread.cli_version || "current"}`;
    if (state.selectedThread && thread.id === state.selectedThread.id) {
      card.classList.add("active");
    }
    card.addEventListener("click", () => {
      selectConversation(thread);
    });
    fragment.append(card);
  }
  elements.sessionList.append(fragment);
}

function renderTranscript() {
  const thread = state.selectedThread;
  elements.threadTurns.innerHTML = "";

  if (!thread) {
    elements.threadTitle.textContent = "No thread selected";
    elements.threadMeta.textContent =
      "Select a thread from the left, then load it to hydrate its turn history.";
    elements.threadMeta.className = "thread-meta muted";
    elements.threadTurns.className = "thread-turns empty-state";
    elements.threadTurns.textContent =
      "Load a stored thread to hydrate its turn history here.";
    return;
  }

  elements.threadTitle.textContent = thread.preview || thread.id;
  elements.threadMeta.className = "thread-meta";
  elements.threadMeta.innerHTML = "";
  [
    `id: ${thread.id}`,
    `source: ${thread.source}`,
    `provider: ${thread.model_provider || "default"}`,
    `cwd: ${thread.cwd}`,
    `updated: ${formatTimestamp(thread.updated_at || thread.created_at)}`,
  ].forEach((value) => {
    const badge = document.createElement("span");
    badge.className = "muted";
    badge.textContent = value;
    elements.threadMeta.append(badge);
  });

  if (!state.hydratedTurns.length) {
    elements.threadTurns.className = "thread-turns empty-state";
    elements.threadTurns.textContent =
      "This spike hydrates history through thread/read and updates live turn state from typed notifications.";
    return;
  }

  elements.threadTurns.className = "thread-turns";
  const fragment = document.createDocumentFragment();
  for (const turn of state.hydratedTurns) {
    fragment.append(renderHydratedTurn(turn));
  }
  elements.threadTurns.append(fragment);
}

function renderHydratedTurn(turn) {
  const card = document.createElement("article");
  card.className = "item-card";
  const head = document.createElement("div");
  head.className = "item-head";
  const kind = document.createElement("span");
  kind.className = "item-kind";
  kind.textContent = `${turn.id} • ${turn.status || "unknown"}`;
  head.append(kind);
  const body = document.createElement("pre");
  body.className = "item-body";
  body.textContent = JSON.stringify(turn.items || turn, null, 2);
  card.append(head, body);
  return card;
}

function renderEventLog() {
  elements.eventLog.innerHTML = "";
  if (!state.liveEvents.length) {
    elements.eventLog.className = "event-log empty-state";
    elements.eventLog.textContent =
      "Event deltas will appear here once a live turn starts.";
    return;
  }

  elements.eventLog.className = "event-log";
  const fragment = document.createDocumentFragment();
  for (const entry of state.liveEvents) {
    const row = document.createElement("article");
    row.className = "event-entry";
    const head = document.createElement("div");
    head.className = "item-head";
    const method = document.createElement("span");
    method.className = "event-method";
    method.textContent = entry.method;
    const when = document.createElement("span");
    when.className = "muted";
    when.textContent = entry.at.toLocaleTimeString();
    head.append(method, when);
    const body = document.createElement("pre");
    body.className = "event-body";
    body.textContent = entry.conversationId
      ? `[${entry.conversationId}] ${entry.summary}`
      : entry.summary;
    row.append(head, body);
    fragment.append(row);
  }
  elements.eventLog.append(fragment);
}

function formatTimestamp(value) {
  if (!value) {
    return "unknown";
  }
  if (typeof value === "number") {
    const date = new Date(value * 1000);
    if (!Number.isNaN(date.getTime())) {
      return date.toLocaleString();
    }
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

function handleError(error) {
  renderConnectionState("error", error.message || String(error));
}
