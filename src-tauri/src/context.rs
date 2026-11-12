use crate::types::{ChatMessage, CompactionResult, ContextInspection, ToolDefinition};

const DEFAULT_CONTEXT: u64 = 16_384;
const RESERVED_TOKENS: u64 = 1_536;
const MAX_TOOL_OUTPUT_BYTES: usize = 8_000;
const MAX_WORKSPACE_OUTPUT_BYTES: usize = 120_000;
const MIN_COMPACTED_TOOL_OUTPUT_BYTES: usize = 2_000;
const SUMMARY_MAX_CHARS: usize = 2_400;

pub fn configured_context(requested: Option<u64>, model_maximum: Option<u64>) -> u64 {
    let maximum = model_maximum.unwrap_or(262_144).max(4_096);
    requested.unwrap_or(DEFAULT_CONTEXT).clamp(4_096, maximum)
}

pub fn estimate_tokens(messages: &[ChatMessage]) -> u64 {
    messages
        .iter()
        .map(|message| {
            estimate_text_tokens(&message.content)
                + message
                    .name
                    .as_deref()
                    .map(estimate_text_tokens)
                    .unwrap_or_default()
                + message
                    .tool_call_id
                    .as_deref()
                    .map(estimate_text_tokens)
                    .unwrap_or_default()
                + message
                    .tool_calls
                    .as_ref()
                    .and_then(|calls| serde_json::to_string(calls).ok())
                    .as_deref()
                    .map(estimate_text_tokens)
                    .unwrap_or_default()
                + 8
                + (message.images.len() as u64 * 512)
        })
        .sum()
}

/// Conservatively estimate tokenizer usage without charging a full base64
/// payload for an image. Using three ASCII characters per token leaves room
/// for code, JSON, and provider framing, which commonly tokenize more densely
/// than ordinary prose.
pub fn estimate_text_tokens(text: &str) -> u64 {
    let mut ascii = 0u64;
    let mut non_ascii = 0u64;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            non_ascii += 1;
        }
    }
    ascii.div_ceil(3) + non_ascii
}

pub fn estimate_tool_tokens(tools: &[ToolDefinition]) -> u64 {
    serde_json::to_string(tools)
        .map(|schema| estimate_text_tokens(&schema))
        .unwrap_or_default()
}

/// Assign the model all remaining context for a response, after accounting
/// for input, tool schemas, and provider/message framing. Providers still
/// apply their own documented output limits at the inference boundary.
pub fn available_output_tokens(
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    configured_length: u64,
) -> u64 {
    const FRAMING_RESERVE: u64 = 256;
    configured_length
        .saturating_sub(estimate_tokens(messages))
        .saturating_sub(estimate_tool_tokens(tools))
        .saturating_sub(FRAMING_RESERVE)
}

pub fn inspect(
    messages: &[ChatMessage],
    configured_length: u64,
    model_maximum: u64,
    compacted: bool,
    summary_tokens: u64,
) -> ContextInspection {
    let configured_length = configured_length.min(model_maximum.max(4_096));
    let mut system_tokens = 0;
    let mut runtime_tokens = 0;
    let mut message_tokens = 0;
    let mut reasoning_tokens = 0;
    let mut tool_result_tokens = 0;
    for message in messages {
        let tokens = estimate_tokens(std::slice::from_ref(message));
        match message.name.as_deref() {
            Some("runtime_identity") | Some("context_summary") => runtime_tokens += tokens,
            Some("reasoning") => reasoning_tokens += tokens,
            _ if message.role == "system" => system_tokens += tokens,
            _ if message.role == "tool" => tool_result_tokens += tokens,
            _ => message_tokens += tokens,
        }
    }
    ContextInspection {
        configured_length,
        model_maximum,
        estimated_tokens: system_tokens
            + runtime_tokens
            + message_tokens
            + reasoning_tokens
            + tool_result_tokens,
        system_tokens,
        runtime_tokens,
        message_tokens,
        reasoning_tokens,
        tool_result_tokens,
        tool_schema_tokens: 0,
        reserved_tokens: RESERVED_TOKENS,
        summary_tokens,
        compaction_threshold: (configured_length as f64 * 0.75) as u64,
        emergency_threshold: (configured_length as f64 * 0.90) as u64,
        compacted,
    }
}

