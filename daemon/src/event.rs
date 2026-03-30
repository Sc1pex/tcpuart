use common::ctl::{CtlMessage, CtlResponse};
use tokio::sync::oneshot;

pub enum DaemonEvent {
    CliCommand(CtlMessage, oneshot::Sender<CtlResponse>),
    ConnectionClosed(String),
}
