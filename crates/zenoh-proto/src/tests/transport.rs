use rand::{Rng, thread_rng};

use crate::{exts::*, fields::*, msgs::*, *};

const NUM_ITER: usize = 100;
const MAX_PAYLOAD_SIZE: usize = 512;

fn net_rand<'a>(w: &mut impl crate::ZStoreable<'a>) -> NetworkMessage<'a> {
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    let choices = [
        Push::ID,
        Request::ID,
        Response::ID,
        ResponseFinal::ID,
        Interest::ID,
        Declare::ID,
    ];

    let body = match *choices.choose(&mut rng).unwrap() {
        Push::ID => NetworkBody::Push(Push::rand(w)),
        Request::ID => NetworkBody::Request(Request::rand(w)),
        Response::ID => NetworkBody::Response(Response::rand(w)),
        ResponseFinal::ID => NetworkBody::ResponseFinal(ResponseFinal::rand(w)),
        Interest::ID => {
            if rng.gen_bool(0.5) {
                NetworkBody::Interest(Interest::rand(w))
            } else {
                NetworkBody::InterestFinal(InterestFinal::rand(w))
            }
        }
        Declare::ID => NetworkBody::Declare(Declare::rand(w)),
        _ => unreachable!(),
    };

    NetworkMessage {
        reliability: Reliability::rand(w),
        qos: QoS::rand(w),
        body,
    }
}

#[test]
fn transport_codec() {
    extern crate std;
    use std::collections::VecDeque;

    let mut rand = [0u8; MAX_PAYLOAD_SIZE * NUM_ITER];
    let mut rw = rand.as_mut_slice();

    let mut messages = {
        let mut msgs = VecDeque::new();
        for _ in 0..thread_rng().gen_range(1..16) {
            msgs.push_back(net_rand(&mut rw));
        }
        msgs
    };

    let mut socket = [0u8; MAX_PAYLOAD_SIZE * NUM_ITER];
    let mut writer = &mut socket[..];

    let mut transport = Transport::new([0u8; MAX_PAYLOAD_SIZE * NUM_ITER]).codec();

    for chunk in transport.tx.write(messages.clone().into_iter()) {
        writer[..chunk.len()].copy_from_slice(chunk);
        let (_, remain) = writer.split_at_mut(chunk.len());
        writer = remain;
    }

    let len = MAX_PAYLOAD_SIZE * NUM_ITER - writer.len();
    transport.rx.feed(&socket[..len]);

    for msg in transport.rx.flush(&mut transport.state) {
        let actual = messages.pop_front().unwrap();
        assert_eq!(msg, actual);
    }

    assert!(messages.is_empty());
}

#[test]
fn transport_handshake() {
    let mut t1 = Transport::new([0u8; MAX_PAYLOAD_SIZE * NUM_ITER]).connect();
    let mut t2 = Transport::new([0u8; MAX_PAYLOAD_SIZE * NUM_ITER]).listen();
    const_array_dumb_handshake(&mut t1, &mut t2);
    assert!(t1.opened() && t2.opened());

    let mut t1 = Transport::new([0u8; MAX_PAYLOAD_SIZE * NUM_ITER]).listen();
    let mut t2 = Transport::new([0u8; MAX_PAYLOAD_SIZE * NUM_ITER]).connect();
    const_array_dumb_handshake(&mut t1, &mut t2);
    assert!(t1.opened() && t2.opened());
}
