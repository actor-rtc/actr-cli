use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "fixtures/templates/"]
pub struct TemplateAssets;

#[derive(RustEmbed)]
#[folder = "fixtures/"]
pub struct FixtureAssets;
