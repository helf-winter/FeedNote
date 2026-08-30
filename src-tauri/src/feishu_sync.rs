use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use reqwest::{Client, Url};
use serde_json::{json, Value};

use crate::{
    ai,
    db::Database,
    error::{AppError, AppResult},
    models::{
        FeishuSheetState, FeishuSourceState, FeishuSourceStatus, FeishuSyncStatus, PlanItem,
        PlanProposal,
    },
    secrets::parse_env,
};

const API_BASE: &str = "https://open.feishu.cn/open-apis";
const SHEET_TITLE: &str = "FeedNote 计划";
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

struct FeishuSecrets {
    app_id: String,
    app_secret: String,
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
        Ok(Self { app_id, app_secret })
    }

    fn is_configured(path: &Path) -> bool {
        Self::load(path).is_ok()
    }
}

pub fn start_scheduler(
    database: Arc<Database>,
    secrets_path: std::path::PathBuf,
    syncing: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let settings = match database.get_settings() {
                Ok(settings) => settings,
                Err(_) => continue,
            };
            if settings.feishu_sync_enabled {
                if let Some(_guard) = SyncGuard::acquire(&syncing) {
                    match sync_pending(&database, &secrets_path).await {
                        Ok(_) => {
                            let _ = database.save_feishu_sync_error(None);
                        }
                        Err(error) => {
                            let _ = database.save_feishu_sync_error(Some(&error.to_string()));
                        }
                    }
                }
            }
        }
    });
}

