use crate::error::Result;
use crate::templates::ProjectTemplate;
use std::collections::HashMap;
use std::path::Path;

pub fn load(files: &mut HashMap<String, String>, service_name: &str) -> Result<()> {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");

    // Cargo.toml
    ProjectTemplate::load_file(
        &fixtures_root.join("rust/Cargo.toml.hbs"),
        files,
        "Cargo.toml",
    )?;

    // src/lib.rs
    ProjectTemplate::load_file(&fixtures_root.join("rust/lib.rs.hbs"), files, "src/lib.rs")?;

    // Actr.toml
    ProjectTemplate::load_file(
        &fixtures_root.join("rust/Actr.toml.hbs"),
        files,
        "Actr.toml",
    )?;

    // proto/remote/{service_name}/echo.proto
    let proto_path = format!("protos/remote/{}/echo.proto", service_name);
    ProjectTemplate::load_file(
        &fixtures_root.join("echo-service/echo.proto"),
        files,
        &proto_path,
    )?;

    // build.rs
    ProjectTemplate::load_file(&fixtures_root.join("rust/build.rs.hbs"), files, "build.rs")?;

    // README.md
    ProjectTemplate::load_file(
        &fixtures_root.join("rust/README.md.hbs"),
        files,
        "README.md",
    )?;

    // .gitignore
    ProjectTemplate::load_file(
        &fixtures_root.join("rust/gitignore.hbs"),
        files,
        ".gitignore",
    )?;

    Ok(())
}
