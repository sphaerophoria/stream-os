use crate::{
    net::{self, Ipv4Protocol},
    rng::Rng,
    sleep::WakeupList,
    time::MonotonicTime,
    util::{
        async_channel::{self, Receiver, Sender},
        async_mutex::Mutex,
        bit_manipulation::{GetBits, SetBits},
    },
    IpAddr,
};

use alloc::{boxed::Box, rc::Rc, vec::Vec};

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use hashbrown::HashMap;

pub struct TcpFlags(pub u8);

#[allow(unused)]
impl TcpFlags {
    pub fn cwr(&self) -> bool {
        self.0.get_bit(7)
    }

    pub fn ece(&self) -> bool {
        self.0.get_bit(6)
    }

    pub fn urg(&self) -> bool {
        self.0.get_bit(5)
    }

    pub fn ack(&self) -> bool {
        self.0.get_bit(4)
    }

    pub fn psh(&self) -> bool {
        self.0.get_bit(3)
    }

    pub fn rst(&self) -> bool {
        self.0.get_bit(2)
    }

    pub fn syn(&self) -> bool {
        self.0.get_bit(1)
    }

    pub fn fin(&self) -> bool {
        self.0.get_bit(0)
    }
}

impl core::fmt::Debug for TcpFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "cwr: {}", self.cwr())?;
        write!(f, "ece: {}", self.ece())?;
        write!(f, "urg: {}", self.urg())?;
        write!(f, "ack: {}", self.ack())?;
        write!(f, "psh: {}", self.psh())?;
        write!(f, "rst: {}", self.rst())?;
        write!(f, "syn: {}", self.syn())?;
        write!(f, "fin: {}", self.fin())?;
        Ok(())
    }
}

pub struct TcpFlagsParams {
    pub cwr: bool,
    pub ece: bool,
    pub urg: bool,
    pub ack: bool,
    pub psh: bool,
    pub rst: bool,
    pub syn: bool,
    pub fin: bool,
}

pub fn generate_tcp_flags(params: &TcpFlagsParams) -> u8 {
    let mut ret = 0u8;

    ret.set_bit(7, params.cwr);
    ret.set_bit(6, params.ece);
    ret.set_bit(5, params.urg);
    ret.set_bit(4, params.ack);
    ret.set_bit(3, params.psh);
    ret.set_bit(2, params.rst);
    ret.set_bit(1, params.syn);
    ret.set_bit(0, params.fin);

    ret
}

pub struct TcpFrame<'a> {
    data: &'a [u8],
}

impl TcpFrame<'_> {
    pub(super) fn new(data: &[u8]) -> TcpFrame<'_> {
        TcpFrame { data }
    }

    pub fn source_port(&self) -> u16 {
        u16::from_be_bytes(
            self.data[0..2]
                .try_into()
                .expect("tcp source port length wrong"),
        )
    }

    pub fn dest_port(&self) -> u16 {
        u16::from_be_bytes(
            self.data[2..4]
                .try_into()
                .expect("tcp dest port length wrong"),
        )
    }

    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(
            self.data[4..8]
                .try_into()
                .expect("tcp seq num length wrong"),
        )
    }

    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(
            self.data[8..12]
                .try_into()
                .expect("tcp ack num length wrong"),
        )
    }

    pub fn data_offset_bytes(&self) -> usize {
        let data_offset_words = self.data[12].get_bits(4, 4);
        assert!(data_offset_words != 0);
        (data_offset_words as usize) * 4
    }

    pub fn flags(&self) -> TcpFlags {
        TcpFlags(self.data[13])
    }

    pub fn window_size(&self) -> u16 {
        u16::from_be_bytes(
            self.data[14..16]
                .try_into()
                .expect("tcp window size length wrong"),
        )
    }

    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes(
            self.data[16..18]
                .try_into()
                .expect("tcp checksum length wrong"),
        )
    }

    pub fn urgent_ptr(&self) -> u16 {
        u16::from_be_bytes(
            self.data[18..20]
                .try_into()
                .expect("tcp checksum length wrong"),
        )
    }

    pub fn payload(&self) -> &[u8] {
        &self.data[self.data_offset_bytes()..]
    }
}

