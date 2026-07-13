use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Sender};

pub struct WifiControl {
    sender: Sender<'static, NoopRawMutex, bool, 1>,
}

impl WifiControl {
    pub fn new(sender: Sender<'static, NoopRawMutex, bool, 1>) -> Self {
        Self { sender }
    }

    pub async fn set_led(&self, state: bool) {
        self.sender.send(state).await;
    }
}
