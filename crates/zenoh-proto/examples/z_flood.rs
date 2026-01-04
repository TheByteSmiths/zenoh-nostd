use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

use zenoh_proto::{
    Transport,
    exts::QoS,
    fields::{Reliability, WireExpr},
    keyexpr,
    msgs::*,
};

const BATCH_SIZE: usize = u16::MAX as usize;

fn open_listen(stream: &mut std::net::TcpStream) -> Transport<[u8; BATCH_SIZE]> {
    let mut transport = Transport::new([0u8; BATCH_SIZE]).streamed().listen();

    for _ in 0..2 {
        let mut scope = transport.scope();

        if scope
            .rx
            .feed_with(|data| stream.read_exact(data).map_or(0, |_| data.len()))
            .is_err()
        {
            continue;
        }

        for _ in scope.rx.flush(&mut scope.state) {}

        if let Some(bytes) = scope.tx.interact(&mut scope.state) {
            stream.write_all(bytes).ok();
        }
    }

    transport
}

fn open_connect(stream: &mut std::net::TcpStream) -> Transport<[u8; BATCH_SIZE]> {
    let mut transport = Transport::new([0u8; BATCH_SIZE]).streamed().connect();

    if let Some(bytes) = transport.init() {
        stream.write_all(bytes).ok();
    }

    for _ in 0..2 {
        let mut scope = transport.scope();

        if scope
            .rx
            .feed_with(|data| stream.read_exact(data).map_or(0, |_| data.len()))
            .is_err()
        {
            continue;
        }

        for _ in scope.rx.flush(&mut scope.state) {}

        if let Some(bytes) = scope.tx.interact(&mut scope.state) {
            stream.write_all(bytes).ok();
        }
    }

    transport
}

fn handle_client(mut stream: std::net::TcpStream, mut transport: Transport<[u8; BATCH_SIZE]>) {
    assert!(transport.opened());

    let put = NetworkMessage {
        reliability: Reliability::default(),
        qos: QoS::default(),
        body: NetworkBody::Push(Push {
            wire_expr: WireExpr::from(keyexpr::from_str_unchecked("test/thr")),
            payload: PushBody::Put(Put {
                payload: &[0, 1, 2, 3, 4, 5, 6, 7],
                ..Default::default()
            }),
            ..Default::default()
        }),
    };

    for _ in 0..200 {
        transport.tx.push(&put).expect("Transport too small");
    }

    let bytes = transport.tx.flush().unwrap();

    println!("Sending indefinitely to {:?}...", stream.peer_addr());
    loop {
        if stream.write_all(&bytes).is_err() {
            break;
        }
    }
}

fn main() {
    match std::env::args().nth(1) {
        None => {
            let listener = TcpListener::bind("127.0.0.1:7447").expect("Could not bind");
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let transport = open_listen(&mut stream);
                        handle_client(stream, transport)
                    }
                    Err(e) => {
                        panic!("Error accepting connection: {}", e);
                    }
                }
            }
        }
        Some(str) => match str.as_str() {
            "--listen" => {
                let listener = TcpListener::bind("127.0.0.1:7447").expect("Could not bind");
                for stream in listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            let transport = open_listen(&mut stream);
                            handle_client(stream, transport)
                        }
                        Err(e) => {
                            panic!("Error accepting connection: {}", e);
                        }
                    }
                }
            }
            "--connect" => {
                let mut stream = TcpStream::connect("127.0.0.1:7447").expect("Couldn't connect");
                let transport = open_connect(&mut stream);
                handle_client(stream, transport)
            }
            _ => {
                panic!("Invalid argument")
            }
        },
    }
}
