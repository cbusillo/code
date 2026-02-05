use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use code_core::SessionCatalog;
use code_protocol::ConversationId;
use once_cell::sync::OnceCell;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, SeekFrom};
use tracing::warn;

use crate::{HISTORY_MAX_BYTES, ROLLOUT_SNAPSHOT_MAX_BYTES, ROLLOUT_SNAPSHOT_MAX_LINES};

struct LineCountCacheEntry {
    modified: Option<u128>,
    len: u64,
    lines: usize,
}

#[derive(Clone)]
struct LineIndex {
    modified: Option<u128>,
    len: u64,
    stride: usize,
    offsets: Vec<u64>,
    total_lines: usize,
}

enum LineIndexState {
    Ready(LineIndex),
    Building,
}

static LINE_COUNT_CACHE: OnceCell<Mutex<HashMap<PathBuf, LineCountCacheEntry>>> = OnceCell::new();
static LINE_INDEX_CACHE: OnceCell<Mutex<HashMap<PathBuf, LineIndexState>>> = OnceCell::new();

const LINE_INDEX_STRIDE: usize = 512;

pub(crate) struct RolloutSnapshot {
    pub(crate) rollout: Vec<serde_json::Value>,
    pub(crate) truncated: bool,
    pub(crate) start_index: Option<usize>,
    pub(crate) end_index: Option<usize>,
}

pub(crate) struct HistoryChunk {
    pub(crate) items: Vec<serde_json::Value>,
    pub(crate) start_index: Option<usize>,
    pub(crate) end_index: Option<usize>,
    pub(crate) truncated: bool,
}

fn is_system_status_rollout_line(value: &serde_json::Value) -> bool {
    let Some(typ) = value.get("type").and_then(|v| v.as_str()) else {
        return false;
    };
    if typ != "response_item" {
        return false;
    }
    let Some(payload) = value.get("payload") else {
        return false;
    };
    let Some(message_type) = payload.get("type").and_then(|v| v.as_str()) else {
        return false;
    };
    if message_type != "message" {
        return false;
    }
    let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
    if role != "user" {
        return false;
    }
    let Some(content) = payload.get("content").and_then(|v| v.as_array()) else {
        return false;
    };
    let mut text = String::new();
    for item in content {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "input_text" || item_type == "output_text" {
            if let Some(snippet) = item.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(snippet);
            }
        }
    }
    text.trim_start().starts_with("== System Status ==")
}

pub(crate) async fn load_rollout_snapshot_with_limit(
    code_home: &Path,
    conversation_id: &ConversationId,
    max_lines: usize,
) -> std::io::Result<RolloutSnapshot> {
    let Some(path) = resolve_rollout_path(code_home, &conversation_id.to_string()).await? else {
        return Ok(RolloutSnapshot {
            rollout: Vec::new(),
            truncated: false,
            start_index: None,
            end_index: None,
        });
    };

    load_rollout_snapshot_from_path_with_limit(&path, max_lines).await
}

pub(crate) async fn load_rollout_snapshot_from_path_with_limit(
    path: &Path,
    max_lines: usize,
) -> std::io::Result<RolloutSnapshot> {
    load_rollout_snapshot_from_path_with_limit_and_total(path, max_lines, None).await
}

