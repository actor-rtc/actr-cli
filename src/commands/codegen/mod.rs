mod kotlin;
mod python;
mod swift;
mod traits;

pub use crate::commands::SupportedLanguage;
use crate::error::Result;
use kotlin::KotlinGenerator;
use python::PythonGenerator;
use swift::SwiftGenerator;
use tracing::info;
pub use traits::{GenContext, LanguageGenerator};

pub struct GeneratorFactory;

impl GeneratorFactory {
    pub fn get_generator(language: SupportedLanguage) -> Box<dyn LanguageGenerator> {
        match language {
            SupportedLanguage::Rust => unreachable!("Rust is handled directly in GenCommand"),
            SupportedLanguage::Python => Box::new(PythonGenerator),
            SupportedLanguage::Swift => Box::new(SwiftGenerator),
            SupportedLanguage::Kotlin => Box::new(KotlinGenerator),
        }
    }
}

pub async fn execute_codegen(language: SupportedLanguage, context: &GenContext) -> Result<()> {
    let generator = GeneratorFactory::get_generator(language);

    let mut all_files = generator.generate_infrastructure(context).await?;
    if !context.no_scaffold {
        all_files.extend(generator.generate_scaffold(context).await?);
    }
    if !context.no_format {
        generator.format_code(context, &all_files).await?;
    }

    generator.validate_code(context).await?;

    info!("✅ 代码生成完成！");
    generator.print_next_steps(context);
    Ok(())
}
