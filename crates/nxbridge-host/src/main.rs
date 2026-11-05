use nxbridge_host::{bind_local, serve_store, Catalog, CatalogStore, Endpoint};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut port = 48200u16;
    let mut endpoints = Vec::new();
    let mut catalog_path = None;
    let mut write_catalog = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next().ok_or("--port requires a number")?.parse()?,
            "--endpoint" => endpoints.push(Endpoint::parse(
                &args.next().ok_or("--endpoint requires LABEL=IP:PORT")?,
            )?),
            "--catalog" => {
                if catalog_path.is_some() {
                    return Err("only one --catalog may be supplied".into());
                }
                let path = args.next().ok_or("--catalog requires a file path")?;
                catalog_path = Some(path);
            }
            "--catalog-write" => write_catalog = true,
            "--help" => {
                println!(
                    "nxbridge-host [--port PORT] [--endpoint LABEL=IP:PORT]... [--catalog FILE] [--catalog-write]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if endpoints.len() > 8 {
        return Err("at most eight endpoints".into());
    }
    let store = match (catalog_path, write_catalog) {
        (Some(path), true) => CatalogStore::writable(std::path::Path::new(&path))?,
        (Some(path), false) => CatalogStore::readonly(Catalog::load(std::path::Path::new(&path))?),
        (None, false) => CatalogStore::readonly(Catalog::default()),
        (None, true) => return Err("--catalog-write requires --catalog".into()),
    };
    let listener = bind_local(port)?;
    println!(
        "NXBridge local dashboard: http://{}",
        listener.local_addr()?
    );
    serve_store(listener, endpoints, Arc::new(store))?;
    Ok(())
}
