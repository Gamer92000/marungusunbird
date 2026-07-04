use rocket_dyn_templates::tera::{Context, Tera};
use std::fs;
use std::sync::OnceLock;

use crate::errors::Error;
use crate::helper::base64_encode;
use crate::tree::Tree;

/// A standalone Tera instance for rendering just the channel tree to an HTML
/// fragment. Needed because the WebSocket handler runs in a `'static` task and
/// cannot borrow Rocket's template engine (`Metadata`/`Template`). It reuses the
/// same `templates/tree.html.tera` macros, so the markup stays single-sourced.
static TREE_TERA: OnceLock<Tera> = OnceLock::new();

fn tree_tera() -> &'static Tera {
    TREE_TERA.get_or_init(|| {
        let mut tera = Tera::default();
        let macros = fs::read_to_string("templates/tree.html.tera")
            .expect("templates/tree.html.tera must exist");
        tera.add_raw_template("tree", &macros)
            .expect("tree macros parse");
        tera.add_raw_template(
            "tree_fragment",
            "{% import \"tree\" as t %}{{ t::tree(tree=tree) }}",
        )
        .expect("tree fragment parses");
        tera.register_filter("base64_encode", base64_encode);
        tera
    })
}

/// Render the tree to an HTML fragment identical to what the `#tree` container
/// holds on a full page load.
pub fn render_tree_html(tree: &Tree) -> Result<String, Error> {
    let mut ctx = Context::new();
    ctx.insert("tree", tree);
    Ok(tree_tera().render("tree_fragment", &ctx)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper::{Channel, Client};
    use std::cell::Cell;
    use std::collections::HashMap;

    #[test]
    fn renders_tree_fragment() {
        // An empty tree exercises the macro import, filter registration, and the
        // (empty) channel loop without needing a live server.
        let tree = Tree {
            server_name: "Test Server".to_string(),
            channel_order: Vec::new(),
            channel_map: HashMap::new(),
            clients: HashMap::new(),
        };
        let html = render_tree_html(&tree).expect("fragment renders");
        assert!(html.contains("tree_item server"));
        assert!(html.contains("Test Server"));
    }

    #[test]
    fn renders_client_state_icon() {
        let channel = Channel {
            id: 1,
            name: "General".to_string(),
            parent_id: 0,
            talk_power: 0,
            is_augmented: false,
            augmentation_id: None,
            highlight_color: None,
            indent_level: Cell::new(0),
            spacer: None,
        };
        let client = Client {
            id: 5,
            name: "Muted Mike".to_string(),
            channel: 1,
            is_query: false,
            talk_power: 0,
            can_talk: true,
            state: "mic_muted",
            badges: Vec::new(),
            country: None,
        };
        let tree = Tree {
            server_name: "S".to_string(),
            channel_order: vec![1],
            channel_map: HashMap::from([(1, channel)]),
            clients: HashMap::from([(1, vec![client])]),
        };
        let html = render_tree_html(&tree).expect("fragment renders");
        // The mute state must win over can_talk and render the dedicated icon.
        assert!(html.contains("client_mic_muted.svg"));
        assert!(!html.contains("client_talk.svg"));
    }
}
