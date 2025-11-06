#![cfg(feature = "network-quic")]

use std::time::Duration;

use theta_engine::network::transport::WebRtcTransport;
use theta_engine::network::voice::VoicePacket;
use tokio::runtime::Builder as RuntimeBuilder;

#[test]
fn send_and_receive_voice_packet_over_loopback() {
    let (tx, rx) = {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(WebRtcTransport::pair()).expect("pair")
    };

    let payload = vec![1u8; 32];
    let packet = VoicePacket::new(1, 0, payload.clone());

    let runtime_sender = RuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .expect("sender runtime");

    runtime_sender.block_on(async {
        tx.send_voice_packet(&packet).await.expect("send packet");
    });

    let runtime_receiver = RuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .expect("receiver runtime");

    let received = runtime_receiver
        .block_on(async {
            rx.receive_voice_packet(Duration::from_secs(1))
                .await
                .expect("receive result")
        })
        .expect("packet available");

    assert_eq!(received.sequence, packet.sequence);
    assert_eq!(received.payload, payload);
}
