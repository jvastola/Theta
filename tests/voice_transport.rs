#![cfg(feature = "network-quic")]

use std::time::Duration;

use theta_engine::network::transport::WebRtcTransport;
use theta_engine::network::voice::VoicePacket;
use tokio::runtime::Builder as RuntimeBuilder;

#[test]
fn send_and_receive_voice_packet_over_loopback() {
    let runtime = RuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let payload = vec![1u8; 32];
    let packet = VoicePacket::new(1, 0, payload.clone());

    let received = runtime.block_on(async {
        let (tx, rx) = WebRtcTransport::pair().await.expect("pair");
        tx.send_voice_packet(&packet).await.expect("send packet");

        let received = rx
            .receive_voice_packet(Duration::from_secs(2))
            .await
            .expect("receive result")
            .expect("packet available");

        tx.close().await;
        rx.close().await;

        received
    });

    assert_eq!(received.sequence, packet.sequence);
    assert_eq!(received.payload, payload);
}
