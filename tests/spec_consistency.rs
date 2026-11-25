//! 0.4 线格式的规格一致性检查。
//!
//! 这里只锁定“名称/编号存在且不漂移”，不替代人工评审字段语义。

use std::collections::BTreeSet;

use aerlyn_wire::data::{
    AudioHeader, ClipAck, ClipDataHeader, CursorPos, FileAck, FileChunkHeader,
};
use aerlyn_wire::error::ErrorCode;
use aerlyn_wire::media::MEDIA_HEADER_LEN;
use aerlyn_wire::message::{FrameAck, CONTROL_MESSAGE_TYPES};
use aerlyn_wire::payload::FileErrorPayload;
use aerlyn_wire::plane::{
    AudioFmt, CursorReset, CursorShapeHeader, FileChunkZstd, FileEnd, FileResumeHint, NetReport,
    SliceNack, StreamEnd, StreamHello,
};
use aerlyn_wire::{MAX_EXTENSIONS, MAX_EXT_BYTES};

const SPEC: &str = include_str!("../docs/design/protocol-v4-archive.md");
const ACCEPTANCE_MATRIX: &str = include_str!("../docs/architecture/acceptance-matrix.md");
const STATUS_DOC: &str = include_str!("../docs/architecture/implementation-status.md");
const DRIFT_DOC: &str = include_str!("../docs/architecture/drift-audit.md");
const REQUIREMENTS_DOC: &str = include_str!("../docs/architecture/requirements.md");

fn control_registry() -> &'static str {
    let start = SPEC
        .find("### 7.5 control 消息注册表")
        .expect("docs/design/protocol-v4-archive.md 缺少 §7.5 control 注册表");
    let rest = &SPEC[start..];
    let end = rest.find("\n---").expect("§7.5 缺少结束分隔线");
    &rest[..end]
}

fn extension_section() -> &'static str {
    let start = SPEC
        .find("#### 7.7.2 TLV 扩展容器")
        .expect("docs/design/protocol-v4-archive.md 缺少 §7.7.2 TLV 扩展容器");
    let rest = &SPEC[start..];
    let end = rest
        .find("### 7.8 常驻会话编排")
        .expect("§7.7.2 缺少 §7.8 结束边界");
    &rest[..end]
}

fn data_plane_registry() -> &'static str {
    let start = SPEC
        .find("#### 7.7.1 二进制 type 注册表")
        .expect("docs/design/protocol-v4-archive.md 缺少 §7.7.1 二进制 type 注册表");
    let rest = &SPEC[start..];
    let end = rest
        .find("### 7.8 常驻会话编排")
        .expect("§7.7.1 缺少 §7.8 结束边界");
    &rest[..end]
}

fn control_field_section() -> &'static str {
    let start = SPEC
        .find("#### 7.5.1 关键 control JSON 字段")
        .expect("docs/design/protocol-v4-archive.md 缺少 §7.5.1 关键 control JSON 字段");
    let rest = &SPEC[start..];
    let end = rest
        .find("恢复票不是独立消息")
        .expect("§7.5.1 缺少结束边界");
    &rest[..end]
}

fn struct_block<'a>(src: &'a str, name: &str) -> &'a str {
    let needle = format!("pub struct {name} {{");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("源码缺少 {name} 结构体"));
    let rest = &src[start..];
    let end = rest.find("\n}").expect("结构体缺少结束花括号");
    &rest[..end]
}

const CONTROL_SRC: &str = include_str!("../src/control.rs");
const MESSAGE_SRC: &str = include_str!("../src/message.rs");

