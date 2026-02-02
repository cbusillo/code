<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  ref,
  watch,
} from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";

import type { TimelineItem } from "../../api/types";
import TimelineItemCard from "./TimelineItemCard.vue";
import styles from "./TimelinePanel.module.css";

type Props = {
  items: TimelineItem[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => Promise<void>;
  loadMorePlacement?: "prepend" | "append";
  resetKey?: string | number | null;
  onOpenPanels?: () => void;
  onJumpToStart?: () => Promise<void>;
  onJumpToEnd?: () => Promise<void>;
  statusText?: string;
  onUserScroll?: () => void;
};

const props = withDefaults(defineProps<Props>(), {
  loadMorePlacement: "prepend",
});

const listRef = ref<HTMLDivElement | null>(null);
const topSentinelRef = ref<HTMLDivElement | null>(null);
const bottomRef = ref<HTMLDivElement | null>(null);
const loadInFlightRef = ref(false);
const hasMoreRef = ref(props.hasMore);
const loadingRef = ref(props.loading);
const newItems = ref(0);
const followOutput = ref(true);
const jumpOpen = ref(false);
const lastCountRef = ref(props.items.length);
const jumpButtonRef = ref<HTMLButtonElement | null>(null);
const jumpMenuRef = ref<HTMLDivElement | null>(null);
const programmaticScrollRef = ref(false);
const programmaticScrollTimerRef = ref<number | null>(null);

const markProgrammaticScroll = () => {
  programmaticScrollRef.value = true;
  if (programmaticScrollTimerRef.value !== null) {
    window.clearTimeout(programmaticScrollTimerRef.value);
  }
  programmaticScrollTimerRef.value = window.setTimeout(() => {
    programmaticScrollRef.value = false;
    programmaticScrollTimerRef.value = null;
  }, 150);
};

const computeNearBottom = () => {
  const node = listRef.value;
  if (!node) return true;
  return node.scrollHeight - node.scrollTop - node.clientHeight < 40;
};

const scrollToBottom = () => {
  markProgrammaticScroll();
  const node = bottomRef.value;
  if (node) {
    node.scrollIntoView({ block: "end" });
  } else if (listRef.value) {
    listRef.value.scrollTop = listRef.value.scrollHeight;
  }
  newItems.value = 0;
};

const scrollToTop = () => {
  markProgrammaticScroll();
  const node = listRef.value;
  if (!node) return;
  node.scrollTop = 0;
};

const handleLoadMore = async () => {
  if (loadInFlightRef.value || loadingRef.value || !hasMoreRef.value) {
    return;
  }
  loadInFlightRef.value = true;
  const node = listRef.value;
  if (!node) {
    try {
      await props.onLoadMore();
    } finally {
      loadInFlightRef.value = false;
    }
    return;
  }
  const prevScrollHeight = node.scrollHeight;
  const prevScrollTop = node.scrollTop;
  const nearBottom =
    node.scrollHeight - node.scrollTop - node.clientHeight < 40;
  try {
    await props.onLoadMore();
    requestAnimationFrame(() => {
      const nextScrollHeight = node.scrollHeight;
      const delta = nextScrollHeight - prevScrollHeight;
      if (props.loadMorePlacement === "prepend") {
        node.scrollTop = prevScrollTop + delta;
      } else if (nearBottom) {
        node.scrollTop = nextScrollHeight;
      }
    });
  } finally {
    loadInFlightRef.value = false;
  }
};

const handleJumpToStart = async () => {
  followOutput.value = false;
  if (props.onJumpToStart) {
    await props.onJumpToStart();
  } else if (props.hasMore) {
    await handleLoadMore();
  }
  nextTick(scrollToTop);
};

const handleJumpToEnd = async () => {
  followOutput.value = true;
  if (props.onJumpToEnd) {
    await props.onJumpToEnd();
  }
  nextTick(scrollToBottom);
};

watch(
  () => props.items,
  (items) => {
    const previous = lastCountRef.value;
    const next = items.length;
    if (!followOutput.value && next > previous) {
      newItems.value += next - previous;
    }
    lastCountRef.value = next;
    if (followOutput.value) {
      markProgrammaticScroll();
      nextTick(scrollToBottom);
    }
  },
  { deep: true },
);

watch(
  () => props.resetKey,
  () => {
    newItems.value = 0;
    followOutput.value = true;
    lastCountRef.value = props.items.length;
    markProgrammaticScroll();
    nextTick(scrollToBottom);
  },
);

watch(
  () => followOutput.value,
  (value) => {
    if (value && newItems.value !== 0) {
      newItems.value = 0;
    }
  },
);

watch(
  () => props.hasMore,
  (value) => {
    hasMoreRef.value = value;
  },
);

watch(
  () => props.loading,
  (value) => {
    loadingRef.value = value;
  },
);

watch(
  [
    () => props.loadMorePlacement,
    () => followOutput.value,
    () => listRef.value,
    () => topSentinelRef.value,
    () => bottomRef.value,
  ],
  ([placement, follow], _prev, onCleanup) => {
    const root = listRef.value;
    if (!root) return;
    if (placement === "prepend" && follow) {
      return;
    }
    const target = placement === "prepend" ? topSentinelRef.value : bottomRef.value;
    if (!target) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          void handleLoadMore();
        }
      },
      { root, rootMargin: "200px 0px", threshold: 0.01 },
    );
    observer.observe(target);
    onCleanup(() => observer.disconnect());
  },
  { immediate: true },
);

