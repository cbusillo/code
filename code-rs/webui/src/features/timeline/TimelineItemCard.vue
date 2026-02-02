<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import MarkdownIt from "markdown-it";
import hljs from "highlight.js";

import type { TimelineItem } from "../../api/types";
import styles from "./TimelineItemCard.module.css";

type Props = {
  item: TimelineItem;
};

type DiffLineKind = "add" | "remove" | "context";
type DiffLine = {
  kind: DiffLineKind;
  oldLine?: number | null;
  newLine?: number | null;
  text: string;
};

type DiffFile = {
  path: string;
  lines: DiffLine[];
  added: number;
  removed: number;
};

type DiffInfo = {
  files: DiffFile[];
};

const props = defineProps<Props>();

const copyToClipboard = async (value: string): Promise<boolean> => {
  if (!value) {
    return false;
  }
  if (navigator?.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // fall back to legacy copy path
    }
  }
  try {
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.style.position = "fixed";
    textarea.style.left = "-9999px";
    textarea.style.top = "-9999px";
    document.body.appendChild(textarea);
    textarea.focus();
    textarea.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
};

const parseUnifiedDiff = (text: string): DiffInfo => {
  if (!text) {
    return { files: [] };
  }
  const fileMap = new Map<string, DiffFile>();
  let currentFile: DiffFile | null = null;
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;

  const ensureFile = (raw: string | null | undefined) => {
    if (!raw || raw === "/dev/null") {
      return null;
    }
    const trimmed = raw.replace(/^([ab])\//, "");
    let entry = fileMap.get(trimmed);
    if (!entry) {
      entry = { path: trimmed, lines: [], added: 0, removed: 0 };
      fileMap.set(trimmed, entry);
    }
    currentFile = entry;
    return entry;
  };

  const ensureFallback = () => {
    if (!currentFile) {
      currentFile = ensureFile("changes") ?? null;
    }
    return currentFile;
  };

  text.split("\n").forEach((raw) => {
    if (raw.startsWith("diff --git ")) {
      const match = /diff --git a\/(.+?) b\/(.+)/.exec(raw);
      if (match) {
        ensureFile(match[2]);
      }
      inHunk = false;
      return;
    }
    if (raw.startsWith("+++ ") || raw.startsWith("--- ")) {
      ensureFile(raw.slice(4).trim());
      return;
    }
    if (raw.startsWith("index ")) {
      return;
    }
    if (raw.startsWith("@@")) {
      const match = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw);
      if (match) {
        oldLine = Number.parseInt(match[1], 10);
        newLine = Number.parseInt(match[2], 10);
        inHunk = true;
      }
      return;
    }
    if (!inHunk) {
      return;
    }
    if (raw.startsWith("\\ No newline at end of file")) {
      return;
    }
    const target = ensureFallback();
    if (!target) {
      return;
    }
    if (raw.startsWith("+")) {
      target.lines.push({
        kind: "add",
        oldLine: null,
        newLine,
        text: raw.slice(1),
      });
      target.added += 1;
      newLine += 1;
      return;
    }
    if (raw.startsWith("-")) {
      target.lines.push({
        kind: "remove",
        oldLine,
        newLine: null,
        text: raw.slice(1),
      });
      target.removed += 1;
      oldLine += 1;
      return;
    }
    if (raw.startsWith(" ")) {
      target.lines.push({
        kind: "context",
        oldLine,
        newLine,
        text: raw.slice(1),
      });
      oldLine += 1;
      newLine += 1;
    }
  });

  return { files: Array.from(fileMap.values()) };
};

const addClass = (token: any, className: string) => {
  if (!className) return;
  const current = token.attrGet("class");
  token.attrSet("class", current ? `${current} ${className}` : className);
};

