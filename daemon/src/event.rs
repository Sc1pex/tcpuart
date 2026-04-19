use common::ctl::{DaemonRequest, DaemonResponse};
use tokio::sync::oneshot;

pub enum DaemonEvent {
    CliCommand(DaemonRequest, oneshot::Sender<DaemonResponse>),
    ConnectionClosed(String),
}