const groupedItems = computed(() => {
  const groups: Array<{ index?: number; items: TimelineItem[] }> = [];
  let current: { index?: number; items: TimelineItem[] } | null = null;
  const finalizeGroup = (
    group: { index?: number; items: TimelineItem[] } | null,
  ) => {
    if (!group) return;
    if (group.index !== undefined) {
      const assistant: TimelineItem[] = [];
      const other: TimelineItem[] = [];
      group.items.forEach((item) => {
        if (item.kind === "assistant") {
          assistant.push(item);
        } else {
          other.push(item);
        }
      });
      group.items = [...other, ...assistant];
    }
  };
  props.items.forEach((item) => {
    const key = item.index;
    if (!current || current.index !== key) {
      finalizeGroup(current);
      current = { index: key, items: [] };
      groups.push(current);
    }
    current.items.push(item);
  });
  finalizeGroup(current);
  return groups;
});

const rowVirtualizer = useVirtualizer(
  computed(() => ({
    count: groupedItems.value.length,
    getScrollElement: () => listRef.value,
    estimateSize: () => 200,
    overscan: 6,
  })),
);

watch(
  () => props.items,
  () => {
    rowVirtualizer.value.measure();
  },
  { deep: true },
);

const diffGroups = computed(() => {
  const indices: number[] = [];
  groupedItems.value.forEach((group, index) => {
    if (group.items.some((item) => item.kind === "diff")) {
      indices.push(index);
    }
  });
  return indices;
});

const getCurrentGroupIndex = () => {
  const visible = rowVirtualizer.value.getVirtualItems();
  if (!visible.length) {
    return null;
  }
  return visible[0].index;
};

const scrollToGroup = (index: number, align: "start" | "end") => {
  rowVirtualizer.value.scrollToIndex(index, { align });
};

const jumpToTurnStart = () => {
  const currentIndex = getCurrentGroupIndex();
  if (currentIndex === null) return;
  followOutput.value = false;
  scrollToGroup(currentIndex, "start");
};

const jumpToTurnEnd = () => {
  const currentIndex = getCurrentGroupIndex();
  if (currentIndex === null) return;
  followOutput.value = false;
  scrollToGroup(currentIndex, "end");
};

const jumpToNextDiff = () => {
  const currentIndex = getCurrentGroupIndex();
  if (currentIndex === null) return;
  const next = diffGroups.value.find((index) => index > currentIndex);
  if (next === undefined) return;
  followOutput.value = false;
  scrollToGroup(next, "start");
};

const jumpToPrevDiff = () => {
  const currentIndex = getCurrentGroupIndex();
  if (currentIndex === null) return;
  const prev = [...diffGroups.value].reverse().find((index) => index < currentIndex);
  if (prev === undefined) return;
  followOutput.value = false;
  scrollToGroup(prev, "start");
};

const handleToggleFollow = () => {
  followOutput.value = !followOutput.value;
  if (followOutput.value) {
    nextTick(scrollToBottom);
  }
};

watch(
  () => jumpOpen.value,
  (open, _prev, onCleanup) => {
    if (!open) return;
    const handleClick = (event: MouseEvent) => {
      const target = event.target as Node;
      if (jumpMenuRef.value?.contains(target)) return;
      if (jumpButtonRef.value?.contains(target)) return;
      jumpOpen.value = false;
    };
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        jumpOpen.value = false;
      }
    };
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKey);
    onCleanup(() => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKey);
    });
  },
);

const handleScroll = () => {
  props.onUserScroll?.();
  const node = listRef.value;
  if (!node) return;
  const nearBottom = computeNearBottom();
  if (!followOutput.value && nearBottom && !programmaticScrollRef.value) {
    followOutput.value = true;
    newItems.value = 0;
  }
  if (followOutput.value && !nearBottom && !programmaticScrollRef.value) {
    followOutput.value = false;
  }
  if (loadInFlightRef.value || loadingRef.value || !hasMoreRef.value) {
    return;
  }
  if (props.loadMorePlacement === "prepend") {
    if (followOutput.value) {
      return;
    }
    if (node.scrollTop < 80) {
      void handleLoadMore();
    }
    return;
  }
  if (props.loadMorePlacement === "append" && nearBottom) {
    void handleLoadMore();
  }
};

const handleWheel = (event: WheelEvent) => {
  props.onUserScroll?.();
  const node = listRef.value;
  if (!node) return;
  if (loadInFlightRef.value || loadingRef.value || !hasMoreRef.value) {
    return;
  }
  if (props.loadMorePlacement === "prepend") {
    if (followOutput.value) {
      return;
    }
    if (event.deltaY < 0 && node.scrollTop < 20) {
      void handleLoadMore();
    }
    return;
  }
  if (
    props.loadMorePlacement === "append" &&
    event.deltaY > 0 &&
    node.scrollHeight - node.scrollTop - node.clientHeight < 20
  ) {
    void handleLoadMore();
  }
};