impl core::fmt::Debug for TcpFrame<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "source_port: {}", self.source_port())?;
        writeln!(f, "dest_port: {}", self.dest_port())?;
        writeln!(f, "seq_num: {}", self.seq_num())?;
        writeln!(f, "ack_num: {}", self.ack_num())?;
        writeln!(f, "data_offset_bytes: {}", self.data_offset_bytes())?;
        writeln!(f, "flags: {:?}", self.flags())?;
        writeln!(f, "window_size: {}", self.window_size())?;
        writeln!(f, "checksum: {:x}", self.checksum())?;
        writeln!(f, "urgent_ptr: {}", self.urgent_ptr())?;
        Ok(())
    }
}

pub struct TcpFrameParams<'a> {
    pub source_address: IpAddr,
    pub dest_address: IpAddr,
    pub source_port: u16,
    pub dest_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub flags: TcpFlags,
    pub window_size: u16,
    pub urgent_ptr: u16,
    pub payload: &'a [u8],
}

pub fn generate_tcp_frame(params: &TcpFrameParams) -> Vec<u8> {
    const HEADER_LEN: usize = 20;

    let mut ret = Vec::with_capacity(HEADER_LEN + params.payload.len());

    ret.extend_from_slice(&params.source_port.to_be_bytes());
    ret.extend_from_slice(&params.dest_port.to_be_bytes());
    ret.extend_from_slice(&params.seq_num.to_be_bytes());
    ret.extend_from_slice(&params.ack_num.to_be_bytes());
    ret.push(((HEADER_LEN / 4) << 4) as u8);
    ret.push(params.flags.0);
    ret.extend_from_slice(&params.window_size.to_be_bytes());
    const CHECKSUM: u16 = 0;
    let checksum_idx = ret.len();
    ret.extend_from_slice(&CHECKSUM.to_be_bytes());
    ret.extend_from_slice(&params.urgent_ptr.to_be_bytes());
    ret.extend_from_slice(params.payload);

    let mut checksum_frame = Vec::new();
    checksum_frame.extend_from_slice(&params.source_address);
    checksum_frame.extend_from_slice(&params.dest_address);
    checksum_frame.push(0);
    checksum_frame.push(Ipv4Protocol::Tcp.into());
    checksum_frame.extend_from_slice(&(ret.len() as u16).to_be_bytes());

    checksum_frame.extend_from_slice(&ret);

    if checksum_frame.len() % 2 != 0 {
        checksum_frame.push(0);
    }
    let checksum = net::calculate_ipv4_checksum(&checksum_frame);
    ret[checksum_idx..checksum_idx + 2].copy_from_slice(&checksum.to_be_bytes());

    ret
}

