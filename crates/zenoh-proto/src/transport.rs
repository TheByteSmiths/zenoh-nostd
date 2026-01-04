use core::time::Duration;

use crate::{
    fields::ZenohIdProto,
    transport::{rx::TransportRx, state::TransportState, tx::TransportTx},
};

mod scope;
mod state;

mod rx;
mod tx;

#[derive(Debug)]
pub struct Transport<Buff> {
    pub tx: TransportTx<Buff>,
    pub rx: TransportRx<Buff>,

    pub state: TransportState,
}

impl<Buff> Transport<Buff> {
    pub fn new(buff: Buff) -> Self
    where
        Buff: Clone + AsRef<[u8]>,
    {
        Self {
            state: TransportState::codec().with_batch_size(buff.as_ref().len() as u16),

            tx: TransportTx::new(buff.clone()),
            rx: TransportRx::new(buff),
        }
    }

    pub fn codec(mut self) -> Self {
        self.state = self.state.into_codec();
        self
    }

    pub fn listen(mut self) -> Self {
        self.state = self.state.into_listen();
        self
    }

    pub fn connect(mut self) -> Self {
        self.state = self.state.into_connect();
        self
    }

    pub fn batch_size(mut self, batch_size: u16) -> Self {
        self.state.batch_size = self.state.batch_size.min(batch_size);
        self
    }

    pub fn zid(mut self, zid: ZenohIdProto) -> Self {
        self.state = self.state.with_zid(zid);
        self
    }

    pub fn streamed(mut self) -> Self {
        self.rx.streamed = true;
        self.tx.streamed = true;
        self
    }

    pub fn lease(mut self, lease: Duration) -> Self {
        self.state.lease = lease;
        self
    }

    pub fn opened(&self) -> bool {
        self.state.opened()
    }

    pub fn init(&mut self) -> Option<&[u8]>
    where
        Buff: AsMut<[u8]>,
    {
        self.tx.answer(&mut self.state.init().ok())
    }
}
