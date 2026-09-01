use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

use chrono::{FixedOffset, TimeZone, Utc};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{
    db::Database,
    error::{AppError, AppResult},
    models::{
        ApplicationRecordProposal, ApplicationWriteResult, FeishuMemoStatus, FeishuPlanTaskMapping,
        FeishuSecretStatus, FeishuSheetState, FeishuSourceStatus, FeishuSyncStatus, MemoItem,
        PlanItem, SecretItem,
    },
    secrets::parse_env,
    vault::Vault,
};

const API_BASE: &str = "https://open.feishu.cn/open-apis";
const SHEET_TITLE: &str = "FeedNote 计划";
const MEMO_SHEET_TITLE: &str = "FeedNote 备忘录";
const SECRET_SHEET_TITLE: &str = "FeedNote 秘密";
const TASK_REMINDER_MINUTES: i64 = 180;
const HEADERS: [&str; 9] = [
    "本地计划ID",
    "状态",
    "时间",
    "标题",
    "内容",
    "链接",
    "注意事项",
    "来源",
    "更新时间",
];
const TASK_UPDATE_FIELDS: [&str; 5] = ["summary", "description", "start", "due", "completed_at"];
const SECRET_HEADERS: [&str; 8] = [
    "本地秘密ID",
    "类型",
    "名称",
    "账号",
    "秘密值（明文）",
    "网站",
    "备注",
    "更新时间",
];
const MEMO_HEADERS: [&str; 4] = ["本地备忘ID", "内容", "来源", "记录时间"];

struct MemoSheetRow {
    row_number: usize,
    cells: Vec<String>,
}

struct CachedTenantToken {
    app_id: String,
    token: String,
    expires_at: i64,
}

static TENANT_TOKEN_CACHE: OnceLock<Mutex<Option<CachedTenantToken>>> = OnceLock::new();
static FEISHU_CLIENT: OnceLock<Client> = OnceLock::new();

struct FeishuSecrets {
    app_id: String,
    app_secret: String,
    task_assignee_id: Option<String>,
    task_assignee_id_type: Option<String>,
}

impl FeishuSecrets {
    fn load(path: &Path) -> AppResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            AppError::FeishuUnavailable(format!("无法读取 {}：{}", path.display(), error))
        })?;
        let values = parse_env(&content);
        let app_id = required_secret(&values, "FEISHU_APP_ID")?;
        let app_secret = required_secret(&values, "FEISHU_APP_SECRET")?;
        if !app_id.starts_with("cli_") {
            return Err(AppError::FeishuUnavailable(
                "FEISHU_APP_ID 格式无效".to_string(),
            ));
        }
        let task_assignee_id = values
            .get("FEISHU_TASK_ASSIGNEE_ID")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let task_assignee_id_type = values
            .get("FEISHU_TASK_ASSIGNEE_ID_TYPE")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Ok(Self {
            app_id,
            app_secret,
            task_assignee_id,
            task_assignee_id_type,
        })
    }

    fn is_configured(path: &Path) -> bool {
        Self::load(path).is_ok()
    }
}

pub fn start_scheduler(
    database: Arc<Database>,
    secrets_path: std::path::PathBuf,
    syncing: Arc<AtomicBool>,
    app: AppHandle,
    vault: Arc<Vault>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let settings = match database.get_settings() {
                Ok(settings) => settings,
                Err(_) => continue,
            };
            let cleanup_pending = database
                .list_feishu_plan_cleanup_ids()
                .is_ok_and(|plan_ids| !plan_ids.is_empty());
            let memo_pending = database
                .count_pending_feishu_memos()
                .is_ok_and(|pending| pending > 0);
            if settings.feishu_sync_enabled
                || settings.feishu_task_reminders_enabled
                || settings.feishu_secret_enabled
                || memo_pending
                || cleanup_pending
            {
                if let Some(_guard) = SyncGuard::acquire(&syncing) {
                    if let Err(error) = cleanup_legacy_plan_rows(&database, &secrets_path).await {
                        let _ = database.save_feishu_sync_error(Some(&error.to_string()));
                        continue;
                    }
                    if settings.feishu_sync_enabled {
                        match sync_pending(&database, &secrets_path, &app).await {
                            Ok(_) => {
                                let _ = database.save_feishu_sync_error(None);
                            }
                            Err(error) => {
                                let _ = database.save_feishu_sync_error(Some(&error.to_string()));
                            }
                        }
                    }
                    if settings.feishu_task_reminders_enabled {
                        match sync_plan_tasks(&database, &secrets_path, &app).await {
                            Ok(_) => {
                                let _ = database.save_feishu_task_sync_error(None);
                            }
                            Err(error) => {
                                let _ =
                                    database.save_feishu_task_sync_error(Some(&error.to_string()));
                            }
                        }
                    }
                    if settings.feishu_secret_enabled
                        && vault.status(&database).is_ok_and(|status| status.unlocked)
                    {
                        match sync_secret_pending(&database, &secrets_path, &vault).await {
                            Ok(_) => {
                                let _ = database.save_feishu_secret_sync_error(None);
                            }
                            Err(error) => {
                                let _ = database
                                    .save_feishu_secret_sync_error(Some(&error.to_string()));
                            }
                        }
                    }
                    if memo_pending {
                        match sync_memo_pending(&database, &secrets_path).await {
                            Ok(_) => {
                                let _ = database.save_feishu_memo_sync_error(None);
                            }
                            Err(error) => {
                                let _ =
                                    database.save_feishu_memo_sync_error(Some(&error.to_string()));
                            }
                        }
                    }
                }
            }
        }
    });
}

pub fn status(database: &Database, secrets_path: &Path) -> AppResult<FeishuSyncStatus> {
    let settings = database.get_settings()?;
    let state = database.get_feishu_sheet_state()?;
    Ok(FeishuSyncStatus {
        enabled: settings.feishu_sync_enabled,
        configured: FeishuSecrets::is_configured(secrets_path),
        spreadsheet_url: state.map(|state| state.spreadsheet_url),
        pending_plans: database.count_pending_feishu_plans()?,
        last_error: database.get_feishu_sync_error()?,
        task_reminders_enabled: settings.feishu_task_reminders_enabled,
        pending_task_reminders: database.count_pending_feishu_plan_tasks()?,
        task_reminder_error: database.get_feishu_task_sync_error()?,
    })
}

pub fn secret_status(database: &Database, secrets_path: &Path) -> AppResult<FeishuSecretStatus> {
    let settings = database.get_settings()?;
    let state = database.get_feishu_secret_sheet_state()?;
    Ok(FeishuSecretStatus {
        enabled: settings.feishu_secret_enabled,
        configured: FeishuSecrets::is_configured(secrets_path),
        spreadsheet_url: state.map(|state| state.spreadsheet_url),
        pending_secrets: database.count_pending_feishu_secrets()?,
        last_error: database.get_feishu_secret_sync_error()?,
    })
}

pub fn memo_status(database: &Database, secrets_path: &Path) -> AppResult<FeishuMemoStatus> {
    let state = database.get_feishu_memo_sheet_state()?;
    Ok(FeishuMemoStatus {
        configured: FeishuSecrets::is_configured(secrets_path),
        spreadsheet_url: state.map(|state| state.spreadsheet_url),
        pending_memos: database.count_pending_feishu_memos()?,
        last_error: database.get_feishu_memo_sync_error()?,
    })
}

pub async fn sync_memos_now(
    database: &Database,
    secrets_path: &Path,
    syncing: &AtomicBool,
) -> AppResult<String> {
    let _guard = SyncGuard::acquire(syncing)
        .ok_or_else(|| AppError::FeishuUnavailable("同步正在进行，请稍后查看状态".to_string()))?;
    let synced = match sync_memo_pending(database, secrets_path).await {
        Ok(synced) => {
            database.save_feishu_memo_sync_error(None)?;
            synced
        }
        Err(error) => {
            database.save_feishu_memo_sync_error(Some(&error.to_string()))?;
            return Err(error);
        }
    };
    Ok(if synced == 0 {
        "飞书备忘录已同步".to_string()
    } else {
        format!("已同步 {synced} 条备忘录到飞书")
    })
}

