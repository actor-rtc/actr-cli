use crate::error::Result;
use crate::templates::ProjectTemplate;
use std::collections::HashMap;
use std::path::Path;

pub fn load(files: &mut HashMap<String, String>, service_name: &str) -> Result<()> {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let python_fixtures = fixtures_root.join("python/echo");

    // proto/remote/{service_name}/echo.proto
    let proto_path = format!("proto/remote/{}/echo.proto", service_name);
    ProjectTemplate::load_file(&fixtures_root.join("echo.proto"), files, &proto_path)?;
    ProjectTemplate::load_file(
        &python_fixtures.join("Actr.server.toml.jinja2"),
        files,
        "server/Actr.toml",
    )?;
    ProjectTemplate::load_file(
        &python_fixtures.join("Actr.client.toml.jinja2"),
        files,
        "client/Actr.toml",
    )?;
    ProjectTemplate::load_file(
        &python_fixtures.join("server.py.jinja2"),
        files,
        "server/server.py",
    )?;
    ProjectTemplate::load_file(
        &python_fixtures.join("client.py.jinja2"),
        files,
        "client/client.py",
    )?;
    ProjectTemplate::load_file(
        &python_fixtures.join("README.md.jinja2"),
        files,
        "README.md",
    )?;
    ProjectTemplate::load_file(
        &python_fixtures.join("gitignore.jinja2"),
        files,
        ".gitignore",
    )?;

    files.insert("generated/__init__.py".to_string(), "".to_string());

    Ok(())
}