#[test]
fn control_field_registry_matches_spec() {
    let section = control_field_section();
    let expected: &[(&str, &str, &[&str])] = &[
        (
            "VideoConfig",
            MESSAGE_SRC,
            &[
                "req_id",
                "display",
                "codec_pref",
                "max_fps",
                "max_kbps",
                "active",
                "priority",
                "scale",
                "quality",
                "quality_mode",
                "chroma_pref",
                "bit_depth_pref",
            ],
        ),
        (
            "VideoReady",
            MESSAGE_SRC,
            &[
                "req_id",
                "display",
                "codec",
                "profile",
                "level",
                "chroma",
                "bit_depth",
                "width",
                "height",
                "max_fps",
                "max_kbps",
                "hw",
            ],
        ),
        (
            "FileOffer",
            CONTROL_SRC,
            &[
                "task_id",
                "dir",
                "name",
                "size",
                "sha256",
                "block_size",
                "block_sha_root",
                "mtime_ms",
                "conflict",
                "compress",
                "resume",
                "extensions",
            ],
        ),
        (
            "FileGet",
            CONTROL_SRC,
            &["task_id", "dir", "name", "offset"],
        ),
        (
            "FileList",
            CONTROL_SRC,
            &["task_id", "dir", "offset", "limit", "show_hidden"],
        ),
        ("FileCancel", CONTROL_SRC, &["task_id", "reason"]),
    ];

    for (name, src, fields) in expected {
        let block = struct_block(src, name);
        assert!(
            section.contains(&format!("`{name}`")),
            "docs/design/protocol-v4-archive.md §7.5.1 未登记 {name}"
        );
        for field in *fields {
            assert!(
                section.contains(&format!("`{field}`")),
                "docs/design/protocol-v4-archive.md §7.5.1 的 {name} 缺少字段 {field}"
            );
            assert!(
                block.contains(&format!("pub {field}:")),
                "{name} 结构体缺少字段 {field}"
            );
        }
    }
}

#[test]
fn control_registry_matches_spec() {
    let section = control_registry();
    let mut seen = BTreeSet::new();
    for name in CONTROL_MESSAGE_TYPES {
        assert!(seen.insert(*name), "CONTROL_MESSAGE_TYPES 重复登记 {name}");
        assert!(
            section.contains(&format!("`{name}`")),
            "docs/design/protocol-v4-archive.md §7.5 未登记 control 类型 {name}"
        );
    }
}

#[test]
fn published_error_codes_match_spec() {
    let section_start = SPEC
        .find("### 7.4 错误码")
        .expect("docs/design/protocol-v4-archive.md 缺少 §7.4 错误码");
    let section = &SPEC[section_start..];
    let end = section.find("\n---").expect("§7.4 缺少结束分隔线");
    let section = &section[..end];

    let expected = [
        (50, "CodecUnsupported", ErrorCode::CodecUnsupported),
        (51, "VideoStalled", ErrorCode::VideoStalled),
        (53, "EncoderInitFailed", ErrorCode::EncoderInitFailed),
        (54, "DecoderError", ErrorCode::DecoderError),
        (55, "CodecFallback", ErrorCode::CodecFallback),
        (60, "InjectFailed", ErrorCode::InjectFailed),
        (61, "NoDesktop", ErrorCode::NoDesktop),
        (62, "UipiBlocked", ErrorCode::UipiBlocked),
    ];
    for (code, name, variant) in expected {
        assert_eq!(variant.code(), code, "{name} 代码漂移");
        assert!(
            section.contains(&format!("| {code} | `{name}`")),
            "docs/design/protocol-v4-archive.md §7.4 未登记错误码 {code} {name}"
        );
    }
    assert!(section.contains("| 52 | — | — | 保留"));
    assert!(ErrorCode::from_code(52).is_none(), "52 必须保持保留空洞");
}

