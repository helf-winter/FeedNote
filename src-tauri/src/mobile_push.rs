use std::{path::Path, sync::Arc, time::Duration};

use chrono::{FixedOffset, TimeZone, Utc};
use reqwest::{Client, Url};
use serde_json::json;

use crate::{
    db::Database,
    error::{AppError, AppResult},
    models::PlanItem,
    secrets::parse_env,
};

struct MobilePushSecrets {
    endpoint: Url,
    token: Option<String>,
}

impl MobilePushSecrets {
    fn load(path: &Path) -> AppResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            AppError::PushUnavailable(format!("无法读取 {}：{}", path.display(), error))
        })?;
        let values = parse_env(&content);
        let endpoint = values
            .get("MOBILE_PUSH_ENDPOINT")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AppError::PushUnavailable(
                    "data\\secrets.env 中缺少 MOBILE_PUSH_ENDPOINT".to_string(),
                )
            })?;
        let endpoint = validate_endpoint(endpoint)?;
        let token = values
            .get("MOBILE_PUSH_TOKEN")
            .filter(|value| !value.trim().is_empty())
            .cloned();
        Ok(Self { endpoint, token })
    }
}

pub fn start_scheduler(database: Arc<Database>, secrets_path: std::path::PathBuf) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let _ = deliver_due(&database, &secrets_path).await;
        }
    });
}

async fn deliver_due(database: &Database, secrets_path: &Path) -> AppResult<()> {
    let settings = database.get_settings()?;
    if !settings.mobile_push_enabled {
        return Ok(());
    }
    let secrets = MobilePushSecrets::load(secrets_path)?;
    let now = Utc::now().timestamp_millis();
    let due_at = now + i64::from(settings.mobile_reminder_minutes) * 60_000;
    for plan in database.list_due_plan_reminders(now, due_at)? {
        send_plan(&settings.mobile_push_provider, &secrets, &plan).await?;
        database.mark_plan_reminded(&plan.id, Utc::now().timestamp_millis())?;
    }
    Ok(())
}

pub async fn send_test(provider: &str, secrets_path: &Path) -> AppResult<String> {
    let secrets = MobilePushSecrets::load(secrets_path)?;
    send_payload(
        provider,
        &secrets,
        "FeedNote 手机提醒测试",
        "连接成功。之后到期计划会按设置的提前量发送到这里。",
        None,
        None,
    )
    .await?;
    Ok("测试提醒已发送，请检查手机".to_string())
}

async fn send_plan(provider: &str, secrets: &MobilePushSecrets, plan: &PlanItem) -> AppResult<()> {
    let scheduled_at = plan
        .scheduled_at
        .ok_or_else(|| AppError::PushUnavailable("待提醒计划没有时间".to_string()))?;
    let time = format_time(scheduled_at);
    let content = compact_content(plan);
    let mut lines = vec![format!("时间：{time}"), format!("内容：{content}")];
    if let Some(notes) = plan
        .notes
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("注意：{notes}"));
    }
    send_payload(
        provider,
        secrets,
        &plan.title,
        &lines.join("\n"),
        plan.link_url.as_deref(),
        Some(scheduled_at),
    )
    .await
}

async fn send_payload(
    provider: &str,
    secrets: &MobilePushSecrets,
    title: &str,
    body: &str,
    link_url: Option<&str>,
    scheduled_at: Option<i64>,
) -> AppResult<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| AppError::PushUnavailable(error.to_string()))?;
    let mut request = match provider {
        "ntfy" => {
            let mut request = client
                .post(secrets.endpoint.clone())
                .header("X-Title", "FeedNote reminder")
                .header("X-Priority", "high")
                .body(format!("{title}\n{body}"));
            if let Some(link_url) = link_url {
                request = request.header("X-Click", link_url);
            }
            request
        }
        "webhook" => client.post(secrets.endpoint.clone()).json(&json!({
            "event": "plan.reminder",
            "title": title,
            "body": body,
            "linkUrl": link_url,
            "scheduledAt": scheduled_at,
        })),
        _ => {
            return Err(AppError::PushUnavailable(
                "推送通道只能是 ntfy 或 webhook".to_string(),
            ))
        }
    };
    if let Some(token) = &secrets.token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| AppError::PushUnavailable(clean_network_error(&error)))?;
    if !response.status().is_success() {
        return Err(AppError::PushUnavailable(format!(
            "推送服务返回 {}",
            response.status()
        )));
    }
    Ok(())
}

fn validate_endpoint(value: &str) -> AppResult<Url> {
    let url = Url::parse(value.trim())
        .map_err(|_| AppError::PushUnavailable("推送地址不是有效 URL".to_string()))?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost"));
    if url.scheme() != "https" && !local_http {
        return Err(AppError::PushUnavailable(
            "推送地址必须使用 HTTPS，本机服务可使用 HTTP".to_string(),
        ));
    }
    Ok(url)
}

fn format_time(timestamp: i64) -> String {
    let offset = FixedOffset::east_opt(8 * 60 * 60).expect("valid Shanghai offset");
    offset
        .timestamp_millis_opt(timestamp)
        .single()
        .unwrap_or_else(|| Utc::now().with_timezone(&offset))
        .format("%m月%d日 %H:%M")
        .to_string()
}

fn compact_content(plan: &PlanItem) -> String {
    if !plan.content.trim().is_empty() {
        return plan.content.trim().to_string();
    }
    for known_type in [
        "AI面",
        "AI 面",
        "笔试",
        "面试",
        "电话面",
        "视频面",
        "会议",
        "答辩",
        "考试",
        "复试",
    ] {
        if plan.details.contains(known_type) {
            return known_type.replace(' ', "");
        }
    }
    let sentence = plan
        .details
        .split(['。', '！', '？', '\n'])
        .next()
        .unwrap_or(&plan.details)
        .trim();
    let compact = sentence.chars().take(60).collect::<String>();
    if compact.is_empty() {
        "计划提醒".to_string()
    } else {
        compact
    }
}

fn clean_network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "推送服务响应超时".to_string()
    } else if error.is_connect() {
        "无法连接推送服务".to_string()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allows_https_or_loopback_push_endpoints() {
        assert!(validate_endpoint("https://ntfy.sh/private-topic").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8080/hook").is_ok());
        assert!(validate_endpoint("http://example.com/hook").is_err());
        assert!(validate_endpoint("file:///tmp/secret").is_err());
    }
}
