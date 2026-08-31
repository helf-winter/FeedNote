use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedEvent {
    pub id: String,
    pub raw_content: String,
    pub source_type: String,
    pub processing_status: String,
    pub created_at: i64,
    pub memory_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MemorySummary {
    pub id: String,
    pub memory_type: String,
    pub lifecycle_status: String,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub confidence: f64,
    pub author_type: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryVersion {
    pub id: String,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub confidence: f64,
    pub author_type: String,
    pub model_info: Option<String>,
    pub change_reason: String,
    pub created_at: i64,
    pub source_event_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDetail {
    pub memory: MemorySummary,
    pub versions: Vec<MemoryVersion>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub id: String,
    pub proposed_action: String,
    pub risk_level: String,
    pub reason: String,
    pub status: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_feeds: i64,
    pub total_memories: i64,
    pub pending_reviews: i64,
    pub pending_processing: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub ai_enabled: bool,
    pub llm_endpoint: String,
    pub llm_model: String,
    pub embedding_endpoint: String,
    pub embedding_model: String,
    pub embedding_dimensions: u32,
    pub mobile_push_enabled: bool,
    pub mobile_push_provider: String,
    pub mobile_reminder_minutes: u32,
    pub feishu_sync_enabled: bool,
    pub feishu_task_reminders_enabled: bool,
    pub feishu_source_enabled: bool,
    pub feishu_source_url: String,
    pub feishu_secret_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ai_enabled: true,
            llm_endpoint: "https://open.bigmodel.cn/api/anthropic".to_string(),
            llm_model: "glm-5.3".to_string(),
            embedding_endpoint: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            embedding_model: "embedding-3".to_string(),
            embedding_dimensions: 512,
            mobile_push_enabled: false,
            mobile_push_provider: "ntfy".to_string(),
            mobile_reminder_minutes: 15,
            feishu_sync_enabled: false,
            feishu_task_reminders_enabled: false,
            feishu_source_enabled: false,
            feishu_source_url: String::new(),
            feishu_secret_enabled: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFeedInput {
    pub content: String,
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFeedResult {
    pub feed_id: String,
    pub memory_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteConfirmation {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResult {
    pub status: String,
    pub message: String,
    pub review_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub ai_enabled: bool,
    pub llm_endpoint: String,
    pub llm_model: String,
    pub embedding_endpoint: String,
    pub embedding_model: String,
    pub embedding_dimensions: u32,
    pub mobile_push_enabled: bool,
    pub mobile_push_provider: String,
    pub mobile_reminder_minutes: u32,
    pub feishu_sync_enabled: bool,
    pub feishu_task_reminders_enabled: bool,
    pub feishu_source_enabled: bool,
    pub feishu_source_url: String,
    pub feishu_secret_enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPayload {
    pub title: String,
    pub secret_type: String,
    pub account: Option<String>,
    pub secret_value: String,
    pub website: Option<String>,
    pub notes: Option<String>,
    pub source_title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretItem {
    pub id: String,
    #[serde(flatten)]
    pub payload: SecretPayload,
    pub created_at: i64,
    pub updated_at: i64,
    pub feishu_synced_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSecretInput {
    pub title: String,
    pub secret_type: String,
    pub account: Option<String>,
    pub secret_value: String,
    pub website: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadataProposal {
    pub title: String,
    pub secret_type: String,
    pub account: Option<String>,
    pub website: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub secret_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStashResult {
    pub secret_id: String,
    pub message: String,
    pub undo_until: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuSecretStatus {
    pub enabled: bool,
    pub configured: bool,
    pub spreadsheet_url: Option<String>,
    pub pending_secrets: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VaultMeta {
    pub salt: Vec<u8>,
    pub verifier_nonce: Vec<u8>,
    pub verifier_ciphertext: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct EncryptedSecretRecord {
    pub id: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
    pub feishu_synced_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProposal {
    pub memory_type: String,
    pub title: String,
    pub summary: String,
    pub body: Option<String>,
    pub action: String,
    pub target_memory_id: Option<String>,
    pub relation_type: Option<String>,
    pub confidence: f64,
    pub reason: String,
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanProposal {
    pub title: String,
    pub details: String,
    pub content: String,
    pub link_url: Option<String>,
    pub notes: Option<String>,
    pub scheduled_for: Option<String>,
    pub time_evidence: Option<String>,
    pub needs_clarification: bool,
    pub clarification_question: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRecordProposal {
    pub status: String,
    pub company: String,
    pub role: Option<String>,
    pub link_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRoutingProposal {
    pub create_plan: bool,
    pub plan_confidence: f64,
    pub write_application_record: bool,
    pub application_confidence: f64,
    pub reason: String,
    pub plan: Option<PlanProposal>,
    pub application_record: Option<ApplicationRecordProposal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationWriteResult {
    pub action: String,
    pub row_number: usize,
    pub sheet_title: String,
    pub company: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: String,
    pub feed_event_id: String,
    pub title: String,
    pub details: String,
    pub content: String,
    pub link_url: Option<String>,
    pub notes: Option<String>,
    pub scheduled_at: Option<i64>,
    pub status: String,
    pub clarification_question: Option<String>,
    pub source_title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub reminded_at: Option<i64>,
    pub feishu_synced_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuSheetState {
    pub spreadsheet_token: String,
    pub sheet_id: String,
    pub spreadsheet_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuSyncStatus {
    pub enabled: bool,
    pub configured: bool,
    pub spreadsheet_url: Option<String>,
    pub pending_plans: i64,
    pub last_error: Option<String>,
    pub task_reminders_enabled: bool,
    pub pending_task_reminders: i64,
    pub task_reminder_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeishuPlanTaskMapping {
    pub plan_id: String,
    pub task_guid: String,
    pub task_url: Option<String>,
    pub plan_updated_at: i64,
    pub completed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuSourceStatus {
    pub enabled: bool,
    pub configured: bool,
    pub spreadsheet_url: String,
    pub sheet_title: Option<String>,
    pub total_rows: usize,
    pub actionable_rows: usize,
    pub tracked_rows: usize,
    pub imported_plans: usize,
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCommitResult {
    pub destination: String,
    pub message: String,
    pub plan: Option<PlanItem>,
    pub application_record: Option<ApplicationWriteResult>,
    pub needs_clarification: bool,
}
