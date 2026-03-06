pub mod error;
pub mod foreground;
pub mod paths;
pub mod sound;
pub use error::AppError;

/// 为 std::sync::Mutex 提供自动恢复 poisoned 锁的便捷方法。
///
/// 在多线程环境中，如果持有 Mutex 的线程 panic，锁会进入 poisoned 状态。
/// 对于本应用来说，poisoned 锁中的数据仍然可用，因此统一恢复并继续使用。
pub trait MutexRecover<T> {
    fn lock_or_recover(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> MutexRecover<T> for std::sync::Mutex<T> {
    fn lock_or_recover(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            log::warn!("Mutex poisoned, recovering");
            poisoned.into_inner()
        })
    }
}
