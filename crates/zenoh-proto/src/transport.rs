use crate::{ZReadable, exts::*, fields::*, msgs::*};

#[derive(Debug, PartialEq, Default)]
pub enum TransportState {
    #[default]
    /// No state handling:
    Codec,

    /// States for `listen` mode.
    WaitingInitSyn,
    SendingInitAck,
    WaitingOpenSyn,
    SendingOpenAck,

    /// States for `connect` mode.
    SendingInitSyn,
    WaitingInitAck,
    SendingOpenSyn,
    WaitingOpenAck,
}

impl TransportState {
    fn handle(&mut self, msg: TransportMessage<'_>) {
        if matches!(self, Self::Codec) {
            return;
        }

        let _ = msg;
    }

    fn is_sending(&self) -> bool {
        return matches!(
            self,
            TransportState::SendingInitSyn { .. }
                | TransportState::SendingInitAck { .. }
                | TransportState::SendingOpenSyn { .. }
                | TransportState::SendingOpenAck { .. }
        );
    }

    fn is_waiting(&self) -> bool {
        return matches!(
            self,
            TransportState::WaitingInitSyn { .. }
                | TransportState::WaitingInitAck { .. }
                | TransportState::WaitingOpenSyn { .. }
                | TransportState::WaitingOpenAck { .. }
        );
    }
}

#[derive(Debug)]
pub struct TransportRx<Buff> {
    rx: Buff,
    cursor: usize,

    frame: Option<FrameHeader>,
}

#[derive(Debug)]
pub struct TransportTx<Buff> {
    tx: Buff,

    batch_size: u16,
    streamed: bool,

    sn: u32,
    reliability: Option<Reliability>,
    qos: Option<QoS>,
}

#[derive(Debug)]
pub struct Transport<Buff> {
    pub tx: TransportTx<Buff>,
    pub rx: TransportRx<Buff>,

    streamed: bool,

    pub state: TransportState,
}

impl<Buff> Transport<Buff> {
    pub fn new(buff: Buff) -> Self
    where
        Buff: Clone + AsRef<[u8]>,
    {
        Self {
            tx: TransportTx {
                tx: buff.clone(),
                batch_size: buff.as_ref().len().min(u16::MAX as usize) as u16,
                streamed: false,

                sn: 0,
                qos: None,
                reliability: None,
            },
            rx: TransportRx {
                rx: buff,
                cursor: 0,
                frame: None,
            },
            streamed: false,
            state: TransportState::Codec,
        }
    }

    pub fn codec(mut self) -> Self {
        self.state = TransportState::Codec;
        return self;
    }

    pub fn batch_size(mut self, batch_size: u16) -> Self {
        self.tx.batch_size = batch_size;
        self
    }

    pub fn streamed(mut self) -> Self {
        self.tx.streamed = true;
        self.streamed = true;
        self
    }

    pub fn feed(&mut self, data: &[u8])
    where
        Buff: AsMut<[u8]>,
    {
        if self.state.is_sending() {
            return;
        }

        let rx = self.rx.rx.as_mut();
        let rx = &mut rx[self.rx.cursor..];

        let len = data.len().min(rx.len());
        rx[..len].copy_from_slice(&data[..len]);
        self.rx.cursor += len;
    }

    pub fn feed_exact(&mut self, len: usize, mut data: impl FnMut(&mut [u8]))
    where
        Buff: AsMut<[u8]>,
    {
        if self.state.is_sending() {
            return;
        }

        let rx = self.rx.rx.as_mut();
        let rx = &mut rx[self.rx.cursor..];

        let len = len.min(rx.len());
        data(&mut rx[..len]);
        self.rx.cursor += len;
    }

    pub fn feed_with(&mut self, mut data: impl FnMut(&mut [u8]) -> usize)
    where
        Buff: AsMut<[u8]>,
    {
        if self.state.is_sending() {
            return;
        }

        let rx = self.rx.rx.as_mut();
        let rx = &mut rx[self.rx.cursor..];

        if self.streamed {
            let mut lbuf = [0u8; 2];
            data(&mut lbuf);
            let l = u16::from_be_bytes(lbuf) as usize;
            data(&mut rx[..l]);
            self.rx.cursor += l;
        } else {
            let l = data(&mut rx[..]);
            self.rx.cursor += l;
        }
    }

    pub fn interact<'a>(&'a mut self) -> &'a [u8]
    where
        Buff: AsMut<[u8]>,
    {
        if self.state.is_waiting() {
            return &[];
        }

        &[]
    }
}