pub(crate) async fn load_rollout_snapshot_from_path_with_limit_and_total(
    path: &Path,
    max_lines: usize,
    total_lines: Option<usize>,
) -> std::io::Result<RolloutSnapshot> {
    let total_lines = match total_lines {
        Some(lines) => lines,
        None => count_lines_fast(path).await?,
    };
    let max_lines = max_lines.min(ROLLOUT_SNAPSHOT_MAX_LINES).max(1);
    let tail_lines = read_tail_lines(path, max_lines, ROLLOUT_SNAPSHOT_MAX_BYTES).await?;
    let truncated = total_lines > tail_lines.len();
    let start_index = total_lines.saturating_sub(tail_lines.len());
    let mut rollout: VecDeque<(usize, serde_json::Value, usize)> = VecDeque::new();
    let mut index = start_index;
    for line in tail_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let line_bytes = line.as_bytes().len();
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) => {
                if is_system_status_rollout_line(&value) {
                    index = index.saturating_add(1);
                    continue;
                }
                rollout.push_back((index, value, line_bytes));
            }
            Err(err) => warn!("failed to parse rollout line: {err}"),
        }
        index = index.saturating_add(1);
    }

    let start_index = rollout.front().map(|(idx, _, _)| *idx);
    let end_index = rollout.back().map(|(idx, _, _)| *idx);
    let rollout = rollout
        .into_iter()
        .map(|(idx, value, _)| serde_json::json!({"index": idx, "value": value}))
        .collect();

    Ok(RolloutSnapshot {
        rollout,
        truncated,
        start_index,
        end_index,
    })
}

pub(crate) async fn load_history_chunk_from_path(
    path: &Path,
    before: Option<usize>,
    after: Option<usize>,
    limit: usize,
) -> std::io::Result<HistoryChunk> {
    if before.is_some() || after.is_some() {
        if let Some(index) = get_line_index(path).await {
            if let Some(chunk) =
                load_history_chunk_from_index(path, &index, before, after, limit).await?
            {
                return Ok(chunk);
            }
        } else if before.is_some() ^ after.is_some() {
            // If we don't yet have a line index, building it is still typically much cheaper than
            // scanning and JSON-parsing the entire file just to serve a bounded page.
            if let Ok(index) = build_line_index(path).await {
                let cache = LINE_INDEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
                if let Ok(mut guard) = cache.lock() {
                    guard.insert(path.to_path_buf(), LineIndexState::Ready(index.clone()));
                }

                if let Some(chunk) =
                    load_history_chunk_from_index(path, &index, before, after, limit).await?
                {
                    return Ok(chunk);
                }
            }
        }
    }
    let file = tokio::fs::File::open(path).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut input_lines = reader.lines();
    let mut items: VecDeque<(usize, serde_json::Value, usize)> = VecDeque::new();
    let mut bytes_read = 0usize;
    let mut truncated = false;
    let mut index = 0usize;

    if let Some(after_index) = after {
        while let Some(line) = input_lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if index <= after_index {
                index = index.saturating_add(1);
                continue;
            }
            let line_bytes = line.as_bytes().len();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if !is_system_status_rollout_line(&value) {
                    items.push_back((index, value, line_bytes));
                    bytes_read = bytes_read.saturating_add(line_bytes);
                }
            }
            index = index.saturating_add(1);

            if items.len() >= limit || bytes_read > HISTORY_MAX_BYTES {
                truncated = true;
                break;
            }
        }
        // We don't peek ahead here; truncation reflects limit/byte caps only.
    } else {
        let before_index = before.unwrap_or(usize::MAX);
        while let Some(line) = input_lines.next_line().await? {
            if index >= before_index {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let line_bytes = line.as_bytes().len();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if !is_system_status_rollout_line(&value) {
                    items.push_back((index, value, line_bytes));
                    bytes_read = bytes_read.saturating_add(line_bytes);
                }
            }
            index = index.saturating_add(1);

            while items.len() > limit || bytes_read > HISTORY_MAX_BYTES {
                if let Some((_idx, _value, size)) = items.pop_front() {
                    bytes_read = bytes_read.saturating_sub(size);
                    truncated = true;
                } else {
                    break;
                }
            }
        }
    }

    let start_index = items.front().map(|(idx, _, _)| *idx);
    let end_index = items.back().map(|(idx, _, _)| *idx);
    if before.is_some() && start_index.unwrap_or(0) > 0 {
        truncated = true;
    }

    let items = items
        .into_iter()
        .map(|(idx, value, _)| serde_json::json!({"index": idx, "value": value}))
        .collect();

    Ok(HistoryChunk {
        items,
        start_index,
        end_index,
        truncated,
    })
}