fn generate_tcp_push(tcp_key: &TcpKey, state: &mut ConnectedState, data: &Rc<[u8]>) -> Vec<u8> {
    let ret = generate_tcp_frame(&TcpFrameParams {
        source_address: tcp_key.local_ip,
        dest_address: tcp_key.remote_ip,
        source_port: tcp_key.local_port,
        dest_port: tcp_key.remote_port,
        seq_num: state.seq_num,
        ack_num: state.ack_num,
        flags: TcpFlags(generate_tcp_flags(&TcpFlagsParams {
            cwr: false,
            ece: false,
            urg: false,
            ack: true,
            psh: false,
            rst: false,
            syn: false,
            fin: false,
        })),
        // FIXME: Set this to something sane?
        window_size: 512,
        urgent_ptr: 0,
        payload: data,
    });

    state.seq_num += data.len() as u32;

    ret
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct TcpListenerKey {
    ip: IpAddr,
    port: u16,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct TcpKey {
    remote_ip: IpAddr,
    local_ip: IpAddr,
    remote_port: u16,
    local_port: u16,
}

struct ConnectedState {
    seq_num: u32,
    ack_num: u32,
    tx: Sender<Vec<u8>>,
    rx: Receiver<Rc<[u8]>>,
}

enum TcpState {
    Uninit,
    SynAckSent {
        seq_num: u32,
        ack_num: u32,
        timeout: usize,
        sent_frame: OutgoingTcpPacket,
    },
    Connected(ConnectedState),
}

pub struct TcpConnection {
    rx: Receiver<Vec<u8>>,
    tx: Sender<Rc<[u8]>>,
}

impl TcpConnection {
    pub async fn read(&self) -> Vec<u8> {
        self.rx.recv().await
    }

    pub async fn write<T>(&self, data: T)
    where
        T: Into<Rc<[u8]>>,
    {
        self.tx.send(data.into()).await;
    }
}

pub struct TcpListener {
    rx: Receiver<TcpConnection>,
}

impl TcpListener {
    pub async fn connection(&self) -> TcpConnection {
        self.rx.recv().await
    }
}

pub struct Tcp {
    listeners: Mutex<HashMap<TcpListenerKey, Sender<TcpConnection>>>,
    tcp_states: Mutex<HashMap<TcpKey, TcpState>>,
    time: Rc<MonotonicTime>,
    wakeup_list: Rc<WakeupList>,
}

impl Tcp {
    pub fn new(time: Rc<MonotonicTime>, wakeup_list: Rc<WakeupList>) -> Tcp {
        Tcp {
            listeners: Mutex::new(Default::default()),
            tcp_states: Mutex::new(Default::default()),
            time,
            wakeup_list,
        }
    }

    pub async fn listen(&self, ip: IpAddr, port: u16) -> TcpListener {
        let (tx, rx) = async_channel::channel();
        let ret = TcpListener { rx };
        self.listeners
            .lock()
            .await
            .insert(TcpListenerKey { ip, port }, tx);
        ret
    }

    #[allow(clippy::type_complexity)]
    pub fn handle_frame<'a>(
        &'a self,
        frame: &'a TcpFrame<'_>,
        source_ip: &'a IpAddr,
        dest_ip: &'a IpAddr,
        rng: &'a Mutex<Rng>,
    ) -> Pin<Box<dyn Future<Output = Option<Rc<[u8]>>> + 'a>> {
        Box::pin(async move {
            let tcp_key = TcpKey {
                remote_ip: *source_ip,
                local_ip: *dest_ip,
                remote_port: frame.source_port(),
                local_port: frame.dest_port(),
            };

            let mut tcp_states = self.tcp_states.lock().await;
            let state = tcp_states.entry(tcp_key).or_insert(TcpState::Uninit);
            let flags = frame.flags();

            match state {
                TcpState::Uninit => {
                    if !flags.syn() {
                        return None;
                    }

                    let seq_num = rng.lock().await.u64() as u32;
                    let ack_num = frame.seq_num() + 1;
                    let response_frame = net::tcp::generate_tcp_frame(&TcpFrameParams {
                        source_address: *dest_ip,
                        dest_address: *source_ip,
                        ack_num,
                        seq_num,
                        dest_port: frame.source_port(),
                        source_port: frame.dest_port(),
                        window_size: frame.window_size(),
                        flags: net::tcp::TcpFlags(net::tcp::generate_tcp_flags(&TcpFlagsParams {
                            cwr: false,
                            ece: false,
                            urg: false,
                            ack: true,
                            psh: false,
                            rst: false,
                            syn: true,
                            fin: false,
                        })),
                        urgent_ptr: 0,
                        payload: &[],
                    })
                    .into();

                    let sent_frame = OutgoingTcpPacket {
                        local_ip: *dest_ip,
                        remote_ip: *source_ip,
                        payload: Rc::clone(&response_frame),
                    };

                    let timeout = (self.time.get() as f32 + 1.0 * self.time.tick_freq()) as usize;
                    self.wakeup_list.register_wakeup_time(timeout);
                    *state = TcpState::SynAckSent {
                        seq_num,
                        ack_num,
                        sent_frame,
                        timeout,
                    };

                    Some(response_frame)
                }
                TcpState::SynAckSent {
                    ack_num, seq_num, ..
                } => {
                    if flags.syn() {
                        debug!("Resetting connection, unexpected syn");
                        *state = TcpState::Uninit;
                        return self.handle_frame(frame, source_ip, dest_ip, rng).await;
                    }

                    if flags.psh() {
                        debug!("Unexpected push in syn-ack state");
                        return None;
                    }

                    if !flags.ack() {
                        debug!("Ack unset, ignoring");
                        return None;
                    }

                    if frame.seq_num() != *ack_num {
                        error!(
                            "Sequence number not expected in syn ack: ack_num: {}, seq_num: {}",
                            ack_num,
                            frame.seq_num()
                        );
                        return None;
                    }

                    let (tx_in, rx_in) = async_channel::channel();
                    let (tx_out, rx_out) = async_channel::channel();
                    let connection = TcpConnection {
                        rx: rx_in,
                        tx: tx_out,
                    };

                    let listener_key = TcpListenerKey {
                        ip: *dest_ip,
                        port: frame.dest_port(),
                    };

                    let listeners = self.listeners.lock().await;
                    let listener = match listeners.get(&listener_key) {
                        Some(x) => x,
                        None => {
                            error!("Syn ack ack for non existent listener");
                            return None;
                        }
                    };

                    *state = TcpState::Connected(ConnectedState {
                        seq_num: *seq_num + 1,
                        ack_num: *ack_num + frame.payload().len() as u32,
                        tx: tx_in,
                        rx: rx_out,
                    });

                    listener.send(connection).await;

                    None
                }
                TcpState::Connected(ref mut state) => {
                    if state.ack_num != frame.seq_num() {
                        debug!(
                            "ack num did not match seq num: {} {}",
                            state.ack_num,
                            frame.seq_num()
                        );
                        return None;
                    }

                    state.ack_num = frame.seq_num() + frame.payload().len() as u32;

                    let response_frame = net::tcp::generate_tcp_frame(&TcpFrameParams {
                        source_address: *dest_ip,
                        dest_address: *source_ip,
                        ack_num: state.ack_num,
                        seq_num: state.seq_num,
                        dest_port: frame.source_port(),
                        source_port: frame.dest_port(),
                        window_size: frame.window_size(),
                        flags: net::tcp::TcpFlags(net::tcp::generate_tcp_flags(&TcpFlagsParams {
                            cwr: false,
                            ece: false,
                            urg: false,
                            ack: true,
                            psh: false,
                            rst: false,
                            syn: false,
                            fin: false,
                        })),
                        urgent_ptr: 0,
                        payload: &[],
                    })
                    .into();

                    if frame.flags().psh() {
                        state.tx.send(frame.payload().to_vec()).await;
                    }

                    Some(response_frame)
                }
            }
        })
    }

    pub async fn service(&self) -> OutgoingTcpPacket {
        OutgoingPoller {
            tcp_states: &self.tcp_states,
            time: &self.time,
        }
        .await
    }
}

