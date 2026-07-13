use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::utils::paths;

const HISTORY_DB_FILE: &str = "transcription_history.sqlite3";
const HISTORY_AUDIO_DIR: &str = "history_audio";
const MAX_PAGE_SIZE: u32 = 200;
const HISTORY_SCHEMA_VERSION: i64 = 2;

static HISTORY_INITIALIZED: OnceCell<()> = OnceCell::const_new();

#[derive(Debug, Clone)]
pub struct HistoryDraft {
    pub session_id: u64,
    pub mode: String,
    pub workflow: String,
    pub status: String,
    pub text: String,
    pub original_text: String,
    pub source_text: Option<String>,
    pub duration_sec: Option<f64>,
    pub language: Option<String>,
    pub engine: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub app_process: Option<String>,
    pub app_window_title: Option<String>,
    pub app_rule_name: Option<String>,
    pub audio_file: Option<String>,
    pub asr_ms: Option<u64>,
    pub polish_ms: Option<u64>,
    pub total_ms: Option<u64>,
    pub raw_first_status: Option<String>,
    pub error: Option<String>,
    pub reprocessed_from_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub id: i64,
    pub session_id: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub mode: String,
    pub workflow: String,
    pub status: String,
    pub text: String,
    pub original_text: String,
    pub source_text: Option<String>,
    pub duration_sec: Option<f64>,
    pub language: Option<String>,
    pub engine: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub app_process: Option<String>,
    pub app_window_title: Option<String>,
    pub app_rule_name: Option<String>,
    pub audio_available: bool,
    pub asr_ms: Option<u64>,
    pub polish_ms: Option<u64>,
    pub total_ms: Option<u64>,
    pub raw_first_status: Option<String>,
    pub error: Option<String>,
    pub reprocessed_from_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct StoredHistoryRecord {
    pub record: HistoryRecord,
    pub audio_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub status: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistoryRecord>,
    pub total: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStats {
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub total_characters: u64,
    pub asr: LatencyStats,
    pub polish: LatencyStats,
    pub total_latency: LatencyStats,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn history_db_path() -> PathBuf {
    paths::get_data_dir().join(HISTORY_DB_FILE)
}

fn history_audio_dir() -> PathBuf {
    paths::get_data_dir().join(HISTORY_AUDIO_DIR)
}

fn safe_audio_path(file_name: &str) -> Option<PathBuf> {
    let candidate = Path::new(file_name);
    if candidate.components().count() != 1 || candidate.file_name()?.to_string_lossy() != file_name
    {
        return None;
    }
    Some(history_audio_dir().join(candidate))
}

fn open_connection() -> Result<Connection, String> {
    std::fs::create_dir_all(paths::get_data_dir())
        .map_err(|error| format!("创建历史数据目录失败: {error}"))?;
    let connection = Connection::open(history_db_path())
        .map_err(|error| format!("打开历史数据库失败: {error}"))?;
    configure_runtime_connection(&connection)?;
    Ok(connection)
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    configure_runtime_connection(connection)?;
    migrate_schema(connection)
}

fn configure_runtime_connection(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("设置历史数据库等待时间失败: {error}"))?;
    connection
        .execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|error| format!("配置历史数据库连接失败: {error}"))?;
    Ok(())
}

fn table_has_column(connection: &Connection, column_name: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(transcription_history)")
        .map_err(|error| format!("读取历史数据库结构失败: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("查询历史数据库结构失败: {error}"))?;
    for column in columns {
        if column.map_err(|error| format!("解析历史数据库结构失败: {error}"))? == column_name
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA journal_mode = WAL;")
        .map_err(|error| format!("启用历史数据库 WAL 失败: {error}"))?;
    let current_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("读取历史数据库版本失败: {error}"))?;
    if current_version > HISTORY_SCHEMA_VERSION {
        return Err(format!(
            "历史数据库版本 {current_version} 高于当前支持的 {HISTORY_SCHEMA_VERSION}"
        ));
    }
    if current_version == HISTORY_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始历史数据库迁移失败: {error}"))?;
    transaction
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS transcription_history (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id          INTEGER NOT NULL,
                created_at          INTEGER NOT NULL,
                updated_at          INTEGER NOT NULL,
                mode                TEXT NOT NULL,
                status              TEXT NOT NULL,
                text                TEXT NOT NULL,
                original_text       TEXT NOT NULL,
                source_text         TEXT,
                duration_sec        REAL,
                language            TEXT,
                engine              TEXT NOT NULL,
                provider            TEXT,
                model               TEXT,
                app_process         TEXT,
                app_window_title    TEXT,
                app_rule_name       TEXT,
                audio_file          TEXT,
                asr_ms              INTEGER,
                polish_ms           INTEGER,
                total_ms            INTEGER,
                raw_first_status    TEXT,
                error               TEXT,
                reprocessed_from_id INTEGER,
                workflow            TEXT NOT NULL DEFAULT 'dictation'
            );
            CREATE INDEX IF NOT EXISTS idx_history_created_at
                ON transcription_history(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_history_mode_status
                ON transcription_history(mode, status);
            CREATE INDEX IF NOT EXISTS idx_history_audio_file
                ON transcription_history(audio_file);
            CREATE TABLE IF NOT EXISTS history_audio_leases (
                audio_file TEXT PRIMARY KEY,
                lease_count INTEGER NOT NULL CHECK (lease_count > 0),
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .map_err(|error| format!("初始化历史数据库失败: {error}"))?;

    if !table_has_column(&transaction, "workflow")? {
        transaction
            .execute(
                "ALTER TABLE transcription_history ADD COLUMN workflow TEXT NOT NULL DEFAULT 'dictation'",
                [],
            )
            .map_err(|error| format!("迁移历史处理类型失败: {error}"))?;
    }
    if !table_has_column(&transaction, "source_text")? {
        transaction
            .execute(
                "ALTER TABLE transcription_history ADD COLUMN source_text TEXT",
                [],
            )
            .map_err(|error| format!("迁移历史编辑原文失败: {error}"))?;
    }
    transaction
        .execute(
            "UPDATE transcription_history SET workflow = 'assistant' WHERE mode = 'assistant' AND workflow = 'dictation'",
            [],
        )
        .map_err(|error| format!("迁移助手历史处理类型失败: {error}"))?;
    transaction
        .pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION)
        .map_err(|error| format!("更新历史数据库版本失败: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交历史数据库迁移失败: {error}"))
}

pub async fn initialize() -> Result<(), String> {
    HISTORY_INITIALIZED
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(|| {
                let connection = open_connection()?;
                configure_connection(&connection)?;
                // 初始化完成前所有历史操作都会等待此 OnceCell，因此这里清理的
                // 只能是上次进程崩溃遗留的租约和孤立音频。
                connection
                    .execute("DELETE FROM history_audio_leases", [])
                    .map_err(|error| format!("清理过期历史音频租约失败: {error}"))?;
                cleanup_orphan_audio_files_with_connection(&connection);
                Ok(())
            })
            .await
            .map_err(|error| format!("初始化历史数据库任务失败: {error}"))?
        })
        .await
        .map(|_| ())
}

pub async fn save_audio(session_id: u64, wav: Vec<u8>) -> Result<String, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let directory = history_audio_dir();
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("创建历史音频目录失败: {error}"))?;
        let file_name = format!("{}-{session_id}.wav", now_millis());
        let path = directory.join(&file_name);
        paths::atomic_write(&path, &wav).map_err(|error| format!("保存历史音频失败: {error}"))?;
        Ok(file_name)
    })
    .await
    .map_err(|error| format!("保存历史音频任务失败: {error}"))?
}