#[test]
fn acceptance_matrix_counts_match_docs() {
    let mut ids = BTreeSet::new();
    let mut green = 0;
    let mut yellow = 0;
    let mut red = 0;
    for line in ACCEPTANCE_MATRIX.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 7 || !cells[1].starts_with('T') {
            continue;
        }
        let id = cells[1];
        let status = cells[4];
        assert!(ids.insert(id.to_string()), "T1–T22 重复行 {id}");
        match status {
            "绿" => green += 1,
            "黄" => yellow += 1,
            "红" => red += 1,
            other => panic!("{id} 状态非法: {other}"),
        }
    }
    assert_eq!(
        ids.len(),
        22,
        "docs/architecture/acceptance-matrix.md 应有 T1–T22 共 22 行，实际 {}",
        ids.len()
    );
    for n in 1..=22 {
        assert!(
            ids.contains(&format!("T{n}")),
            "docs/architecture/acceptance-matrix.md 缺少 T{n}"
        );
    }

    let summary = |label: &str| -> usize {
        let prefix = format!("| {label} |");
        ACCEPTANCE_MATRIX
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("docs/architecture/acceptance-matrix.md 汇总表缺少 {label}"))
            .split('|')
            .nth(2)
            .expect("汇总行缺少数值")
            .trim()
            .parse()
            .expect("汇总数值不是整数")
    };
    assert_eq!(
        (summary("绿"), summary("黄"), summary("红")),
        (green, yellow, red)
    );
    assert_eq!(
        (green, yellow, red),
        (22, 0, 0),
        "docs/architecture/acceptance-matrix.md 矩阵计数已变化，请先更新汇总与 docs/architecture/implementation-status.md/52"
    );

    let summary_line = format!("{green} 绿 / {yellow} 黄 / {red} 红");
    assert!(
        STATUS_DOC.contains(&summary_line),
        "docs/architecture/implementation-status.md 未同步 {summary_line}"
    );
    assert!(
        DRIFT_DOC.contains(&summary_line),
        "docs/architecture/drift-audit.md 未同步 {summary_line}"
    );
}

#[test]
fn data_plane_registry_matches_spec() {
    let section = data_plane_registry();
    let expected = [
        (1, 1, "InputBatch", "变长"),
        (2, 0, "StreamHello", "14 B"),
        (2, 1, "VideoFrame", "36 B"),
        (2, 2, "StreamEnd", "1 B"),
        (3, 1, "FileChunk", "8 B"),
        (3, 2, "FileAck", "10 B"),
        (3, 3, "FileEnd", "32 B"),
        (3, 4, "FileError", "8 B + UTF-8"),
        (3, 5, "FileChunkZstd", "12 B"),
        (3, 6, "FileResumeHint", "12 B"),
        (4, 1, "CursorShape", "15 B"),
        (4, 2, "CursorReset", "2 B"),
        (4, 3, "CursorPos", "15 B"),
        (5, 1, "AudioFmt", "7 B"),
        (5, 2, "AudioFrame", "12 B"),
        (6, 1, "FrameAck", "20 B"),
        (6, 2, "NetReport", "18 B"),
        (6, 3, "SliceNack", "12 B + 8n（n≤16）"),
        (7, 1, "ClipData", "10 B"),
        (7, 2, "ClipAck", "10 B"),
    ];
    let mut pairs = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (channel, type_id, name, header) in expected {
        assert!(
            pairs.insert((channel, type_id)),
            "data plane 重复登记 channel={channel} type={type_id}"
        );
        assert!(names.insert(name), "data plane 重复登记名称 {name}");
        let row = format!("| {channel} | {type_id} | `{name}` | {header} |");
        assert!(
            section.contains(&row),
            "docs/design/protocol-v4-archive.md §7.7.1 缺少行 {row}"
        );
    }

    assert_eq!(StreamHello::LEN, 14);
    assert_eq!(MEDIA_HEADER_LEN, 36);
    assert_eq!(StreamEnd::LEN, 1);
    assert_eq!(FileChunkHeader { offset: 0 }.encode().len(), 8);
    assert_eq!(
        FileAck {
            received_upto: 0,
            credit: 0
        }
        .encode()
        .len(),
        10
    );
    assert_eq!(FileEnd::LEN, 32);
    assert_eq!(FileErrorPayload::LEN, 8);
    assert_eq!(FileChunkZstd::LEN, 12);
    assert_eq!(FileResumeHint::LEN, 12);
    assert_eq!(CursorShapeHeader::LEN, 15);
    assert_eq!(CursorReset::LEN, 2);
    assert_eq!(
        CursorPos {
            x: 0,
            y: 0,
            shape_id: 0,
            generation: 0,
            visible: false
        }
        .encode()
        .len(),
        15
    );
    assert_eq!(AudioFmt::LEN, 7);
    assert_eq!(AudioHeader { seq: 0, pts_us: 0 }.encode().len(), 12);
    assert_eq!(FrameAck::LEN, 20);
    assert_eq!(NetReport::LEN, 18);
    assert_eq!(SliceNack::LEN, 12);
    assert_eq!(
        ClipDataHeader {
            seq: 0,
            format: 0,
            offset: 0
        }
        .encode()
        .len(),
        10
    );
    assert_eq!(
        ClipAck {
            seq: 0,
            received_upto: 0,
            credit: 0
        }
        .encode()
        .len(),
        10
    );
}