const createMarkdownRenderer = (styleMap: Record<string, string>) => {
  const renderIcon = (className: string, path: string) =>
    `<svg class="${className}" viewBox="0 0 24 24" aria-hidden="true" focusable="false">${path}</svg>`;
  const copyIconSmall = renderIcon(
    styleMap.iconSmall,
    "<rect x=\"9\" y=\"9\" width=\"10\" height=\"10\" rx=\"2\" />" +
      "<rect x=\"5\" y=\"5\" width=\"10\" height=\"10\" rx=\"2\" />",
  );
  const md = new MarkdownIt({
    html: false,
    linkify: true,
    breaks: true,
    highlight: (code: string, language: string) => {
      const trimmed = code.endsWith("\n") ? code.slice(0, -1) : code;
      if (language && hljs.getLanguage(language)) {
        try {
          return hljs.highlight(trimmed, { language }).value;
        } catch {
          // ignore highlighting errors
        }
      }
      return md.utils.escapeHtml(trimmed);
    },
  });

  const wrapRule = (ruleName: string, className: string) => {
    const fallback = md.renderer.rules[ruleName] ?? md.renderer.renderToken;
    md.renderer.rules[ruleName] = (tokens, idx, options, env, self) => {
      addClass(tokens[idx], className);
      return fallback.call(md.renderer, tokens, idx, options, env, self);
    };
  };

  wrapRule("paragraph_open", styleMap.paragraph);
  wrapRule("heading_open", styleMap.heading);
  wrapRule("bullet_list_open", styleMap.list);
  wrapRule("ordered_list_open", styleMap.list);
  wrapRule("blockquote_open", styleMap.blockquote);
  wrapRule("thead_open", styleMap.tableHead);
  wrapRule("tbody_open", styleMap.tableBody);
  wrapRule("tr_open", styleMap.tableRow);
  wrapRule("th_open", styleMap.tableHeader);
  wrapRule("td_open", styleMap.tableCell);

  md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
    const token = tokens[idx];
    addClass(token, styleMap.link);
    token.attrSet("target", "_blank");
    token.attrSet("rel", "noreferrer");
    return self.renderToken(tokens, idx, options);
  };

  md.renderer.rules.table_open = () =>
    `<div class="${styleMap.tableWrap}"><table class="${styleMap.table}">`;
  md.renderer.rules.table_close = () => "</table></div>";
  md.renderer.rules.hr = () => `<hr class="${styleMap.divider}" />`;
  md.renderer.rules.code_inline = (tokens, idx) => {
    const code = md.utils.escapeHtml(tokens[idx].content);
    return `<code class="${styleMap.inlineCode}">${code}</code>`;
  };

  const renderCodeBlock = (code: string, language?: string) => {
    const trimmed = code.endsWith("\n") ? code.slice(0, -1) : code;
    const lang = language?.split(/\s+/)[0] || "";
    let highlighted = md.utils.escapeHtml(trimmed);
    if (lang && hljs.getLanguage(lang)) {
      try {
        highlighted = hljs.highlight(trimmed, { language: lang }).value;
      } catch {
        highlighted = md.utils.escapeHtml(trimmed);
      }
    }
    const className = [styleMap.code, lang ? `language-${lang}` : "", "hljs"]
      .filter(Boolean)
      .join(" ");
    return `
      <div class="${styleMap.codeBlockWrap}" data-code-wrap="true">
        <button class="${styleMap.codeCopy}" data-copy="code" type="button" aria-label="Copy code" title="Copy">
          ${copyIconSmall}
        </button>
        <pre class="${styleMap.codeBlock}"><code class="${className}">${highlighted}</code></pre>
      </div>
    `;
  };

  md.renderer.rules.fence = (tokens, idx) =>
    renderCodeBlock(tokens[idx].content, tokens[idx].info);
  md.renderer.rules.code_block = (tokens, idx) =>
    renderCodeBlock(tokens[idx].content);

  return md;
};

const markdown = createMarkdownRenderer(styles);

const className = computed(() => [styles.card, styles[props.item.kind] || ""]);
const isLongText = computed(
  () => Boolean(props.item.text && props.item.text.length > 500),
);
const diffInfo = computed(() => {
  if (props.item.kind === "diff" && props.item.text) {
    return parseUnifiedDiff(props.item.text);
  }
  return null;
});
const diffFiles = computed(() => diffInfo.value?.files ?? []);
const markdownHtml = computed(() =>
  props.item.text ? markdown.render(props.item.text) : "",
);
const cardCopied = ref(false);
const copiedFile = ref<string | null>(null);
const diffCompact = ref(false);
const fileSectionRefs = new Map<string, HTMLDetailsElement>();
const cardCopyTimeoutRef = ref<number | null>(null);
const fileCopyTimeoutRef = ref<number | null>(null);
const codeCopyTimeouts = new Map<HTMLElement, number>();

const handleCardCopy = async () => {
  const value = props.item.text || props.item.imageUrl || "";
  if (!value) return;
  const ok = await copyToClipboard(value);
  if (!ok) return;
  cardCopied.value = true;
  if (cardCopyTimeoutRef.value !== null) {
    window.clearTimeout(cardCopyTimeoutRef.value);
  }
  cardCopyTimeoutRef.value = window.setTimeout(() => {
    cardCopied.value = false;
  }, 1600);
};

