//! V4 控制面端到端模拟：不涉及真实 QUIC/编码器，只验证
//! caps → 选择/回落 → VideoReady → CodecConfig → 帧头 → FrameAck 的闭环。

use aerlyn_wire::{
    bit_depth, chroma, select_codec, validate_extradata, Codec, CodecCap, CodecConfig,
    ControlMessage, FrameAck, MediaHeader, VideoConfig, VideoConstraints, VideoReady,
};

fn cap(codec: Codec) -> CodecCap {
    CodecCap::new(codec)
}

fn h264_extradata() -> Vec<u8> {
    vec![
        0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0, 0, 0, 1, 0x68, 0xee, 0x3c,
    ]
}

#[test]
fn full_negotiation_flow() {
    let client_caps = vec![cap(Codec::H264), cap(Codec::Av1), cap(Codec::Raw)];
    let host_caps = vec![cap(Codec::Av1), cap(Codec::H264), cap(Codec::Raw)];

    let config = VideoConfig {
        req_id: 1,
        codec_pref: vec![Codec::Av1.id(), Codec::H264.id(), Codec::Raw.id()],
        max_fps: 60,
        max_kbps: 8_000,
        quality_mode: 2,
        ..VideoConfig::default()
    };
    let wire = ControlMessage::VideoConfig(config.clone())
        .to_json()
        .unwrap();
    let decoded = ControlMessage::from_json(&wire).unwrap();
    let ControlMessage::VideoConfig(config) = decoded else {
        panic!("expected VideoConfig");
    };
    assert_eq!(config.preference()[0], Codec::Av1);

    let selected = select_codec(
        &config.preference(),
        &client_caps,
        &host_caps,
        &VideoConstraints::default(),
    )
    .unwrap();
    assert_eq!(selected.codec, Codec::Av1);

    let ready = ControlMessage::VideoReady(VideoReady {
        req_id: config.req_id,
        display: config.display,
        codec: selected.codec.id(),
        profile: selected.profile_bit.trailing_zeros() as u8,
        level: 13,
        chroma: selected.chroma,
        bit_depth: selected.bit_depth,
        width: 1920,
        height: 1080,
        max_fps: selected.max_fps,
        max_kbps: selected.max_kbps,
        hw: selected.host_hw,
    });
    let ready_wire = ready.to_json().unwrap();
    assert_eq!(ControlMessage::from_json(&ready_wire).unwrap(), ready);

    let codec_config = CodecConfig {
        display: 0,
        codec: Codec::H264.id(),
        profile: 2,
        level: 51,
        chroma: chroma::I420,
        bit_depth: bit_depth::EIGHT,
        extradata: h264_extradata(),
    };
    assert_eq!(
        validate_extradata(Codec::H264, &codec_config.extradata),
        Ok(())
    );
    let cfg_wire = ControlMessage::CodecConfig(codec_config).to_json().unwrap();
    assert!(cfg_wire.contains("\"type\":\"CodecConfig\""));

    let header = MediaHeader::new(selected.codec, 100, 1_700_000_000_000);
    let header_wire = header.encode().unwrap();
    let header_back = MediaHeader::decode(&header_wire).unwrap();
    assert_eq!(header_back.frame_id, 100);
    assert_eq!(header_back.codec(), Some(Codec::Av1));

    let ack = FrameAck {
        ack_frame_id: header_back.frame_id,
        recv_ts_us: 1_700_000_010_000,
        decode_done_us: 6_000,
        queue_depth: 1,
        dropped: 0,
    };
    let ack_wire = serde_json::to_string(&ack).unwrap();
    let ack_back: FrameAck = serde_json::from_str(&ack_wire).unwrap();
    assert_eq!(ack_back.ack_frame_id, header_back.frame_id);
}

#[test]
fn raw_fallback_when_performance_codecs_cannot_meet_fps() {
    let client_caps = vec![
        CodecCap::new(Codec::H264).with_fps(15),
        CodecCap::new(Codec::Raw),
    ];
    let host_caps = vec![
        CodecCap::new(Codec::H264).with_fps(15),
        CodecCap::new(Codec::Raw),
    ];
    let constraints = VideoConstraints {
        max_fps: 60,
        ..VideoConstraints::default()
    };
    let selected = select_codec(
        &[Codec::H264, Codec::Raw],
        &client_caps,
        &host_caps,
        &constraints,
    )
    .unwrap();
    assert_eq!(selected.codec, Codec::Raw);
    assert_eq!(selected.chroma, chroma::BGRA);
    assert_eq!(selected.max_fps, 5);
    assert_eq!(
        validate_extradata(Codec::Raw, &[]),
        Ok(()),
        "RAW needs no extradata"
    );
}

#[test]
fn codec_switch_keeps_frame_id_monotonic() {
    let caps = vec![cap(Codec::Av1), cap(Codec::H264), cap(Codec::Raw)];
    let constraints = VideoConstraints::default();

    let first = select_codec(&[Codec::Av1], &caps, &caps, &constraints).unwrap();
    let mut h1 = MediaHeader::new(first.codec, 7, 1_000);
    h1.set_key(true);
    assert_eq!(h1.frame_id, 7);

    let second = select_codec(&[Codec::H264], &caps, &caps, &constraints).unwrap();
    let h2 = MediaHeader::new(second.codec, 8, 2_000);
    assert_eq!(second.codec, Codec::H264);
    assert_eq!(h2.frame_id, 8);
    assert!(h2.frame_id > h1.frame_id);
}

#[test]
fn unknown_control_type_does_not_break_known_messages() {
    let unknown = r#"{"type":"FutureVideoMessage","display":0,"payload":{"x":1}}"#;
    let parsed = ControlMessage::from_json(unknown).unwrap();
    assert_eq!(parsed.type_name(), "FutureVideoMessage");
    assert!(parsed.to_json().unwrap().contains("FutureVideoMessage"));

    let known = r#"{"type":"VideoConfig","req_id":2,"display":0,"max_fps":30,"max_kbps":4000}"#;
    let known = ControlMessage::from_json(known).unwrap();
    assert!(matches!(known, ControlMessage::VideoConfig(_)));
}

#[test]
fn invalid_h264_extradata_is_rejected() {
    assert!(validate_extradata(Codec::H264, &[0, 0, 1, 0x67]).is_err());
    assert!(validate_extradata(Codec::H265, &[]).is_err());
    assert!(validate_extradata(Codec::Av1, &[0x10]).is_err());
}