pub(crate) async fn load_history_head_from_path(
    path: &Path,
    limit: usize,
) -> std::io::Result<HistoryChunk> {
    let file = tokio::fs::File::open(path).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut input_lines = reader.lines();
    let mut items: VecDeque<(usize, serde_json::Value, usize)> = VecDeque::new();
    let mut bytes_read = 0usize;
    let mut truncated = false;
    let mut index = 0usize;

    while let Some(line) = input_lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let line_bytes = line.as_bytes().len();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            items.push_back((index, value, line_bytes));
            bytes_read = bytes_read.saturating_add(line_bytes);
        }
        index = index.saturating_add(1);

        if items.len() >= limit || bytes_read > HISTORY_MAX_BYTES {
            truncated = true;
            break;
        }
    }

    let start_index = items.front().map(|(idx, _, _)| *idx);
    let end_index = items.back().map(|(idx, _, _)| *idx);
    let items = items
        .into_iter()
        .map(|(idx, value, _)| serde_json::json!({"index": idx, "value": value}))
        .collect();

    Ok(HistoryChunk {
        items,
        start_index,
        end_index,
        truncated,
    })
}

pub(crate) async fn load_history_tail_from_path_with_total(
    path: &Path,
    limit: usize,
    total_lines: Option<usize>,
) -> std::io::Result<HistoryChunk> {
    let total_lines = match total_lines {
        Some(lines) => lines,
        None => count_lines_fast(path).await?,
    };
    let tail_lines = read_tail_lines(path, limit.max(1), HISTORY_MAX_BYTES).await?;
    let start_offset = total_lines.saturating_sub(tail_lines.len());
    let mut items: VecDeque<(usize, serde_json::Value)> = VecDeque::new();
    let mut index = start_offset;

    for line in tail_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            items.push_back((index, value));
        }
        index = index.saturating_add(1);
    }

    let start_index = items.front().map(|(idx, _)| *idx);
    let end_index = items.back().map(|(idx, _)| *idx);
    let truncated = start_index.unwrap_or(0) > 0;
    let items = items
        .into_iter()
        .map(|(idx, value)| serde_json::json!({"index": idx, "value": value}))
        .collect();

    Ok(HistoryChunk {
        items,
        start_index,
        end_index,
        truncated,
    })
}

pub(crate) async fn resolve_rollout_path(
    code_home: &Path,
    id: &str,
) -> std::io::Result<Option<PathBuf>> {
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    match catalog.find_by_id_cached(id).await {
        Ok(Some(entry)) => return Ok(Some(catalog.entry_rollout_path(&entry))),
        Ok(None) => {}
        Err(err) => {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, err.to_string()));
        }
    }

    let path = code_core::find_conversation_path_by_id_str(code_home, id).await?;
    if path.is_some() {
        return Ok(path);
    }

    match catalog.find_by_id(id).await {
        Ok(Some(entry)) => Ok(Some(catalog.entry_rollout_path(&entry))),
        Ok(None) => Ok(None),
        Err(err) => Err(std::io::Error::new(std::io::ErrorKind::Other, err.to_string())),
    }
}

pub(crate) async fn estimate_total_lines(
    code_home: &Path,
    conversation_id: &ConversationId,
) -> Option<usize> {
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    match catalog.find_by_id_cached(&conversation_id.to_string()).await {
        Ok(Some(entry)) => Some(entry.message_count),
        _ => None,
    }
}

pub(crate) fn summarize_text(text: &str, max_words: usize) -> Option<String> {
    let mut words = Vec::new();
    for word in text.split_whitespace() {
        if word.is_empty() {
            continue;
        }
        words.push(word.to_string());
        if words.len() >= max_words {
            break;
        }
    }
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

pub(crate) fn normalize_snapshot_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) => 0,
        Some(value) => value.clamp(1, ROLLOUT_SNAPSHOT_MAX_LINES),
        None => ROLLOUT_SNAPSHOT_MAX_LINES,
    }
}

pub(crate) fn warm_rollout_index(path: PathBuf) {
    tokio::spawn(async move {
        let _ = get_line_index(&path).await;
    });
}

