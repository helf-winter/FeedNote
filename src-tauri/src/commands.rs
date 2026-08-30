use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};

use crate::{
    ai,
    error::AppResult,
    feishu_sync, mobile_push,
    models::{
        AppSettings, CaptureCommitResult, CreateFeedInput, CreateFeedResult, DeleteConfirmation,
        FeedEvent, FeishuSourceStatus, FeishuSyncStatus, MemoryDetail, MemorySummary, PlanItem,
        ProcessResult, ReviewItem, Stats, UpdateSettingsInput,
    },
    windows_selection::{self, SelectionSnapshot},
    AppState, PendingCapture,
};

const PLAN_ROUTE_THRESHOLD: f64 = 0.68;
const APPLICATION_ROUTE_THRESHOLD: f64 = 0.82;

#[tauri::command]
pub fn create_feed(
    input: CreateFeedInput,
    state: State<'_, AppState>,
) -> AppResult<CreateFeedResult> {
    state.database.create_feed(input)
}

#[tauri::command]
pub fn list_feeds(
    query: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<FeedEvent>> {
    state.database.list_feeds(query, limit.unwrap_or(100))
}

#[tauri::command]
pub fn list_memories(
    query: Option<String>,
    memory_type: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<MemorySummary>> {
    state
        .database
        .list_memories(query, memory_type, limit.unwrap_or(200))
}

#[tauri::command]
pub fn get_memory(memory_id: String, state: State<'_, AppState>) -> AppResult<MemoryDetail> {
    state.database.get_memory(&memory_id)
}

#[tauri::command]
pub fn list_reviews(state: State<'_, AppState>) -> AppResult<Vec<ReviewItem>> {
    state.database.list_reviews()
}

#[tauri::command]
pub fn resolve_review(
    review_id: String,
    accept: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.database.resolve_review(&review_id, accept)
}

#[tauri::command]
pub fn request_delete_feed(
    feed_id: String,
    state: State<'_, AppState>,
) -> AppResult<DeleteConfirmation> {
    state.database.get_feed_for_processing(&feed_id)?;
    let token = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now().timestamp_millis() + 120_000;
    state
        .delete_confirmations
        .lock()
        .expect("delete confirmation lock poisoned")
        .insert(token.clone(), (feed_id, expires_at));
    Ok(DeleteConfirmation { token, expires_at })
}

#[tauri::command]
pub fn delete_feed(
    feed_id: String,
    confirmation_token: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let confirmation = state
        .delete_confirmations
        .lock()
        .expect("delete confirmation lock poisoned")
        .remove(&confirmation_token);
    let now = chrono::Utc::now().timestamp_millis();
    match confirmation {
        Some((confirmed_feed_id, expires_at))
            if confirmed_feed_id == feed_id && expires_at >= now => {}
        _ => {
            return Err(crate::error::AppError::Validation(
                "删除确认已失效，请重新确认".to_string(),
            ));
        }
    }
    state.database.delete_feed(&feed_id)
}

#[tauri::command]
pub fn get_stats(state: State<'_, AppState>) -> AppResult<Stats> {
    state.database.get_stats()
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.database.get_settings()
}

#[tauri::command]
pub fn update_settings(input: UpdateSettingsInput, state: State<'_, AppState>) -> AppResult<()> {
    state.database.update_settings(&AppSettings {
        ai_enabled: input.ai_enabled,
        llm_endpoint: input.llm_endpoint,
        llm_model: input.llm_model,
        embedding_endpoint: input.embedding_endpoint,
        embedding_model: input.embedding_model,
        embedding_dimensions: input.embedding_dimensions,
        mobile_push_enabled: input.mobile_push_enabled,
        mobile_push_provider: input.mobile_push_provider,
        mobile_reminder_minutes: input.mobile_reminder_minutes,
        feishu_sync_enabled: input.feishu_sync_enabled,
        feishu_task_reminders_enabled: input.feishu_task_reminders_enabled,
        feishu_source_enabled: input.feishu_source_enabled,
        feishu_source_url: input.feishu_source_url,
    })
}

#[tauri::command]
pub async fn check_ai(state: State<'_, AppState>) -> AppResult<String> {
    let settings = state.database.get_settings()?;
    let secrets = ai::ProviderSecrets::load(&state.secrets_path)?;
    ai::healthcheck(&settings, &secrets).await
}

#[tauri::command]
pub async fn test_mobile_push(provider: String, state: State<'_, AppState>) -> AppResult<String> {
    mobile_push::send_test(&provider, &state.secrets_path).await
}

#[tauri::command]
pub fn get_feishu_sync_status(state: State<'_, AppState>) -> AppResult<FeishuSyncStatus> {
    feishu_sync::status(&state.database, &state.secrets_path)
}

#[tauri::command]
pub async fn sync_feishu_now(state: State<'_, AppState>) -> AppResult<String> {
    feishu_sync::sync_now(&state.database, &state.secrets_path, &state.feishu_syncing).await
}

#[tauri::command]
pub fn get_feishu_source_status(state: State<'_, AppState>) -> AppResult<FeishuSourceStatus> {
    feishu_sync::source_status(&state.database, &state.secrets_path)
}

#[tauri::command]
pub async fn sync_feishu_source_now(state: State<'_, AppState>) -> AppResult<String> {
    feishu_sync::check_application_target(&state.database, &state.secrets_path).await
}

#[tauri::command]
pub async fn process_feed(feed_id: String, state: State<'_, AppState>) -> AppResult<ProcessResult> {
    process_feed_inner(&state.database, &state.secrets_path, &feed_id).await
}

async fn process_feed_inner(
    database: &crate::db::Database,
    secrets_path: &Path,
    feed_id: &str,
) -> AppResult<ProcessResult> {
    let settings = database.get_settings()?;
    if !settings.ai_enabled {
        return Ok(ProcessResult {
            status: "disabled".to_string(),
            message: "AI 整理已关闭，原始记录已安全保存".to_string(),
            review_id: None,
        });
    }
    let (raw_content, memory_id) = database.get_feed_for_processing(feed_id)?;
    let candidates = database.recent_context(&memory_id, 6)?;
    let run_id = database.start_processing(feed_id, &settings.llm_model)?;
    let secrets = ai::ProviderSecrets::load(secrets_path)?;
    let proposal = match ai::propose(&settings, &secrets, &raw_content, &candidates).await {
        Ok(proposal) => proposal,
        Err(error) => {
            database.fail_processing(&run_id, feed_id, &error.to_string())?;
            return Err(error);
        }
    };
    if proposal.action == "ask" {
        let review_id = match database.create_review(
            &run_id,
            feed_id,
            &memory_id,
            &proposal,
            &settings.llm_model,
        ) {
            Ok(review_id) => review_id,
            Err(error) => {
                database.fail_processing(&run_id, feed_id, &error.to_string())?;
                return Err(error);
            }
        };
        Ok(ProcessResult {
            status: "review".to_string(),
            message: "这条内容有歧义，已进入待澄清区".to_string(),
            review_id: Some(review_id),
        })
    } else {
        if let Err(error) =
            database.apply_ai_proposal(&run_id, feed_id, &memory_id, &proposal, &settings.llm_model)
        {
            database.fail_processing(&run_id, feed_id, &error.to_string())?;
            return Err(error);
        }
        Ok(ProcessResult {
            status: "classified".to_string(),
            message: "已自动理解并归入记忆".to_string(),
            review_id: None,
        })
    }
}

#[tauri::command]
pub fn prepare_capture(app: AppHandle, state: State<'_, AppState>) -> AppResult<SelectionSnapshot> {
    state.selection_suppressed.store(true, Ordering::Relaxed);
    let snapshot = match windows_selection::read_current_selection() {
        Ok(snapshot) => snapshot,
        Err(message) => {
            state.selection_suppressed.store(false, Ordering::Relaxed);
            return Err(crate::error::AppError::Validation(message));
        }
    };
    *state
        .pending_capture
        .lock()
        .expect("pending capture lock poisoned") = Some(PendingCapture {
        snapshot: snapshot.clone(),
        feed: None,
    });
    show_capture_menu(&app);
    let _ = app.emit_to("capture-menu", "capture-prepared", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn discard_capture(app: AppHandle, state: State<'_, AppState>) {
    *state
        .pending_capture
        .lock()
        .expect("pending capture lock poisoned") = None;
    close_capture_menu(&app, &state);
}

#[tauri::command]
pub fn get_capture_preview(state: State<'_, AppState>) -> Option<SelectionSnapshot> {
    state
        .pending_capture
        .lock()
        .expect("pending capture lock poisoned")
        .as_ref()
        .map(|capture| capture.snapshot.clone())
}

#[tauri::command]
pub async fn commit_capture(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CaptureCommitResult> {
    let snapshot = state
        .pending_capture
        .lock()
        .expect("pending capture lock poisoned")
        .as_ref()
        .map(|capture| capture.snapshot.clone())
        .ok_or_else(|| {
            crate::error::AppError::Validation("选区授权已失效，请重新选择".to_string())
        })?;

    let feed = {
        let existing = state
            .pending_capture
            .lock()
            .expect("pending capture lock poisoned")
            .as_ref()
            .and_then(|capture| capture.feed.clone());
        if let Some(feed) = existing {
            feed
        } else {
            let feed = state.database.create_selection_feed(
                &snapshot.selected_text,
                &snapshot.surrounding_text,
                &snapshot.source_title,
            )?;
            if let Some(capture) = state
                .pending_capture
                .lock()
                .expect("pending capture lock poisoned")
                .as_mut()
            {
                capture.feed = Some(feed.clone());
            }
            feed
        }
    };

    let settings = state.database.get_settings()?;
    if !settings.ai_enabled {
        return Err(crate::error::AppError::Validation(
            "AI 已关闭，无法从上下文安排时间".to_string(),
        ));
    }
    let secrets = ai::ProviderSecrets::load(&state.secrets_path)?;
    let routing = ai::route_capture(
        &settings,
        &secrets,
        &snapshot.selected_text,
        &snapshot.surrounding_text,
        &now_in_shanghai(),
    )
    .await?;

    let application_route_accepted = routing.write_application_record
        && routing.application_confidence >= APPLICATION_ROUTE_THRESHOLD;
    let application_channel_ready =
        settings.feishu_source_enabled && !settings.feishu_source_url.trim().is_empty();
    let should_write_application = application_route_accepted && application_channel_ready;
    let should_create_plan = routing.create_plan && routing.plan_confidence >= PLAN_ROUTE_THRESHOLD;
    let application_record = if should_write_application {
        Some(
            feishu_sync::write_application_record(
                &state.database,
                &state.secrets_path,
                routing.application_record.as_ref().ok_or_else(|| {
                    crate::error::AppError::AiInvalid("投递记录路由缺少结构化内容".to_string())
                })?,
            )
            .await?,
        )
    } else {
        None
    };
    let (plan, scheduled_at) = if should_create_plan {
        let proposal = routing.plan.as_ref().ok_or_else(|| {
            crate::error::AppError::AiInvalid("计划路由缺少结构化内容".to_string())
        })?;
        let scheduled_at = parse_scheduled_at(proposal.scheduled_for.as_deref())?;
        let plan = state.database.create_plan(
            &feed.feed_id,
            proposal,
            scheduled_at,
            &snapshot.source_title,
        )?;
        (Some(plan), scheduled_at)
    } else {
        (None, None)
    };

    let database = state.database.clone();
    let secrets_path = state.secrets_path.clone();
    let feed_id = feed.feed_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = process_feed_inner(&database, &secrets_path, &feed_id).await;
    });

    if let Some(plan) = &plan {
        let _ = app.emit("plans-changed", plan);
    }
    *state
        .pending_capture
        .lock()
        .expect("pending capture lock poisoned") = None;
    let destination = match (application_record.is_some(), plan.is_some()) {
        (true, true) => "application_and_plan",
        (true, false) => "application",
        (false, true) => "plan",
        (false, false) => "memory",
    };
    let mut message = match destination {
        "application_and_plan" => "已写入投递记录，并创建桌面计划",
        "application" => "已写入飞书投递记录表",
        "plan" => "已创建桌面计划",
        _ if application_route_accepted => "已保存到记忆库",
        _ if routing.write_application_record => "投递判断未达到写入阈值，已安全保存到记忆库",
        _ if routing.create_plan => "计划判断未达到创建阈值，已安全保存到记忆库",
        _ => "未识别为待办或投递记录，已保存到记忆库",
    }
    .to_string();
    if application_route_accepted && !application_channel_ready {
        message.push_str("；投递记录同步未开启，本次未写入投递表");
    }
    let needs_clarification = plan.is_some() && scheduled_at.is_none();
    Ok(CaptureCommitResult {
        destination: destination.to_string(),
        message,
        plan,
        application_record,
        needs_clarification,
    })
}

#[tauri::command]
pub async fn resolve_plan_time(
    plan_id: String,
    answer: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CaptureCommitResult> {
    if answer.trim().is_empty() || answer.chars().count() > 500 {
        return Err(crate::error::AppError::Validation(
            "请用 500 字以内说明安排时间".to_string(),
        ));
    }
    let current = state.database.get_plan(&plan_id)?;
    let settings = state.database.get_settings()?;
    let secrets = ai::ProviderSecrets::load(&state.secrets_path)?;
    let proposal = ai::resolve_plan_time(
        &settings,
        &secrets,
        &current,
        answer.trim(),
        &now_in_shanghai(),
    )
    .await?;
    let scheduled_at = parse_scheduled_at(proposal.scheduled_for.as_deref())?;
    let plan = match scheduled_at {
        Some(timestamp) => state
            .database
            .schedule_plan(&plan_id, &proposal, timestamp)?,
        None => state
            .database
            .update_plan_clarification(&plan_id, &proposal)?,
    };
    let _ = app.emit("plans-changed", &plan);
    if scheduled_at.is_some() {
        close_capture_menu(&app, &state);
    }
    Ok(CaptureCommitResult {
        destination: "plan".to_string(),
        message: if scheduled_at.is_some() {
            "计划时间已补充".to_string()
        } else {
            "仍需补充计划时间".to_string()
        },
        needs_clarification: scheduled_at.is_none(),
        plan: Some(plan),
        application_record: None,
    })
}

#[tauri::command]
pub fn list_plans(
    include_done: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<PlanItem>> {
    state.database.list_plans(include_done.unwrap_or(false))
}

#[tauri::command]
pub fn set_plan_done(
    plan_id: String,
    done: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<PlanItem> {
    let plan = state.database.set_plan_done(&plan_id, done)?;
    let _ = app.emit("plans-changed", &plan);
    Ok(plan)
}

#[tauri::command]
pub fn toggle_plan_dock(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let expanded = !state.dock_expanded.load(Ordering::Relaxed);
    state.dock_expanded.store(expanded, Ordering::Relaxed);
    resize_plan_dock(&app, expanded);
    Ok(expanded)
}

#[tauri::command]
pub fn open_main_window(app: AppHandle) {
    crate::show_main_window(&app);
}

#[tauri::command]
pub fn open_external_link(url: String) -> AppResult<()> {
    let url = reqwest::Url::parse(url.trim())
        .map_err(|_| crate::error::AppError::Validation("链接不是有效网址".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(crate::error::AppError::Validation(
            "只允许打开 http 或 https 链接".to_string(),
        ));
    }
    #[cfg(target_os = "windows")]
    {
        use windows::{
            core::HSTRING,
            Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        };
        let target = HSTRING::from(url.as_str());
        let result = unsafe { ShellExecuteW(None, None, &target, None, None, SW_SHOWNORMAL) };
        if result.0 as isize <= 32 {
            return Err(crate::error::AppError::Validation(
                "系统浏览器未能打开该链接".to_string(),
            ));
        }
    }
    Ok(())
}

fn show_capture_menu(app: &AppHandle) {
    let Some(dot) = app.get_webview_window("capture-dot") else {
        return;
    };
    let Some(menu) = app.get_webview_window("capture-menu") else {
        return;
    };
    let position = dot
        .outer_position()
        .unwrap_or(PhysicalPosition::new(200, 200));
    let (mut x, mut y) = (position.x + 24, position.y + 20);
    if let Ok(Some(monitor)) = dot.current_monitor() {
        let origin = monitor.position();
        let size = monitor.size();
        let right = origin.x + size.width as i32;
        let bottom = origin.y + size.height as i32;
        if x + 300 > right {
            x = position.x - 292;
        }
        if y + 180 > bottom {
            y = position.y - 174;
        }
        x = x.max(origin.x + 8);
        y = y.max(origin.y + 8);
    }
    let _ = dot.hide();
    let _ = menu.set_position(PhysicalPosition::new(x, y));
    let _ = menu.show();
    let _ = menu.set_focus();
}

fn close_capture_menu(app: &AppHandle, state: &AppState) {
    if let Some(menu) = app.get_webview_window("capture-menu") {
        let _ = menu.hide();
    }
    state.selection_suppressed.store(false, Ordering::Relaxed);
}

pub(crate) fn resize_plan_dock(app: &AppHandle, expanded: bool) {
    let Some(window) = app.get_webview_window("plan-dock") else {
        return;
    };
    let old_position = window.outer_position().ok();
    let old_size = window.outer_size().ok();
    let (width, height) = if expanded {
        (380_u32, 520_u32)
    } else {
        (58, 58)
    };
    let _ = window.set_size(PhysicalSize::new(width, height));
    if let (Some(position), Some(size)) = (old_position, old_size) {
        let right = position.x + size.width as i32;
        let _ = window.set_position(PhysicalPosition::new(right - width as i32, position.y));
    }
    let _ = window.show();
}

pub(crate) fn initialize_plan_dock(app: &AppHandle) {
    let Some(window) = app.get_webview_window("plan-dock") else {
        return;
    };
    let width = 58_u32;
    let height = 58_u32;
    let _ = window.set_size(PhysicalSize::new(width, height));
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let origin = monitor.position();
        let size = monitor.size();
        let x = origin.x + size.width as i32 - width as i32;
        let y = origin.y + ((size.height as i32 - height as i32) / 2);
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
}

fn now_in_shanghai() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 60 * 60).expect("valid Shanghai offset");
    chrono::Utc::now().with_timezone(&offset).to_rfc3339()
}

fn parse_scheduled_at(value: Option<&str>) -> AppResult<Option<i64>> {
    value
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(value.trim())
                .map(|time| time.timestamp_millis())
                .map_err(|_| {
                    crate::error::AppError::AiInvalid("计划时间不是有效的 RFC3339".to_string())
                })
        })
        .transpose()
}

#[tauri::command]
pub fn export_data(path: String, state: State<'_, AppState>) -> AppResult<()> {
    state.database.export_json(&PathBuf::from(path))
}