const handleCopyFile = async (path: string, event?: MouseEvent) => {
  event?.preventDefault();
  event?.stopPropagation();
  const ok = await copyToClipboard(path);
  if (!ok) return;
  copiedFile.value = path;
  if (fileCopyTimeoutRef.value !== null) {
    window.clearTimeout(fileCopyTimeoutRef.value);
  }
  fileCopyTimeoutRef.value = window.setTimeout(() => {
    copiedFile.value = null;
  }, 1600);
};

const registerFileRef = (path: string, node: HTMLDetailsElement | null) => {
  if (node) {
    fileSectionRefs.set(path, node);
  } else {
    fileSectionRefs.delete(path);
  }
};

const jumpToFile = (path: string) => {
  const node = fileSectionRefs.get(path);
  if (node) {
    node.scrollIntoView({ block: "start" });
  }
};

const handleFileJump = (path: string) => {
  if (diffCompact.value) {
    diffCompact.value = false;
  }
  nextTick(() => jumpToFile(path));
};

const handleMarkdownClick = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  const button = target.closest(
    "button[data-copy='code']",
  ) as HTMLButtonElement | null;
  if (!button) return;
  const wrapper = button.closest("[data-code-wrap='true']");
  const code = wrapper?.querySelector("code")?.textContent || "";
  if (!code) return;
  const ok = await copyToClipboard(code);
  if (!ok) return;
  button.classList.add(styles.copyButtonActive);
  const previousHtml = button.innerHTML;
  button.innerHTML = `<svg class="${styles.iconSmall}" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M5 12l4 4 10-10" /></svg>`;
  const existing = codeCopyTimeouts.get(button);
  if (existing) {
    window.clearTimeout(existing);
  }
  const timeout = window.setTimeout(() => {
    button.classList.remove(styles.copyButtonActive);
    button.innerHTML = previousHtml;
    codeCopyTimeouts.delete(button);
  }, 1600);
  codeCopyTimeouts.set(button, timeout);
};

const diffLineClass = (line: DiffLine) => {
  if (line.kind === "add") {
    return [styles.diffRow, styles.diffAdd];
  }
  if (line.kind === "remove") {
    return [styles.diffRow, styles.diffRemove];
  }
  return [styles.diffRow, styles.diffContext];
};

const diffLineText = (line: DiffLine) =>
  line.text === "" ? "\u00a0" : line.text;

const diffFileClass = (path: string) => [
  styles.diffFile,
  copiedFile.value === path ? styles.diffFileCopied : "",
];

const diffFileCopyClass = (path: string) => [
  styles.diffFileCopy,
  copiedFile.value === path ? styles.copyButtonActive : "",
];

const cardCopyClass = computed(() => [
  styles.copyButton,
  cardCopied.value ? styles.copyButtonActive : "",
]);

const execLabel = computed(() =>
  props.item.meta?.startsWith("exit") ? "Output" : "Command",
);

onBeforeUnmount(() => {
  if (cardCopyTimeoutRef.value !== null) {
    window.clearTimeout(cardCopyTimeoutRef.value);
  }
  if (fileCopyTimeoutRef.value !== null) {
    window.clearTimeout(fileCopyTimeoutRef.value);
  }
  codeCopyTimeouts.forEach((timeout) => window.clearTimeout(timeout));
  codeCopyTimeouts.clear();
});
</script>

