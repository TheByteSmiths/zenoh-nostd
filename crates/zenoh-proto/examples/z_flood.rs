use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

use zenoh_proto::{
    Transport,
    exts::QoS,
    fields::{Reliability, WireExpr},
    keyexpr,
    msgs::*,
};

fn handle_client(mut stream: std::net::TcpStream) {
    let mut transport = Transport::new([0; u16::MAX as usize]).listen().streamed();

    // Handshake
    for _ in 0..2 {
        let mut scope = transport.scope();

        scope.rx.feed_with(|data| {
            // In streamed mode we can just `read_exact`. Internally it will call this closure
            // twice, the first one to retrieve the length and the second one to retrieve the rest
            // of the data.
            stream.read_exact(data).expect("Couldn't raed");
            0
        });

        for _ in scope.rx.flush(&mut scope.state) {}

        let bytes = scope
            .tx
            .interact(&mut scope.state)
            .expect("During listen handshake there should always be a response");

        stream.write_all(bytes).expect("Couldn't write");
    }

    assert!(transport.opened());

    // Just send messages indefinitely
    let put = Push {
        wire_expr: WireExpr::from(keyexpr::from_str_unchecked("test/thr")),
        payload: PushBody::Put(Put {
            payload: &[0, 1, 2, 3, 4, 5, 6, 7],
            ..Default::default()
        }),
        ..Default::default()
    };

    let batch = core::array::from_fn::<_, 200, _>(|_| NetworkMessage {
        reliability: Reliability::default(),
        qos: QoS::default(),
        body: NetworkBody::Push(put.clone()),
    });

    // let mut batch = BatchWriter::new(&mut tx[2..], 0);
    // for _ in 0..200 {
    //     batch
    //         .framed(&put, Reliability::Reliable, QoS::default())
    //         .expect("Could not encode Push");
    // }

    // let (_, payload_len) = batch.finalize();
    // let len_bytes = (payload_len as u16).to_le_bytes();
    // tx[..2].copy_from_slice(&len_bytes);

    // loop {
    //     if stream.write_all(&tx[..payload_len + 2]).is_err() {
    //         break;
    //     }
    // }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:7447").expect("Could not bind");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_client(stream),
            Err(e) => {
                panic!("Error accepting connection: {}", e);
            }
        }
    }
}