pub async fn sync_secrets_now(
    database: &Database,
    secrets_path: &Path,
    syncing: &AtomicBool,
    vault: &Vault,
) -> AppResult<String> {
    let _guard = SyncGuard::acquire(syncing)
        .ok_or_else(|| AppError::FeishuUnavailable("同步正在进行，请稍后查看状态".to_string()))?;
    if !database.get_settings()?.feishu_secret_enabled {
        return Err(AppError::Validation("飞书秘密表同步尚未开启".to_string()));
    }
    let synced = match sync_secret_pending(database, secrets_path, vault).await {
        Ok(synced) => {
            database.save_feishu_secret_sync_error(None)?;
            synced
        }
        Err(error) => {
            database.save_feishu_secret_sync_error(Some(&error.to_string()))?;
            return Err(error);
        }
    };
    Ok(if synced == 0 {
        "飞书秘密表已同步".to_string()
    } else {
        format!("已同步 {synced} 条秘密记录到飞书明文表")
    })
}

pub async fn sync_now(
    database: &Database,
    secrets_path: &Path,
    syncing: &AtomicBool,
    app: &AppHandle,
) -> AppResult<String> {
    let _guard = SyncGuard::acquire(syncing)
        .ok_or_else(|| AppError::FeishuUnavailable("同步正在进行，请稍后查看状态".to_string()))?;
    cleanup_legacy_plan_rows(database, secrets_path).await?;
    let settings = database.get_settings()?;
    let synced = if settings.feishu_sync_enabled {
        match sync_pending(database, secrets_path, app).await {
            Ok(synced) => {
                database.save_feishu_sync_error(None)?;
                synced
            }
            Err(error) => {
                database.save_feishu_sync_error(Some(&error.to_string()))?;
                return Err(error);
            }
        }
    } else {
        0
    };
    let task_synced = if settings.feishu_task_reminders_enabled {
        match sync_plan_tasks(database, secrets_path, app).await {
            Ok(synced) => {
                database.save_feishu_task_sync_error(None)?;
                synced
            }
            Err(error) => {
                database.save_feishu_task_sync_error(Some(&error.to_string()))?;
                return Err(error);
            }
        }
    } else {
        0
    };
    Ok(if synced == 0 && task_synced == 0 {
        "飞书计划表和待办提醒均已同步".to_string()
    } else if task_synced == 0 {
        format!("已同步 {synced} 条计划到飞书表格")
    } else if synced == 0 {
        format!("已同步 {task_synced} 条飞书待办提醒")
    } else {
        format!("已同步 {synced} 条计划和 {task_synced} 条飞书待办提醒")
    })
}

pub fn source_status(database: &Database, secrets_path: &Path) -> AppResult<FeishuSourceStatus> {
    let settings = database.get_settings()?;
    Ok(FeishuSourceStatus {
        enabled: settings.feishu_source_enabled,
        configured: FeishuSecrets::is_configured(secrets_path)
            && !settings.feishu_source_url.trim().is_empty(),
        spreadsheet_url: settings.feishu_source_url,
        sheet_title: None,
        total_rows: 0,
        actionable_rows: 0,
        tracked_rows: 0,
        imported_plans: 0,
        last_sync_at: None,
        last_error: None,
    })
}

struct SyncGuard<'a>(&'a AtomicBool);

impl<'a> SyncGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for SyncGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct SourceRow {
    row_number: usize,
    status: String,
    company: String,
    role: String,
}

#[derive(Debug)]
struct SourceSheet {
    sheet_id: String,
    title: String,
    row_count: u64,
    rows: Vec<SourceRow>,
}

#[derive(Debug, Clone)]
struct PlanSheetRow {
    row_number: usize,
    completed: Option<bool>,
}