async fn get_line_index(path: &Path) -> Option<LineIndex> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let len = metadata.len();

    let cache = LINE_INDEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut should_spawn = false;
    if let Ok(mut guard) = cache.lock() {
        match guard.get(path) {
            Some(LineIndexState::Ready(index))
                if index.len == len && index.modified == modified =>
            {
                return Some(index.clone());
            }
            Some(LineIndexState::Ready(_)) => {
                guard.insert(path.to_path_buf(), LineIndexState::Building);
                should_spawn = true;
            }
            Some(LineIndexState::Building) => {}
            None => {
                guard.insert(path.to_path_buf(), LineIndexState::Building);
                should_spawn = true;
            }
        }
    }

    if should_spawn {
        spawn_line_index_build(path.to_path_buf());
    }

    None
}

fn spawn_line_index_build(path: PathBuf) {
    tokio::spawn(async move {
        let result = build_line_index(&path).await;
        let cache = LINE_INDEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut guard) = cache.lock() {
            match result {
                Ok(index) => {
                    guard.insert(path, LineIndexState::Ready(index));
                }
                Err(_) => {
                    guard.remove(&path);
                }
            }
        }
    });
}

async fn build_line_index(path: &Path) -> std::io::Result<LineIndex> {
    let metadata = tokio::fs::metadata(path).await?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let len = metadata.len();

    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut offsets = vec![0u64];
    let mut count = 0usize;
    let mut pos = 0u64;
    let mut last_byte = None;

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            pos = pos.saturating_add(1);
            if byte == b'\n' {
                count = count.saturating_add(1);
                if count % LINE_INDEX_STRIDE == 0 {
                    offsets.push(pos);
                }
            }
            last_byte = Some(byte);
        }
    }
    if last_byte.is_some() && last_byte != Some(b'\n') {
        count = count.saturating_add(1);
    }

    update_line_count_cache(path, modified, len, count);

    Ok(LineIndex {
        modified,
        len,
        stride: LINE_INDEX_STRIDE,
        offsets,
        total_lines: count,
    })
}

async fn load_history_chunk_from_index(
    path: &Path,
    index: &LineIndex,
    before: Option<usize>,
    after: Option<usize>,
    limit: usize,
) -> std::io::Result<Option<HistoryChunk>> {
    let total_lines = index.total_lines;
    if total_lines == 0 {
        return Ok(Some(HistoryChunk {
            items: Vec::new(),
            start_index: None,
            end_index: None,
            truncated: false,
        }));
    }
    if before.is_some() && after.is_some() {
        return Ok(None);
    }

    let (start_line, end_line) = if let Some(after_index) = after {
        let start = after_index.saturating_add(1);
        if start >= total_lines {
            return Ok(Some(HistoryChunk {
                items: Vec::new(),
                start_index: None,
                end_index: None,
                truncated: false,
            }));
        }
        let end = start.saturating_add(limit).min(total_lines);
        (start, end)
    } else {
        let end = before.unwrap_or(total_lines).min(total_lines);
        let start = end.saturating_sub(limit);
        (start, end)
    };

    if end_line <= start_line {
        return Ok(Some(HistoryChunk {
            items: Vec::new(),
            start_index: None,
            end_index: None,
            truncated: false,
        }));
    }

    let stride = index.stride.max(1);
    let seek_line = (start_line / stride) * stride;
    let offset_index = seek_line / stride;
    let offset = index.offsets.get(offset_index).copied().unwrap_or(0);

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(SeekFrom::Start(offset)).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut input_lines = reader.lines();
    let mut items: VecDeque<(usize, serde_json::Value, usize)> = VecDeque::new();
    let mut bytes_read = 0usize;
    let mut truncated = false;
    let mut index_cursor = seek_line;

    while let Some(line) = input_lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if index_cursor < start_line {
            index_cursor = index_cursor.saturating_add(1);
            continue;
        }
        if index_cursor >= end_line {
            break;
        }
        let line_bytes = line.as_bytes().len();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if !is_system_status_rollout_line(&value) {
                items.push_back((index_cursor, value, line_bytes));
                bytes_read = bytes_read.saturating_add(line_bytes);
            }
        }
        index_cursor = index_cursor.saturating_add(1);

        if items.len() >= limit || bytes_read > HISTORY_MAX_BYTES {
            truncated = true;
            break;
        }
    }

    let start_index = items.front().map(|(idx, _, _)| *idx);
    let end_index = items.back().map(|(idx, _, _)| *idx);
    if before.is_some() && start_index.unwrap_or(0) > 0 {
        truncated = true;
    }

    let items = items
        .into_iter()
        .map(|(idx, value, _)| serde_json::json!({"index": idx, "value": value}))
        .collect();

    Ok(Some(HistoryChunk {
        items,
        start_index,
        end_index,
        truncated,
    }))
}

