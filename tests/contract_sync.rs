//! Keep the dependency-free binding contract in sync with the engine.

use aerlyn_contract as c;

#[test]
fn contract_constants_match_engine() {
    assert_eq!(c::PROTO_VERSION, aerlyn_wire::PROTO_VERSION);
    assert_eq!(c::ALPN, aerlyn_wire::ALPN);
    assert_eq!(c::ALPN_TCP, aerlyn_wire::ALPN_TCP);
    assert_eq!(c::MEDIA_HEADER_LEN, aerlyn_wire::MEDIA_HEADER_LEN);
    assert_eq!(c::MAX_RECTS, aerlyn_wire::MAX_RECTS);
    assert_eq!(c::CURSOR_POS_LEN, aerlyn_wire::CursorPos::LEN);
    assert_eq!(c::CURSOR_STATE_LEN, aerlyn_wire::CursorState::LEN);
    assert_eq!(c::AUDIO_HEADER_LEN, aerlyn_wire::AudioHeader::LEN);
    assert_eq!(c::FILE_ACK_LEN, aerlyn_wire::FileAck::LEN);
    assert_eq!(c::FILE_END_LEN, aerlyn_wire::FileEnd::LEN);
    assert_eq!(c::PRIVACY_REASON_MAX, aerlyn_wire::MAX_PRIVACY_REASON);
    assert_eq!(c::PREVIEW_MAX_FPS, aerlyn_wire::PREVIEW_MAX_FPS);
    assert_eq!(c::PREVIEW_MAX_KBPS, aerlyn_wire::PREVIEW_MAX_KBPS);
    assert_eq!(c::PREVIEW_MAX_DIM, aerlyn_wire::PREVIEW_MAX_DIM);
    assert_eq!(c::PREVIEW_QUALITY_MAX, aerlyn_wire::PREVIEW_QUALITY_MAX);
    assert_eq!(
        c::FEATURE_SEMANTIC_CURSOR,
        aerlyn_wire::FEATURE_SEMANTIC_CURSOR
    );
}