struct OutgoingPoller<'a> {
    tcp_states: &'a Mutex<HashMap<TcpKey, TcpState>>,
    time: &'a MonotonicTime,
}

impl Future for OutgoingPoller<'_> {
    type Output = OutgoingTcpPacket;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let guard = core::pin::pin!(self.tcp_states.lock()).poll(cx);

        let mut guard = match guard {
            Poll::Ready(v) => v,
            Poll::Pending => return Poll::Pending,
        };

        for (tcp_key, tcp_state) in &mut *guard {
            match tcp_state {
                TcpState::Connected(connection) => {
                    let data = match core::pin::pin!(connection.rx.recv()).poll(cx) {
                        Poll::Ready(v) => v,
                        Poll::Pending => continue,
                    };

                    return Poll::Ready(write_request_to_outgoing_packet(
                        tcp_key, connection, &data,
                    ));
                }
                TcpState::SynAckSent {
                    ref mut timeout,
                    sent_frame,
                    ..
                } => {
                    if self.time.get() > *timeout {
                        *timeout += (self.time.tick_freq() * 1.0) as usize;
                        return Poll::Ready(sent_frame.clone());
                    }
                }
                _ => (),
            }
        }

        Poll::Pending
    }
}

fn write_request_to_outgoing_packet(
    tcp_key: &TcpKey,
    connected_state: &mut ConnectedState,
    data: &Rc<[u8]>,
) -> OutgoingTcpPacket {
    let payload = generate_tcp_push(tcp_key, connected_state, data);
    OutgoingTcpPacket {
        local_ip: tcp_key.local_ip,
        remote_ip: tcp_key.remote_ip,
        payload: payload.into(),
    }
}

#[derive(Clone)]
pub struct OutgoingTcpPacket {
    pub local_ip: IpAddr,
    pub remote_ip: IpAddr,
    pub payload: Rc<[u8]>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;
    use crate::MonotonicTime;
    use crate::WakeupList;