pub fn start_source_scheduler(
    database: Arc<Database>,
    secrets_path: std::path::PathBuf,
    syncing: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
        loop {
            interval.tick().await;
            let settings = match database.get_settings() {
                Ok(settings) => settings,
                Err(_) => continue,
            };
            if settings.feishu_source_enabled {
                if let Some(_guard) = SyncGuard::acquire(&syncing) {
                    match sync_source(&database, &secrets_path).await {
                        Ok(_) => {
                            let _ = database.save_feishu_source_error(None);
                        }
                        Err(error) => {
                            let _ = database.save_feishu_source_error(Some(&error.to_string()));
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
    })
}

pub async fn sync_now(
    database: &Database,
    secrets_path: &Path,
    syncing: &AtomicBool,
) -> AppResult<String> {
    let _guard = SyncGuard::acquire(syncing)
        .ok_or_else(|| AppError::FeishuUnavailable("同步正在进行，请稍后查看状态".to_string()))?;
    let synced = match sync_pending(database, secrets_path).await {
        Ok(synced) => {
            database.save_feishu_sync_error(None)?;
            synced
        }
        Err(error) => {
            database.save_feishu_sync_error(Some(&error.to_string()))?;
            return Err(error);
        }
    };
    Ok(if synced == 0 {
        "飞书表格已连接，没有待同步计划".to_string()
    } else {
        format!("已同步 {synced} 条计划到飞书表格")
    })
}

pub fn source_status(database: &Database, secrets_path: &Path) -> AppResult<FeishuSourceStatus> {
    let settings = database.get_settings()?;
    let state = database.get_feishu_source_state()?;
    Ok(FeishuSourceStatus {
        enabled: settings.feishu_source_enabled,
        configured: FeishuSecrets::is_configured(secrets_path)
            && !settings.feishu_source_url.trim().is_empty(),
        spreadsheet_url: settings.feishu_source_url,
        sheet_title: state.as_ref().map(|value| value.sheet_title.clone()),
        total_rows: state.as_ref().map_or(0, |value| value.total_rows),
        actionable_rows: state.as_ref().map_or(0, |value| value.actionable_rows),
        tracked_rows: state.as_ref().map_or(0, |value| value.tracked_rows),
        imported_plans: state.as_ref().map_or(0, |value| value.imported_plans),
        last_sync_at: state.map(|value| value.last_sync_at),
        last_error: database.get_feishu_source_error()?,
    })
}

pub async fn sync_source_now(
    database: &Database,
    secrets_path: &Path,
    syncing: &AtomicBool,
) -> AppResult<String> {
    let _guard = SyncGuard::acquire(syncing)
        .ok_or_else(|| AppError::FeishuUnavailable("来源分析正在进行，请稍后查看".to_string()))?;
    let result = match sync_source(database, secrets_path).await {
        Ok(result) => {
            database.save_feishu_source_error(None)?;
            result
        }
        Err(error) => {
            database.save_feishu_source_error(Some(&error.to_string()))?;
            return Err(error);
        }
    };
    Ok(format!(
        "已分析 {} 条投递记录，识别 {} 条可执行事项，本次新增 {} 条、更新 {} 条计划",
        result.total_rows, result.actionable_rows, result.created, result.updated
    ))
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
    source_key: String,
    row_hash: String,
    status: String,
    company: String,
    role: String,
    link: Option<String>,
    notes: Option<String>,
}

#[derive(Debug)]
struct SourceSheet {
    title: String,
    rows: Vec<SourceRow>,
}

#[derive(Debug, Default)]
struct SourceSyncResult {
    total_rows: usize,
    actionable_rows: usize,
    created: usize,
    updated: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceDisposition {
    DirectAction,
    NoteAction,
    Passive,
}

async fn sync_source(database: &Database, secrets_path: &Path) -> AppResult<SourceSyncResult> {
    let settings = database.get_settings()?;
    if settings.feishu_source_url.trim().is_empty() {
        return Err(AppError::Validation("请先填写飞书分析来源链接".to_string()));
    }
    crate::db::validate_feishu_source_url(&settings.feishu_source_url)?;
    let spreadsheet_token = source_token(&settings.feishu_source_url)?;
    let secrets = FeishuSecrets::load(secrets_path)?;
    let client = client()?;
    let token = tenant_access_token(&client, &secrets).await?;
    let source = read_source_sheet(&client, &token, &spreadsheet_token).await?;
    let mut result = SourceSyncResult {
        total_rows: source.rows.len(),
        ..Default::default()
    };
    let mut tracked_rows = 0;
    let mut imported_plans = 0;
    let provider_secrets = if settings.ai_enabled {
        Some(ai::ProviderSecrets::load(secrets_path)?)
    } else {
        None
    };

    for row in &source.rows {
        let disposition = source_disposition(row);
        let actionable = matches!(
            disposition,
            SourceDisposition::DirectAction | SourceDisposition::NoteAction
        );
        if actionable {
            result.actionable_rows += 1;
        }
        let existing = database.get_feishu_source_entry(&row.source_key)?;
        if existing
            .as_ref()
            .is_some_and(|entry| entry.row_hash == row.row_hash)
        {
            tracked_rows += 1;
            if existing.and_then(|entry| entry.plan_id).is_some() {
                imported_plans += 1;
            }
            continue;
        }

        if !actionable {
            if let Some(plan_id) = existing.as_ref().and_then(|entry| entry.plan_id.as_deref()) {
                database.complete_feishu_source_plan(plan_id)?;
                result.updated += 1;
                imported_plans += 1;
            }
            database.save_feishu_source_entry(
                &row.source_key,
                &row.row_hash,
                existing.as_ref().and_then(|entry| entry.plan_id.as_deref()),
            )?;
            tracked_rows += 1;
            continue;
        }

        let proposal = match disposition {
            SourceDisposition::DirectAction if !has_action_note(row.notes.as_deref()) => {
                direct_source_plan(row)
            }
            _ => {
                let provider = provider_secrets.as_ref().ok_or_else(|| {
                    AppError::Validation("AI 已关闭，无法分析飞书备注中的安排".to_string())
                })?;
                ai::propose_feishu_source_plan(
                    &settings,
                    provider,
                    &source_row_text(row),
                    &current_shanghai_time(),
                )
                .await?
            }
        };
        let scheduled_at = proposal
            .scheduled_for
            .as_deref()
            .map(parse_source_time)
            .transpose()?;
        let source_title = format!("飞书{}·第 {} 行", source.title, row.row_number);
        if let Some(plan_id) = existing.as_ref().and_then(|entry| entry.plan_id.as_deref()) {
            database.update_feishu_source_plan(plan_id, &proposal, scheduled_at, &source_title)?;
            database.save_feishu_source_entry(&row.source_key, &row.row_hash, Some(plan_id))?;
            result.updated += 1;
        } else {
            database.create_feishu_source_plan(
                &row.source_key,
                &row.row_hash,
                &source_row_text(row),
                &proposal,
                scheduled_at,
                &source_title,
            )?;
            result.created += 1;
        }
        tracked_rows += 1;
        imported_plans += 1;
    }

    database.save_feishu_source_state(&FeishuSourceState {
        spreadsheet_url: settings.feishu_source_url,
        sheet_title: source.title,
        total_rows: result.total_rows,
        actionable_rows: result.actionable_rows,
        tracked_rows,
        imported_plans,
        last_sync_at: Utc::now().timestamp_millis(),
    })?;
    Ok(result)
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
    let link_col = column("链接")?;
    let notes_col = column("备注")?;
    let mut occurrences = HashMap::<String, usize>::new();
    let mut rows = Vec::new();
    for (index, value) in values.iter().enumerate().skip(1) {
        let Some(cells) = value.as_array() else {
            continue;
        };
        let get = |column: usize| cells.get(column).map(cell_text).unwrap_or_default();
        let company = get(company_col).trim().to_string();
        if company.is_empty() {
            continue;
        }
        let status = get(status_col).trim().to_string();
        let role = get(role_col).trim().to_string();
        let link = nonempty(get(link_col));
        let notes = nonempty(get(notes_col));
        let identity = format!("{}|{}", company.to_lowercase(), role.to_lowercase());
        let occurrence = occurrences.entry(identity.clone()).or_insert(0);
        *occurrence += 1;
        let source_key = format!("{spreadsheet_token}|{identity}|{occurrence}");
        let row_hash = source_row_hash(&status, &company, &role, link.as_deref(), notes.as_deref());
        rows.push(SourceRow {
            row_number: index + 1,
            source_key,
            row_hash,
            status,
            company,
            role,
            link,
            notes,
        });
    }
    Ok(SourceSheet { title, rows })
}

fn source_disposition(row: &SourceRow) -> SourceDisposition {
    if has_action_note(row.notes.as_deref()) {
        SourceDisposition::NoteAction
    } else if [
        "待投递",
        "待笔试",
        "待面试",
        "待测评",
        "笔试",
        "面试",
        "测评",
    ]
    .iter()
    .any(|status| row.status.contains(status))
    {
        SourceDisposition::DirectAction
    } else {
        SourceDisposition::Passive
    }
}

fn has_action_note(notes: Option<&str>) -> bool {
    let notes = notes.unwrap_or_default();
    [
        "投", "笔试", "面试", "测评", "完成", "截止", "之前", "提醒", "安排",
    ]
    .iter()
    .any(|keyword| notes.contains(keyword))
}

fn direct_source_plan(row: &SourceRow) -> PlanProposal {
    let kind = if row.status.contains("投递") {
        "投递"
    } else if row.status.contains("笔试") {
        "笔试"
    } else if row.status.contains("面试") {
        "面试"
    } else {
        "测评"
    };
    let role = if row.role.is_empty() {
        String::new()
    } else {
        format!("·{}", row.role)
    };
    PlanProposal {
        title: format!("{}{}{}", row.company, role, kind)
            .chars()
            .take(36)
            .collect(),
        details: format!("{}的{}事项，当前状态：{}。", row.company, kind, row.status),
        content: kind.to_string(),
        link_url: row.link.clone(),
        notes: row.notes.clone(),
        scheduled_for: None,
        time_evidence: None,
        needs_clarification: true,
        clarification_question: Some(format!("准备什么时候处理{}的{}？", row.company, kind)),
    }
}

fn source_row_text(row: &SourceRow) -> String {
    format!(
        "状态：{}\n公司/事项：{}\n岗位/方向：{}\n链接：{}\n备注：{}",
        row.status,
        row.company,
        if row.role.is_empty() {
            "未填写"
        } else {
            &row.role
        },
        row.link.as_deref().unwrap_or("未填写"),
        row.notes.as_deref().unwrap_or("未填写")
    )
}

fn source_row_hash(
    status: &str,
    company: &str,
    role: &str,
    link: Option<&str>,
    notes: Option<&str>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "feishu-source-v2".hash(&mut hasher);
    status.hash(&mut hasher);
    company.hash(&mut hasher);
    role.hash(&mut hasher);
    link.hash(&mut hasher);
    notes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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

fn nonempty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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

fn parse_source_time(value: &str) -> AppResult<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .map_err(|_| AppError::AiInvalid("飞书来源计划时间不是有效 RFC3339".to_string()))
}

fn current_shanghai_time() -> String {
    let offset = FixedOffset::east_opt(8 * 60 * 60).expect("valid Shanghai offset");
    Utc::now().with_timezone(&offset).to_rfc3339()
}

async fn sync_pending(database: &Database, secrets_path: &Path) -> AppResult<usize> {
    let pending = database.list_pending_feishu_plans(200)?;
    let existing_state = database.get_feishu_sheet_state()?;
    if pending.is_empty() && existing_state.is_some() {
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
    Ok(pending.len())
}

async fn tenant_access_token(client: &Client, secrets: &FeishuSecrets) -> AppResult<String> {
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
    payload["tenant_access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::FeishuUnavailable("飞书没有返回 tenant_access_token".to_string()))
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
) -> AppResult<HashMap<String, usize>> {
    let range = format!("{}!A:A", state.sheet_id);
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
            row.as_array()
                .and_then(|cells| cells.first())
                .and_then(Value::as_str)
                .filter(|id| *id != HEADERS[0] && !id.is_empty())
                .map(|id| (id.to_string(), index + 1))
        })
        .collect())
}

async fn upsert_plan(
    client: &Client,
    token: &str,
    state: &FeishuSheetState,
    rows: &HashMap<String, usize>,
    plan: &PlanItem,
) -> AppResult<()> {
    let row = plan_row(plan);
    if let Some(row_index) = rows.get(&plan.id) {
        write_values(
            client,
            token,
            state,
            &format!("A{row_index}:I{row_index}"),
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
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| AppError::FeishuUnavailable(error.to_string()))
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

    fn source_row(status: &str, notes: Option<&str>) -> SourceRow {
        SourceRow {
            row_number: 2,
            source_key: "sheet|company|role|1".to_string(),
            row_hash: "hash".to_string(),
            status: status.to_string(),
            company: "示例公司".to_string(),
            role: "前端工程师".to_string(),
            link: Some("https://example.com/apply".to_string()),
            notes: notes.map(str::to_string),
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
    fn source_status_and_notes_only_create_actionable_plans() {
        assert_eq!(
            source_disposition(&source_row("待笔试", None)),
            SourceDisposition::DirectAction
        );
        assert_eq!(
            source_disposition(&source_row("简历筛选", None)),
            SourceDisposition::Passive
        );
        assert_eq!(
            source_disposition(&source_row("已挂", Some("搁三天再投另一个"))),
            SourceDisposition::NoteAction
        );
    }

    #[test]
    fn direct_source_plan_never_invents_a_time() {
        let proposal = direct_source_plan(&source_row("待投递", None));
        assert!(proposal.needs_clarification);
        assert!(proposal.scheduled_for.is_none());
        assert_eq!(proposal.content, "投递");
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
}
