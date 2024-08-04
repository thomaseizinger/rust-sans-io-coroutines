#![feature(coroutines, coroutine_trait, stmt_expr_attributes)]

use bytecodec::{DecodeExt, EncodeExt};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::ops::{Coroutine, CoroutineState};
use std::pin::Pin;
use stun_codec::rfc5389::attributes::{MappedAddress, XorMappedAddress};
use stun_codec::rfc5389::methods::BINDING;
use stun_codec::rfc5389::Attribute;
use stun_codec::{MessageClass, MessageDecoder, MessageEncoder, Method, TransactionId};

type Message = stun_codec::Message<Attribute>;

const STUN_CLOUDFLARE_COM: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(141, 101, 90, 0), 3478));

fn main() {
    let mut driver = Driver::default();

    // Execute `stun` 5 times to showcase multiplexing.
    driver.add_protocol(stun(STUN_CLOUDFLARE_COM));
    driver.add_protocol(stun(STUN_CLOUDFLARE_COM));
    driver.add_protocol(stun(STUN_CLOUDFLARE_COM));
    driver.add_protocol(stun(STUN_CLOUDFLARE_COM));
    driver.add_protocol(stun(STUN_CLOUDFLARE_COM));

    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    loop {
        if let Some(transmit) = driver.transmits.pop_front() {
            socket.send_to(&transmit.payload, transmit.dst).unwrap();
            continue;
        }

        if let Some(event) = driver.events.pop_front() {
            println!("{event:?}");
            continue;
        }

        let mut buf = [0u8; 1000];
        let n = socket.recv(&mut buf).unwrap();

        driver.handle_input(&buf[..n]);
    }
}

#[derive(Default)]
struct Driver {
    protocols: HashMap<
        u64,
        Pin<Box<dyn Coroutine<Message, Yield = (SocketAddr, Message), Return = Event>>>,
    >,
    next_id: u64,

    /// Remember, where a transaction ID originated from.
    ///
    /// This allows us to know, which coroutine we need to pass the response to.
    pending_protocol_by_transaction_id: HashMap<TransactionId, u64>,

    encoder: MessageEncoder<Attribute>,
    decoder: MessageDecoder<Attribute>,

    transmits: VecDeque<Transmit>,
    events: VecDeque<Event>,
}

impl Driver {
    fn add_protocol(
        &mut self,
        protocol: impl Coroutine<Message, Yield = (SocketAddr, Message), Return = Event> + 'static,
    ) {
        let next_id = self.next_id;
        self.next_id += 1;

        let protocol = Box::pin(protocol);
        self.protocols.insert(next_id, protocol);
        self.dispatch_message(next_id, start_msg()); // Kick it off with a dummy message that is ignored by the protocol.
    }

    fn handle_input(&mut self, msg: &[u8]) {
        let msg = self.decoder.decode_from_bytes(msg).unwrap().unwrap(); // TODO: Error handling.

        let Some(id) = self
            .pending_protocol_by_transaction_id
            .get(&msg.transaction_id())
        else {
            return; // No protocol emitted the request for this transaction ID.
        };
        self.dispatch_message(*id, msg);
    }

    fn dispatch_message(&mut self, id: u64, msg: Message) {
        let Entry::Occupied(mut protocol) = self.protocols.entry(id) else {
            return; // Unknown protocol.
        };

        match protocol.get_mut().as_mut().resume(msg) {
            // The protocol is still executing and would like to send another message.
            CoroutineState::Yielded((dst, msg)) => {
                self.pending_protocol_by_transaction_id
                    .insert(msg.transaction_id(), id);
                self.transmits.push_back(Transmit {
                    dst,
                    payload: self.encoder.encode_into_bytes(msg).unwrap(),
                })
            }
            // The protocol finished.
            CoroutineState::Complete(event) => {
                protocol.remove();

                self.events.push_back(event);
            }
        }
    }
}

struct Transmit {
    dst: SocketAddr,
    payload: Vec<u8>,
}

#[derive(Debug)]
enum Event {
    NewMappedAddress(SocketAddr),
}

fn stun(
    server: SocketAddr,
) -> impl Coroutine<Message, Yield = (SocketAddr, Message), Return = Event> {
    #[coroutine]
    move |_dummy: Message| {
        // We ignore the initial message.
        // Using `Message` as the `resume` type is very ergonomic for yielding but it means we need to pass a `Message` to kick things off.

        let request = Message::new(
            MessageClass::Request,
            BINDING,
            TransactionId::new(rand::random()),
        );

        // TODO: The resume argument here should likely be a `Result`, so the protocol can learn when something went wrong, like a timeout when waiting for the response.
        let response = yield (server, request); // Send the request and suspend until we have a response.

        // Return argument could be a result too so we can fail the protocol.
        Event::NewMappedAddress(
            response
                .get_attribute::<XorMappedAddress>()
                .unwrap()
                .address(),
        )
    }
}

fn start_msg() -> Message {
    Message::new(
        MessageClass::Indication,
        Method::new(0).unwrap(),
        TransactionId::new([0u8; 12]),
    )
}
