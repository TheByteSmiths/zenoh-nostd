use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

use zenoh_proto::*;

const BATCH_SIZE: usize = u16::MAX as usize;

fn open_listen(stream: &mut std::net::TcpStream) -> Transport<[u8; BATCH_SIZE]> {
    let mut transport = Transport::new([0u8; BATCH_SIZE])
        .streamed()
        .listen()
        .lease(core::time::Duration::from_hours(1));

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

fn handle_client(mut stream: std::net::TcpStream, transport: Transport<[u8; BATCH_SIZE]>) {
    assert!(transport.opened());

    println!("Reading indefinitely from {:?}...", stream.peer_addr());
    let mut rx = [0u8; u16::MAX as usize];
    loop {
        if stream.read_exact(&mut rx).is_err() {
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