pub async fn read_audio(file_name: &str) -> Result<Vec<u8>, String> {
    initialize().await?;
    let path = safe_audio_path(file_name).ok_or_else(|| "历史音频路径无效".to_string())?;
    tokio::fs::read(path)
        .await
        .map_err(|error| format!("读取历史音频失败: {error}"))
}

fn optional_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn map_stored_record(row: &Row<'_>) -> rusqlite::Result<StoredHistoryRecord> {
    let audio_file: Option<String> = row.get(15)?;
    let audio_available = audio_file
        .as_deref()
        .and_then(safe_audio_path)
        .is_some_and(|path| path.is_file());
    Ok(StoredHistoryRecord {
        record: HistoryRecord {
            id: row.get(0)?,
            session_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            mode: row.get(4)?,
            workflow: row.get(23)?,
            status: row.get(5)?,
            text: row.get(6)?,
            original_text: row.get(7)?,
            source_text: row.get(24)?,
            duration_sec: row.get(8)?,
            language: row.get(9)?,
            engine: row.get(10)?,
            provider: row.get(11)?,
            model: row.get(12)?,
            app_process: row.get(13)?,
            app_window_title: row.get(14)?,
            app_rule_name: row.get(16)?,
            audio_available,
            asr_ms: optional_u64(row.get(17)?),
            polish_ms: optional_u64(row.get(18)?),
            total_ms: optional_u64(row.get(19)?),
            raw_first_status: row.get(20)?,
            error: row.get(21)?,
            reprocessed_from_id: row.get(22)?,
        },
        audio_file,
    })
}