fn update_line_count_cache(path: &Path, modified: Option<u128>, len: u64, lines: usize) {
    let cache = LINE_COUNT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(
            path.to_path_buf(),
            LineCountCacheEntry {
                modified,
                len,
                lines,
            },
        );
    }
}

async fn count_lines_fast(path: &Path) -> std::io::Result<usize> {
    let metadata = tokio::fs::metadata(path).await?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let len = metadata.len();

    if let Some(cache) = LINE_COUNT_CACHE.get() {
        if let Ok(guard) = cache.lock() {
            if let Some(entry) = guard.get(path) {
                if entry.len == len && entry.modified == modified {
                    return Ok(entry.lines);
                }
            }
        }
    }

    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut count = 0usize;
    let mut last_byte = None;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            if byte == b'\n' {
                count += 1;
            }
            last_byte = Some(byte);
        }
    }
    if last_byte.is_some() && last_byte != Some(b'\n') {
        count += 1;
    }

    update_line_count_cache(path, modified, len, count);

    Ok(count)
}

async fn read_tail_lines(
    path: &Path,
    max_lines: usize,
    max_bytes: usize,
) -> std::io::Result<Vec<String>> {
    let mut file = tokio::fs::File::open(path).await?;
    let file_len = file.metadata().await?.len() as i64;
    if file_len == 0 {
        return Ok(Vec::new());
    }

    let mut last_byte = [0u8; 1];
    file.seek(SeekFrom::End(-1)).await?;
    file.read_exact(&mut last_byte).await?;
    let ends_with_newline = last_byte[0] == b'\n';

    let mut pos = file_len;
    let mut collected: VecDeque<Vec<u8>> = VecDeque::new();
    let mut carry: Vec<u8> = Vec::new();
    let mut bytes_read = 0usize;
    let chunk_size: i64 = 64 * 1024;

    while pos > 0 && collected.len() < max_lines && bytes_read < max_bytes {
        let read_size = chunk_size.min(pos) as usize;
        pos -= read_size as i64;
        file.seek(SeekFrom::Start(pos as u64)).await?;
        let mut buffer = vec![0u8; read_size];
        file.read_exact(&mut buffer).await?;
        bytes_read = bytes_read.saturating_add(buffer.len());

        let mut combined = Vec::with_capacity(buffer.len() + carry.len());
        combined.extend_from_slice(&buffer);
        combined.extend_from_slice(&carry);
        carry = combined;

        while let Some(idx) = carry.iter().rposition(|&byte| byte == b'\n') {
            if collected.len() >= max_lines {
                break;
            }
            let line = carry.split_off(idx + 1);
            carry.truncate(idx);
            collected.push_front(line);
        }
    }

    if pos == 0 && collected.len() < max_lines && !carry.is_empty() {
        collected.push_front(carry);
    }

    if ends_with_newline {
        while collected.back().is_some_and(|line| line.is_empty()) {
            collected.pop_back();
        }
    }

    let mut lines = Vec::with_capacity(collected.len());
    for mut line in collected {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let rendered = String::from_utf8_lossy(&line).to_string();
        lines.push(rendered);
    }

    Ok(lines)
}
