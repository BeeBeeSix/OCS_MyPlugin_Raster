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
                    title: "Ritaglio",
                    tools: vec![
                        cmd("RT_CLIP", "Ritaglia", include_bytes!("../icons/clip.svg")),
                        cmd("RT_CLIPHOLE", "Foro", include_bytes!("../icons/hole.svg")),
                        cmd("RT_CUT", "Separa pezzo", include_bytes!("../icons/cut.svg")),
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
