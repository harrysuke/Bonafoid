// The generated crate exposes a COMPLETE typed client for the backend -
// every entity's CRUD functions, every field parser, every form submit -
// but the generated pages use only a subset of it. That is the point: the
// pages are what the user designed, and the rest of the client is there
// for the pages they design next.
#![allow(dead_code)]

mod api;
mod app;
mod components;
mod convert;
mod pages;
mod types;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
