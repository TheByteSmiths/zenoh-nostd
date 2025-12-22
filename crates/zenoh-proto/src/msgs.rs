pub mod exts;
pub mod fields;

mod err;
mod put;
mod query;
mod reply;

mod declare;
mod interest;
mod push;
mod request;
mod response;

mod close;
mod frame;
mod init;
mod keepalive;
mod open;

pub use err::*;
pub use put::*;
pub use query::*;
pub use reply::*;

pub use declare::*;
pub use interest::*;
pub use push::*;
pub use request::*;
pub use response::*;

pub use close::*;
pub use frame::*;
pub use init::*;
pub use keepalive::*;
pub use open::*;
use zenoh_derive::ZEnum;

use crate::{CodecError, ZEncode, ZWriteable, exts::QoS, fields::Reliability};

#[derive(ZEnum, Debug, PartialEq, Clone)]
pub enum NetworkBody<'a> {
    Push(Push<'a>),
    Request(Request<'a>),
    Response(Response<'a>),
    ResponseFinal(ResponseFinal),
    Interest(Interest<'a>),
    InterestFinal(InterestFinal),
    Declare(Declare<'a>),
}

#[derive(Debug, PartialEq, Clone)]
pub struct NetworkMessage<'a> {
    pub reliability: Reliability,
    pub qos: QoS,
    pub body: NetworkBody<'a>,
}

#[derive(ZEnum, Debug, PartialEq, Clone)]
pub enum TransportMessage<'a> {
    Close(Close),
    InitSyn(InitSyn<'a>),
    InitAck(InitAck<'a>),
    KeepAlive(KeepAlive),
    OpenSyn(OpenSyn<'a>),
    OpenAck(OpenAck<'a>),
}

impl NetworkMessage<'_> {
    pub fn z_encode(
        &self,
        w: &mut impl ZWriteable,
        reliability: &mut Option<Reliability>,
        qos: &mut Option<QoS>,
        sn: &mut u32,
    ) -> core::result::Result<(), CodecError> {
        let r = self.reliability;
        let q = self.qos;

        if reliability.as_ref() != Some(&r) || qos.as_ref() != Some(&q) {
            FrameHeader {
                reliability: r,
                sn: *sn,
                qos: q,
            }
            .z_encode(w)?;

            *reliability = Some(r);
            *qos = Some(q);
            *sn = sn.wrapping_add(1);
        }

        match &self.body {
            NetworkBody::Push(body) => body.z_encode(w),
            NetworkBody::Request(body) => body.z_encode(w),
            NetworkBody::Response(body) => body.z_encode(w),
            NetworkBody::ResponseFinal(body) => body.z_encode(w),
            NetworkBody::Interest(body) => body.z_encode(w),
            NetworkBody::InterestFinal(body) => body.z_encode(w),
            NetworkBody::Declare(body) => body.z_encode(w),
        }
    }
}
