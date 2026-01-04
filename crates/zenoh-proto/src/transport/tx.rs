use crate::{
    TransportError, ZEncode,
    exts::QoS,
    fields::Reliability,
    msgs::NetworkMessage,
    transport::state::{StateResponse, TransportState},
};

#[derive(Debug)]
pub struct TransportTx<Buff> {
    pub(crate) streamed: bool,
    pub(crate) tx: Buff,
    pub(crate) cursor: usize,

    pub(crate) batch_size: u16,
    pub(crate) next_sn: u32,

    pub(crate) last_qos: Option<QoS>,
    pub(crate) last_reliability: Option<Reliability>,
}

impl<Buff> TransportTx<Buff> {
    pub fn new(tx: Buff) -> Self
    where
        Buff: AsRef<[u8]>,
    {
        Self {
            batch_size: tx.as_ref().len() as u16,
            tx,
            cursor: 0,
            streamed: false,
            next_sn: 0,
            last_qos: None,
            last_reliability: None,
        }
    }

    pub(crate) fn sync(&mut self, state: &TransportState) -> &mut Self {
        if self.next_sn == 0 {
            self.next_sn = *state.sn();
        }

        self
    }

    fn push_internal(
        streamed: bool,
        batch_size: u16,
        cursor: &mut usize,
        buffer: &mut [u8],
        last_reliability: &mut Option<Reliability>,
        last_qos: &mut Option<QoS>,
        next_sn: &mut u32,
        msg: &NetworkMessage,
    ) -> core::result::Result<(), TransportError> {
        let batch_size = core::cmp::min(batch_size as usize, buffer.len());
        let mut buffer = &mut buffer[*cursor..batch_size];

        if *cursor == 0 {
            if streamed {
                if buffer.len() < 2 {
                    crate::zbail!(@log TransportError::TransportIsFull);
                }

                buffer = &mut buffer[2..];
                *cursor += 2;
            }
        }

        let start = buffer.len();
        if msg
            .z_encode(&mut buffer, last_reliability, last_qos, next_sn)
            .is_err()
        {
            crate::zbail!(@log TransportError::MessageTooLargeForBatch)
        }

        *cursor += start - buffer.len();
        Ok(())
    }

    pub fn push(&mut self, msg: &NetworkMessage) -> core::result::Result<(), TransportError>
    where
        Buff: AsMut<[u8]>,
    {
        Self::push_internal(
            self.streamed,
            self.batch_size,
            &mut self.cursor,
            self.tx.as_mut(),
            &mut self.last_reliability,
            &mut self.last_qos,
            &mut self.next_sn,
            msg,
        )
    }

    fn flush_internal<'a>(
        streamed: bool,
        cursor: &mut usize,
        buffer: &'a mut [u8],
    ) -> Option<&'a [u8]> {
        let buffer = &mut buffer[..*cursor];

        if streamed {
            if *cursor <= 2 {
                return None;
            }

            let length = (*cursor - 2) as u16;
            let length = length.to_le_bytes();
            buffer[..2].copy_from_slice(&length);
        }

        if *cursor == 0 {
            return None;
        }

        *cursor = 0;
        Some(buffer)
    }

    pub fn flush(&mut self) -> Option<&'_ [u8]>
    where
        Buff: AsMut<[u8]>,
    {
        Self::flush_internal(self.streamed, &mut self.cursor, self.tx.as_mut())
    }

    pub(crate) fn answer<'a, 'b>(
        &'a mut self,
        pending: &mut Option<StateResponse<'b>>,
    ) -> Option<&'a [u8]>
    where
        Buff: AsMut<[u8]>,
    {
        if let Some(pending) = pending.take() {
            let batch_size = core::cmp::min(self.batch_size as usize, self.tx.as_mut().len());
            let batch = &mut self.tx.as_mut()[..batch_size];

            if self.streamed && batch_size < 2 {
                return None;
            }

            let mut writer = &mut batch[if self.streamed { 2 } else { 0 }..];
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

            if self.streamed {
                let l = (length as u16).to_le_bytes();
                batch[..2].copy_from_slice(&l);
            }

            let (ret, _) = self
                .tx
                .as_mut()
                .split_at(length + if self.streamed { 2 } else { 0 });

            Some(&ret[..])
        } else {
            None
        }
    }

    pub fn batch<'a, 'b>(
        &'a mut self,
        msgs: impl Iterator<Item = NetworkMessage<'b>>,
    ) -> impl Iterator<Item = &'a [u8]>
    where
        Buff: AsMut<[u8]>,
    {
        let streamed = self.streamed;
        let batch_size = core::cmp::min(self.batch_size as usize, self.tx.as_mut().len());
        let cursor = &mut self.cursor;
        let mut buffer = &mut self.tx.as_mut()[*cursor..batch_size];

        let mut msgs = msgs.peekable();

        let reliability = &mut self.last_reliability;
        let qos = &mut self.last_qos;
        let sn = &mut self.next_sn;

        core::iter::from_fn(move || {
            let batch_size = core::cmp::min(batch_size as usize, buffer.len());
            let buffer = core::mem::take(&mut buffer);

            while let Some(msg) = msgs.peek() {
                if Self::push_internal(
                    streamed,
                    batch_size as u16,
                    cursor,
                    buffer,
                    reliability,
                    qos,
                    sn,
                    &msg,
                )
                .is_err()
                {
                    break;
                }

                msgs.next();
            }

            Self::flush_internal(streamed, cursor, buffer)
        })
    }
}
