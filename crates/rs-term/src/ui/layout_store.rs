//! `layout.toml`: the pane tree and each pane's session settings.
//!
//! The tree is UI data (§5.7), so this file belongs to the UI crate. The
//! same rules as `settings.toml` apply: `schema = 1`, unknown keys and out
//! of range values reject the whole file, a rejected file is never
//! overwritten automatically, and saving writes `layout.toml.tmp`, syncs it,
//! and renames it over the old file.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use rs_term_core::names;
use rs_term_core::{Newline, SerialParams, SessionEncoding};
use serde::{Deserialize, Serialize};

use super::layout::{Axis, ViewNode};

/// herdr keeps every ratio inside 0.1..=0.9.
pub const RATIO_MILLIS: std::ops::RangeInclusive<u16> = 100..=900;
const MAX_PANES: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct PaneRecord {
    pub id: u64,
    pub params: SerialParams,
    pub encoding: SessionEncoding,
    pub newline: Newline,
    pub local_echo: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedLayout {
    pub tree: ViewNode,
    pub focus: Option<u64>,
    pub panes: Vec<PaneRecord>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    focus: Option<u64>,
    tree: RawNode,
    #[serde(default, rename = "pane")]
    panes: Vec<RawPane>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawNode {
    Leaf {
        id: u64,
    },
    Split {
        /// `side_by_side` (herdr split_vertical) or `stacked`
        /// (herdr split_horizontal).
        axis: String,
        ratio_millis: u16,
        first: Box<RawNode>,
        second: Box<RawNode>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawPane {
    id: u64,
    path: String,
    baud_rate: u32,
    data_bits: String,
    parity: String,
    stop_bits: String,
    flow: String,
    dtr: String,
    rts: bool,
    encoding: String,
    receive: String,
    transmit: String,
    local_echo: bool,
}

pub fn load_str(text: &str, read_timeout: Duration) -> Result<SavedLayout, String> {
    let raw: RawLayout =
        toml::from_str(text).map_err(|error| format!("TOML として読めない: {error}"))?;
    if raw.schema != 1 {
        return Err(format!("schema {} は読めない（1 だけ）", raw.schema));
    }
    let mut count = 0;
    let tree = node_from_raw(raw.tree, &mut count)?;
    if count > MAX_PANES {
        return Err(format!("ペインが多すぎる（{MAX_PANES} まで）"));
    }
    let mut panes = Vec::new();
    for pane in raw.panes {
        if !(1..=4_000_000).contains(&pane.baud_rate) {
            return Err(format!("pane {} の baud_rate が範囲外", pane.id));
        }
        if panes.iter().any(|seen: &PaneRecord| seen.id == pane.id) {
            return Err(format!("pane {} が重複している", pane.id));
        }
        panes.push(PaneRecord {
            id: pane.id,
            params: SerialParams {
                path: pane.path,
                baud_rate: pane.baud_rate,
                data_bits: names::parse_data_bits(&pane.data_bits)?,
                parity: names::parse_parity(&pane.parity)?,
                stop_bits: names::parse_stop(&pane.stop_bits)?,
                flow: names::parse_flow(&pane.flow)?,
                dtr_on_open: match pane.dtr.as_str() {
                    "off" => None,
                    "on" => Some(true),
                    other => return Err(format!("dtr `{other}` は off か on")),
                },
                rts: pane.rts,
                read_timeout,
            },
            encoding: names::parse_encoding(&pane.encoding)?,
            newline: Newline {
                receive: names::parse_receive(&pane.receive)?,
                transmit: names::parse_transmit(&pane.transmit)?,
            },
            local_echo: pane.local_echo,
        });
    }
    Ok(SavedLayout {
        tree,
        focus: raw.focus,
        panes,
    })
}

fn node_from_raw(raw: RawNode, count: &mut usize) -> Result<ViewNode, String> {
    match raw {
        RawNode::Leaf { id } => {
            *count += 1;
            Ok(ViewNode::Leaf(id))
        }
        RawNode::Split {
            axis,
            ratio_millis,
            first,
            second,
        } => {
            if !RATIO_MILLIS.contains(&ratio_millis) {
                return Err(format!("ratio_millis {ratio_millis} が範囲外（100〜900）"));
            }
            let axis = match axis.as_str() {
                "side_by_side" => Axis::SideBySide,
                "stacked" => Axis::Stacked,
                other => return Err(format!("axis `{other}` は side_by_side か stacked")),
            };
            Ok(ViewNode::Split {
                axis,
                ratio: f32::from(ratio_millis) / 1000.0,
                path: Vec::new(),
                first: Box::new(node_from_raw(*first, count)?),
                second: Box::new(node_from_raw(*second, count)?),
            })
        }
    }
}

fn node_to_raw(node: &ViewNode) -> RawNode {
    match node {
        ViewNode::Leaf(id) => RawNode::Leaf { id: *id },
        ViewNode::Split {
            axis,
            ratio,
            first,
            second,
            ..
        } => RawNode::Split {
            axis: match axis {
                Axis::SideBySide => "side_by_side".into(),
                Axis::Stacked => "stacked".into(),
            },
            ratio_millis: ((ratio * 1000.0).round() as u16)
                .clamp(*RATIO_MILLIS.start(), *RATIO_MILLIS.end()),
            first: Box::new(node_to_raw(first)),
            second: Box::new(node_to_raw(second)),
        },
    }
}

pub fn to_string(layout: &SavedLayout) -> Result<String, String> {
    let raw = RawLayout {
        schema: 1,
        focus: layout.focus,
        tree: node_to_raw(&layout.tree),
        panes: layout
            .panes
            .iter()
            .map(|pane| RawPane {
                id: pane.id,
                path: pane.params.path.clone(),
                baud_rate: pane.params.baud_rate,
                data_bits: names::data_bits_name(pane.params.data_bits).into(),
                parity: names::parity_name(pane.params.parity).into(),
                stop_bits: names::stop_name(pane.params.stop_bits).into(),
                flow: names::flow_name(pane.params.flow).into(),
                dtr: if pane.params.dtr_on_open == Some(true) {
                    "on".into()
                } else {
                    "off".into()
                },
                rts: pane.params.rts,
                encoding: names::encoding_name(pane.encoding).into(),
                receive: names::receive_name(pane.newline.receive).into(),
                transmit: names::transmit_name(pane.newline.transmit).into(),
                local_echo: pane.local_echo,
            })
            .collect(),
    };
    toml::to_string_pretty(&raw).map_err(|error| error.to_string())
}

pub fn load_file(path: &Path, read_timeout: Duration) -> Result<SavedLayout, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("読めない（OS: {error}）"))?;
    load_str(&text, read_timeout)
}

pub fn save_file(path: &Path, layout: &SavedLayout) -> Result<(), String> {
    let body = to_string(layout)?;
    write_atomically(path, body.as_bytes())
}

/// `path.tmp`, `sync_all`, rename. The old file stays if anything fails.
pub fn write_atomically(path: &Path, body: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("フォルダを作れない（OS: {error}）"))?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let mut file = File::create(&tmp).map_err(|error| format!("書けない（OS: {error}）"))?;
    file.write_all(body)
        .map_err(|error| format!("書けない（OS: {error}）"))?;
    file.sync_all()
        .map_err(|error| format!("書けない（OS: {error}）"))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("置き換えられない（OS: {error}）")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rs_term_core::{DataBits, Flow, Parity, ReceiveNewline, StopBits, TransmitNewline};

    fn sample() -> SavedLayout {
        SavedLayout {
            tree: ViewNode::Split {
                axis: Axis::SideBySide,
                ratio: 0.3,
                path: Vec::new(),
                first: Box::new(ViewNode::Leaf(1)),
                second: Box::new(ViewNode::Split {
                    axis: Axis::Stacked,
                    ratio: 0.5,
                    path: Vec::new(),
                    first: Box::new(ViewNode::Leaf(2)),
                    second: Box::new(ViewNode::Leaf(3)),
                }),
            },
            focus: Some(2),
            panes: vec![PaneRecord {
                id: 1,
                params: SerialParams {
                    path: "COM3".into(),
                    baud_rate: 921_600,
                    data_bits: DataBits::Seven,
                    parity: Parity::Even,
                    stop_bits: StopBits::Two,
                    flow: Flow::RtsCts,
                    dtr_on_open: Some(true),
                    rts: false,
                    read_timeout: Duration::from_millis(20),
                },
                encoding: SessionEncoding::Utf8,
                newline: Newline {
                    receive: ReceiveNewline::Lf,
                    transmit: TransmitNewline::CrLf,
                },
                local_echo: true,
            }],
        }
    }

    #[test]
    fn a_layout_round_trips() {
        let text = to_string(&sample()).unwrap();
        let loaded = load_str(&text, Duration::from_millis(20)).unwrap();
        assert_eq!(loaded, sample());
    }

    #[test]
    fn ratios_outside_herdr_range_reject_the_file() {
        let text = to_string(&sample())
            .unwrap()
            .replace("ratio_millis = 300", "ratio_millis = 950");
        assert!(load_str(&text, Duration::from_millis(20)).is_err());
    }

    #[test]
    fn unknown_keys_and_bad_values_reject_the_file() {
        let base = to_string(&sample()).unwrap();
        for bad in [
            base.replace("schema = 1", "schema = 2"),
            base.replace("local_echo = true", "local_echo = true\nextra = 1"),
            base.replace("dtr = \"on\"", "dtr = \"maybe\""),
            base.replace("axis = \"stacked\"", "axis = \"diagonal\""),
            base.replace("baud_rate = 921600", "baud_rate = 0"),
        ] {
            assert!(load_str(&bad, Duration::from_millis(20)).is_err(), "{bad}");
        }
    }

    #[test]
    fn saving_replaces_the_file() {
        let dir = std::env::temp_dir().join(format!("rs-term-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("layout.toml");
        save_file(&path, &sample()).unwrap();
        let loaded = load_file(&path, Duration::from_millis(20)).unwrap();
        assert_eq!(loaded.focus, Some(2));
        assert!(!dir.join("layout.toml.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
