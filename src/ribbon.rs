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
            filter_name: tr!("Immagini raster", "Raster images").to_string(),
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
            (cmds[0], tr!("Rettangolo", "Rectangle"), IconKind::Svg(icon)),
            (cmds[1], tr!("Poligono", "Polygon"), IconKind::Svg(POLY)),
            (cmds[2], tr!("Da polilinea", "From polyline"), IconKind::Svg(PLINE)),
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
                    title: tr!("Inserisci", "Insert"),
                    tools: vec![
                        cmd("RT_ATTACH", tr!("Inserisci immagine", "Insert image"), include_bytes!("../icons/attach.svg")),
                        cmd("RT_OSM", tr!("Mappa base", "Base map"), include_bytes!("../icons/osm.svg")),
                        file_tool(
                            "RT_FOLDER",
                            tr!("Inserisci cartella", "Insert folder"),
                            include_bytes!("../icons/path.svg"),
                            tr!("Scegli un'immagine qualsiasi nella cartella: vengono inserite tutte, ognuna col suo file world", "Choose any image in the folder: all of them are inserted, each at its world file position"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: tr!("Posizione", "Position"),
                    tools: vec![
                        cmd("RT_ALIGN", tr!("Allinea 2 punti", "Align 2 points"), include_bytes!("../icons/align.svg")),
                        cmd("RT_WORLD", tr!("Da file world", "From world file"), include_bytes!("../icons/align.svg")),
                        cmd("RT_ROTATE", tr!("Ruota", "Rotate"), include_bytes!("../icons/rotate.svg")),
                        cmd("RT_BACK", tr!("Porta dietro", "Send behind"), include_bytes!("../icons/back.svg")),
                    ],
                },
                RibbonGroup {
                    title: tr!("Ritaglio", "Clip"),
                    tools: vec![
                        clip_menu("RT_CLIP_MENU", tr!("Ritaglia", "Clip"), CLIP, ["RT_CLIP", "RT_CLIPPOLY", "RT_CLIPPL"]),
                        clip_menu("RT_HOLE_MENU", tr!("Foro", "Hole"), HOLE, ["RT_CLIPHOLE", "RT_HOLEPOLY", "RT_HOLEPL"]),
                        clip_menu("RT_CUT_MENU", tr!("Separa pezzo", "Split piece"), CUT, ["RT_CUT", "RT_CUTPOLY", "RT_CUTPL"]),
                        cmd("RT_UNCLIP", tr!("Togli ritaglio", "Remove clip"), include_bytes!("../icons/unclip.svg")),
                    ],
                },
                RibbonGroup {
                    title: tr!("Trasparenza", "Transparency"),
                    tools: vec![
                        cmd("RT_TRANSP", tr!("Colore trasparente", "Transparent colour"), include_bytes!("../icons/transp.svg")),
                        cmd("RT_ALPHAON", tr!("Attiva alfa", "Alpha on"), include_bytes!("../icons/alphaon.svg")),
                        cmd("RT_TRANSPOFF", tr!("Togli trasparenza", "Transparency off"), include_bytes!("../icons/transpoff.svg")),
                    ],
                },
                RibbonGroup {
                    title: tr!("Dimensioni", "Size"),
                    tools: vec![
                        cmd("RT_RESIZE", tr!("Trascina", "Drag"), include_bytes!("../icons/resize.svg")),
                        cmd("RT_SIZE", tr!("Dimensioni / DPI", "Size / DPI"), include_bytes!("../icons/size.svg")),
                    ],
                },
                RibbonGroup {
                    title: "File",
                    tools: vec![
                        file_tool(
                            "RT_PATH",
                            tr!("Cambia percorso", "Change path"),
                            include_bytes!("../icons/path.svg"),
                            tr!("Nuovo file per le immagini selezionate", "New file for the selected images"),
                        ),
                        file_tool(
                            "RT_FIND",
                            tr!("Cerca in cartella", "Find in folder"),
                            include_bytes!("../icons/find.svg"),
                            tr!("Scegli un file qualsiasi nella cartella dove cercare le immagini mancanti", "Choose any file in the folder to search for missing images"),
                        ),
                        cmd("RT_LIST", tr!("Elenco", "List"), include_bytes!("../icons/list.svg")),
                        cmd("RT_PURGE", tr!("Pulisci copie", "Clean up copies"), include_bytes!("../icons/purge.svg")),
                    ],
                },
                RibbonGroup {
                    title: tr!("Aiuto", "Help"),
                    tools: vec![cmd("RT_GUIDE", tr!("Guida", "Guide"), include_bytes!("../icons/guide.svg"))],
                },
            ]
        })
    }
}
