//! 六平面线格式的内存回环：同一套 `write_frame` / `read_frame` 与真实 QUIC
//! 平面共用，验证每个平面的固定头 + 变长体在通道上可往返。

use aerlyn_wire::plane::type_id;
use aerlyn_wire::{
    memory, AudioFmt, AudioFrame, AudioHeader, ClipAck, ClipData, ClipDataHeader, CursorPos,
    CursorReset, CursorShape, CursorShapeHeader, FileAck, FileChunk, FileChunkZstd,
    FileChunkZstdPayload, FileEnd, InputBatch, InputEvent, NetReport, SliceNack, CH_AUDIO,
    CH_CLIPBOARD, CH_CURSOR, CH_FEEDBACK, CH_FILE, CH_INPUT,
};

#[test]
fn six_planes_roundtrip_over_memory_connection() {
    let (mut host, mut client) = memory::pair();

    // 1) 输入：批量事件。
    let input = InputBatch {
        seq: 7,
        ts_us: 1_700_000_000_000,
        monitor: 0,
        events: vec![
            InputEvent::Move { x: 100, y: 200 },
            InputEvent::Key {
                vk: 65,
                scan: 30,
                chr: 97,
                mods: 0,
                down: true,
            },
        ],
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_INPUT,
        type_id::input::INPUT_BATCH,
        0,
        &input.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(frame.channel, CH_INPUT);
    assert_eq!(frame.type_id, type_id::input::INPUT_BATCH);
    assert_eq!(InputBatch::decode(&frame.payload).unwrap(), input);

    // 2) 光标：形状、位置、重置。
    let shape = CursorShape {
        header: CursorShapeHeader {
            cursor_id: 3,
            generation: 1,
            hx: 0,
            hy: 0,
            width: 2,
            height: 2,
            format: 0,
        },
        data: vec![0x11; 16],
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_CURSOR,
        type_id::cursor::CURSOR_SHAPE,
        0,
        &shape.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(CursorShape::decode(&frame.payload).unwrap(), shape);

    let pos = CursorPos {
        x: 10,
        y: 20,
        shape_id: 3,
        generation: 1,
        visible: true,
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_CURSOR,
        type_id::cursor::CURSOR_POS,
        0,
        &pos.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(CursorPos::decode(&frame.payload).unwrap(), pos);

    let reset = CursorReset { generation: 2 };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_CURSOR,
        type_id::cursor::CURSOR_RESET,
        0,
        &reset.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(CursorReset::decode(&frame.payload).unwrap(), reset);

    // 3) 文件：普通块、ack、结束、zstd 块。
    let file_chunk = FileChunk {
        offset: 0,
        data: vec![0xAB; 1024],
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_FILE,
        type_id::file::FILE_CHUNK,
        0,
        &file_chunk.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(FileChunk::decode(&frame.payload).unwrap(), file_chunk);

    let file_ack = FileAck {
        received_upto: 1024,
        credit: 8,
    };
    aerlyn_wire::channel::write_frame(
        &mut client,
        CH_FILE,
        type_id::file::FILE_ACK,
        0,
        &file_ack.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut host).unwrap();
    assert_eq!(FileAck::decode(&frame.payload).unwrap(), file_ack);

    let file_end = FileEnd { sha256: [9; 32] };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_FILE,
        type_id::file::FILE_END,
        0,
        &file_end.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(FileEnd::decode(&frame.payload).unwrap(), file_end);

    let zstd = FileChunkZstdPayload {
        header: FileChunkZstd {
            offset: 1024,
            uncompressed_len: 4096,
        },
        data: vec![0x28, 0xB5, 0x2F, 0xFD],
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_FILE,
        type_id::file::FILE_CHUNK_ZSTD,
        0,
        &zstd.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(FileChunkZstdPayload::decode(&frame.payload).unwrap(), zstd);

    // 4) 剪贴板：数据块与 ack。
    let clip = ClipData {
        header: ClipDataHeader {
            seq: 5,
            format: 1,
            offset: 0,
        },
        data: b"clipboard".to_vec(),
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_CLIPBOARD,
        type_id::clipboard::CLIP_DATA,
        0,
        &clip.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(ClipData::decode(&frame.payload).unwrap(), clip);

    let clip_ack = ClipAck {
        seq: 5,
        received_upto: 9,
        credit: 4,
    };
    aerlyn_wire::channel::write_frame(
        &mut client,
        CH_CLIPBOARD,
        type_id::clipboard::CLIP_ACK,
        0,
        &clip_ack.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut host).unwrap();
    assert_eq!(ClipAck::decode(&frame.payload).unwrap(), clip_ack);

    // 5) 音频：格式与帧。
    let fmt = AudioFmt {
        codec: 0,
        rate: 48_000,
        channels: 2,
        frame_ms: 10,
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_AUDIO,
        type_id::audio::AUDIO_FMT,
        0,
        &fmt.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(AudioFmt::decode(&frame.payload).unwrap(), fmt);

    let audio = AudioFrame {
        header: AudioHeader {
            seq: 1,
            pts_us: 10_000,
        },
        data: vec![0x5A; 960],
    };
    aerlyn_wire::channel::write_frame(
        &mut host,
        CH_AUDIO,
        type_id::audio::AUDIO_FRAME,
        0,
        &audio.encode().unwrap(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut client).unwrap();
    assert_eq!(AudioFrame::decode(&frame.payload).unwrap(), audio);

    // 6) 反馈：网络报告与 SliceNack。
    let report = NetReport {
        rtt_us: 25_000,
        loss_ppm: 1_000,
        recv_bps: 20_000_000,
        queue_ms: 8,
    };
    aerlyn_wire::channel::write_frame(
        &mut client,
        CH_FEEDBACK,
        type_id::feedback::NET_REPORT,
        0,
        &report.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut host).unwrap();
    assert_eq!(NetReport::decode(&frame.payload).unwrap(), report);

    let nack = SliceNack {
        frame_id: 42,
        missing_bitmap: 0b101,
        ..Default::default()
    };
    aerlyn_wire::channel::write_frame(
        &mut client,
        CH_FEEDBACK,
        type_id::feedback::SLICE_NACK,
        0,
        &nack.encode(),
    )
    .unwrap();
    let frame = aerlyn_wire::channel::read_frame(&mut host).unwrap();
    assert_eq!(SliceNack::decode(&frame.payload).unwrap(), nack);
}
