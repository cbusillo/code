<script setup lang="ts">
import { computed } from "vue";
import type { SessionSummary } from "../../api/types";
import styles from "./SessionsPanel.module.css";

type Props = {
  sessions: SessionSummary[];
  activeId: string | null;
  search: string;
  loading: boolean;
  pulseKey: number;
  className?: string;
};

const props = defineProps<Props>();
const emit = defineEmits<{
  (event: "search-change", value: string): void;
  (event: "select", id: string): void;
  (event: "new"): void;
}>();

const formatSubtitle = (session: SessionSummary) => {
  if (session.last_user_snippet) {
    return session.last_user_snippet.trim();
  }
  if (session.cwd) {
    return session.cwd;
  }
  return session.updated_at || "";
};

const startOfDay = (value: Date) =>
  new Date(value.getFullYear(), value.getMonth(), value.getDate());

const groupLabel = (value: Date) => {
  const now = new Date();
  const diffMs = startOfDay(now).getTime() - startOfDay(value).getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));
  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  if (diffDays <= 7) return "This week";
  return value.toLocaleDateString();
};

const groupSessions = (sessions: SessionSummary[]) => {
  const groups = new Map<string, SessionSummary[]>();
  sessions.forEach((session) => {
    const stamp = session.updated_at || session.created_at;
    const date = stamp ? new Date(stamp) : new Date(0);
    const label = groupLabel(date);
    if (!groups.has(label)) {
      groups.set(label, []);
    }
    groups.get(label)?.push(session);
  });
  return Array.from(groups.entries()).map(([label, items]) => ({
    label,
    items,
  }));
};

const filtered = computed(() => {
  const query = props.search.trim().toLowerCase();
  if (!query) {
    return props.sessions;
  }
  return props.sessions.filter((session) => {
    const haystack = [
      session.conversation_id,
      session.nickname,
      session.summary,
      session.last_user_snippet,
      session.cwd,
      session.git_branch,
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(query);
  });
});

const grouped = computed(() => groupSessions(filtered.value));

const handleSearch = (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  emit("search-change", target?.value ?? "");
};

const itemClass = (id: string) => [
  styles.item,
  props.activeId === id ? styles.active : "",
];
</script>

<template>
  <aside :class="[styles.panel, props.className || '']">
    <header :class="styles.header">
      <div>
        <div :class="styles.titleRow">
          <div :class="styles.title">Sessions</div>
          <span
            :key="props.pulseKey"
            :class="styles.pulseDot"
            aria-hidden="true"
          />
        </div>
        <div :class="styles.subtitle">{{ filtered.length }} total</div>
      </div>
      <div :class="styles.actions">
        <button :class="styles.ghost" type="button" @click="emit('new')">
          New
        </button>
      </div>
    </header>
    <div :class="styles.searchRow">
      <input
        :class="styles.search"
        :value="props.search"
        placeholder="Search sessions"
        @input="handleSearch"
      />
    </div>
    <div :class="styles.list">
      <div v-for="group in grouped" :key="group.label" :class="styles.group">
        <div :class="styles.groupHeader">
          <span>{{ group.label }}</span>
          <span :class="styles.groupLine" />
        </div>
        <button
          v-for="session in group.items"
          :key="session.conversation_id"
          :class="itemClass(session.conversation_id)"
          type="button"
          @click="emit('select', session.conversation_id)"
        >
          <div :class="styles.itemTitle">
            {{ session.nickname || session.summary || "Untitled" }}
          </div>
          <div :class="styles.itemSubtitle">{{ formatSubtitle(session) }}</div>
          <div :class="styles.itemMeta">
            <span>{{ session.conversation_id.slice(0, 8) }}</span>
            <span v-if="session.git_branch">{{ session.git_branch }}</span>
          </div>
        </button>
      </div>
      <div v-if="props.loading" :class="styles.loading">Loading…</div>
    </div>
  </aside>
</template>