const HISTORY_COLUMNS: &str = r#"
    id, session_id, created_at, updated_at, mode, status, text, original_text,
    duration_sec, language, engine, provider, model, app_process, app_window_title,
    audio_file, app_rule_name, asr_ms, polish_ms, total_ms, raw_first_status,
    error, reprocessed_from_id, workflow, source_text
"#;

pub async fn insert(draft: HistoryDraft, retention_days: u32) -> Result<i64, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let mut connection = open_connection()?;
        let now = now_millis();
        connection
            .execute(
                r#"
                INSERT INTO transcription_history (
                    session_id, created_at, updated_at, mode, status, text, original_text,
                    duration_sec, language, engine, provider, model, app_process,
                    app_window_title, app_rule_name, audio_file, asr_ms, polish_ms,
                    total_ms, raw_first_status, error, reprocessed_from_id, workflow,
                    source_text
                ) VALUES (
                    ?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23
                )
                "#,
                params![
                    i64::try_from(draft.session_id).unwrap_or(i64::MAX),
                    now,
                    draft.mode,
                    draft.status,
                    draft.text,
                    draft.original_text,
                    draft.duration_sec,
                    draft.language,
                    draft.engine,
                    draft.provider,
                    draft.model,
                    draft.app_process,
                    draft.app_window_title,
                    draft.app_rule_name,
                    draft.audio_file,
                    draft.asr_ms.and_then(|value| i64::try_from(value).ok()),
                    draft.polish_ms.and_then(|value| i64::try_from(value).ok()),
                    draft.total_ms.and_then(|value| i64::try_from(value).ok()),
                    draft.raw_first_status,
                    draft.error,
                    draft.reprocessed_from_id,
                    draft.workflow,
                    draft.source_text,
                ],
            )
            .map_err(|error| format!("写入转写历史失败: {error}"))?;
        let id = connection.last_insert_rowid();
        if let Err(error) = cleanup_expired_with_connection(&mut connection, retention_days) {
            log::warn!("历史记录已保存，但自动清理失败，将在下次启动重试: {error}");
        }
        Ok(id)
    })
    .await
    .map_err(|error| format!("写入转写历史任务失败: {error}"))?
}

pub async fn get(id: i64) -> Result<Option<StoredHistoryRecord>, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let connection = open_connection()?;
        connection
            .query_row(
                &format!("SELECT {HISTORY_COLUMNS} FROM transcription_history WHERE id = ?1"),
                params![id],
                map_stored_record,
            )
            .optional()
            .map_err(|error| format!("读取转写历史失败: {error}"))
    })
    .await
    .map_err(|error| format!("读取转写历史任务失败: {error}"))?
}

/// 原子读取待重处理记录并为其音频建立租约。删除命令与这里都使用 IMMEDIATE
/// 事务，因此不可能在“读到记录”和“保护音频”之间把最后一个 WAV 引用删掉。
fn get_for_reprocess_with_connection(
    connection: &mut Connection,
    id: i64,
) -> Result<Option<StoredHistoryRecord>, String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("开始历史重处理租约事务失败: {error}"))?;
    let stored = transaction
        .query_row(
            &format!("SELECT {HISTORY_COLUMNS} FROM transcription_history WHERE id = ?1"),
            params![id],
            map_stored_record,
        )
        .optional()
        .map_err(|error| format!("读取待重处理历史失败: {error}"))?;
    if let Some(audio_file) = stored
        .as_ref()
        .and_then(|value| value.audio_file.as_deref())
    {
        transaction
            .execute(
                r#"
                    INSERT INTO history_audio_leases (audio_file, lease_count, updated_at)
                    VALUES (?1, 1, ?2)
                    ON CONFLICT(audio_file) DO UPDATE SET
                        lease_count = lease_count + 1,
                        updated_at = excluded.updated_at
                    "#,
                params![audio_file, now_millis()],
            )
            .map_err(|error| format!("建立历史音频租约失败: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交历史音频租约失败: {error}"))?;
    Ok(stored)
}