    create_test!(test_tcp_frame_parsing, {
        const TCP_SYN: &[u8] = &[
            0x80, 0xd8, 0x17, 0x70, 0x5a, 0x5b, 0x14, 0x47, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x02,
            0xfa, 0xf0, 0x7e, 0xa4, 0x00, 0x00, 0x02, 0x04, 0x05, 0xb4, 0x04, 0x02, 0x08, 0x0a,
            0x41, 0xcf, 0x00, 0x5d, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03, 0x03, 0x07,
        ];

        let frame = TcpFrame::new(TCP_SYN);
        test_eq!(frame.source_port(), 32984);
        test_eq!(frame.dest_port(), 6000);
        test_eq!(frame.seq_num(), 1515918407);
        test_eq!(frame.ack_num(), 0);
        test_eq!(frame.data_offset_bytes(), 40);
        test_eq!(frame.flags().0, 2);
        test_eq!(frame.window_size(), 64240);
        test_eq!(frame.checksum(), 0x7ea4);
        test_eq!(frame.urgent_ptr(), 0);

        Ok(())
    });

    create_test!(test_tcp_flags_back_and_forth, {
        let flags = generate_tcp_flags(&TcpFlagsParams {
            cwr: true,
            ece: false,
            urg: false,
            ack: true,
            psh: false,
            rst: true,
            syn: true,
            fin: true,
        });

        let parsed_flags = TcpFlags(flags);
        test_eq!(parsed_flags.cwr(), true);
        test_eq!(parsed_flags.ece(), false);
        test_eq!(parsed_flags.urg(), false);
        test_eq!(parsed_flags.ack(), true);
        test_eq!(parsed_flags.psh(), false);
        test_eq!(parsed_flags.rst(), true);
        test_eq!(parsed_flags.syn(), true);
        test_eq!(parsed_flags.fin(), true);

        Ok(())
    });

    create_test!(test_dropped_syn_ack_ack, {
        const TCP_SYN: &[u8] = b"\x89\x06\x27\x0f\xcc\x6b\x38\x32\x00\x00\x00\x00\xa0\x02\xfa\xf0\x22\xb5\x00\x00\x02\x04\x05\xb4\x04\x02\x08\x0a\xc3\x8b\x2c\xc7\x00\x00\x00\x00\x01\x03\x03\x07";
        const TCP_ACK: &[u8] =
            b"\x89\x06\x27\x0f\xcc\x6b\x38\x33\x00\x39\x84\x21\x50\x10\xfa\xf0\xf6\x80\x00\x00";
        const SOURCE_IP: IpAddr = [192, 168, 2, 1];
        const DEST_IP: IpAddr = [192, 168, 2, 2];

        let time = Rc::new(MonotonicTime::new(10.0));
        let wakeup_list = Rc::new(WakeupList::new());
        let rng = Mutex::new(Rng::new());

        let tcp = Tcp::new(Rc::clone(&time), wakeup_list);
        let listener = tcp.listen(DEST_IP, 9999).await;

        let frame = TcpFrame::new(TCP_SYN);
        tcp.handle_frame(&frame, &SOURCE_IP, &DEST_IP, &rng).await;

        // We should get a syn-ack response from the initial syn
        if futures::future::poll_immediate(tcp.service())
            .await
            .is_some()
        {
            return Err("Unexpected serivce response".into());
        }

        // After 2 seconds we should have waited enough to trigger a syn-ack resend
        time.set_tick((time.tick_freq() * 2.0) as usize);

        let syn_ack = match futures::future::poll_immediate(tcp.service()).await {
            Some(v) => v,
            None => return Err("Syn ack retransmit missing".into()),
        };

        let syn_ack = TcpFrame::new(&syn_ack.payload);

        test_true!(syn_ack.flags().syn());
        test_true!(syn_ack.flags().ack());

        let frame = TcpFrame::new(TCP_ACK);
        tcp.handle_frame(&frame, &SOURCE_IP, &DEST_IP, &rng).await;

        if futures::future::poll_immediate(listener.connection())
            .await
            .is_none()
        {
            return Err("Connection not ready".into());
        }

        Ok(())
    });
}
