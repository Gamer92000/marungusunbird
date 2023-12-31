use rocket::{catch, Request};
use rocket_dyn_templates::Template;
use serde_json::json;

#[catch(500)]
pub fn internal_error() -> Template {
    Template::render("500", json!({}))
}

#[catch(404)]
pub fn not_found(req: &Request) -> String {
    format!("I couldn't find '{}'. Try something else?", req.uri())
}