<template>
  <article :class="className">
    <header :class="styles.header">
      <div :class="styles.headerLeft">
        <div :class="styles.title">{{ props.item.title }}</div>
        <div v-if="diffFiles.length" :class="styles.diffFiles">
          <button
            v-for="file in diffFiles"
            :key="`${props.item.id}-${file.path}`"
            :class="diffFileClass(file.path)"
            type="button"
            @click="handleFileJump(file.path)"
          >
            <span :class="styles.diffFileLabel">{{ file.path }}</span>
            <span :class="styles.diffFileStats">+{{ file.added }} -{{ file.removed }}</span>
          </button>
        </div>
      </div>
      <div :class="styles.headerRight">
        <div :class="styles.tags">
          <span v-if="props.item.subkind" :class="styles.chip">
            {{ props.item.subkind }}
          </span>
          <span v-if="props.item.meta" :class="styles.meta">
            {{ props.item.meta }}
          </span>
        </div>
        <button
          v-if="props.item.kind === 'diff' && diffFiles.length"
          :class="styles.diffToggle"
          type="button"
          @click="diffCompact = !diffCompact"
        >
          {{ diffCompact ? "Show diff" : "Files only" }}
        </button>
        <button
          v-if="props.item.text || props.item.imageUrl"
          :class="cardCopyClass"
          type="button"
          aria-label="Copy card"
          title="Copy"
          @click="handleCardCopy"
        >
          <svg
            v-if="cardCopied"
            :class="styles.icon"
            viewBox="0 0 24 24"
            aria-hidden="true"
            focusable="false"
          >
            <path d="M5 12l4 4 10-10" />
          </svg>
          <svg
            v-else
            :class="styles.icon"
            viewBox="0 0 24 24"
            aria-hidden="true"
            focusable="false"
          >
            <rect x="9" y="9" width="10" height="10" rx="2" />
            <rect x="5" y="5" width="10" height="10" rx="2" />
          </svg>
        </button>
      </div>
    </header>

    <div v-if="props.item.kind === 'diff' && props.item.text">
      <pre v-if="!diffFiles.length" :class="styles.body">
        {{ props.item.text }}
      </pre>
      <div v-else-if="diffCompact" :class="styles.diffOnlyList">
        <div
          v-for="file in diffFiles"
          :key="`${props.item.id}-only-${file.path}`"
          :class="styles.diffOnlyRow"
        >
          <span :class="styles.diffFileName">{{ file.path }}</span>
          <span :class="styles.diffOnlyStats">+{{ file.added }} -{{ file.removed }}</span>
        </div>
      </div>
      <div v-else :class="styles.diffWrapper">
        <details
          v-for="(file, index) in diffFiles"
          :key="`${props.item.id}-file-${file.path}`"
          :class="styles.diffFileSection"
          :open="diffFiles.length === 1 || index === 0"
          :ref="(el) => registerFileRef(file.path, el as HTMLDetailsElement | null)"
        >
          <summary :class="styles.diffFileSummary">
            <span :class="styles.diffFileName">{{ file.path }}</span>
            <span :class="styles.diffFileMeta">+{{ file.added }} -{{ file.removed }}</span>
            <button
              :class="diffFileCopyClass(file.path)"
              type="button"
              aria-label="Copy file path"
              title="Copy"
              @click="handleCopyFile(file.path, $event)"
            >
              <svg
                v-if="copiedFile === file.path"
                :class="styles.iconSmall"
                viewBox="0 0 24 24"
                aria-hidden="true"
                focusable="false"
              >
                <path d="M5 12l4 4 10-10" />
              </svg>
              <svg
                v-else
                :class="styles.iconSmall"
                viewBox="0 0 24 24"
                aria-hidden="true"
                focusable="false"
              >
                <rect x="9" y="9" width="10" height="10" rx="2" />
                <rect x="5" y="5" width="10" height="10" rx="2" />
              </svg>
            </button>
          </summary>
          <div v-if="file.lines.length" :class="styles.diffGrid">
            <div
              v-for="(line, idx) in file.lines"
              :key="`${props.item.id}-${file.path}-line-${idx}`"
              :class="diffLineClass(line)"
            >
              <span :class="styles.diffLineNumber">{{ line.oldLine ?? "" }}</span>
              <span :class="styles.diffLineNumber">{{ line.newLine ?? "" }}</span>
              <span :class="styles.diffContent">{{ diffLineText(line) }}</span>
            </div>
          </div>
          <div v-else :class="styles.diffEmpty">No diff content</div>
        </details>
      </div>
    </div>

    <details
      v-else-if="props.item.subkind === 'reasoning' && props.item.text"
      :class="styles.details"
    >
      <summary :class="styles.detailsSummary">Reasoning</summary>
      <div :class="styles.detailsBody">
        <div
          :class="styles.markdown"
          v-html="markdownHtml"
          @click="handleMarkdownClick"
        />
      </div>
    </details>

    <div v-else-if="props.item.kind === 'exec' && props.item.text">
      <details v-if="isLongText" :class="styles.details">
        <summary :class="styles.detailsSummary">{{ execLabel }}</summary>
        <div :class="styles.detailsBody">
          <pre :class="styles.body">{{ props.item.text }}</pre>
        </div>
      </details>
      <div v-else :class="styles.section">
        <div :class="styles.sectionTitle">{{ execLabel }}</div>
        <pre :class="styles.body">{{ props.item.text }}</pre>
      </div>
    </div>

    <div v-else-if="props.item.kind === 'tool' && props.item.text">
      <details v-if="isLongText" :class="styles.details">
        <summary :class="styles.detailsSummary">Tool details</summary>
        <div :class="styles.detailsBody">
          <pre :class="styles.body">{{ props.item.text }}</pre>
        </div>
      </details>
      <pre v-else :class="styles.body">{{ props.item.text }}</pre>
    </div>

    <div
      v-else-if="props.item.text"
      :class="styles.markdown"
      v-html="markdownHtml"
      @click="handleMarkdownClick"
    />

    <img
      v-if="props.item.imageUrl"
      :class="styles.image"
      :src="props.item.imageUrl"
      :alt="props.item.title"
    />
  </article>
</template>
