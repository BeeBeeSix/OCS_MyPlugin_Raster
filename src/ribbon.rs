//! The "Raster" ribbon tab. Pure data — the host renders it.

use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

use crate::imgsize::EXTENSIONS;

pub struct RasterModule;

fn cmd(id: &'static str, label: &'static str, svg: &'static [u8]) -> RibbonItem {
    RibbonItem::LargeTool(ToolDef {
        id,
        label,
        icon: IconKind::Svg(svg),
        event: ModuleEvent::Command(id.to_string()),
    })
}

/// A tool that opens the native file picker first; the host then dispatches
/// `"<command> <path>"` with the path's case preserved.
fn file_tool(id: &'static str, label: &'static str, svg: &'static [u8], title: &str) -> RibbonItem {
    RibbonItem::LargeTool(ToolDef {
        id,
        label,
        icon: IconKind::Svg(svg),
        event: ModuleEvent::PluginFileDialog {
            command: id.to_string(),
            title: title.to_string(),
            filter_name: "Immagini raster".to_string(),
            extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
        },
    })
}

const CLIP: &[u8] = include_bytes!("../icons/clip.svg");
const HOLE: &[u8] = include_bytes!("../icons/hole.svg");
const CUT: &[u8] = include_bytes!("../icons/cut.svg");
const POLY: &[u8] = include_bytes!("../icons/poly.svg");
const PLINE: &[u8] = include_bytes!("../icons/pline.svg");

/// A clip tool with its three outline choices: window, polygon drawn by
/// clicks, existing closed polyline. The face runs the window form.
fn clip_menu(id: &'static str, label: &'static str, icon: &'static [u8], cmds: [&'static str; 3]) -> RibbonItem {
    RibbonItem::LargeDropdown {
        id,
        label,
        icon: IconKind::Svg(icon),
        items: vec![
            (cmds[0], "Rettangolo", IconKind::Svg(icon)),
            (cmds[1], "Poligono", IconKind::Svg(POLY)),
            (cmds[2], "Da polilinea", IconKind::Svg(PLINE)),
        ],
        default: cmds[0],
    }
}

impl CadModule for RasterModule {
    fn id(&self) -> &'static str {
        "rastertools"
    }

    fn title(&self) -> &'static str {
        "Raster"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Inserisci",
                    tools: vec![cmd("RT_ATTACH", "Inserisci immagine", include_bytes!("../icons/attach.svg"))],
                },
                RibbonGroup {
                    title: "Ritaglio",
                    tools: vec![
                        clip_menu("RT_CLIP_MENU", "Ritaglia", CLIP, ["RT_CLIP", "RT_CLIPPOLY", "RT_CLIPPL"]),
                        clip_menu("RT_HOLE_MENU", "Foro", HOLE, ["RT_CLIPHOLE", "RT_HOLEPOLY", "RT_HOLEPL"]),
                        clip_menu("RT_CUT_MENU", "Separa pezzo", CUT, ["RT_CUT", "RT_CUTPOLY", "RT_CUTPL"]),
                        cmd("RT_UNCLIP", "Togli ritaglio", include_bytes!("../icons/unclip.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Dimensioni",
                    tools: vec![
                        cmd("RT_RESIZE", "Trascina", include_bytes!("../icons/resize.svg")),
                        cmd("RT_SIZE", "Dimensioni / DPI", include_bytes!("../icons/size.svg")),
                    ],
                },
                RibbonGroup {
                    title: "File",
                    tools: vec![
                        file_tool(
                            "RT_PATH",
                            "Cambia percorso",
                            include_bytes!("../icons/path.svg"),
                            "Nuovo file per le immagini selezionate",
                        ),
                        file_tool(
                            "RT_FIND",
                            "Cerca in cartella",
                            include_bytes!("../icons/find.svg"),
                            "Scegli un file qualsiasi nella cartella dove cercare le immagini mancanti",
                        ),
                        cmd("RT_LIST", "Elenco", include_bytes!("../icons/list.svg")),
                    ],
                },
            ]
        })
    }
}
