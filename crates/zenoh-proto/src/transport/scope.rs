use crate::{
    Transport, TransportError, ZEncode, ZReadable,
    msgs::NetworkMessage,
    msgs::*,
    transport::{
        rx::TransportRx,
        state::{StateRequest, StateResponse, TransportState},
        tx::TransportTx,
    },
};

#[derive(Debug)]
pub struct TransportStateScoped<'a> {
    state: &'a mut TransportState,
    pub(crate) pending: Option<StateResponse<'a>>,
}

impl<'a> TransportStateScoped<'a> {
    pub(crate) fn is_codec(&self) -> bool {
        self.state.is_codec()
    }

    pub(crate) fn process(
        &mut self,
        request: Option<StateRequest<'a>>,
    ) -> core::result::Result<(), TransportError> {
        let response = self.state.process(request)?;
        self.pending = response;
        Ok(())
    }
}

#[derive(Debug)]
pub struct TransportTxScoped<'a, Buff> {
    tx: &'a mut TransportTx<Buff>,
}

impl<Buff> TransportTxScoped<'_, Buff> {
    pub fn write<'a, 'b>(
        &'a mut self,
        msgs: impl Iterator<Item = NetworkMessage<'b>>,
    ) -> impl Iterator<Item = &'a [u8]>
    where
        Buff: AsMut<[u8]>,
    {
        self.tx.write(msgs)
    }

    pub fn interact<'a>(&mut self, state: &mut TransportStateScoped<'a>) -> Option<&'_ [u8]>
    where
        Buff: AsMut<[u8]>,
    {
        if let Some(pending) = state.pending.take() {
            let batch_size = core::cmp::min(self.tx.batch_size as usize, self.tx.tx.as_mut().len());
            let batch = &mut self.tx.tx.as_mut()[..batch_size];

            if self.tx.streamed && batch_size < 2 {
                return None;
            }

            let mut writer = &mut batch[if self.tx.streamed { 2 } else { 0 }..];
            let start = writer.len();

            let length = if pending.0.z_encode(&mut writer).is_ok() {
                start - writer.len()
            } else {
                crate::error!("Couldn't encode msg {:?}", pending.0);
                return None;
            };

            if length == 0 {
                return None;
            }

            if self.tx.streamed {
                let l = (length as u16).to_be_bytes();
                batch[..2].copy_from_slice(&l);
            }

            let (ret, _) = self
                .tx
                .tx
                .as_mut()
                .split_at(length + if self.tx.streamed { 2 } else { 0 });

            Some(&ret[..])
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct TransportRxScoped<'a, Buff> {
    rx: &'a mut TransportRx<Buff>,
}

impl<Buff> TransportRxScoped<'_, Buff> {
    pub fn feed(&mut self, data: &[u8])
    where
        Buff: AsMut<[u8]>,
    {
        self.rx.feed(data);
    }
    pub fn feed_exact(&mut self, len: usize, data: impl FnMut(&mut [u8]))
    where
        Buff: AsMut<[u8]>,
    {
        self.rx.feed_exact(len, data);
    }

    pub fn feed_with(&mut self, data: impl FnMut(&mut [u8]) -> usize)
    where
        Buff: AsMut<[u8]>,
    {
        self.rx.feed_with(data);
    }

    fn read_one<'a>(
        reader: &mut &'a [u8],
        state: &mut TransportStateScoped<'a>,
        last_frame: &mut Option<FrameHeader>,
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
        let net = last_frame.is_some();
        let ifinal = header & 0b0110_0000 == 0;
        let id = header & 0b0001_1111;

        if !state.is_codec() {
            if let Some(msg) = match id {
                InitAck::ID if ack => Some(TransportMessage::InitAck(decode!(InitAck))),
                InitSyn::ID => Some(TransportMessage::InitSyn(decode!(InitSyn))),
                OpenAck::ID if ack => Some(TransportMessage::OpenAck(decode!(OpenAck))),
                OpenSyn::ID => Some(TransportMessage::OpenSyn(decode!(OpenSyn))),
                Close::ID => Some(TransportMessage::Close(decode!(Close))),
                KeepAlive::ID => Some(TransportMessage::KeepAlive(decode!(KeepAlive))),
                _ => None,
            } {
                if let Err(e) = state.process(Some(StateRequest(msg))) {
                    crate::error!("Error while processing a TransportMessage. {:?}", e);
                }

                return Self::read_one(reader, state, last_frame);
            }
        }

        let reliability = last_frame.as_ref().map(|f| f.reliability);
        let qos = last_frame.as_ref().map(|f| f.qos);
        let sn = last_frame.as_ref().map(|f| f.sn);

        let body = match id {
            FrameHeader::ID => {
                let header = decode!(FrameHeader);
                if !state.is_codec() {
                    if let Some(sn) = sn {
                        if header.sn <= sn {
                            crate::error!(
                                "Inconsistent `SN` value {}, expected higher than {}",
                                header.sn,
                                sn
                            );
                            return None;
                        } else if header.sn != sn + 1 {
                            crate::debug!("Transport missed {} messages", header.sn - sn - 1);
                        }
                    }
                }

                last_frame.replace(header);
                return Self::read_one(reader, state, last_frame);
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
        state: &mut TransportStateScoped<'a>,
    ) -> impl Iterator<Item = NetworkMessage<'a>>
    where
        Buff: AsRef<[u8]>,
    {
        let rx = self.rx.rx.as_ref();
        let mut reader = &rx[..self.rx.cursor];
        let frame = &mut self.rx.frame;

        if reader.is_empty() {
            if let Err(e) = state.process(None) {
                crate::error!("Error in the Transport State Machine: {}", e);
            }
        }

        core::iter::from_fn(move || Self::read_one(&mut reader, state, frame))
    }
}

#[derive(Debug)]
pub struct TransportScope<'a, Buff> {
    pub tx: TransportTxScoped<'a, Buff>,
    pub rx: TransportRxScoped<'a, Buff>,
    pub state: TransportStateScoped<'a>,
}

impl<Buff> Transport<Buff> {
    pub fn scope(&mut self) -> TransportScope<'_, Buff> {
        TransportScope {
            tx: TransportTxScoped {
                tx: self.tx.sync(&self.state),
            },
            rx: TransportRxScoped {
                rx: self.rx.sync(&self.state),
            },
            state: TransportStateScoped {
                state: &mut self.state,
                pending: None,
            },
        }
    }
}