pub async fn get_for_reprocess(id: i64) -> Result<Option<StoredHistoryRecord>, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let mut connection = open_connection()?;
        get_for_reprocess_with_connection(&mut connection, id)
    })
    .await
    .map_err(|error| format!("读取待重处理历史任务失败: {error}"))?
}

fn release_audio_lease_with_connection(
    connection: &mut Connection,
    audio_file: &str,
) -> Result<(), String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("开始释放历史音频租约事务失败: {error}"))?;
    transaction
        .execute(
            "UPDATE history_audio_leases SET lease_count = lease_count - 1, updated_at = ?2 WHERE audio_file = ?1 AND lease_count > 1",
            params![audio_file, now_millis()],
        )
        .map_err(|error| format!("递减历史音频租约失败: {error}"))?;
    transaction
        .execute(
            "DELETE FROM history_audio_leases WHERE audio_file = ?1 AND lease_count = 1",
            params![audio_file],
        )
        .map_err(|error| format!("释放历史音频租约失败: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交历史音频租约释放失败: {error}"))?;
    cleanup_audio_if_unreferenced_with_connection(connection, audio_file);
    Ok(())
}

pub async fn release_audio_lease(audio_file: String) -> Result<(), String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let mut connection = open_connection()?;
        release_audio_lease_with_connection(&mut connection, &audio_file)
    })
    .await
    .map_err(|error| format!("释放历史音频租约任务失败: {error}"))?
}

pub async fn cleanup_audio_if_unreferenced(audio_file: String) -> Result<(), String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let connection = open_connection()?;
        cleanup_audio_if_unreferenced_with_connection(&connection, &audio_file);
        Ok(())
    })
    .await
    .map_err(|error| format!("回收未引用历史音频任务失败: {error}"))?
}

pub async fn list(query: HistoryQuery) -> Result<HistoryPage, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let connection = open_connection()?;
        let search = query.query.trim().to_string();
        let mode = query.mode.trim().to_string();
        let status = query.status.trim().to_string();
        let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
        let where_clause = r#"
            WHERE (
                ?1 = '' OR text LIKE '%' || ?1 || '%' OR original_text LIKE '%' || ?1 || '%'
                OR COALESCE(source_text, '') LIKE '%' || ?1 || '%'
                OR COALESCE(app_process, '') LIKE '%' || ?1 || '%'
                OR COALESCE(app_window_title, '') LIKE '%' || ?1 || '%'
            )
            AND (?2 = '' OR mode = ?2)
            AND (
                ?3 = '' OR status = ?3 OR (?3 = 'failed' AND status != 'success')
            )
        "#;
        let total: u64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM transcription_history {where_clause}"),
                params![search, mode, status],
                |row| row.get::<_, i64>(0),
            )
            .map(|value| u64::try_from(value).unwrap_or_default())
            .map_err(|error| format!("统计转写历史失败: {error}"))?;

        let sql = format!(
            "SELECT {HISTORY_COLUMNS} FROM transcription_history {where_clause} \
             ORDER BY created_at DESC, id DESC LIMIT ?4 OFFSET ?5"
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("准备历史查询失败: {error}"))?;
        let rows = statement
            .query_map(
                params![
                    search,
                    mode,
                    status,
                    i64::from(limit),
                    i64::from(query.offset)
                ],
                map_stored_record,
            )
            .map_err(|error| format!("查询转写历史失败: {error}"))?;
        let items = rows
            .map(|row| row.map(|stored| stored.record))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("解析转写历史失败: {error}"))?;
        let has_more = u64::from(query.offset) + (items.len() as u64) < total;
        Ok(HistoryPage {
            items,
            total,
            has_more,
        })
    })
    .await
    .map_err(|error| format!("查询转写历史任务失败: {error}"))?
}

