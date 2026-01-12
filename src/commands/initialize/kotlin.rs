use super::{InitContext, ProjectInitializer};
use crate::error::{ActrCliError, Result};
use crate::templates::ProjectTemplateName;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;

pub struct KotlinInitializer;

#[async_trait]
impl ProjectInitializer for KotlinInitializer {
    async fn generate_project_structure(&self, context: &InitContext) -> Result<()> {
        if context.template != ProjectTemplateName::Echo {
            return Err(ActrCliError::InvalidProject(format!(
                "Unknown template: {}",
                context.template
            )));
        }

        // Note: proto files are no longer created during init, they will be pulled via actr install

        let project_name_pascal = to_pascal_case(&context.project_name);
        let package_name = to_package_name(&context.project_name);
        let package_path = package_name.replace('.', "/");

        // Extract host from signaling URL (e.g., "ws://10.30.3.206:8081/signaling/ws" -> "10.30.3.206")
        let signaling_host = context
            .signaling_url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .split(':')
            .next()
            .unwrap_or("10.0.2.2")
            .to_string();

        let replacements = vec![
            ("{{PROJECT_NAME}}".to_string(), context.project_name.clone()),
            (
                "{{PROJECT_NAME_PASCAL}}".to_string(),
                project_name_pascal.clone(),
            ),
            ("{{PACKAGE_NAME}}".to_string(), package_name.clone()),
            ("{{PACKAGE_PATH}}".to_string(), package_path.clone()),
            (
                "{{SIGNALING_URL}}".to_string(),
                context.signaling_url.clone(),
            ),
            ("{{SIGNALING_HOST}}".to_string(), signaling_host),
        ];

        let fixtures_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let app_dir = context.project_dir.join("app");
        let java_dir = app_dir.join("src/main/java").join(&package_path);

        // Note: proto files are no longer created during init, they will be pulled via actr install

        // Root level files
        let files = vec![
            (
                fixtures_root.join("kotlin/settings.gradle.kts"),
                context.project_dir.join("settings.gradle.kts"),
            ),
            (
                fixtures_root.join("kotlin/build.gradle.kts"),
                context.project_dir.join("build.gradle.kts"),
            ),
            (
                fixtures_root.join("kotlin/gradle.properties"),
                context.project_dir.join("gradle.properties"),
            ),
            (
                fixtures_root.join("kotlin/Actr.toml"),
                context.project_dir.join("Actr.toml"),
            ),
            (
                fixtures_root.join("kotlin/gitignore"),
                context.project_dir.join(".gitignore"),
            ),
            // Note: proto files are no longer created during init, they will be pulled via actr install
            // App module files
            (
                fixtures_root.join("kotlin/app/build.gradle.kts"),
                app_dir.join("build.gradle.kts"),
            ),
            (
                fixtures_root.join("kotlin/app/src/main/AndroidManifest.xml"),
                app_dir.join("src/main/AndroidManifest.xml"),
            ),
            // Resources
            (
                fixtures_root.join("kotlin/app/src/main/res/values/strings.xml"),
                app_dir.join("src/main/res/values/strings.xml"),
            ),
            (
                fixtures_root.join("kotlin/app/src/main/res/values/colors.xml"),
                app_dir.join("src/main/res/values/colors.xml"),
            ),
            (
                fixtures_root.join("kotlin/app/src/main/res/values/themes.xml"),
                app_dir.join("src/main/res/values/themes.xml"),
            ),
            (
                fixtures_root.join("kotlin/app/src/main/res/layout/activity_main.xml"),
                app_dir.join("src/main/res/layout/activity_main.xml"),
            ),
            // Assets
            (
                fixtures_root.join("kotlin/app/src/main/assets/actr-config.toml"),
                app_dir.join("src/main/assets/actr-config.toml"),
            ),
            // Kotlin source files
            (
                fixtures_root.join("kotlin/app/src/main/java/MainActivity.kt"),
                java_dir.join("MainActivity.kt"),
            ),
            (
                fixtures_root.join("kotlin/app/src/main/java/ActrService.kt"),
                java_dir.join("ActrService.kt"),
            ),
            // Android Test files
            (
                fixtures_root.join("kotlin/app/src/androidTest/java/EchoIntegrationTest.kt"),
                app_dir
                    .join("src/androidTest/java")
                    .join(&package_path)
                    .join("EchoIntegrationTest.kt"),
            ),
        ];

        for (fixture_path, output_path) in files {
            let template = std::fs::read_to_string(&fixture_path).map_err(|e| {
                ActrCliError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Failed to read fixture {}: {}", fixture_path.display(), e),
                ))
            })?;
            let rendered = apply_placeholders(&template, &replacements);
            write_file(&output_path, &rendered)?;
        }

        // Copy gradle wrapper
        copy_gradle_wrapper(&context.project_dir)?;

        info!("📁 Created Android project structure");

        // Note: Framework code generation is skipped during init
        // Users should run 'actr install' first to get proto files, then 'actr gen'

        Ok(())
    }

    fn print_next_steps(&self, context: &InitContext) {
        let _project_name_pascal = to_pascal_case(&context.project_name);
        let package_path = to_package_name(&context.project_name).replace('.', "/");

        info!("");
        info!("Next steps:");
        if !context.is_current_dir {
            info!("  cd {}", context.project_dir.display());
        }
        info!("  actr install  # Install remote protobuf dependencies from Actr.toml");
        info!(
            "  actr gen -l kotlin -i protos/remote/{{service-name}}/{{proto-file}} -o app/src/main/java/{}/generated",
            package_path
        );
        info!("  ./gradlew assembleDebug");
        info!("  # Install APK: adb install app/build/outputs/apk/debug/app-debug.apk");
        info!("");
        info!("💡 Tips:");
        info!("  - For Android emulator, use ws://10.0.2.2:PORT to reach host localhost");
        info!("  - actr-kotlin library is fetched from JitPack automatically");
        info!(
            "  - Generated framework code is in app/src/main/java/{}/generated/",
            package_path
        );
        info!("  - Run tests: ./gradlew connectedDebugAndroidTest");
    }
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn apply_placeholders(template: &str, replacements: &[(String, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        rendered = rendered.replace(key, value);
    }
    rendered
}

fn to_pascal_case(input: &str) -> String {
    let mut result = String::new();
    let mut start_of_word = true;

    for c in input.chars() {
        if !c.is_alphanumeric() {
            start_of_word = true;
            continue;
        }

        if c.is_uppercase() {
            result.push(c);
            start_of_word = false;
        } else if start_of_word {
            result.push(c.to_uppercase().next().unwrap_or(c));
            start_of_word = false;
        } else {
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
    }

    result
}

fn to_package_name(project_name: &str) -> String {
    // Convert project name to valid Android package name
    // e.g., "my-echo-client" -> "io.actr.myechoclient"
    let clean_name: String = project_name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    format!("io.actr.{}", clean_name)
}

fn copy_gradle_wrapper(project_dir: &Path) -> Result<()> {
    // Create gradle wrapper directory
    let wrapper_dir = project_dir.join("gradle/wrapper");
    std::fs::create_dir_all(&wrapper_dir)?;

    // Create gradle-wrapper.properties
    // Note: AGP 8.12+ requires Gradle 8.13+
    let wrapper_properties = r#"distributionBase=GRADLE_USER_HOME
distributionPath=wrapper/dists
distributionUrl=https\://services.gradle.org/distributions/gradle-8.13-bin.zip
networkTimeout=10000
validateDistributionUrl=true
zipStoreBase=GRADLE_USER_HOME
zipStorePath=wrapper/dists
"#;
    std::fs::write(
        wrapper_dir.join("gradle-wrapper.properties"),
        wrapper_properties,
    )?;

    // Copy gradle-wrapper.jar (binary file)
    let wrapper_jar = include_bytes!("../../../fixtures/kotlin/gradle-wrapper.jar");
    std::fs::write(wrapper_dir.join("gradle-wrapper.jar"), wrapper_jar)?;

    // Create gradlew script
    let gradlew = include_str!("../../../fixtures/kotlin/gradlew");
    if !gradlew.is_empty() {
        std::fs::write(project_dir.join("gradlew"), gradlew)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                project_dir.join("gradlew"),
                std::fs::Permissions::from_mode(0o755),
            )?;
        }
    } else {
        // Fallback: create a minimal gradlew that downloads the wrapper
        let gradlew_fallback = r#"#!/bin/sh
echo "Please download gradle wrapper from https://gradle.org/releases/"
echo "Or run: gradle wrapper --gradle-version 8.11.1"
exit 1
"#;
        std::fs::write(project_dir.join("gradlew"), gradlew_fallback)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                project_dir.join("gradlew"),
                std::fs::Permissions::from_mode(0o755),
            )?;
        }
    }

    info!("📦 Created Gradle wrapper configuration");
    Ok(())
}
