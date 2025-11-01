#!/usr/bin/env rust-script
//! Test MQTT client to verify NetGet MQTT broker
//!
//! ```cargo
//! [dependencies]
//! rumqttc = "0.24"
//! tokio = { version = "1", features = ["full"] }
//! ```

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🔌 Connecting to MQTT broker on 127.0.0.1:1883...");

    let mut mqttoptions = MqttOptions::new("test-client", "127.0.0.1", 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    println!("📡 Attempting MQTT connection...");

    // Try to connect and receive CONNACK
    let mut connected = false;
    for i in 0..10 {
        match tokio::time::timeout(Duration::from_secs(2), eventloop.poll()).await {
            Ok(Ok(Event::Incoming(Packet::ConnAck(connack)))) => {
                println!("✅ Received CONNACK from MQTT broker!");
                println!("   Session present: {}", connack.session_present);
                connected = true;
                break;
            }
            Ok(Ok(event)) => {
                println!("📬 MQTT event #{}: {:?}", i + 1, event);
            }
            Ok(Err(e)) => {
                eprintln!("❌ MQTT error: {}", e);
                break;
            }
            Err(_) => {
                eprintln!("⏱️  Timeout waiting for CONNACK (attempt {})", i + 1);
            }
        }
    }

    if connected {
        println!("\n🎉 MQTT connection successful!");

        // Try to subscribe to a topic
        println!("\n📝 Subscribing to test/# topic...");
        match client.subscribe("test/#", QoS::AtMostOnce).await {
            Ok(_) => println!("✅ Subscribe request sent"),
            Err(e) => eprintln!("❌ Subscribe failed: {}", e),
        }

        // Wait a bit for any responses
        tokio::time::sleep(Duration::from_secs(1)).await;

        println!("\n✅ Test complete - MQTT broker is working!");
    } else {
        eprintln!("\n❌ Failed to connect to MQTT broker");
        std::process::exit(1);
    }
}