/// Keep tool output useful to the next model turn without allowing a command
/// such as `pdftotext` or a recursive directory listing to consume the whole
/// context window. The complete output is still retained by the caller for
/// inspection; this is only the prompt copy.
pub fn prompt_tool_output(output: &str) -> String {
    prompt_tool_output_bounded(output, MAX_TOOL_OUTPUT_BYTES)
}

/// Preserve source evidence in proportion to the selected model's context.
/// One workspace observation uses at most roughly one quarter of the context
/// under the conservative text-token estimate. The full result remains in
/// activity history; only the prompt copy is shortened, so the agent can issue
/// a focused follow-up read if the retained head/tail omitted needed lines.
pub fn prompt_workspace_output(output: &str, configured_length: u64) -> String {
    prompt_tool_output_bounded(output, workspace_output_budget(configured_length))
}

fn workspace_output_budget(configured_length: u64) -> usize {
    (configured_length as usize)
        .saturating_mul(3)
        .div_euclid(4)
        .clamp(2_048, MAX_WORKSPACE_OUTPUT_BYTES)
}

fn prompt_tool_output_bounded(output: &str, max_prompt_bytes: usize) -> String {
    const OMITTED: &str = "\n\n[tool output shortened for the next model turn; the full result remains available in activity history]\n\n";
    if output.len() <= max_prompt_bytes {
        return output.to_string();
    }
    if max_prompt_bytes <= OMITTED.len() + 2 {
        let end = safe_boundary_before(output, max_prompt_bytes);
        return format!("{}…", &output[..end]);
    }
    let content_bytes = max_prompt_bytes - OMITTED.len();
    let head_target = content_bytes / 2;
    let tail_target = content_bytes - head_target;
    let head = safe_boundary_before(output, head_target);
    let tail_start = safe_boundary_after(output, output.len().saturating_sub(tail_target));
    format!("{}{}{}", &output[..head], OMITTED, &output[tail_start..])
}

