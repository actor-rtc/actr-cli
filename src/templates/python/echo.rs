use crate::error::Result;
use crate::templates::ProjectTemplate;
use std::collections::HashMap;
use std::path::Path;

pub fn load(files: &mut HashMap<String, String>) -> Result<()> {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let python_fixtures = fixtures_root.join("python/echo");

    // Note: proto files are no longer created during init, they will be pulled via actr install
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
