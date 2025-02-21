use std::collections::HashMap;
pub type HandlerFn = fn() -> &'static str;
pub mod mod;
pub mod tes;

pub fn generate_routes() -> HashMap<&'static str, HandlerFn> {
let mut map = HashMap::new();
    map.insert("/mod", mod::get);
    map.insert("/tes", tes::get);

map
}