
use std::collections::HashMap;
use lazy_static::lazy_static;
use crate::PgConnection;

pub mod tes;

pub type HandlerFn = fn(&PgConnection) -> Result<String, may_postgres::Error>;

lazy_static! {
    pub static ref ROUTES: HashMap<&'static str, HandlerFn> = {
        let mut map = HashMap::new();
        map.insert("/tes", tes::get as HandlerFn);;
        map
    };
}