fn percentile(values: &mut [u64], percentile: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values.get(index).copied()
}

fn latency_stats(mut values: Vec<u64>) -> LatencyStats {
    let mut p95_values = values.clone();
    LatencyStats {
        p50_ms: percentile(&mut values, 0.50),
        p95_ms: percentile(&mut p95_values, 0.95),
    }
}

pub async fn stats() -> Result<HistoryStats, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let connection = open_connection()?;
        let (total, success, failed, total_characters) = connection
            .query_row(
                r#"
                SELECT COUNT(*),
                       SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                       SUM(CASE WHEN status != 'success' THEN 1 ELSE 0 END),
                       SUM(LENGTH(text))
                FROM transcription_history
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                        row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                        row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                    ))
                },
            )
            .map_err(|error| format!("统计历史概览失败: {error}"))?;

        let mut asr = Vec::new();
        let mut polish = Vec::new();
        let mut total_latency = Vec::new();
        let mut statement = connection
            .prepare(
                "SELECT asr_ms, polish_ms, total_ms FROM transcription_history WHERE status = 'success'",
            )
            .map_err(|error| format!("准备历史延迟统计失败: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    optional_u64(row.get(0)?),
                    optional_u64(row.get(1)?),
                    optional_u64(row.get(2)?),
                ))
            })
            .map_err(|error| format!("查询历史延迟统计失败: {error}"))?;
        for row in rows {
            let (asr_ms, polish_ms, total_ms) =
                row.map_err(|error| format!("解析历史延迟统计失败: {error}"))?;
            asr.extend(asr_ms);
            polish.extend(polish_ms);
            total_latency.extend(total_ms);
        }

        Ok(HistoryStats {
            total: u64::try_from(total).unwrap_or_default(),
            success: u64::try_from(success).unwrap_or_default(),
            failed: u64::try_from(failed).unwrap_or_default(),
            total_characters: u64::try_from(total_characters).unwrap_or_default(),
            asr: latency_stats(asr),
            polish: latency_stats(polish),
            total_latency: latency_stats(total_latency),
        })
    })
    .await
    .map_err(|error| format!("统计转写历史任务失败: {error}"))?
}

fn cleanup_audio_if_unreferenced_with_connection(connection: &Connection, audio_file: &str) {
    let references = connection.query_row(
        "SELECT COUNT(*) FROM transcription_history WHERE audio_file = ?1",
        params![audio_file],
        |row| row.get::<_, i64>(0),
    );
    let leases = connection.query_row(
        "SELECT COALESCE(SUM(lease_count), 0) FROM history_audio_leases WHERE audio_file = ?1",
        params![audio_file],
        |row| row.get::<_, i64>(0),
    );
    let (Ok(references), Ok(leases)) = (references, leases) else {
        log::warn!("检查历史音频引用失败，保留文件等待下次启动清理: {audio_file}");
        return;
    };
    if references != 0 || leases != 0 {
        return;
    }
    let Some(path) = safe_audio_path(audio_file) else {
        log::warn!("拒绝清理非法历史音频路径: {audio_file}");
        return;
    };
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            // 数据库操作已经提交，文件回收属于可重试垃圾回收，不能把已经
            // 成功的删除伪装成整体失败。
            log::warn!("删除历史音频失败，保留到下次启动重试: {audio_file}: {error}");
        }
    }
}

fn cleanup_orphan_audio_files_with_connection(connection: &Connection) {
    let directory = history_audio_dir();
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            log::warn!("扫描历史音频目录失败: {error}");
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        cleanup_audio_if_unreferenced_with_connection(connection, file_name);
    }
}

pub async fn delete(id: i64) -> Result<bool, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let mut connection = open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始删除转写历史事务失败: {error}"))?;
        let audio_file: Option<String> = transaction
            .query_row(
                "SELECT audio_file FROM transcription_history WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取待删除历史失败: {error}"))?
            .flatten();
        let removed = transaction
            .execute(
                "DELETE FROM transcription_history WHERE id = ?1",
                params![id],
            )
            .map_err(|error| format!("删除转写历史失败: {error}"))?
            > 0;
        transaction
            .commit()
            .map_err(|error| format!("提交删除转写历史事务失败: {error}"))?;
        if let Some(audio_file) = audio_file {
            cleanup_audio_if_unreferenced_with_connection(&connection, &audio_file);
        }
        Ok(removed)
    })
    .await
    .map_err(|error| format!("删除转写历史任务失败: {error}"))?
}

