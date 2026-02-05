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

type FilterNode = {
  id: string;
  label: string;
  kind?: TimelineKind;
  subkind?: string;
  children?: FilterNode[];
};

const TOOL_SUBKINDS: FilterNode[] = [
  { id: "tool-web", label: "Web search" },
  { id: "tool-browser", label: "Browser" },
  { id: "tool-mcp", label: "MCP" },
  { id: "tool-custom", label: "Custom" },
  { id: "tool-call", label: "Tool calls" },
  { id: "tool-output", label: "Tool output" },
].map((item) => ({ ...item, subkind: item.id }));

const EXEC_SUBKINDS: FilterNode[] = [
  { id: "exec-begin", label: "Start" },
  { id: "exec-output", label: "Output" },
  { id: "exec-end", label: "Result" },
].map((item) => ({ ...item, subkind: item.id }));

const FILTER_TREE: FilterNode[] = [
  {
    id: "assistant",
    label: "Assistant",
    kind: "assistant",
    children: [
      { id: "reasoning", label: "Reasoning", subkind: "reasoning" },
    ],
  },
  { id: "user", label: "User", kind: "user" },
  { id: "system", label: "System", kind: "system" },
  { id: "review", label: "Review", kind: "review" },
  { id: "images", label: "Images", subkind: "image" },
  { id: "tools", label: "Tools", kind: "tool", children: TOOL_SUBKINDS },
  { id: "exec", label: "Exec", kind: "exec", children: EXEC_SUBKINDS },
  { id: "diff", label: "Diff", kind: "diff" },
];

const isChecked = (node: FilterNode) => {
  if (node.kind) {
    return props.filters.kinds[node.kind];
  }
  if (node.subkind) {
    return props.filters.subkinds[node.subkind];
  }
  return true;
};

const countFor = (node: FilterNode) => {
  if (node.kind) {
    return props.counts[node.kind] || 0;
  }
  if (node.subkind) {
    return props.subCounts[node.subkind] || 0;
  }
  return 0;
};

const toggleNode = (node: FilterNode) => {
  if (node.kind) {
    emit("toggle-kind", node.kind);
    return;
  }
  if (node.subkind) {
    emit("toggle-subkind", node.subkind);
  }
};
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
      <div
        v-for="node in FILTER_TREE"
        :key="node.id"
        :class="styles.node"
      >
        <label :class="styles.item">
          <input
            type="checkbox"
            :checked="isChecked(node)"
            @change="toggleNode(node)"
          />
          <span :class="styles.label">{{ node.label }}</span>
          <span :class="styles.count">{{ countFor(node) }}</span>
        </label>
        <div v-if="node.children?.length" :class="styles.children">
          <label
            v-for="child in node.children"
            :key="child.id"
            :class="styles.subItem"
          >
            <input
              type="checkbox"
              :checked="isChecked(child)"
              @change="toggleNode(child)"
            />
            <span :class="styles.label">{{ child.label }}</span>
            <span :class="styles.count">{{ countFor(child) }}</span>
          </label>
        </div>
      </div>
    </div>
  </aside>
</template>
