// Notification outbox (M4 Task 5).
//
// Scheduler must not hold an owned tauri AppHandle (a managed state holding
// one produced a broken test exe on this toolchain), so notifications travel
// over a tokio channel: jobs push `Notification` into the bus, and a single
// consumer task spawned in setup owns the AppHandle and shows them through
// `tauri-plugin-notification`. Notification bodies carry counts and dates
// only — never raw plan/record/wrong-question text.

use tauri::AppHandle;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;

#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// Cloneable sender half managed as Tauri state; jobs enqueue notifications.
#[derive(Clone)]
pub struct NotificationBus(pub mpsc::Sender<Notification>);

impl NotificationBus {
    pub fn send(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<(), AgentError> {
        self.0
            .try_send(Notification {
                title: title.into(),
                body: body.into(),
            })
            .map_err(|_| AgentError::Persistence("notification queue full".to_owned()))
    }
}

/// Spawn the consumer that owns the AppHandle and shows queued notifications.
/// Returns the bus for `app.manage`.
pub fn spawn_notifier(app: AppHandle) -> NotificationBus {
    let (tx, mut rx) = mpsc::channel::<Notification>(64);
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_notification::NotificationExt;
        while let Some(notification) = rx.recv().await {
            let _ = app
                .notification()
                .builder()
                .title(&notification.title)
                .body(&notification.body)
                .show();
        }
    });
    NotificationBus(tx)
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn bus_send_carries_title_and_body() {
        let (tx, mut rx) = mpsc::channel::<Notification>(4);
        let bus = NotificationBus(tx);
        bus.send("标题", "正文").unwrap();
        let received = rx.blocking_recv().unwrap();
        assert_eq!(received.title, "标题");
        assert_eq!(received.body, "正文");
    }
}