fn cleanup_expired_with_connection(
    connection: &mut Connection,
    retention_days: u32,
) -> Result<u64, String> {
    if retention_days == 0 {
        return Ok(0);
    }
    let cutoff = now_millis().saturating_sub(i64::from(retention_days) * 86_400_000);
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("开始历史清理事务失败: {error}"))?;
    let mut statement = transaction
        .prepare(
            "SELECT DISTINCT audio_file FROM transcription_history \
             WHERE created_at < ?1 AND audio_file IS NOT NULL",
        )
        .map_err(|error| format!("准备历史清理失败: {error}"))?;
    let audio_files = statement
        .query_map(params![cutoff], |row| row.get::<_, String>(0))
        .map_err(|error| format!("查询过期历史音频失败: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("解析过期历史音频失败: {error}"))?;
    drop(statement);
    let removed = transaction
        .execute(
            "DELETE FROM transcription_history WHERE created_at < ?1",
            params![cutoff],
        )
        .map_err(|error| format!("清理过期历史失败: {error}"))? as u64;
    transaction
        .commit()
        .map_err(|error| format!("提交历史清理事务失败: {error}"))?;
    for audio_file in audio_files {
        cleanup_audio_if_unreferenced_with_connection(connection, &audio_file);
    }
    Ok(removed)
}

pub async fn cleanup(retention_days: u32) -> Result<u64, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let mut connection = open_connection()?;
        cleanup_expired_with_connection(&mut connection, retention_days)
    })
    .await
    .map_err(|error| format!("清理历史任务失败: {error}"))?
}

