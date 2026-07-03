use rocket_dyn_templates::tera::{Context, Tera};
use std::fs;
use std::sync::OnceLock;

use crate::errors::Error;
use crate::helper::{base64_encode, extract_spacer_name};
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
        tera.register_filter("extract_spacer_name", extract_spacer_name);
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
}
