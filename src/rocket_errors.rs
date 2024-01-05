use rocket::catch;
use rocket_dyn_templates::Template;
use serde_json::json;

#[catch(500)]
pub fn internal_error() -> Template {
    Template::render("500", json!({}))
}

#[catch(404)]
pub fn not_found() -> Template {
    Template::render("404", json!({}))
}
