export type SessionSummary = {
  conversation_id: string;
  created_at?: string;
  updated_at?: string;
  nickname?: string | null;
  summary?: string | null;
  last_user_snippet?: string | null;
  cwd?: string | null;
  git_branch?: string | null;
  source?: string | null;
  message_count?: number;
};

export type HistoryLine = {
  index?: number;
  value?: RolloutEntry;
} & RolloutEntry;

export type HistoryResponse = {
  items: HistoryLine[];
  start_index?: number | null;
  end_index?: number | null;
  truncated?: boolean;
};

export type RolloutEntry = {
  type: string;
  payload?: Record<string, unknown> | null;
};

export type TimelineKind =
  | "assistant"
  | "user"
  | "system"
  | "tool"
  | "exec"
  | "diff"
  | "review";

export type TimelineItem = {
  id: string;
  kind: TimelineKind;
  subkind?: string;
  title: string;
  text?: string;
  meta?: string;
  imageUrl?: string;
  index?: number;
};
