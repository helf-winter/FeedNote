use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("数据库操作失败：{0}")]
    Database(#[from] rusqlite::Error),
    #[error("输入无效：{0}")]
    Validation(String),
    #[error("记录不存在：{0}")]
    NotFound(String),
    #[error("AI 模型不可用：{0}")]
    AiUnavailable(String),
    #[error("模型返回无法解析：{0}")]
    AiInvalid(String),
    #[error("手机推送不可用：{0}")]
    PushUnavailable(String),
    #[error("飞书同步不可用：{0}")]
    FeishuUnavailable(String),
    #[error("秘密保险箱不可用：{0}")]
    Vault(String),
    #[error("系统集成失败：{0}")]
    SystemIntegration(String),
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("序列化失败：{0}")]
    Serialization(#[from] serde_json::Error),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
