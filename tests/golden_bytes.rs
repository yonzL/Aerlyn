//! L5: compile-time size locks plus byte-for-byte wire snapshots.

use aerlyn_wire::{
    AudioHeader, Codec, CursorPos, CursorState, FileAck, FileEnd, MediaHeader, SliceNack,
    MEDIA_HEADER_LEN,
};

const _: () = assert!(MEDIA_HEADER_LEN == 36);
const _: () = assert!(CursorPos::LEN == 15);
const _: () = assert!(CursorState::LEN == 22);
const _: () = assert!(AudioHeader::LEN == 12);
const _: () = assert!(FileEnd::LEN == 32);
const _: () = assert!(SliceNack::LEN == 12);

#[test]
fn media_header_golden_bytes() {
    let mut h = MediaHeader::new(Codec::H264, 1, 0x0102_0304_0506_0708);
    h.set_key(true);
    let bytes = h.encode().unwrap();
    assert_eq!(bytes.len(), 36);
    assert_eq!(
        &bytes[..8],
        &[1, 1, 0, 36, 0, 0, 0, 1],
        "fmt, flags(KEY), codec, hdr_len, frame_id"
    );
    assert_eq!(&bytes[16..24], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&bytes[32..36], &[0, 0, 0, 0]);
}

#[test]
fn cursor_pos_golden_bytes() {
    let p = CursorPos {
        x: -1,
        y: 42,
        shape_id: 7,
        generation: 3,
        visible: true,
    };
    assert_eq!(
        p.encode(),
        [
            0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x07, 0x00, 0x03,
            0x01
        ]
    );
}

#[test]
fn cursor_state_golden_bytes() {
    let s = CursorState {
        x: -12,
        y: 34,
        semantic: 999,
        render_mode: 1,
        hidden_reason: 0,
        visual_offset_x: -3,
        visual_offset_y: 5,
        display_id: 2,
        position_valid: true,
        visible: true,
        seq: 17,
    };
    assert_eq!(
        s.encode(),
        [
            0xFF, 0xFF, 0xFF, 0xF4, 0x00, 0x00, 0x00, 0x22, 0x03, 0xE7, 0x01, 0x00, 0xFF, 0xFD,
            0x00, 0x05, 0x02, 0x03, 0x00, 0x00, 0x00, 0x11
        ]
    );
}

#[test]
fn audio_and_file_golden_bytes() {
    let a = AudioHeader {
        seq: 1,
        pts_us: 0x0102_0304_0506_0708,
    };
    assert_eq!(a.encode(), [0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7, 8]);

    let f = FileAck {
        received_upto: 0x0102_0304_0506_0708,
        credit: 9,
    };
    assert_eq!(f.encode(), [1, 2, 3, 4, 5, 6, 7, 8, 0, 9]);

    let mut sha = [0u8; 32];
    for (i, b) in sha.iter_mut().enumerate() {
        *b = i as u8;
    }
    assert_eq!(FileEnd { sha256: sha }.encode(), sha);
}

#[test]
fn slice_nack_golden_bytes() {
    let n = SliceNack::with_words(0x0102_0304, &[1, 2]);
    let bytes = n.encode();
    assert_eq!(bytes.len(), 20);
    assert_eq!(&bytes[..12], &[1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 1]);
    assert_eq!(&bytes[12..], &[0, 0, 0, 0, 0, 0, 0, 2]);
}