impl<Buff> TransportRx<Buff> {
    fn next<'a>(
        reader: &mut &'a [u8],
        state: &mut TransportState,
        frame: &mut Option<FrameHeader>,
    ) -> Option<NetworkMessage<'a>> {
        if !reader.can_read() {
            return None;
        }

        let header = reader
            .read_u8()
            .expect("reader should not be empty at this stage");

        macro_rules! decode {
            ($ty:ty) => {
                match <$ty as $crate::ZBodyDecode>::z_body_decode(reader, header) {
                    Ok(msg) => msg,
                    Err(e) => {
                        crate::error!(
                            "Failed to decode message of type {}: {}. Skipping the rest of the message - {}",
                            core::any::type_name::<$ty>(),
                            e,
                            crate::zctx!()
                        );

                        return None;
                    }
                }
            };
        }

        let ack = header & 0b0010_0000 != 0;
        let net = frame.is_some();
        let ifinal = header & 0b0110_0000 == 0;
        let id = header & 0b0001_1111;

        if let Some(msg) = match id {
            InitAck::ID if ack => Some(TransportMessage::InitAck(decode!(InitAck))),
            InitSyn::ID => Some(TransportMessage::InitSyn(decode!(InitSyn))),
            OpenAck::ID if ack => Some(TransportMessage::OpenAck(decode!(OpenAck))),
            OpenSyn::ID => Some(TransportMessage::OpenSyn(decode!(OpenSyn))),
            Close::ID => Some(TransportMessage::Close(decode!(Close))),
            KeepAlive::ID => Some(TransportMessage::KeepAlive(decode!(KeepAlive))),
            _ => None,
        } {
            frame.take();
            state.handle(msg);
            return Self::next(reader, state, frame);
        }

        let reliability = frame.as_ref().map(|f| f.reliability);
        let qos = frame.as_ref().map(|f| f.qos);

        // TODO! negociate `sn` and expect increasing `sn` in frames

        let body = match id {
            FrameHeader::ID => {
                let header = decode!(FrameHeader);
                frame.replace(header);
                return Self::next(reader, state, frame);
            }
            Push::ID if net => NetworkBody::Push(decode!(Push)),
            Request::ID if net => NetworkBody::Request(decode!(Request)),
            Response::ID if net => NetworkBody::Response(decode!(Response)),
            ResponseFinal::ID if net => NetworkBody::ResponseFinal(decode!(ResponseFinal)),
            InterestFinal::ID if net && ifinal => {
                NetworkBody::InterestFinal(decode!(InterestFinal))
            }
            Interest::ID if net => NetworkBody::Interest(decode!(Interest)),
            Declare::ID if net => NetworkBody::Declare(decode!(Declare)),
            _ => {
                crate::error!(
                    "Unrecognized message header: {:08b}. Skipping the rest of the message - {}",
                    header,
                    crate::zctx!()
                );
                return None;
            }
        };

        Some(NetworkMessage {
            reliability: reliability.expect("Should be a frame. Something went wrong."),
            qos: qos.expect("Should be a frame. Something went wrong."),
            body,
        })
    }

    pub fn flush<'a>(
        &'a mut self,
        state: &mut TransportState,
    ) -> impl Iterator<Item = NetworkMessage<'a>>
    where
        Buff: AsRef<[u8]>,
    {
        let rx = self.rx.as_ref();
        let mut reader = &rx[..self.cursor];
        let frame = &mut self.frame;

        core::iter::from_fn(move || {
            return Self::next(&mut reader, state, frame);
        })
    }
}

impl<Tx> TransportTx<Tx> {
    pub fn write<'a, 'b>(
        &'a mut self,
        msgs: impl Iterator<Item = NetworkMessage<'b>>,
    ) -> impl Iterator<Item = &'a [u8]>
    where
        Tx: AsMut<[u8]>,
    {
        let streamed = self.streamed;
        let mut buffer = self.tx.as_mut();
        let batch_size = core::cmp::min(self.batch_size as usize, buffer.len());

        let mut msgs = msgs.peekable();

        let reliability = &mut self.reliability;
        let qos = &mut self.qos;
        let sn = &mut self.sn;

        core::iter::from_fn(move || {
            let batch_size = core::cmp::min(batch_size as usize, buffer.len());
            let batch = &mut buffer[..batch_size];

            if streamed && batch_size < 2 {
                return None;
            }

            let mut writer = &mut batch[if streamed { 2 } else { 0 }..];
            let start = writer.len();

            let mut length = 0;
            while let Some(msg) = msgs.peek() {
                if msg.z_encode(&mut writer, reliability, qos, sn).is_ok() {
                    length = start - writer.len();
                    msgs.next();
                } else {
                    break;
                }
            }

            if length == 0 {
                return None;
            }

            if streamed {
                let l = (length as u16).to_be_bytes();
                batch[..2].copy_from_slice(&l);
            }

            let (ret, remain) =
                core::mem::take(&mut buffer).split_at_mut(length + if streamed { 2 } else { 0 });
            buffer = remain;

            Some(&ret[..])
        })
    }
}
