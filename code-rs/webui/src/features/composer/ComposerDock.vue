<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import styles from "./ComposerDock.module.css";

type Props = {
  value: string;
  disabled?: boolean;
  status?: string | null;
};

const props = defineProps<Props>();
const emit = defineEmits<{
  (event: "change", value: string): void;
  (event: "send"): void;
}>();

const inputRef = ref<HTMLTextAreaElement | null>(null);

const resizeInput = () => {
  const node = inputRef.value;
  if (!node) return;
  node.style.height = "auto";
  const maxHeight = Math.max(140, Math.round(window.innerHeight * 0.32));
  const nextHeight = Math.min(node.scrollHeight, maxHeight);
  node.style.height = `${nextHeight}px`;
  node.style.overflowY = node.scrollHeight > maxHeight ? "auto" : "hidden";
};

watch(
  () => props.value,
  () => {
    resizeInput();
  },
);

onMounted(() => {
  resizeInput();
  window.addEventListener("resize", resizeInput);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", resizeInput);
});

const handleKeyDown = (event: KeyboardEvent) => {
  if (event.key !== "Enter") {
    return;
  }
  if (event.isComposing) {
    return;
  }
  if (event.shiftKey) {
    return;
  }
  event.preventDefault();
  emit("send");
};

const handleInput = (event: Event) => {
  const target = event.target as HTMLTextAreaElement | null;
  emit("change", target?.value ?? "");
};
</script>

<template>
  <footer :class="styles.dock">
    <div :class="styles.header">
      <div :class="styles.title">
        <span :class="styles.label">Composer</span>
        <span v-if="props.status" :class="styles.pill">{{ props.status }}</span>
      </div>
      <span :class="styles.hint">Enter to send · Shift+Enter newline</span>
    </div>
    <textarea
      ref="inputRef"
      :class="styles.input"
      :value="props.value"
      placeholder="Send a message"
      rows="3"
      :disabled="props.disabled"
      @input="handleInput"
      @keydown="handleKeyDown"
    />
    <div :class="styles.actions">
      <button
        :class="styles.primary"
        type="button"
        :disabled="props.disabled"
        @click="emit('send')"
      >
        Send
      </button>
    </div>
  </footer>
</template>