#[test]
fn extension_container_matches_spec() {
    let section = extension_section();
    assert_eq!(MAX_EXTENSIONS, 64);
    assert_eq!(MAX_EXT_BYTES, 4096);
    assert!(section.contains("最多 64 项"), "§7.7.2 缺扩展项数上限");
    assert!(section.contains("4096 B"), "§7.7.2 缺扩展总长上限");
    assert!(section.contains("Extensions"), "§7.7.2 缺容器名");
}

#[test]
fn requirements_doc_covers_all_ids_and_key_constants() {
    const EXPECTED: &[&str] = &[
        "AW-REQ-001",
        "AW-REQ-002",
        "AW-REQ-003",
        "AW-REQ-004",
        "AW-REQ-005",
        "AW-REQ-006",
        "AW-REQ-010",
        "AW-REQ-011",
        "AW-REQ-012",
        "AW-REQ-013",
        "AW-REQ-014",
        "AW-REQ-015",
        "AW-REQ-016",
        "AW-REQ-017",
        "AW-REQ-018",
        "AW-REQ-019",
        "AW-REQ-020",
        "AW-REQ-021",
        "AW-REQ-022",
        "AW-REQ-023",
        "AW-REQ-030",
        "AW-REQ-031",
        "AW-REQ-032",
        "AW-REQ-033",
        "AW-REQ-034",
        "AW-REQ-035",
        "AW-REQ-036",
        "AW-REQ-037",
        "AW-REQ-038",
        "AW-REQ-040",
        "AW-REQ-041",
        "AW-REQ-042",
        "AW-REQ-043",
        "AW-REQ-044",
        "AW-REQ-045",
        "AW-REQ-046",
        "AW-REQ-050",
        "AW-REQ-051",
        "AW-REQ-052",
        "AW-REQ-053",
        "AW-REQ-054",
        "AW-REQ-060",
        "AW-REQ-061",
        "AW-REQ-062",
        "AW-REQ-063",
        "INV-01",
        "INV-02",
        "INV-03",
        "INV-04",
        "INV-05",
        "INV-06",
    ];
    let expected: BTreeSet<&str> = EXPECTED.iter().copied().collect();
    let found: BTreeSet<&str> = REQUIREMENTS_DOC
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|token| token.starts_with("AW-REQ-") || token.starts_with("INV-"))
        .collect();
    assert_eq!(
        found, expected,
        "docs/architecture/requirements.md 的需求 ID 集合漂移"
    );
    for needle in [
        "53 个已结构化类型",
        "Auth→Welcome 3 s",
        "clock_skew_ppm",
        "36 B",
        "1100 B",
        "1024 片",
        "8192 块",
        "48 kHz",
        "64 MiB",
        "128 KiB",
        "16 MiB",
        "256 KiB",
        "20 ms",
        "132 B",
    ] {
        assert!(
            REQUIREMENTS_DOC.contains(needle),
            "docs/architecture/requirements.md 缺少关键常量 {needle}"
        );
    }
}