pub async fn all_records() -> Result<Vec<HistoryRecord>, String> {
    initialize().await?;
    tokio::task::spawn_blocking(move || {
        let connection = open_connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {HISTORY_COLUMNS} FROM transcription_history ORDER BY created_at DESC, id DESC"
            ))
            .map_err(|error| format!("准备历史导出失败: {error}"))?;
        let records = statement
            .query_map([], map_stored_record)
            .map_err(|error| format!("查询历史导出数据失败: {error}"))?
            .map(|row| row.map(|stored| stored.record))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("解析历史导出数据失败: {error}"))?;
        Ok(records)
    })
    .await
    .map_err(|error| format!("读取历史导出数据任务失败: {error}"))?
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

    use super::{
        cleanup_expired_with_connection, configure_connection, get_for_reprocess_with_connection,
        latency_stats, map_stored_record, now_millis, release_audio_lease_with_connection,
        safe_audio_path, table_has_column, HISTORY_COLUMNS, HISTORY_SCHEMA_VERSION,
    };

    #[test]
    fn latency_percentiles_are_stable() {
        let stats = latency_stats(vec![10, 20, 30, 40, 50]);
        assert_eq!(stats.p50_ms, Some(30));
        assert_eq!(stats.p95_ms, Some(50));
    }

    #[test]
    fn audio_paths_reject_traversal() {
        assert!(safe_audio_path("../secret.wav").is_none());
        assert!(safe_audio_path("folder/voice.wav").is_none());
        assert!(safe_audio_path("voice.wav").is_some());
    }

    #[test]
    fn sqlite_schema_round_trips_and_cleans_expired_history() {
        let mut connection = Connection::open_in_memory().expect("open in-memory history database");
        configure_connection(&connection).expect("initialize history schema");
        let created_at = now_millis() - 2 * 86_400_000;
        connection
            .execute(
                r#"
                INSERT INTO transcription_history (
                    session_id, created_at, updated_at, mode, status, text, original_text, source_text,
                    duration_sec, language, engine, provider, model, app_process,
                    app_window_title, app_rule_name, audio_file, asr_ms, polish_ms,
                    total_ms, raw_first_status, error, reprocessed_from_id
                ) VALUES (
                    ?1, ?2, ?2, 'dictation', 'success', 'final text', 'raw text', 'selected text',
                    1.25, 'en', 'sensevoice', 'openai', 'gpt-test', 'Code.exe',
                    'README - Code', 'Editor', NULL, 120, 240, 380, 'replaced', NULL, NULL
                )
                "#,
                params![42_i64, created_at],
            )
            .expect("insert history fixture");

        let stored = connection
            .query_row(
                &format!(
                    "SELECT {HISTORY_COLUMNS} FROM transcription_history WHERE session_id = 42"
                ),
                [],
                map_stored_record,
            )
            .expect("read history fixture");
        assert_eq!(stored.record.text, "final text");
        assert_eq!(stored.record.original_text, "raw text");
        assert_eq!(stored.record.source_text.as_deref(), Some("selected text"));
        assert_eq!(stored.record.app_rule_name.as_deref(), Some("Editor"));
        assert_eq!(stored.record.workflow, "dictation");
        assert_eq!(stored.record.total_ms, Some(380));
        assert!(!stored.record.audio_available);

        assert_eq!(
            cleanup_expired_with_connection(&mut connection, 1).expect("clean expired history"),
            1
        );
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM transcription_history", [], |row| {
                row.get(0)
            })
            .expect("count remaining history");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn audio_lease_survives_source_row_deletion_until_reprocess_finishes() {
        let mut connection = Connection::open_in_memory().expect("open in-memory history database");
        configure_connection(&connection).expect("initialize history schema");
        connection
            .execute(
                r#"
                INSERT INTO transcription_history (
                    session_id, created_at, updated_at, mode, status, text, original_text,
                    engine, audio_file, workflow
                ) VALUES (7, 1, 1, 'dictation', 'success', 'final', 'raw',
                          'sensevoice', 'leased.wav', 'dictation')
                "#,
                [],
            )
            .expect("insert leased history fixture");
        let id = connection.last_insert_rowid();

        let stored = get_for_reprocess_with_connection(&mut connection, id)
            .expect("acquire record and audio lease")
            .expect("history record exists");
        assert_eq!(stored.audio_file.as_deref(), Some("leased.wav"));
        connection
            .execute(
                "DELETE FROM transcription_history WHERE id = ?1",
                params![id],
            )
            .expect("delete source record while reprocessing");
        let lease_count: i64 = connection
            .query_row(
                "SELECT lease_count FROM history_audio_leases WHERE audio_file = 'leased.wav'",
                [],
                |row| row.get(0),
            )
            .expect("lease remains after source deletion");
        assert_eq!(lease_count, 1);

        release_audio_lease_with_connection(&mut connection, "leased.wav")
            .expect("release audio lease");
        let leases: i64 = connection
            .query_row("SELECT COUNT(*) FROM history_audio_leases", [], |row| {
                row.get(0)
            })
            .expect("count released leases");
        assert_eq!(leases, 0);
    }

    #[test]
    fn legacy_schema_migrates_assistant_workflow() {
        let connection = Connection::open_in_memory().expect("open legacy history database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE transcription_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    mode TEXT NOT NULL,
                    status TEXT NOT NULL,
                    text TEXT NOT NULL,
                    original_text TEXT NOT NULL,
                    duration_sec REAL,
                    language TEXT,
                    engine TEXT NOT NULL,
                    provider TEXT,
                    model TEXT,
                    app_process TEXT,
                    app_window_title TEXT,
                    app_rule_name TEXT,
                    audio_file TEXT,
                    asr_ms INTEGER,
                    polish_ms INTEGER,
                    total_ms INTEGER,
                    raw_first_status TEXT,
                    error TEXT,
                    reprocessed_from_id INTEGER
                );
                INSERT INTO transcription_history (
                    session_id, created_at, updated_at, mode, status, text,
                    original_text, engine
                ) VALUES (1, 1, 1, 'assistant', 'success', 'answer', 'question', 'sensevoice');
                "#,
            )
            .expect("create legacy schema");

        configure_connection(&connection).expect("migrate legacy history schema");
        configure_connection(&connection).expect("reopening an up-to-date schema is idempotent");
        let workflow: String = connection
            .query_row(
                "SELECT workflow FROM transcription_history WHERE session_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("read migrated workflow");
        assert_eq!(workflow, "assistant");
        assert!(table_has_column(&connection, "source_text").expect("inspect source_text column"));
        let schema_version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read migrated schema version");
        assert_eq!(schema_version, HISTORY_SCHEMA_VERSION);
    }
}
