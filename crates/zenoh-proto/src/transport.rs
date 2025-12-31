use crate::transport::{rx::TransportRx, state::TransportState, tx::TransportTx};

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

    pub fn streamed(mut self) -> Self {
        self.rx.streamed = true;
        self.tx.streamed = true;
        self
    }

    pub fn opened(&self) -> bool {
        self.state.opened()
    }
}

#[cfg(test)]
pub fn const_array_dumb_handshake<const N: usize>(
    t1: &mut Transport<[u8; N]>,
    t2: &mut Transport<[u8; N]>,
) {
    fn step<const N: usize>(transport: &mut Transport<[u8; N]>, socket: (&mut [u8], &mut usize)) {
        let mut scope = transport.scope();

        scope.rx.feed(&socket.0[..*socket.1]);
        for _ in scope.rx.flush(&mut scope.state) {}

        if let Some(i) = scope.tx.interact(&mut scope.state) {
            socket.0[..i.len()].copy_from_slice(i);
            *socket.1 = i.len();
        }
    }

    let mut socket = [0u8; N];
    let mut length = 0;

    // We may only need 2.5 but we do it 3 times anyway
    for _ in 0..3 {
        step(t1, (&mut socket, &mut length));
        step(t2, (&mut socket, &mut length));
    }
}
