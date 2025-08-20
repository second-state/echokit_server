use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerEvent {
    // set Hello
    HelloStart,
    HelloChunk { data: Vec<u8> },
    HelloEnd,

    ASR { text: String },
    Action { action: String },
    StartAudio { text: String },
    AudioChunk { data: Vec<u8> },
    EndAudio,
    StartVideo,
    EndVideo,
    EndResponse,
}

#[test]
fn test_rmp_command() {
    let event = ServerEvent::Action {
        action: "say".to_string(),
    };
    let data = rmp_serde::to_vec(&event).unwrap();
    println!("Serialized data: {:?}", data);
    println!("Serialized data: {}", String::from_utf8_lossy(&data));
    let data = rmp_serde::to_vec_named(&event).unwrap();
    println!("Serialized data: {:?}", data);
    println!("Serialized data: {}", String::from_utf8_lossy(&data));
    let cmd: ServerEvent = rmp_serde::from_slice(&data).unwrap();
    match cmd {
        ServerEvent::Action { action } => {
            assert_eq!(action, "say");
        }
        _ => panic!("Unexpected command: {:?}", cmd),
    }
}