const registerMeasure = (el: Element | null) => {
  if (el) {
    rowVirtualizer.value.measureElement(el);
  }
};

onBeforeUnmount(() => {
  if (programmaticScrollTimerRef.value !== null) {
    window.clearTimeout(programmaticScrollTimerRef.value);
  }
});

const virtualItems = computed(() => rowVirtualizer.value.getVirtualItems());
const totalSize = computed(() => rowVirtualizer.value.getTotalSize());
const followClass = computed(() => [
  styles.follow,
  followOutput.value ? styles.followActive : "",
]);

const toggleJumpMenu = () => {
  jumpOpen.value = !jumpOpen.value;
};

const handleJumpStartClick = () => {
  jumpOpen.value = false;
  void handleJumpToStart();
};

const handleJumpEndClick = () => {
  jumpOpen.value = false;
  void handleJumpToEnd();
};

const handleTurnStartClick = () => {
  jumpOpen.value = false;
  jumpToTurnStart();
};

const handleTurnEndClick = () => {
  jumpOpen.value = false;
  jumpToTurnEnd();
};

const handlePrevDiffClick = () => {
  jumpOpen.value = false;
  jumpToPrevDiff();
};

const handleNextDiffClick = () => {
  jumpOpen.value = false;
  jumpToNextDiff();
};
</script>

<template>
  <section :class="styles.panel">
    <header :class="styles.header">
      <div :class="styles.title">
        Timeline
        <span v-if="props.statusText" :class="styles.status">{{ props.statusText }}</span>
      </div>
      <div :class="styles.actions">
        <button
          v-if="props.onOpenPanels"
          :class="styles.mobileToggle"
          type="button"
          @click="props.onOpenPanels"
        >
          Panels
        </button>
        <button
          :class="followClass"
          type="button"
          @click="handleToggleFollow"
        >
          {{ followOutput ? "Following" : "Follow" }}
        </button>
        <div :class="styles.jumpWrapper">
          <button
            ref="jumpButtonRef"
            :class="styles.jumpButton"
            type="button"
            @click="toggleJumpMenu"
          >
            Jump
          </button>
          <div v-if="jumpOpen" ref="jumpMenuRef" :class="styles.jumpMenu">
            <button
              :class="styles.jumpItem"
              type="button"
              @click="handleJumpStartClick"
            >
              Beginning
            </button>
            <button
              :class="styles.jumpItem"
              type="button"
              @click="handleJumpEndClick"
            >
              End
            </button>
            <div :class="styles.jumpDivider" />
            <button
              :class="styles.jumpItem"
              type="button"
              :disabled="!groupedItems.length"
              @click="handleTurnStartClick"
            >
              Turn start
            </button>
            <button
              :class="styles.jumpItem"
              type="button"
              :disabled="!groupedItems.length"
              @click="handleTurnEndClick"
            >
              Turn end
            </button>
            <div :class="styles.jumpDivider" />
            <button
              :class="styles.jumpItem"
              type="button"
              :disabled="!diffGroups.length"
              @click="handlePrevDiffClick"
            >
              Prev diff
            </button>
            <button
              :class="styles.jumpItem"
              type="button"
              :disabled="!diffGroups.length"
              @click="handleNextDiffClick"
            >
              Next diff
            </button>
          </div>
        </div>
      </div>
    </header>
    <div
      ref="listRef"
      :class="styles.timeline"
      @scroll="handleScroll"
      @wheel="handleWheel"
    >
      <div ref="topSentinelRef" :class="styles.sentinel" />
      <div v-if="props.loading" :class="styles.loading">Loading…</div>
      <div v-else-if="props.items.length === 0" :class="styles.empty">
        No timeline items yet.
      </div>
      <div :class="styles.virtualizer" :style="{ height: `${totalSize}px` }">
        <div
          v-for="virtualRow in virtualItems"
          :key="`${groupedItems[virtualRow.index]?.index ?? 'group'}-${virtualRow.index}`"
          :ref="registerMeasure"
          :data-index="virtualRow.index"
          :class="styles.group"
          :style="{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            transform: `translateY(${virtualRow.start}px)`,
          }"
        >
          <TimelineItemCard
            v-for="item in groupedItems[virtualRow.index]?.items || []"
            :key="item.id"
            :item="item"
          />
        </div>
        <div
          ref="bottomRef"
          :style="{
            position: 'absolute',
            top: `${totalSize}px`,
            left: 0,
            height: '1px',
            width: '100%',
          }"
        />
      </div>
      <button
        v-if="newItems > 0 && !followOutput"
        :class="styles.newReplies"
        type="button"
        @click="handleJumpToEnd"
      >
        {{ newItems }} new replies
      </button>
    </div>
  </section>
</template>
