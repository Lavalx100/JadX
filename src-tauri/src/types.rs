use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuantizationInfo {
    pub name: Option<String>,
    pub bits_per_weight: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoadedInstance {
    pub id: Option<String>,
    pub context_length: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub reasoning: bool,
    pub trained_for_tool_use: bool,
    #[serde(default)]
    pub image_generation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelInfo {
    pub key: String,
    pub publisher: Option<String>,
    pub display_name: Option<String>,
    pub architecture: Option<String>,
    pub quantization: Option<QuantizationInfo>,
    pub size_bytes: Option<u64>,
    pub params_string: Option<String>,
    pub loaded_instances: Vec<LoadedInstance>,
    pub max_context_length: Option<u64>,
    pub compatibility_type: Option<String>,
    pub state: Option<String>,
    pub selected_variant: Option<String>,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageStats {
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_miss_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub total_ms: Option<u64>,
    pub tokens_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageRecord {
    pub id: String,
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub surface: String,
    pub conversation_id: Option<String>,
    pub turn_id: String,
    pub usage: UsageStats,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageTotals {
    pub requests: u64,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_miss_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub input_metrics_reported_requests: u64,
    pub output_metrics_reported_requests: u64,
    pub total_metrics_reported_requests: u64,
    pub cache_metrics_reported_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageModelBreakdown {
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub surface: String,
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub totals: UsageTotals,
    pub by_model: Vec<UsageModelBreakdown>,
    #[serde(default)]
    pub month_by_model: Vec<UsageModelBreakdown>,
    pub recent: Vec<UsageRecord>,
    #[serde(default)]
    pub hku_requests: Vec<HkuRequestUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HkuRequestUsage {
    pub model: String,
    pub last_minute: u64,
    pub last_day: u64,
    pub last_week: u64,
    pub failed_last_day: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HkuEmbeddingResult {
    pub model: String,
    pub embedding: Vec<f32>,
    pub usage: UsageStats,
    pub transport: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryReport {
    pub total_bytes: u64,
    pub available_bytes: Option<u64>,
    pub pressure: String,
    pub recommended_context: u64,
    pub candidates: Vec<MemoryCandidate>,
    pub selected_context: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryCandidate {
    pub context_length: u64,
    pub accepted: bool,
    pub estimated_bytes: Option<u64>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentSnapshot {
    pub app_ok: bool,
    pub server_running: bool,
    pub server_url: String,
    pub models: Vec<ModelInfo>,
    pub selected_model: Option<String>,
    pub runtime: Option<String>,
    pub memory: MemoryReport,
    pub tools_enabled: usize,
    pub workspaces: Vec<Workspace>,
    pub context: ContextInspection,
    pub security: SecurityPolicy,
    pub model_identity: Option<ModelRuntimeIdentity>,
    pub resources: ResourceTelemetry,
    pub mac: MacDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub images: Vec<ImageInput>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Ephemeral provider continuation data. Never serialize into stored chats or UI events.
    #[serde(skip)]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageInput {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub usage_surface: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<u64>,
    pub profile: Option<String>,
    #[serde(default)]
    pub tools_enabled: bool,
    #[serde(default)]
    pub agent_mode: bool,
    #[serde(default)]
    pub plan_mode: bool,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub context_mode: Option<String>,
    #[serde(default)]
    pub thinking_mode: ThinkingMode,
    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub resource_policy: ResourcePolicy,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchmarkResult {
    pub model: String,
    pub context_length: u64,
    pub ttft_ms: u64,
    pub total_ms: u64,
    pub tokens_per_second: f64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub memory_before: u64,
    pub memory_after: u64,
    pub transport: String,
    pub thinking_mode: ThinkingMode,
    pub reasoning_effort: ReasoningEffort,
    pub resource_policy: ResourcePolicy,
    pub memory_during: Option<u64>,
    pub thermal_state: String,
}

fn default_max_tool_iterations() -> u32 {
    32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionLevel {
    Safe,
    Ask,
    Full,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    #[default]
    ApproveForMe,
    Safe,
    DeveloperUnrestricted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ResourcePolicy {
    #[default]
    Adaptive,
    Performance,
    Cool,
}

impl Default for PermissionLevel {
    fn default() -> Self {
        Self::Ask
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub permission: PermissionLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool_call: ToolCall,
    pub tool: ToolDefinition,
    #[serde(default)]
    pub preconfirmed_term_ids: Vec<String>,
    #[serde(default)]
    pub locally_generated_password_field_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserAuthRequest {
    pub request_id: String,
    pub browser: String,
    pub url: String,
    pub action: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutlookAccount {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub tenant: String,
    pub connected_at: String,
    pub listener_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutlookAuthChallenge {
    pub request_id: String,
    pub verification_uri: String,
    pub user_code: String,
    pub message: String,
    pub expires_in: u64,
    pub browser: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutlookStatus {
    pub account: Option<OutlookAccount>,
    pub connected: bool,
    pub listener_enabled: bool,
    pub folder: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmailCodeNotice {
    pub request_id: String,
    pub sender: String,
    pub subject: String,
    pub received_at: String,
    pub masked_code: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileChange {
    pub path: String,
    pub mode: String,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub preview_title: Option<String>,
    pub recovery_snapshot: Option<String>,
    #[serde(default)]
    pub diff_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerationEvent {
    pub request_id: String,
    pub kind: String,
    pub delta: Option<String>,
    pub text: Option<String>,
    pub message: Option<String>,
    pub usage: Option<UsageStats>,
    #[serde(default)]
    pub run_metrics: Option<AgentRunMetrics>,
    pub tool_call: Option<ToolCall>,
    pub approval: Option<ApprovalRequest>,
    pub transport: Option<String>,
    pub reasoning: Option<String>,
    pub context: Option<ContextInspection>,
    pub security: Option<SecurityEvent>,
    #[serde(default)]
    pub browser_auth: Option<BrowserAuthRequest>,
    #[serde(default)]
    pub outlook_auth: Option<OutlookAuthChallenge>,
    #[serde(default)]
    pub email_code: Option<EmailCodeNotice>,
    pub model_identity: Option<ModelRuntimeIdentity>,
    pub resources: Option<ResourceTelemetry>,
    #[serde(default)]
    pub plan: Option<RefactorPlan>,
    #[serde(default)]
    pub file_changes: Vec<FileChange>,
    #[serde(default)]
    pub images: Vec<ImageInput>,
    #[serde(default)]
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub images: Vec<ImageInput>,
    pub created_at: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub usage: Option<UsageStats>,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub exists: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextConfig {
    pub mode: String,
    pub requested_length: Option<u64>,
    pub compaction_threshold: f64,
    pub emergency_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextInspection {
    pub configured_length: u64,
    pub model_maximum: u64,
    pub estimated_tokens: u64,
    #[serde(default)]
    pub system_tokens: u64,
    #[serde(default)]
    pub runtime_tokens: u64,
    #[serde(default)]
    pub message_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub tool_result_tokens: u64,
    #[serde(default)]
    pub tool_schema_tokens: u64,
    pub reserved_tokens: u64,
    pub summary_tokens: u64,
    pub compaction_threshold: u64,
    pub emergency_threshold: u64,
    pub compacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityPolicy {
    pub profile: String,
    pub shell_enabled: bool,
    pub python_enabled: bool,
    pub network_enabled: bool,
    pub os_isolation_available: bool,
    pub session_approvals: bool,
    pub execution_mode: ExecutionMode,
    pub developer_unrestricted: bool,
    pub active_processes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityEvent {
    pub tool: String,
    pub action: String,
    pub decision: String,
    pub reason: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityAuditRecord {
    pub id: String,
    pub event: SecurityEvent,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRuntimeIdentity {
    pub provider: String,
    pub key: String,
    pub display_name: Option<String>,
    pub architecture: Option<String>,
    pub quantization: Option<String>,
    pub variant: Option<String>,
    pub context_length: u64,
    pub max_context_length: u64,
    pub thinking_mode: ThinkingMode,
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceTelemetry {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: Option<u64>,
    pub memory_pressure: String,
    pub swap_used_bytes: Option<u64>,
    pub process_cpu_percent: Option<f64>,
    pub system_cpu_percent: Option<f64>,
    pub process_memory_bytes: Option<u64>,
    #[serde(default)]
    pub provider_cpu_percent: Option<f64>,
    #[serde(default)]
    pub provider_memory_bytes: Option<u64>,
    #[serde(default)]
    pub provider_processes: u32,
    #[serde(default)]
    pub top_processes: Vec<ProcessResourceUsage>,
    pub thermal_state: String,
    pub load_average: Option<f64>,
    pub headroom_target_bytes: u64,
    pub emergency_floor_bytes: u64,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessResourceUsage {
    pub name: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub is_harness: bool,
    pub is_provider: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MacDetails {
    pub product_name: Option<String>,
    pub model_identifier: Option<String>,
    pub chip: Option<String>,
    pub os_version: Option<String>,
    pub architecture: String,
    pub total_memory_bytes: u64,
    pub uptime_seconds: Option<u64>,
    pub battery_percent: Option<f64>,
    pub battery_charging: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveSystemSnapshot {
    pub resources: ResourceTelemetry,
    pub mac: MacDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompactionResult {
    pub summary: String,
    pub retained_messages: usize,
    pub compacted_messages: usize,
    pub inspection: ContextInspection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PlanStatus {
    #[default]
    Draft,
    AwaitingApproval,
    Approved,
    Executing,
    Completed,
    Rejected,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub verification: Vec<String>,
    #[serde(default)]
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RefactorPlan {
    pub id: String,
    pub conversation_id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    #[serde(default)]
    pub status: PlanStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceMetadata {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub modified_at: String,
    pub ignored: bool,
    pub git_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspacePage {
    pub entries: Vec<WorkspaceMetadata>,
    pub next_cursor: Option<String>,
    pub scanned: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationSuggestion {
    pub command: String,
    pub reason: String,
    #[serde(default)]
    pub required_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationResult {
    pub command: String,
    pub status: String,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub duration_ms: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRunMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub repair_attempts: u64,
    pub changed_files: u64,
    pub checks_passed: u64,
    pub checks_failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceFileChange {
    pub path: String,
    pub mode: String,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub before_fingerprint: Option<String>,
    pub after_fingerprint: Option<String>,
    pub recovery_snapshot: Option<String>,
    pub diff_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceChangeSet {
    pub turn_id: String,
    #[serde(default)]
    pub files: Vec<WorkspaceFileChange>,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub recovery_available: bool,
    #[serde(default)]
    pub verification: Vec<VerificationSuggestion>,
    #[serde(default)]
    pub validation_results: Vec<VerificationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolArgumentRepair {
    pub tool_name: String,
    pub original: String,
    pub normalized: Value,
    pub reason: String,
    #[serde(default)]
    pub rejected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionEventRecord {
    pub id: String,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ThinkingMode {
    #[default]
    Off,
    On,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ReasoningEffort {
    #[default]
    Low,
    Medium,
    High,
    XHigh,
}