fn safe_boundary_before(value: &str, target: usize) -> usize {
    let target = target.min(value.len());
    if value.is_char_boundary(target) {
        return target;
    }
    value
        .char_indices()
        .take_while(|(index, _)| *index < target)
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn safe_boundary_after(value: &str, target: usize) -> usize {
    let mut index = target.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub fn compact_messages(
    messages: &[ChatMessage],
    configured_length: u64,
) -> (Vec<ChatMessage>, Option<CompactionResult>) {
    compact_messages_with_extra_tokens(messages, configured_length, 0)
}

pub fn compact_messages_with_extra_tokens(
    messages: &[ChatMessage],
    configured_length: u64,
    extra_prompt_tokens: u64,
) -> (Vec<ChatMessage>, Option<CompactionResult>) {
    let inspection = inspect(messages, configured_length, configured_length, false, 0);
    let threshold = inspection.compaction_threshold;
    let input_budget = threshold
        .saturating_sub(RESERVED_TOKENS)
        .saturating_sub(extra_prompt_tokens);
    if inspection.estimated_tokens <= input_budget {
        return (messages.to_vec(), None);
    }

    // Keep all application/system instructions and merge old compaction
    // summaries into the next short summary instead of duplicating them.
    let retained_system = messages
        .iter()
        .filter(|message| {
            message.role == "system" && message.name.as_deref() != Some("context_summary")
        })
        .cloned()
        .collect::<Vec<_>>();
    let old_summaries = messages
        .iter()
        .filter(|message| message.name.as_deref() == Some("context_summary"))
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    let non_system = messages
        .iter()
        .filter(|message| message.role != "system")
        .collect::<Vec<_>>();
    let last_user = non_system
        .iter()
        .rposition(|message| message.role == "user");
    let mut start = non_system.len().saturating_sub(8);
    if let Some(index) = last_user {
        start = start.max(index);
    }
    while start < non_system.len() && non_system[start].role == "tool" {
        start += 1;
    }

    let mut tool_output_limit = workspace_output_budget(configured_length);
    let (retained, summary, compacted_count) = loop {
        let summary = build_context_summary(&non_system, last_user, start, &old_summaries);
        let mut candidate = retained_system.clone();
        if !summary.is_empty() {
            candidate.push(ChatMessage {
                role: "system".into(),
                content: summary.clone(),
                images: Vec::new(),
                name: Some("context_summary".into()),
                tool_call_id: None,
                tool_calls: None,
                ..Default::default()
            });
        }
        if let Some(index) = last_user {
            if index < start {
                candidate.push(non_system[index].clone());
            }
        }
        candidate.extend(non_system.iter().skip(start).map(|message| {
            let mut message = (*message).clone();
            if message.role == "tool" {
                message.content = prompt_tool_output_bounded(&message.content, tool_output_limit);
            }
            message
        }));
        let compacted_count = non_system.len().saturating_sub(
            candidate
                .iter()
                .filter(|message| message.role != "system")
                .count(),
        );
        let fits = estimate_tokens(&candidate)
            .saturating_add(RESERVED_TOKENS)
            .saturating_add(extra_prompt_tokens)
            <= threshold;
        if fits
            || (tool_output_limit <= MIN_COMPACTED_TOOL_OUTPUT_BYTES && start >= non_system.len())
        {
            break (candidate, summary, compacted_count);
        }
        if tool_output_limit > 512 {
            tool_output_limit = (tool_output_limit / 2).max(MIN_COMPACTED_TOOL_OUTPUT_BYTES);
            continue;
        }
        if start < non_system.len() {
            start += 1;
            while start < non_system.len() && non_system[start].role == "tool" {
                start += 1;
            }
            continue;
        }
        // If the fixed system prompt alone exceeds the budget, preserve it and
        // the latest user request. available_output_tokens will report zero so
        // the caller can return an explicit context error instead of asking
        // the provider for a one-token response.
        break (candidate, summary, compacted_count);
    };
    // Maintain original order: system instructions, merged summary, latest
    // user request, then the retained tool/assistant tail.
    let summary_tokens = estimate_text_tokens(&summary);
    let retained_messages = retained
        .iter()
        .filter(|message| message.role != "system")
        .count();
    let inspection = inspect(
        &retained,
        configured_length,
        configured_length,
        true,
        summary_tokens,
    );
    (
        retained,
        Some(CompactionResult {
            summary,
            retained_messages,
            compacted_messages: compacted_count,
            inspection,
        }),
    )
}

fn build_context_summary(
    messages: &[&ChatMessage],
    last_user: Option<usize>,
    tail_start: usize,
    old_summaries: &[&str],
) -> String {
    let end = last_user.unwrap_or(messages.len()).min(tail_start);
    let mut lines = Vec::new();
    for summary in old_summaries.iter().rev().take(2).rev() {
        let excerpt = summary.chars().take(320).collect::<String>();
        lines.push(format!("Earlier context: {}", excerpt.replace('\n', " ")));
    }
    for message in messages[..end]
        .iter()
        .filter(|message| message.role != "tool")
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let content = message.content.replace('\n', " ");
        let excerpt = content.chars().take(180).collect::<String>();
        if !excerpt.is_empty() {
            lines.push(format!("{}: {}", message.role, excerpt));
        }
    }
    if tail_start > end {
        lines.push("Some older tool output was omitted; reread relevant files if needed.".into());
    }
    let summary = format!(
        "Compacted conversation context. Preserve relevant user requests and decisions:\n{}",
        lines.join("\n")
    );
    summary.chars().take(SUMMARY_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            images: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            ..Default::default()
        }
    }

    #[test]
    fn estimates_and_compacts_old_messages() {
        let mut messages = vec![message("system", "rules")];
        messages.extend(
            (0..30).map(|index| message("user", &format!("message {index} {}", "x".repeat(500)))),
        );
        let (compacted, result) = compact_messages(&messages, 4_096);
        assert!(result.is_some());
        assert!(compacted.len() < messages.len());
        assert_eq!(compacted[0].role, "system");
    }

    #[test]
    fn compaction_bounds_large_tool_history_and_keeps_response_room() {
        let mut messages = vec![message("system", "rules")];
        for index in 0..12 {
            let call_id = format!("call-{index}");
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                images: Vec::new(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![crate::types::ToolCall {
                    id: call_id.clone(),
                    name: "filesystem_read".into(),
                    arguments: serde_json::json!({"path": format!("file-{index}.js")}),
                }]),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: "tool".into(),
                content: "large source output ".repeat(1_200),
                images: Vec::new(),
                name: Some("filesystem_read".into()),
                tool_call_id: Some(call_id),
                tool_calls: None,
                ..Default::default()
            });
        }
        messages.push(message("user", "Fix the note selection bug and verify it."));

        let (compacted, result) = compact_messages_with_extra_tokens(&messages, 4_096, 500);
        assert!(result.is_some());
        assert!(compacted
            .iter()
            .any(|item| item.content == "Fix the note selection bug and verify it."));
        assert!(estimate_tokens(&compacted) + RESERVED_TOKENS + 500 <= 3_072);
        assert!(available_output_tokens(&compacted, &[], 4_096) >= 1_000);

        let non_system = compacted
            .iter()
            .filter(|item| item.role != "system")
            .collect::<Vec<_>>();
        for (index, item) in non_system.iter().enumerate() {
            if item.role != "tool" {
                continue;
            }
            assert!(index > 0);
            let previous = non_system[index - 1];
            assert_eq!(previous.role, "assistant");
            assert!(previous.tool_calls.as_ref().is_some_and(|calls| calls
                .iter()
                .any(|call| Some(&call.id) == item.tool_call_id.as_ref())));
        }
    }

    #[test]
    fn token_estimate_is_conservative_for_ascii_code() {
        assert_eq!(estimate_text_tokens("const x = 1;"), 4);
    }

    #[test]
    fn compaction_never_orphans_tool_results() {
        let mut messages = vec![message("system", "rules")];
        messages.extend(
            (0..7).map(|index| message("user", &format!("filler {index} {}", "y".repeat(1_200)))),
        );
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            images: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![
                crate::types::ToolCall {
                    id: "call-1".into(),
                    name: "filesystem_list".into(),
                    arguments: serde_json::json!({}),
                },
                crate::types::ToolCall {
                    id: "call-2".into(),
                    name: "filesystem_list".into(),
                    arguments: serde_json::json!({}),
                },
            ]),
            ..Default::default()
        });
        for call in ["call-1", "call-2"] {
            messages.push(ChatMessage {
                role: "tool".into(),
                content: format!("result for {call}"),
                images: Vec::new(),
                name: Some("filesystem_list".into()),
                tool_call_id: Some(call.into()),
                tool_calls: None,
                ..Default::default()
            });
        }
        messages.extend((0..6).map(|index| {
            message(
                if index % 2 == 0 { "user" } else { "assistant" },
                &format!("tail {index} {}", "z".repeat(1_200)),
            )
        }));
        let (compacted, result) = compact_messages(&messages, 4_096);
        assert!(result.is_some());
        let non_system: Vec<_> = compacted
            .iter()
            .filter(|item| item.role != "system")
            .collect();
        assert!(!non_system.is_empty());
        assert_ne!(non_system[0].role, "tool");
        for (index, item) in non_system.iter().enumerate() {
            if item.role != "tool" {
                continue;
            }
            let previous = non_system[index - 1];
            assert_eq!(previous.role, "assistant");
            assert!(previous.tool_calls.as_ref().is_some_and(|calls| calls
                .iter()
                .any(|call| Some(&call.id) == item.tool_call_id.as_ref())));
        }
    }

    #[test]
    fn clamps_requested_context() {
        assert_eq!(configured_context(Some(1), Some(8_192)), 4_096);
        assert_eq!(configured_context(Some(64_000), Some(8_192)), 8_192);
    }

    #[test]
    fn bounds_tool_output_without_splitting_utf8() {
        let output = "🙂".repeat(20_000);
        let prompt = prompt_tool_output(&output);
        assert!(prompt.len() < output.len());
        assert!(prompt.len() <= MAX_TOOL_OUTPUT_BYTES);
        assert!(prompt.contains("tool output shortened"));
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn workspace_source_output_scales_with_model_context() {
        let output = "source line\n".repeat(8_000);
        let default_context = prompt_workspace_output(&output, 16_384);
        let large_context = prompt_workspace_output(&output, 65_536);
        assert!(default_context.len() <= 12_288);
        assert!(large_context.len() > default_context.len());
        assert!(large_context.len() <= 49_152);
        assert!(large_context.contains("source line"));
        assert!(default_context.contains("tool output shortened"));
        assert_eq!(workspace_output_budget(4_096), 3_072);
    }

    #[test]
    fn short_workspace_reads_are_not_pruned() {
        let output = "source line\n".repeat(200);
        assert_eq!(prompt_workspace_output(&output, 16_384), output);
    }

    #[test]
    fn output_budget_reports_no_capacity_instead_of_one_token() {
        let messages = vec![message("system", &"large system context ".repeat(2_000))];
        assert_eq!(available_output_tokens(&messages, &[], 4_096), 0);
    }
}