pub async fn write_application_record(
    database: &Database,
    secrets_path: &Path,
    proposal: &ApplicationRecordProposal,
) -> AppResult<ApplicationWriteResult> {
    validate_application_record(proposal)?;
    let settings = database.get_settings()?;
    if !settings.feishu_source_enabled {
        return Err(AppError::Validation(
            "已识别为投递记录，但飞书投递记录写入尚未开启".to_string(),
        ));
    }
    if settings.feishu_source_url.trim().is_empty() {
        return Err(AppError::Validation(
            "已识别为投递记录，但尚未配置目标飞书表格".to_string(),
        ));
    }
    crate::db::validate_feishu_source_url(&settings.feishu_source_url)?;

    let spreadsheet_token = source_token(&settings.feishu_source_url)?;
    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let source = read_source_sheet(&client, &token, &spreadsheet_token).await?;
    let existing = source
        .rows
        .iter()
        .find(|row| application_row_matches(row, proposal));

    let state = FeishuSheetState {
        spreadsheet_token: spreadsheet_token.clone(),
        sheet_id: source.sheet_id.clone(),
        spreadsheet_url: settings.feishu_source_url,
    };
    let (action, expected_row) = if let Some(row) = existing {
        write_values(
            &client,
            &token,
            &state,
            &format!("A{}:A{}", row.row_number, row.row_number),
            vec![application_status_cell(proposal)],
        )
        .await?;
        ("updated", row.row_number)
    } else {
        let fallback_row = source
            .rows
            .iter()
            .map(|row| row.row_number)
            .max()
            .unwrap_or(1)
            + 1;
        let row = application_row(proposal);
        let row_number = append_application_values(&client, &token, &state, vec![row])
            .await?
            .unwrap_or(fallback_row);
        ("created", row_number)
    };

    let verified_rows = read_source_rows(
        &client,
        &token,
        &spreadsheet_token,
        &source.sheet_id,
        source.row_count.max(expected_row as u64),
    )
    .await?;
    let verified_row = verified_rows
        .iter()
        .find(|row| {
            row.row_number == expected_row
                && application_row_matches(row, proposal)
                && row.status == proposal.status.trim()
        })
        .ok_or_else(|| {
            AppError::FeishuUnavailable("投递记录写入后回读校验失败，请重试".to_string())
        })?;
    Ok(ApplicationWriteResult {
        action: action.to_string(),
        row_number: if action == "updated" {
            expected_row
        } else {
            verified_row.row_number
        },
        sheet_title: source.title,
        company: proposal.company.trim().to_string(),
        role: proposal
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

pub async fn check_application_target(
    database: &Database,
    secrets_path: &Path,
) -> AppResult<String> {
    let settings = database.get_settings()?;
    if settings.feishu_source_url.trim().is_empty() {
        return Err(AppError::Validation(
            "请先填写飞书投递记录表链接".to_string(),
        ));
    }
    crate::db::validate_feishu_source_url(&settings.feishu_source_url)?;
    let spreadsheet_token = source_token(&settings.feishu_source_url)?;
    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let source = read_source_sheet(&client, &token, &spreadsheet_token).await?;
    Ok(format!(
        "已连接‘{}’，当前有 {} 条投递记录",
        source.title,
        source.rows.len()
    ))
}

async fn read_source_sheet(
    client: &Client,
    token: &str,
    spreadsheet_token: &str,
) -> AppResult<SourceSheet> {
    let response = client
        .get(format!(
            "{API_BASE}/sheets/v3/spreadsheets/{spreadsheet_token}/sheets/query"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let sheets = payload["data"]["sheets"]
        .as_array()
        .ok_or_else(|| AppError::FeishuUnavailable("来源表没有工作表".to_string()))?;
    let sheet = sheets
        .iter()
        .find(|sheet| sheet["title"].as_str() == Some("投递记录"))
        .or_else(|| sheets.first())
        .ok_or_else(|| AppError::FeishuUnavailable("来源表没有工作表".to_string()))?;
    let sheet_id = string_field(sheet, "sheet_id")?;
    let title = string_field(sheet, "title")?;
    let row_count = sheet["grid_properties"]["row_count"]
        .as_u64()
        .unwrap_or(200)
        .clamp(1, 5_000);
    let rows = read_source_rows(client, token, spreadsheet_token, &sheet_id, row_count).await?;
    Ok(SourceSheet {
        sheet_id,
        title,
        row_count,
        rows,
    })
}

async fn read_source_rows(
    client: &Client,
    token: &str,
    spreadsheet_token: &str,
    sheet_id: &str,
    row_count: u64,
) -> AppResult<Vec<SourceRow>> {
    let range = format!("{sheet_id}!A1:E{row_count}");
    let mut url = Url::parse(&format!(
        "{API_BASE}/sheets/v2/spreadsheets/{spreadsheet_token}/values/"
    ))
    .map_err(|_| AppError::FeishuUnavailable("飞书来源读取地址无效".to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::FeishuUnavailable("飞书来源读取地址无效".to_string()))?
        .pop_if_empty()
        .push(&range);
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let values = payload["data"]["valueRange"]["values"]
        .as_array()
        .ok_or_else(|| AppError::FeishuUnavailable("来源表没有可读取的数据".to_string()))?;
    let headers = values
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::FeishuUnavailable("来源表缺少表头".to_string()))?;
    let column = |name: &str| {
        headers
            .iter()
            .position(|cell| cell_text(cell).trim() == name)
            .ok_or_else(|| AppError::FeishuUnavailable(format!("来源表缺少‘{name}’列")))
    };
    let status_col = column("状态")?;
    let company_col = column("公司/事项")?;
    let role_col = column("岗位/方向")?;
    column("链接")?;
    column("备注")?;
    let mut rows = Vec::new();
    for (index, value) in values.iter().enumerate().skip(1) {
        let Some(cells) = value.as_array() else {
            continue;
        };
        let get = |column: usize| cells.get(column).map(cell_text).unwrap_or_default();
        let status = get(status_col).trim().to_string();
        let company = get(company_col).trim().to_string();
        if company.is_empty() {
            continue;
        }
        let role = get(role_col).trim().to_string();
        rows.push(SourceRow {
            row_number: index + 1,
            status,
            company,
            role,
        });
    }
    Ok(rows)
}

fn validate_application_record(proposal: &ApplicationRecordProposal) -> AppResult<()> {
    const STATUSES: [&str; 10] = [
        "待投递",
        "简历筛选",
        "待笔试",
        "待AI面",
        "待一面",
        "待二面",
        "待三面",
        "待HR面",
        "已挂",
        "Offer",
    ];
    if !STATUSES.contains(&proposal.status.trim()) {
        return Err(AppError::AiInvalid("投递状态不在允许范围内".to_string()));
    }
    let company = proposal.company.trim();
    if company.is_empty() || company.chars().count() > 200 {
        return Err(AppError::AiInvalid("公司/事项为空或过长".to_string()));
    }
    let role = proposal.role.as_deref().map(str::trim).unwrap_or_default();
    if role.is_empty() || role.chars().count() > 200 {
        return Err(AppError::AiInvalid("岗位/方向为空或过长".to_string()));
    }
    if proposal
        .notes
        .as_deref()
        .is_some_and(|value| value.chars().count() > 2_000)
    {
        return Err(AppError::AiInvalid("投递备注过长".to_string()));
    }
    if let Some(link) = proposal.link_url.as_deref() {
        let url = Url::parse(link.trim())
            .map_err(|_| AppError::AiInvalid("投递链接不是有效 URL".to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AppError::AiInvalid("投递链接只允许 HTTP/HTTPS".to_string()));
        }
    }
    Ok(())
}

fn application_row_matches(row: &SourceRow, proposal: &ApplicationRecordProposal) -> bool {
    let Some(proposed_role) = proposal.role.as_deref() else {
        return false;
    };
    company_matches(&row.company, &proposal.company) && role_matches(&row.role, proposed_role)
}

fn company_matches(existing: &str, incoming: &str) -> bool {
    let existing = normalize_company(existing);
    let incoming = normalize_company(incoming);
    if existing == incoming {
        return true;
    }
    company_alias(&existing, &incoming) || company_alias(&incoming, &existing)
}

fn company_alias(shorter: &str, longer: &str) -> bool {
    if shorter.chars().count() < 4 {
        return false;
    }
    let Some(remainder) = longer.strip_prefix(shorter) else {
        return false;
    };
    matches!(
        remainder,
        "科技" | "集团" | "科技集团" | "网络" | "信息技术"
    )
}

fn normalize_company(value: &str) -> String {
    let mut value: String = value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect();
    for suffix in ["股份有限公司", "有限责任公司", "有限公司", "公司"] {
        if let Some(stripped) = value.strip_suffix(suffix) {
            value = stripped.to_string();
            break;
        }
    }
    value
}

fn normalize_role(value: &str) -> String {
    let mut value: String = value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect();
    for suffix in ["岗位", "职位", "岗"] {
        if let Some(stripped) = value.strip_suffix(suffix) {
            value = stripped.to_string();
            break;
        }
    }
    value
}

fn role_matches(existing: &str, incoming: &str) -> bool {
    let existing = normalize_role(existing);
    let incoming = normalize_role(incoming);
    if existing.is_empty() || incoming.is_empty() {
        return false;
    }
    existing == incoming || role_alias(&existing) == role_alias(&incoming)
}

fn role_alias(value: &str) -> &str {
    match value {
        "前端"
        | "前端开发"
        | "前端研发"
        | "前端工程师"
        | "前端开发工程师"
        | "前端研发工程师"
        | "web前端"
        | "web前端开发"
        | "web前端工程师"
        | "web前端开发工程师" => "前端",
        "后端" | "后端开发" | "后端研发" | "后端工程师" | "后端开发工程师" | "后端研发工程师" => {
            "后端"
        }
        _ => value,
    }
}

fn application_row(proposal: &ApplicationRecordProposal) -> Vec<Value> {
    [
        proposal.status.trim().to_string(),
        proposal.company.trim().to_string(),
        proposal
            .role
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        proposal
            .link_url
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        proposal
            .notes
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
    ]
    .into_iter()
    .map(|value| json!(safe_cell(&value)))
    .collect()
}

fn application_status_cell(proposal: &ApplicationRecordProposal) -> Vec<Value> {
    vec![json!(safe_cell(proposal.status.trim()))]
}

async fn append_application_values(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    values: Vec<Vec<Value>>,
) -> AppResult<Option<usize>> {
    let response = client
        .post(format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_append?insertDataOption=INSERT_ROWS",
            state.spreadsheet_token
        ))
        .bearer_auth(token)
        .json(&json!({
            "valueRange": {
                "range": format!("{}!A:E", state.sheet_id),
                "values": values,
            }
        }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let updated_range = payload
        .pointer("/data/updates/updatedRange")
        .or_else(|| payload.pointer("/data/updates/updated_range"))
        .and_then(Value::as_str);
    Ok(updated_range.and_then(last_row_from_range))
}

fn last_row_from_range(range: &str) -> Option<usize> {
    range
        .rsplit_once(':')
        .map(|(_, end)| end)
        .unwrap_or(range)
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn cell_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Array(values) => values
            .iter()
            .filter_map(|value| {
                value["link"]
                    .as_str()
                    .or_else(|| value["text"].as_str())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(_) => value["link"]
            .as_str()
            .or_else(|| value["text"].as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

fn source_token(value: &str) -> AppResult<String> {
    let url = Url::parse(value.trim())
        .map_err(|_| AppError::Validation("飞书来源链接无效".to_string()))?;
    let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
    segments
        .windows(2)
        .find(|parts| parts[0] == "sheets")
        .map(|parts| parts[1].to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| AppError::Validation("飞书来源链接缺少表格 token".to_string()))
}

struct TaskAssignee {
    id: String,
    id_type: String,
}

async fn sync_plan_tasks(
    database: &Database,
    secrets_path: &Path,
    app: &AppHandle,
) -> AppResult<usize> {
    let initial_mappings = database.list_feishu_plan_task_mappings()?;
    if database.count_pending_feishu_plan_tasks()? == 0 && initial_mappings.is_empty() {
        return Ok(0);
    }
    let settings = database.get_settings()?;
    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let assignee = resolve_task_assignee(&client, &token, &settings, &secrets).await?;
    let mut synced = pull_plan_task_statuses(
        database,
        &client,
        &token,
        &assignee.id_type,
        &initial_mappings,
        app,
    )
    .await?;
    let plans = database.list_plans(true)?;
    let mappings = database.list_feishu_plan_task_mappings()?;
    let plans_by_id: HashMap<&str, &PlanItem> =
        plans.iter().map(|plan| (plan.id.as_str(), plan)).collect();
    let mappings_by_plan: HashMap<&str, &FeishuPlanTaskMapping> = mappings
        .iter()
        .map(|mapping| (mapping.plan_id.as_str(), mapping))
        .collect();
    for mapping in &mappings {
        if plans_by_id.contains_key(mapping.plan_id.as_str()) {
            continue;
        }
        complete_task(&client, &token, &assignee.id_type, &mapping.task_guid).await?;
        database.delete_feishu_plan_task_mapping(&mapping.plan_id)?;
        synced += 1;
    }

    for plan in &plans {
        if plan.scheduled_at.is_none() {
            continue;
        }
        let mapping = mappings_by_plan.get(plan.id.as_str()).copied();
        if plan.status == "done" && mapping.is_none() {
            continue;
        }
        let completed = plan.status == "done";
        if let Some(mapping) = mapping {
            if mapping.plan_updated_at >= plan.updated_at && mapping.completed == completed {
                continue;
            }
            let task_url = patch_task(
                &client,
                &token,
                &assignee.id_type,
                &mapping.task_guid,
                plan,
                completed,
            )
            .await?
            .or_else(|| mapping.task_url.clone());
            database.save_feishu_plan_task_mapping(&FeishuPlanTaskMapping {
                plan_id: plan.id.clone(),
                task_guid: mapping.task_guid.clone(),
                task_url,
                plan_updated_at: plan.updated_at,
                completed,
            })?;
            synced += 1;
        } else {
            let mapping = create_task(&client, &token, &assignee, plan).await?;
            database.save_feishu_plan_task_mapping(&mapping)?;
            synced += 1;
        }
    }
    Ok(synced)
}

struct RemoteTaskStatus {
    completed: bool,
    title: Option<String>,
    scheduled_at: Option<i64>,
    task_url: Option<String>,
}

async fn pull_plan_task_statuses(
    database: &Database,
    client: &Client,
    token: &str,
    user_id_type: &str,
    mappings: &[FeishuPlanTaskMapping],
    app: &AppHandle,
) -> AppResult<usize> {
    let plans = database.list_plans(true)?;
    let plans_by_id: HashMap<&str, &PlanItem> =
        plans.iter().map(|plan| (plan.id.as_str(), plan)).collect();
    let mut changed = 0;
    for mapping in mappings {
        let Some(plan) = plans_by_id.get(mapping.plan_id.as_str()).copied() else {
            continue;
        };
        if plan.updated_at > mapping.plan_updated_at {
            continue;
        }
        let remote = read_task_status(client, token, user_id_type, &mapping.task_guid).await?;
        let local_completed = plan.status == "done";
        let remote_title_changed = remote
            .title
            .as_deref()
            .is_some_and(|title| title != plan.title);
        let remote_time_changed = remote
            .scheduled_at
            .is_some_and(|scheduled_at| Some(scheduled_at) != plan.scheduled_at);
        if remote.completed == local_completed && !remote_title_changed && !remote_time_changed {
            if mapping.completed != remote.completed || mapping.plan_updated_at < plan.updated_at {
                database.save_feishu_plan_task_mapping(&FeishuPlanTaskMapping {
                    plan_id: plan.id.clone(),
                    task_guid: mapping.task_guid.clone(),
                    task_url: remote.task_url.or_else(|| mapping.task_url.clone()),
                    plan_updated_at: plan.updated_at,
                    completed: remote.completed,
                })?;
            }
            continue;
        }
        if let Some(updated) = database.apply_remote_plan_task_update(
            &plan.id,
            remote.title.as_deref(),
            remote.scheduled_at,
            remote.completed,
            plan.updated_at,
        )? {
            database.save_feishu_plan_task_mapping(&FeishuPlanTaskMapping {
                plan_id: plan.id.clone(),
                task_guid: mapping.task_guid.clone(),
                task_url: remote.task_url.or_else(|| mapping.task_url.clone()),
                plan_updated_at: updated.updated_at,
                completed: remote.completed,
            })?;
            let _ = app.emit("plans-changed", &updated);
            changed += 1;
        }
    }
    Ok(changed)
}

async fn read_task_status(
    client: &Client,
    token: &str,
    user_id_type: &str,
    task_guid: &str,
) -> AppResult<RemoteTaskStatus> {
    let response = client
        .get(format!("{API_BASE}/task/v2/tasks/{task_guid}"))
        .bearer_auth(token)
        .query(&[("user_id_type", user_id_type)])
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let task = &payload["data"]["task"];
    Ok(parse_remote_task(task))
}

fn parse_remote_task(task: &Value) -> RemoteTaskStatus {
    RemoteTaskStatus {
        completed: task_completed(task),
        title: task["summary"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= 80)
            .map(str::to_string),
        scheduled_at: task_time(&task["start"]).or_else(|| task_time(&task["due"])),
        task_url: task["url"].as_str().map(str::to_string),
    }
}

fn task_time(value: &Value) -> Option<i64> {
    if value["is_all_day"].as_bool() == Some(true) {
        return None;
    }
    let timestamp = value["timestamp"]
        .as_str()
        .and_then(|raw| raw.parse::<i64>().ok())
        .or_else(|| value["timestamp"].as_i64())?;
    let timestamp = if timestamp < 100_000_000_000 {
        timestamp.checked_mul(1_000)?
    } else {
        timestamp
    };
    (946_684_800_000..=4_102_444_800_000)
        .contains(&timestamp)
        .then_some(timestamp)
}

fn task_completed(task: &Value) -> bool {
    task["completed_at"]
        .as_str()
        .unwrap_or_default()
        .parse::<i64>()
        .is_ok_and(|timestamp| timestamp > 0)
        || task["status"].as_str() == Some("done")
}

async fn resolve_task_assignee(
    client: &Client,
    token: &str,
    settings: &crate::models::AppSettings,
    secrets: &FeishuSecrets,
) -> AppResult<TaskAssignee> {
    if let Some(id) = secrets.task_assignee_id.as_deref() {
        let id_type = secrets
            .task_assignee_id_type
            .as_deref()
            .unwrap_or("open_id");
        validate_user_id_type(id_type)?;
        return Ok(TaskAssignee {
            id: id.to_string(),
            id_type: id_type.to_string(),
        });
    }
    if settings.feishu_source_url.trim().is_empty() {
        return Err(AppError::FeishuUnavailable(
            "无法确定飞书待办负责人：请先配置用户拥有的投递记录表".to_string(),
        ));
    }
    let spreadsheet_token = source_token(&settings.feishu_source_url)?;
    let response = client
        .get(format!(
            "{API_BASE}/sheets/v3/spreadsheets/{spreadsheet_token}"
        ))
        .bearer_auth(token)
        .query(&[("user_id_type", "union_id")])
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let owner_id = string_field(&payload["data"]["spreadsheet"], "owner_id")?;
    if owner_id.starts_with("cli_") {
        return Err(AppError::FeishuUnavailable(
            "投递记录表所有者是应用而不是用户，无法投递个人待办提醒".to_string(),
        ));
    }
    Ok(TaskAssignee {
        id: owner_id,
        id_type: "union_id".to_string(),
    })
}

fn validate_user_id_type(value: &str) -> AppResult<()> {
    if matches!(value, "open_id" | "union_id" | "user_id") {
        Ok(())
    } else {
        Err(AppError::FeishuUnavailable(
            "FEISHU_TASK_ASSIGNEE_ID_TYPE 只允许 open_id、union_id 或 user_id".to_string(),
        ))
    }
}

async fn create_task(
    client: &Client,
    token: &str,
    assignee: &TaskAssignee,
    plan: &PlanItem,
) -> AppResult<FeishuPlanTaskMapping> {
    let timestamp = plan
        .scheduled_at
        .ok_or_else(|| AppError::Validation("没有时间的计划不能创建飞书待办".to_string()))?
        .to_string();
    let response = client
        .post(format!("{API_BASE}/task/v2/tasks"))
        .bearer_auth(token)
        .query(&[("user_id_type", assignee.id_type.as_str())])
        .json(&json!({
            "summary": truncate_chars(plan.title.trim(), 3_000),
            "description": task_description(plan),
            "start": { "timestamp": timestamp, "is_all_day": false },
            "due": { "timestamp": timestamp, "is_all_day": false },
            "reminders": [{ "relative_fire_minute": TASK_REMINDER_MINUTES }],
            "members": [{ "id": assignee.id, "type": "user", "role": "assignee" }],
            "client_token": plan.id,
        }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let task = &payload["data"]["task"];
    Ok(FeishuPlanTaskMapping {
        plan_id: plan.id.clone(),
        task_guid: string_field(task, "guid")?,
        task_url: task["url"].as_str().map(str::to_string),
        plan_updated_at: plan.updated_at,
        completed: false,
    })
}

async fn patch_task(
    client: &Client,
    token: &str,
    user_id_type: &str,
    task_guid: &str,
    plan: &PlanItem,
    completed: bool,
) -> AppResult<Option<String>> {
    let timestamp = plan
        .scheduled_at
        .ok_or_else(|| AppError::Validation("没有时间的计划不能更新飞书待办".to_string()))?
        .to_string();
    let completed_at = if completed {
        Utc::now().timestamp_millis().to_string()
    } else {
        "0".to_string()
    };
    let response = client
        .patch(format!("{API_BASE}/task/v2/tasks/{task_guid}"))
        .bearer_auth(token)
        .query(&[("user_id_type", user_id_type)])
        .json(&json!({
            "task": {
                "summary": truncate_chars(plan.title.trim(), 3_000),
                "description": task_description(plan),
                "start": { "timestamp": timestamp, "is_all_day": false },
                "due": { "timestamp": timestamp, "is_all_day": false },
                "completed_at": completed_at,
            },
            "update_fields": TASK_UPDATE_FIELDS,
        }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    Ok(payload["data"]["task"]["url"].as_str().map(str::to_string))
}

async fn complete_task(
    client: &Client,
    token: &str,
    user_id_type: &str,
    task_guid: &str,
) -> AppResult<()> {
    let response = client
        .patch(format!("{API_BASE}/task/v2/tasks/{task_guid}"))
        .bearer_auth(token)
        .query(&[("user_id_type", user_id_type)])
        .json(&json!({
            "task": { "completed_at": Utc::now().timestamp_millis().to_string() },
            "update_fields": ["completed_at"],
        }))
        .send()
        .await
        .map_err(network_error)?;
    checked_json(response).await.map(|_| ())
}

fn task_description(plan: &PlanItem) -> String {
    let mut parts = vec![
        format!("事项：{}", compact_content(plan)),
        format!("详情：{}", plan.details.trim()),
    ];
    if let Some(notes) = plan
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("注意：{notes}"));
    }
    if let Some(link) = plan
        .link_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("链接：{link}"));
    }
    if !plan.source_title.trim().is_empty() {
        parts.push(format!("来源：{}", plan.source_title.trim()));
    }
    parts.push(format!("FeedNote 计划 ID：{}", plan.id));
    truncate_chars(&parts.join("\n"), 3_000)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

async fn cleanup_legacy_plan_rows(database: &Database, secrets_path: &Path) -> AppResult<usize> {
    let plan_ids = database.list_feishu_plan_cleanup_ids()?;
    if plan_ids.is_empty() {
        return Ok(0);
    }
    let Some(state) = database.get_feishu_sheet_state()? else {
        database.complete_feishu_plan_cleanup(&plan_ids)?;
        return Ok(plan_ids.len());
    };

    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let rows = read_plan_rows(&client, &token, &state).await?;
    for plan_id in &plan_ids {
        let Some(row) = rows.get(plan_id) else {
            continue;
        };
        write_values(
            &client,
            &token,
            &state,
            &format!("A{}:I{}", row.row_number, row.row_number),
            vec![vec![json!(""); HEADERS.len()]],
        )
        .await?;
    }

    let remaining_rows = read_plan_rows(&client, &token, &state).await?;
    let remaining = plan_ids
        .iter()
        .filter(|plan_id| remaining_rows.contains_key(plan_id.as_str()))
        .count();
    if remaining > 0 {
        return Err(AppError::FeishuUnavailable(format!(
            "飞书计划表仍有 {remaining} 条旧投递计划未清除，将自动重试"
        )));
    }
    database.complete_feishu_plan_cleanup(&plan_ids)?;
    Ok(plan_ids.len())
}

async fn sync_pending(
    database: &Database,
    secrets_path: &Path,
    app: &AppHandle,
) -> AppResult<usize> {
    let initial_pending = database.list_pending_feishu_plans(200)?;
    let existing_state = database.get_feishu_sheet_state()?;
    if initial_pending.is_empty() && existing_state.is_none() {
        return Ok(0);
    }

    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let state = match existing_state {
        Some(state) => state,
        None => create_sheet(database, &client, &token).await?,
    };
    let rows = read_plan_rows(&client, &token, &state).await?;
    let pulled = pull_plan_sheet_statuses(database, &rows, app)?;
    let pending = database.list_pending_feishu_plans(200)?;
    if rows.is_empty() {
        write_values(
            &client,
            &token,
            &state,
            "A1:I1",
            vec![HEADERS.iter().map(|value| json!(value)).collect()],
        )
        .await?;
    }
    for plan in &pending {
        upsert_plan(&client, &token, &state, &rows, &plan).await?;
    }

    let verified_rows = read_plan_rows(&client, &token, &state).await?;
    let missing_ids: Vec<&str> = pending
        .iter()
        .filter(|plan| !verified_rows.contains_key(&plan.id))
        .map(|plan| plan.id.as_str())
        .collect();
    if !missing_ids.is_empty() {
        return Err(AppError::FeishuUnavailable(format!(
            "远端回读未找到 {} 条计划，本地仍保留为待同步",
            missing_ids.len()
        )));
    }

    let synced_at = Utc::now().timestamp_millis();
    for plan in &pending {
        database.mark_plan_feishu_synced(&plan.id, synced_at)?;
    }
    Ok(pulled + pending.len())
}

async fn sync_memo_pending(database: &Database, secrets_path: &Path) -> AppResult<usize> {
    let pending = database.list_pending_feishu_memos(500)?;
    let existing_state = database.get_feishu_memo_sheet_state()?;
    if pending.is_empty() && existing_state.is_none() {
        return Ok(0);
    }

    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let state = match existing_state {
        Some(state) => state,
        None => create_memo_sheet(database, &client, &token).await?,
    };
    let rows = read_memo_rows(&client, &token, &state).await?;
    if rows.is_empty() {
        write_values(
            &client,
            &token,
            &state,
            "A1:D1",
            vec![MEMO_HEADERS.iter().map(|value| json!(value)).collect()],
        )
        .await?;
    }
    for memo in &pending {
        if let Some(existing) = rows.get(&memo.id) {
            write_values(
                &client,
                &token,
                &state,
                &format!("A{}:D{}", existing.row_number, existing.row_number),
                vec![memo_row(memo)],
            )
            .await?;
        } else {
            append_memo_values(&client, &token, &state, vec![memo_row(memo)]).await?;
        }
    }

    let verified_rows = read_memo_rows(&client, &token, &state).await?;
    let unverified = pending
        .iter()
        .filter(|memo| {
            !verified_rows
                .get(&memo.id)
                .is_some_and(|row| memo_sheet_row_matches(row, memo))
        })
        .count();
    if unverified > 0 {
        return Err(AppError::FeishuUnavailable(format!(
            "备忘录表有 {unverified} 条记录未通过回读校验，本地仍保留为待同步"
        )));
    }
    let synced_at = Utc::now().timestamp_millis();
    let mut synced = 0;
    for memo in &pending {
        if database.mark_memo_feishu_synced_if_current(memo, synced_at)? {
            synced += 1;
        }
    }
    Ok(synced)
}

async fn create_memo_sheet(
    database: &Database,
    client: &Client,
    token: &str,
) -> AppResult<FeishuSheetState> {
    let response = client
        .post(format!("{API_BASE}/sheets/v3/spreadsheets"))
        .bearer_auth(token)
        .json(&json!({ "title": MEMO_SHEET_TITLE }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let spreadsheet = &payload["data"]["spreadsheet"];
    let spreadsheet_token = string_field(spreadsheet, "spreadsheet_token")?;
    let spreadsheet_url = spreadsheet["url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://feishu.cn/sheets/{spreadsheet_token}"));
    let response = client
        .get(format!(
            "{API_BASE}/sheets/v3/spreadsheets/{spreadsheet_token}/sheets/query"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let sheet_id = payload["data"]["sheets"]
        .as_array()
        .and_then(|sheets| sheets.first())
        .and_then(|sheet| sheet["sheet_id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| AppError::FeishuUnavailable("新备忘录表没有工作表".to_string()))?;
    let state = FeishuSheetState {
        spreadsheet_token,
        sheet_id,
        spreadsheet_url,
    };
    database.save_feishu_memo_sheet_state(&state)?;
    Ok(state)
}

async fn read_memo_rows(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
) -> AppResult<HashMap<String, MemoSheetRow>> {
    let range = format!("{}!A:B", state.sheet_id);
    let mut url = Url::parse(&format!(
        "{API_BASE}/sheets/v2/spreadsheets/{}/values/",
        state.spreadsheet_token
    ))
    .map_err(|_| AppError::FeishuUnavailable("飞书备忘录表读取地址无效".to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::FeishuUnavailable("飞书备忘录表读取地址无效".to_string()))?
        .pop_if_empty()
        .push(&range);
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let values = payload["data"]["valueRange"]["values"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(values
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let values = row.as_array()?;
            let cells = (0..2)
                .map(|column| values.get(column).map(cell_text).unwrap_or_default())
                .collect::<Vec<_>>();
            let id = cells.first()?.clone();
            (!id.is_empty() && id != MEMO_HEADERS[0]).then_some((
                id,
                MemoSheetRow {
                    row_number: index + 1,
                    cells,
                },
            ))
        })
        .collect())
}

async fn append_memo_values(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    values: Vec<Vec<Value>>,
) -> AppResult<()> {
    let response = client
        .post(format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_append?insertDataOption=INSERT_ROWS",
            state.spreadsheet_token
        ))
        .bearer_auth(token)
        .json(&json!({
            "valueRange": {
                "range": format!("{}!A:D", state.sheet_id),
                "values": values,
            }
        }))
        .send()
        .await
        .map_err(network_error)?;
    checked_json(response).await.map(|_| ())
}

fn memo_row(item: &MemoItem) -> Vec<Value> {
    [
        item.id.clone(),
        item.content.clone(),
        item.source_title.clone(),
        format_time(item.created_at),
    ]
    .into_iter()
    .map(|value| json!(safe_cell(&value)))
    .collect()
}

fn memo_sheet_row_matches(row: &MemoSheetRow, item: &MemoItem) -> bool {
    let expected = memo_row(item)
        .iter()
        .take(2)
        .map(cell_text)
        .collect::<Vec<_>>();
    row.cells.len() >= expected.len()
        && row
            .cells
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| {
                actual == expected
                    || expected
                        .strip_prefix('\'')
                        .is_some_and(|unescaped| actual == unescaped)
            })
}

async fn sync_secret_pending(
    database: &Database,
    secrets_path: &Path,
    vault: &Vault,
) -> AppResult<usize> {
    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let state = match database.get_feishu_secret_sheet_state()? {
        Some(state) => state,
        None => create_secret_sheet(database, &client, &token).await?,
    };
    let rows = read_secret_rows(&client, &token, &state).await?;
    if rows.is_empty() {
        write_values(
            &client,
            &token,
            &state,
            "A1:H1",
            vec![SECRET_HEADERS.iter().map(|value| json!(value)).collect()],
        )
        .await?;
    }

    let cleanup = database.list_feishu_secret_cleanup()?;
    for id in &cleanup {
        if let Some(row_number) = rows.get(id) {
            write_values(
                &client,
                &token,
                &state,
                &format!("A{row_number}:H{row_number}"),
                vec![vec![json!(""); SECRET_HEADERS.len()]],
            )
            .await?;
        }
    }
    database.complete_feishu_secret_cleanup(&cleanup)?;

    let items = vault.list(database)?;
    let pending: Vec<&SecretItem> = items
        .iter()
        .filter(|item| {
            item.feishu_synced_at
                .is_none_or(|synced_at| synced_at < item.updated_at)
        })
        .collect();
    for item in &pending {
        let row = secret_row(item);
        if let Some(row_number) = rows.get(&item.id) {
            write_values(
                &client,
                &token,
                &state,
                &format!("A{row_number}:H{row_number}"),
                vec![row],
            )
            .await?;
        } else {
            append_secret_values(&client, &token, &state, vec![row]).await?;
        }
    }

    let verified_rows = read_secret_rows(&client, &token, &state).await?;
    let missing = pending
        .iter()
        .filter(|item| !verified_rows.contains_key(&item.id))
        .count();
    if missing > 0 {
        return Err(AppError::FeishuUnavailable(format!(
            "秘密表回读未找到 {missing} 条记录，本地仍保留为待同步"
        )));
    }
    let synced_at = Utc::now().timestamp_millis();
    for item in &pending {
        database.mark_secret_feishu_synced(&item.id, synced_at)?;
    }
    Ok(cleanup.len() + pending.len())
}

async fn create_secret_sheet(
    database: &Database,
    client: &Client,
    token: &str,
) -> AppResult<FeishuSheetState> {
    let response = client
        .post(format!("{API_BASE}/sheets/v3/spreadsheets"))
        .bearer_auth(token)
        .json(&json!({ "title": SECRET_SHEET_TITLE }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let spreadsheet = &payload["data"]["spreadsheet"];
    let spreadsheet_token = string_field(spreadsheet, "spreadsheet_token")?;
    let spreadsheet_url = spreadsheet["url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://feishu.cn/sheets/{spreadsheet_token}"));
    let response = client
        .get(format!(
            "{API_BASE}/sheets/v3/spreadsheets/{spreadsheet_token}/sheets/query"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let sheet_id = payload["data"]["sheets"]
        .as_array()
        .and_then(|sheets| sheets.first())
        .and_then(|sheet| sheet["sheet_id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| AppError::FeishuUnavailable("新秘密表没有工作表".to_string()))?;
    let state = FeishuSheetState {
        spreadsheet_token,
        sheet_id,
        spreadsheet_url,
    };
    database.save_feishu_secret_sheet_state(&state)?;
    Ok(state)
}

async fn read_secret_rows(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
) -> AppResult<HashMap<String, usize>> {
    let range = format!("{}!A:A", state.sheet_id);
    let mut url = Url::parse(&format!(
        "{API_BASE}/sheets/v2/spreadsheets/{}/values/",
        state.spreadsheet_token
    ))
    .map_err(|_| AppError::FeishuUnavailable("飞书秘密表读取地址无效".to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::FeishuUnavailable("飞书秘密表读取地址无效".to_string()))?
        .pop_if_empty()
        .push(&range);
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let values = payload["data"]["valueRange"]["values"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(values
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let id = row.as_array()?.first().map(cell_text)?;
            (!id.is_empty() && id != SECRET_HEADERS[0]).then_some((id, index + 1))
        })
        .collect())
}

async fn append_secret_values(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    values: Vec<Vec<Value>>,
) -> AppResult<()> {
    let response = client
        .post(format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_append?insertDataOption=INSERT_ROWS",
            state.spreadsheet_token
        ))
        .bearer_auth(token)
        .json(&json!({
            "valueRange": {
                "range": format!("{}!A:H", state.sheet_id),
                "values": values,
            }
        }))
        .send()
        .await
        .map_err(network_error)?;
    checked_json(response).await.map(|_| ())
}

fn secret_row(item: &SecretItem) -> Vec<Value> {
    [
        item.id.clone(),
        item.payload.secret_type.clone(),
        item.payload.title.clone(),
        item.payload.account.clone().unwrap_or_default(),
        item.payload.secret_value.clone(),
        item.payload.website.clone().unwrap_or_default(),
        item.payload.notes.clone().unwrap_or_default(),
        format_time(item.updated_at),
    ]
    .into_iter()
    .map(|value| json!(safe_cell(&value)))
    .collect()
}

fn pull_plan_sheet_statuses(
    database: &Database,
    rows: &HashMap<String, PlanSheetRow>,
    app: &AppHandle,
) -> AppResult<usize> {
    let plans = database.list_plans(true)?;
    let mut changed = 0;
    for plan in &plans {
        let Some(remote_done) = rows.get(&plan.id).and_then(|row| row.completed) else {
            continue;
        };
        let local_done = plan.status == "done";
        let local_is_synced = plan
            .feishu_synced_at
            .is_some_and(|value| value >= plan.updated_at);
        if remote_done == local_done || !local_is_synced {
            continue;
        }
        if let Some(updated) = database.apply_remote_plan_done(
            &plan.id,
            remote_done,
            plan.updated_at,
            "feishu_sheet",
        )? {
            let _ = app.emit("plans-changed", &updated);
            changed += 1;
        }
    }
    Ok(changed)
}

async fn tenant_access_token(client: &Client, secrets: &FeishuSecrets) -> AppResult<String> {
    let now = Utc::now().timestamp_millis();
    let cache = TENANT_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(token) = cache
        .lock()
        .expect("tenant token cache poisoned")
        .as_ref()
        .filter(|cached| cached.app_id == secrets.app_id && cached.expires_at > now + 60_000)
        .map(|cached| cached.token.clone())
    {
        return Ok(token);
    }
    let response = client
        .post(format!("{API_BASE}/auth/v3/tenant_access_token/internal"))
        .json(&json!({
            "app_id": secrets.app_id,
            "app_secret": secrets.app_secret,
        }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let token = payload["tenant_access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::FeishuUnavailable("飞书没有返回 tenant_access_token".to_string())
        })?;
    let expires_in = payload["expire"].as_i64().unwrap_or(3_600).clamp(60, 7_200);
    *cache.lock().expect("tenant token cache poisoned") = Some(CachedTenantToken {
        app_id: secrets.app_id.clone(),
        token: token.clone(),
        expires_at: now + expires_in * 1_000,
    });
    Ok(token)
}

async fn create_sheet(
    database: &Database,
    client: &Client,
    token: &str,
) -> AppResult<FeishuSheetState> {
    let response = client
        .post(format!("{API_BASE}/sheets/v3/spreadsheets"))
        .bearer_auth(token)
        .json(&json!({ "title": SHEET_TITLE }))
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let spreadsheet = &payload["data"]["spreadsheet"];
    let spreadsheet_token = string_field(spreadsheet, "spreadsheet_token")?;
    let spreadsheet_url = spreadsheet["url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://feishu.cn/sheets/{spreadsheet_token}"));

    let response = client
        .get(format!(
            "{API_BASE}/sheets/v3/spreadsheets/{spreadsheet_token}/sheets/query"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let sheet_id = payload["data"]["sheets"]
        .as_array()
        .and_then(|sheets| sheets.first())
        .and_then(|sheet| sheet["sheet_id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| AppError::FeishuUnavailable("新表格没有工作表".to_string()))?;

    let state = FeishuSheetState {
        spreadsheet_token,
        sheet_id,
        spreadsheet_url,
    };
    database.save_feishu_sheet_state(&state)?;
    Ok(state)
}

async fn read_plan_rows(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
) -> AppResult<HashMap<String, PlanSheetRow>> {
    let range = format!("{}!A:B", state.sheet_id);
    let mut url = Url::parse(&format!(
        "{API_BASE}/sheets/v2/spreadsheets/{}/values/",
        state.spreadsheet_token
    ))
    .map_err(|_| AppError::FeishuUnavailable("飞书读取地址无效".to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::FeishuUnavailable("飞书读取地址无效".to_string()))?
        .pop_if_empty()
        .push(&range);
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(network_error)?;
    let payload = checked_json(response).await?;
    let values = payload["data"]["valueRange"]["values"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(values
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let cells = row.as_array()?;
            let id = cells
                .first()
                .map(cell_text)
                .filter(|id| id != HEADERS[0] && !id.is_empty())?;
            let completed = cells.get(1).and_then(remote_done_from_status);
            Some((
                id,
                PlanSheetRow {
                    row_number: index + 1,
                    completed,
                },
            ))
        })
        .collect())
}

async fn upsert_plan(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    rows: &HashMap<String, PlanSheetRow>,
    plan: &PlanItem,
) -> AppResult<()> {
    let row = plan_row(plan);
    if let Some(existing) = rows.get(&plan.id) {
        write_values(
            client,
            token,
            state,
            &format!("A{}:I{}", existing.row_number, existing.row_number),
            vec![row],
        )
        .await
    } else {
        append_values(client, token, state, vec![row]).await
    }
}

async fn write_values(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    cell_range: &str,
    values: Vec<Vec<Value>>,
) -> AppResult<()> {
    let range = format!("{}!{cell_range}", state.sheet_id);
    let response = client
        .put(format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values",
            state.spreadsheet_token
        ))
        .bearer_auth(token)
        .json(&json!({ "valueRange": { "range": range, "values": values } }))
        .send()
        .await
        .map_err(network_error)?;
    checked_json(response).await.map(|_| ())
}

async fn append_values(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    values: Vec<Vec<Value>>,
) -> AppResult<()> {
    let response = client
        .post(format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_append?insertDataOption=INSERT_ROWS",
            state.spreadsheet_token
        ))
        .bearer_auth(token)
        .json(&json!({
            "valueRange": {
                "range": format!("{}!A:I", state.sheet_id),
                "values": values,
            }
        }))
        .send()
        .await
        .map_err(network_error)?;
    checked_json(response).await.map(|_| ())
}

fn plan_row(plan: &PlanItem) -> Vec<Value> {
    [
        plan.id.clone(),
        status_label(&plan.status).to_string(),
        plan.scheduled_at.map(format_time).unwrap_or_default(),
        plan.title.clone(),
        compact_content(plan),
        plan.link_url.clone().unwrap_or_default(),
        plan.notes.clone().unwrap_or_default(),
        plan.source_title.clone(),
        format_time(plan.updated_at),
    ]
    .into_iter()
    .map(|value| json!(safe_cell(&value)))
    .collect()
}

fn status_label(status: &str) -> &str {
    match status {
        "scheduled" => "已安排",
        "needs_clarification" => "待补充时间",
        "done" => "已完成",
        _ => status,
    }
}

fn remote_done_from_status(value: &Value) -> Option<bool> {
    match cell_text(value).trim().to_ascii_lowercase().as_str() {
        "已完成" | "完成" | "done" | "completed" => Some(true),
        "已安排"
        | "待补充时间"
        | "未完成"
        | "待办"
        | "scheduled"
        | "needs_clarification"
        | "todo" => Some(false),
        _ => None,
    }
}

fn compact_content(plan: &PlanItem) -> String {
    if !plan.content.trim().is_empty() {
        return plan.content.trim().to_string();
    }
    plan.details
        .split(['。', '！', '？', '\n'])
        .next()
        .unwrap_or(&plan.details)
        .trim()
        .chars()
        .take(60)
        .collect()
}

fn safe_cell(value: &str) -> String {
    let value = value.trim();
    if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

fn format_time(timestamp: i64) -> String {
    let offset = FixedOffset::east_opt(8 * 60 * 60).expect("valid Shanghai offset");
    offset
        .timestamp_millis_opt(timestamp)
        .single()
        .unwrap_or_else(|| Utc::now().with_timezone(&offset))
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

async fn checked_json(response: reqwest::Response) -> AppResult<Value> {
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| AppError::FeishuUnavailable(error.to_string()))?;
    let code = payload["code"]
        .as_i64()
        .unwrap_or(if status.is_success() { 0 } else { -1 });
    if !status.is_success() || code != 0 {
        let message = payload["msg"].as_str().unwrap_or("未知错误");
        if code == 91403 {
            return Err(AppError::FeishuUnavailable(
                "许科AI助手可以读取目标表格，但没有该文档的编辑权限。请在表格‘分享’中将许科AI助手设为可编辑协作者后重试"
                    .to_string(),
            ));
        }
        return Err(AppError::FeishuUnavailable(format!(
            "飞书接口返回 code={code}：{message}"
        )));
    }
    Ok(payload)
}

fn string_field(value: &Value, field: &str) -> AppResult<String> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::FeishuUnavailable(format!("飞书结果缺少 {field}")))
}

fn required_secret(values: &HashMap<String, String>, key: &str) -> AppResult<String> {
    values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| AppError::FeishuUnavailable(format!("data\\secrets.env 中缺少 {key}")))
}

fn client() -> AppResult<Client> {
    if let Some(client) = FEISHU_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| AppError::FeishuUnavailable(error.to_string()))?;
    let _ = FEISHU_CLIENT.set(client.clone());
    Ok(FEISHU_CLIENT.get().cloned().unwrap_or(client))
}

fn network_error(error: reqwest::Error) -> AppError {
    let message = if error.is_timeout() {
        "飞书服务响应超时".to_string()
    } else if error.is_connect() {
        "无法连接飞书服务".to_string()
    } else {
        error.to_string()
    };
    AppError::FeishuUnavailable(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_row() -> SourceRow {
        SourceRow {
            row_number: 2,
            status: "简历筛选".to_string(),
            company: "烽火通信科技股份有限公司".to_string(),
            role: "前端开发工程师".to_string(),
        }
    }

    fn application(
        company: &str,
        role: Option<&str>,
        link: Option<&str>,
    ) -> ApplicationRecordProposal {
        ApplicationRecordProposal {
            status: "待投递".to_string(),
            company: company.to_string(),
            role: role.map(str::to_string),
            link_url: link.map(str::to_string),
            notes: None,
        }
    }

    #[test]
    fn neutralizes_spreadsheet_formula_prefixes() {
        assert_eq!(safe_cell("=IMPORTXML(...)"), "'=IMPORTXML(...)");
        assert_eq!(safe_cell("普通标题"), "普通标题");
    }

    #[test]
    fn exposes_only_the_fixed_sync_columns() {
        assert_eq!(HEADERS.len(), 9);
        assert!(!HEADERS.contains(&"原始投喂"));
        assert!(!HEADERS.contains(&"周边上下文"));
    }

    #[test]
    fn secret_rows_use_the_explicit_plaintext_schema_and_neutralize_formulas() {
        let item = SecretItem {
            id: "secret-id".to_string(),
            payload: crate::models::SecretPayload {
                title: "=危险标题".to_string(),
                secret_type: "API Key".to_string(),
                account: Some("demo@example.com".to_string()),
                secret_value: "+secret-value".to_string(),
                website: Some("https://example.com/".to_string()),
                notes: Some("仅用于测试".to_string()),
                source_title: "控制台".to_string(),
            },
            created_at: 1_788_142_920_000,
            updated_at: 1_788_142_920_000,
            feishu_synced_at: None,
        };
        let row = secret_row(&item);
        assert_eq!(SECRET_HEADERS.len(), 8);
        assert_eq!(row[2], json!("'=危险标题"));
        assert_eq!(row[4], json!("'+secret-value"));
        assert!(!SECRET_HEADERS.contains(&"周边上下文"));
    }

    #[test]
    fn memo_rows_only_contain_the_explicit_record_fields() {
        let item = MemoItem {
            id: "memo-id".to_string(),
            content: "=创业想法".to_string(),
            source_title: "微信".to_string(),
            created_at: 1_788_142_920_000,
            feishu_synced_at: None,
        };
        let row = memo_row(&item);
        assert_eq!(MEMO_HEADERS, ["本地备忘ID", "内容", "来源", "记录时间"]);
        assert_eq!(row.len(), 4);
        assert_eq!(row[1], json!("'=创业想法"));

        let remote = MemoSheetRow {
            row_number: 2,
            cells: row.iter().map(cell_text).collect(),
        };
        assert!(memo_sheet_row_matches(&remote, &item));
        let mut stale = remote;
        stale.cells[1] = "旧内容".to_string();
        assert!(!memo_sheet_row_matches(&stale, &item));
    }

    #[test]
    fn application_rows_match_by_company_and_role() {
        let row = source_row();
        assert!(!application_row_matches(
            &row,
            &application(
                "另一家公司",
                Some("前端工程师"),
                Some("https://example.com/apply")
            )
        ));
        assert!(!application_row_matches(
            &row,
            &application("烽火通信", Some("完全不同的岗位"), None)
        ));
        assert!(!application_row_matches(
            &row,
            &application("烽火通信", None, None)
        ));
        assert!(application_row_matches(
            &row,
            &application("烽火通信", Some("前端"), None)
        ));
        assert!(application_row_matches(
            &row,
            &application("烽火通信", Some("前端开发岗位"), None)
        ));
        assert!(!application_row_matches(
            &row,
            &application("烽火通信", Some("后端开发工程师"), None)
        ));
        assert!(company_matches("烽火通信科技股份有限公司", "烽火通信"));
        assert!(!company_matches("中国移动研究院", "中国移动"));
    }

    #[test]
    fn role_aliases_are_controlled() {
        assert!(role_matches("前端", "前端开发工程师"));
        assert!(role_matches("Web 前端开发", "前端工程师岗位"));
        assert!(role_matches("后端研发工程师", "后端开发"));
        assert!(!role_matches("前端开发工程师", "后端开发工程师"));
        assert!(!role_matches("测试开发工程师", "测试工程师"));
        assert!(!role_matches("高级前端开发工程师", "前端开发工程师"));
    }

    #[test]
    fn application_record_requires_supported_status_and_company() {
        assert!(
            validate_application_record(&application("示例公司", Some("前端工程师"), None)).is_ok()
        );
        let mut invalid = application("", Some("前端工程师"), None);
        assert!(validate_application_record(&invalid).is_err());
        invalid.company = "示例公司".to_string();
        invalid.status = "自动创建计划".to_string();
        assert!(validate_application_record(&invalid).is_err());
        invalid.status = "面试".to_string();
        assert!(validate_application_record(&invalid).is_err());
    }

    #[test]
    fn existing_application_updates_only_the_status_cell() {
        let mut proposal = application("示例公司", Some("后端工程师"), None);
        proposal.status = "待二面".to_string();
        proposal.notes = Some("不应覆盖旧备注".to_string());
        assert_eq!(application_status_cell(&proposal), vec![json!("待二面")]);
        assert_eq!(application_row(&proposal).len(), 5);
    }

    #[test]
    fn extracts_a_clickable_link_cell() {
        let cell = json!([{
            "link": "https://example.com/apply",
            "text": "申请入口",
            "type": "url"
        }]);
        assert_eq!(cell_text(&cell), "https://example.com/apply");
    }

    #[test]
    fn parses_source_sheet_token_without_query_parameters() {
        assert_eq!(
            source_token("https://team.feishu.cn/sheets/abc123?sheet=one").unwrap(),
            "abc123"
        );
    }

    #[test]
    fn parses_only_supported_remote_plan_statuses() {
        assert_eq!(remote_done_from_status(&json!("已完成")), Some(true));
        assert_eq!(remote_done_from_status(&json!("todo")), Some(false));
        assert_eq!(remote_done_from_status(&json!("随便写的状态")), None);
    }

    #[test]
    fn parses_feishu_task_completion_fields() {
        assert!(!task_completed(&json!({
            "status": "todo",
            "completed_at": "0"
        })));
        assert!(task_completed(&json!({
            "status": "todo",
            "completed_at": "1788142920464"
        })));
        assert!(task_completed(&json!({
            "status": "done",
            "completed_at": "0"
        })));
        assert!(!TASK_UPDATE_FIELDS.contains(&"reminders"));
    }

    #[test]
    fn parses_feishu_task_title_and_start_time() {
        let task = parse_remote_task(&json!({
            "summary": "  飞书改期后的面试  ",
            "status": "todo",
            "completed_at": "0",
            "start": { "timestamp": "1788168600000", "is_all_day": false },
            "due": { "timestamp": "1788170400000", "is_all_day": false },
            "url": "https://applink.feishu.cn/task/example"
        }));
        assert_eq!(task.title.as_deref(), Some("飞书改期后的面试"));
        assert_eq!(task.scheduled_at, Some(1_788_168_600_000));
        assert!(!task.completed);
        assert_eq!(
            task.task_url.as_deref(),
            Some("https://applink.feishu.cn/task/example")
        );

        let all_day = parse_remote_task(&json!({
            "summary": "全天任务",
            "start": { "timestamp": "1788163200000", "is_all_day": true }
        }));
        assert!(all_day.scheduled_at.is_none());
        assert_eq!(
            task_time(&json!({ "timestamp": "1788168600" })),
            Some(1_788_168_600_000)
        );
    }
}
