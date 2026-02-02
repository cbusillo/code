<script setup lang="ts">
import type { TimelineKind } from "../../api/types";
import styles from "./FiltersPanel.module.css";

type Props = {
  filters: {
    kinds: Record<TimelineKind, boolean>;
    subkinds: Record<string, boolean>;
  };
  counts: Record<TimelineKind, number>;
  subCounts: Record<string, number>;
  collapsible?: boolean;
  className?: string;
};

const props = defineProps<Props>();
const emit = defineEmits<{
  (event: "toggle-kind", kind: TimelineKind): void;
  (event: "toggle-subkind", subkind: string): void;
  (event: "collapse"): void;
}>();

const MESSAGE_KINDS: { kind: TimelineKind; label: string }[] = [
  { kind: "assistant", label: "Assistant" },
  { kind: "user", label: "User" },
  { kind: "system", label: "System" },
  { kind: "review", label: "Review" },
];

const OPS_KINDS: { kind: TimelineKind; label: string }[] = [
  { kind: "exec", label: "Exec" },
  { kind: "diff", label: "Diff" },
];

const TOOL_SUBKINDS = [
  { id: "tool-mcp", label: "MCP" },
  { id: "tool-custom", label: "Custom" },
  { id: "tool-web", label: "Web" },
  { id: "tool-browser", label: "Browser" },
  { id: "tool-call", label: "Call" },
  { id: "tool-output", label: "Output" },
];

const MESSAGE_SUBKINDS = [
  { id: "message", label: "Messages" },
  { id: "reasoning", label: "Reasoning" },
  { id: "image", label: "Images" },
];
</script>

<template>
  <aside :class="[styles.panel, props.className || '']">
    <header :class="styles.header">
      <div>
        <div :class="styles.title">Filters</div>
        <div :class="styles.subtitle">Docked</div>
      </div>
      <button
        v-if="props.collapsible"
        :class="styles.collapseButton"
        type="button"
        @click="emit('collapse')"
      >
        Collapse
      </button>
    </header>
    <div :class="styles.list">
      <div :class="styles.group">Messages</div>
      <label v-for="filter in MESSAGE_KINDS" :key="filter.kind" :class="styles.item">
        <input
          type="checkbox"
          :checked="props.filters.kinds[filter.kind]"
          @change="emit('toggle-kind', filter.kind)"
        />
        <span :class="styles.label">{{ filter.label }}</span>
        <span :class="styles.count">{{ props.counts[filter.kind] || 0 }}</span>
      </label>
      <div :class="styles.subGroup">Message details</div>
      <label v-for="item in MESSAGE_SUBKINDS" :key="item.id" :class="styles.subItem">
        <input
          type="checkbox"
          :checked="props.filters.subkinds[item.id]"
          @change="emit('toggle-subkind', item.id)"
        />
        <span :class="styles.label">{{ item.label }}</span>
        <span :class="styles.count">{{ props.subCounts[item.id] || 0 }}</span>
      </label>
      <div :class="styles.group">Tools</div>
      <label :class="styles.item">
        <input
          type="checkbox"
          :checked="props.filters.kinds.tool"
          @change="emit('toggle-kind', 'tool')"
        />
        <span :class="styles.label">All tools</span>
        <span :class="styles.count">{{ props.counts.tool || 0 }}</span>
      </label>
      <label v-for="item in TOOL_SUBKINDS" :key="item.id" :class="styles.subItem">
        <input
          type="checkbox"
          :checked="props.filters.subkinds[item.id]"
          @change="emit('toggle-subkind', item.id)"
        />
        <span :class="styles.label">{{ item.label }}</span>
        <span :class="styles.count">{{ props.subCounts[item.id] || 0 }}</span>
      </label>
      <div :class="styles.group">Ops</div>
      <label v-for="filter in OPS_KINDS" :key="filter.kind" :class="styles.item">
        <input
          type="checkbox"
          :checked="props.filters.kinds[filter.kind]"
          @change="emit('toggle-kind', filter.kind)"
        />
        <span :class="styles.label">{{ filter.label }}</span>
        <span :class="styles.count">{{ props.counts[filter.kind] || 0 }}</span>
      </label>
    </div>
  </aside>
</template>
