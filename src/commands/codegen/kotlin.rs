use crate::commands::codegen::traits::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use tracing::{debug, info, warn};

pub struct KotlinGenerator;

impl KotlinGenerator {
    /// Find the framework-codegen-kotlin plugin
    fn find_kotlin_plugin(&self) -> Result<PathBuf> {
        // First try the environment variable
        if let Ok(plugin_path) = std::env::var("ACTR_KOTLIN_PLUGIN_PATH") {
            let path = PathBuf::from(&plugin_path);
            if path.exists() {
                debug!("Using Kotlin plugin from env: {:?}", path);
                return Ok(path);
            }
        }

        // Try common locations
        let possible_paths = [
            // Development location
            PathBuf::from(
                "/Users/mafeng/Desktop/dev/framework-codegen-kotlin/protoc-gen-actrframework-kotlin",
            ),
            // Relative to current directory
            PathBuf::from("../framework-codegen-kotlin/protoc-gen-actrframework-kotlin"),
            // In PATH
            PathBuf::from("protoc-gen-actrframework-kotlin"),
        ];

        for path in &possible_paths {
            if path.exists() {
                debug!("Found Kotlin plugin at: {:?}", path);
                return Ok(path.clone());
            }
        }

        // Try `which` command
        let output = StdCommand::new("which")
            .arg("protoc-gen-actrframework-kotlin")
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        Err(ActrCliError::config_error(
            "Could not find protoc-gen-actrframework-kotlin plugin.\n\
             Please set ACTR_KOTLIN_PLUGIN_PATH environment variable or ensure the plugin is in PATH.",
        ))
    }

    /// Get Kotlin package name from parameters or infer from proto
    fn get_kotlin_package(&self, context: &GenContext) -> String {
        // Use kotlin_package from context if provided, otherwise use default
        context
            .kotlin_package
            .clone()
            .unwrap_or_else(|| "com.example.generated".to_string())
    }
}

#[async_trait]
impl LanguageGenerator for KotlinGenerator {
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🔧 Generating Kotlin Actor infrastructure code...");

        // Find the Kotlin plugin
        let plugin_path = self.find_kotlin_plugin()?;
        info!("✅ Using Kotlin plugin: {:?}", plugin_path);

        let kotlin_package = self.get_kotlin_package(context);
        let mut generated_files = Vec::new();

        for proto_file in &context.proto_files {
            debug!("Processing proto file: {:?}", proto_file);

            // Get the proto directory for include path
            let proto_dir = proto_file
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));

            // Use protoc with the Kotlin plugin
            let mut cmd = StdCommand::new("protoc");
            cmd.arg(format!("--proto_path={}", proto_dir.display()))
                .arg(format!(
                    "--plugin=protoc-gen-actrframework-kotlin={}",
                    plugin_path.display()
                ))
                .arg(format!(
                    "--actrframework-kotlin_opt=kotlin_package={}",
                    kotlin_package
                ))
                .arg(format!(
                    "--actrframework-kotlin_out={}",
                    context.output.display()
                ))
                .arg(proto_file);

            debug!("Executing protoc: {:?}", cmd);
            let output = cmd.output().map_err(|e| {
                ActrCliError::command_error(format!("Failed to execute protoc: {e}"))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActrCliError::command_error(format!(
                    "protoc (actrframework-kotlin) execution failed: {stderr}"
                )));
            }

            // Track generated files
            let service_name = proto_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            let generated_file = context.output.join(format!("{}_actor.kt", service_name));
            if generated_file.exists() {
                generated_files.push(generated_file);
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.is_empty() {
                debug!("protoc output: {}", stdout);
            }
        }

        info!(
            "✅ Generated {} Kotlin infrastructure files",
            generated_files.len()
        );
        Ok(generated_files)
    }

    async fn generate_scaffold(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        if context.no_scaffold {
            return Ok(vec![]);
        }

        info!("📝 Generating Kotlin user code scaffold...");

        let mut generated_files = Vec::new();
        let kotlin_package = self.get_kotlin_package(context);

        for proto_file in &context.proto_files {
            let service_name = proto_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            let pascal_name = to_pascal_case(service_name);

            // Generate Handler implementation (My{ServiceName}.kt)
            let handler_file = context
                .output
                .parent()
                .unwrap_or(&context.output)
                .join(format!("My{}.kt", pascal_name));

            if !handler_file.exists() || context.overwrite_user_code {
                let handler_content =
                    generate_kotlin_handler_scaffold(service_name, &kotlin_package);
                std::fs::write(&handler_file, handler_content).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to write handler file: {e}"))
                })?;
                info!("📄 Generated handler scaffold: {:?}", handler_file);
                generated_files.push(handler_file);
            } else {
                info!("⏭️  Skipping existing handler file: {:?}", handler_file);
            }

            // Generate Workload class ({ServiceName}Workload.kt)
            let workload_file = context
                .output
                .parent()
                .unwrap_or(&context.output)
                .join(format!("{}Workload.kt", pascal_name));

            if !workload_file.exists() || context.overwrite_user_code {
                let workload_content =
                    generate_kotlin_workload_scaffold(service_name, &kotlin_package);
                std::fs::write(&workload_file, workload_content).map_err(|e| {
                    ActrCliError::config_error(format!("Failed to write workload file: {e}"))
                })?;
                info!("📄 Generated workload scaffold: {:?}", workload_file);
                generated_files.push(workload_file);
            } else {
                info!("⏭️  Skipping existing workload file: {:?}", workload_file);
            }
        }

        Ok(generated_files)
    }

    async fn format_code(&self, _context: &GenContext, files: &[PathBuf]) -> Result<()> {
        info!("🎨 Formatting Kotlin code...");

        // Try to use ktlint if available
        let ktlint_check = StdCommand::new("which").arg("ktlint").output();

        if let Ok(output) = ktlint_check {
            if output.status.success() {
                for file in files {
                    let mut cmd = StdCommand::new("ktlint");
                    cmd.arg("-F").arg(file);

                    let output = cmd.output();
                    if let Err(e) = output {
                        warn!("ktlint formatting failed for {:?}: {}", file, e);
                    }
                }
                info!("✅ Kotlin code formatted with ktlint");
            } else {
                info!("💡 ktlint not found, skipping formatting");
            }
        }

        Ok(())
    }

    async fn validate_code(&self, context: &GenContext) -> Result<()> {
        info!("🔍 Validating Kotlin code...");

        // Check if generated files exist
        let generated_dir = &context.output;
        if !generated_dir.exists() {
            return Err(ActrCliError::config_error(
                "Generated output directory does not exist",
            ));
        }

        let kt_files: Vec<_> = std::fs::read_dir(generated_dir)
            .map_err(|e| {
                ActrCliError::config_error(format!("Failed to read output directory: {e}"))
            })?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "kt").unwrap_or(false))
            .collect();

        if kt_files.is_empty() {
            warn!("No Kotlin files found in output directory");
        } else {
            info!("✅ Found {} Kotlin files", kt_files.len());
        }

        // Note: Full compilation validation would require a Kotlin compiler setup
        // For now, we just check that files were generated
        info!("💡 For full validation, compile the Kotlin project with gradle/kotlinc");

        Ok(())
    }

    fn print_next_steps(&self, context: &GenContext) {
        println!("\n🎉 Kotlin code generation completed!");
        println!("\n📋 Next steps:");
        println!("1. 📖 View generated code: {:?}", context.output);
        println!("2. � Copy generated files to your Android/Kotlin project");
        println!("3. 📦 Ensure protobuf gradle plugin is configured for message classes");
        println!("4. ✏️  Implement the Handler interface in your service class");
        println!("5. 🏗️  Build project: ./gradlew build");
        println!("6. 🧪 Run tests: ./gradlew test");
        println!(
            "\n💡 Tip: The generated Handler interface and Dispatcher work with protobuf-generated message classes"
        );
    }
}

/// Convert a string to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Generate Kotlin Handler implementation scaffold
fn generate_kotlin_handler_scaffold(service_name: &str, kotlin_package: &str) -> String {
    let pascal_name = to_pascal_case(service_name);
    // Derive proto package from service name (e.g., "echo" for EchoService)
    let proto_package = service_name.to_lowercase();
    // Derive outer class name (e.g., "Echo" from "echo.proto")
    let outer_class = to_pascal_case(service_name);

    // Base package is kotlin_package without trailing ".generated" if present
    let base_package = kotlin_package
        .strip_suffix(".generated")
        .unwrap_or(kotlin_package);

    format!(
        r#"/**
 * {pascal_name} User Business Logic Implementation
 *
 * This file is a scaffold generated by the actr gen command.
 * Implement your specific business logic here.
 */
package {base_package}

import android.util.Log
import {kotlin_package}.{pascal_name}ServiceHandler
import io.actor_rtc.actr.ContextBridge
import {proto_package}.{outer_class}.*

/**
 * Implementation of {pascal_name}ServiceHandler
 *
 * This class handles incoming RPC requests for the {pascal_name} service.
 */
class My{pascal_name}Service : {pascal_name}ServiceHandler {{

    companion object {{
        private const val TAG = "My{pascal_name}Service"
    }}

    /**
     * Handle Echo RPC request
     * 
     * @param request The incoming EchoRequest
     * @param ctx Context for making RPC calls to other services
     * @return EchoResponse with the echoed message
     */
    override suspend fun echo(request: {pascal_name}Request, ctx: ContextBridge): {pascal_name}Response {{
        val message = request.message
        Log.i(TAG, "📥 Received echo request: $message")
        
        // Create response with "Echo: " prefix
        val response = {pascal_name}Response.newBuilder()
            .setReply("Echo: $message")
            .setTimestamp(System.currentTimeMillis().toULong().toLong())
            .build()
        
        Log.i(TAG, "📤 Sending response: ${{response.reply}}")
        return response
    }}
}}
"#
    )
}

/// Generate Kotlin Workload scaffold
fn generate_kotlin_workload_scaffold(service_name: &str, kotlin_package: &str) -> String {
    let pascal_name = to_pascal_case(service_name);

    // Base package is kotlin_package without trailing ".generated" if present
    let base_package = kotlin_package
        .strip_suffix(".generated")
        .unwrap_or(kotlin_package);

    format!(
        r#"/**
 * {pascal_name}Service Workload Implementation
 *
 * This Workload uses the generated Dispatcher for message routing,
 * delegating business logic to the {pascal_name}ServiceHandler implementation.
 */
package {base_package}

import android.util.Log
import {kotlin_package}.{pascal_name}ServiceDispatcher
import {kotlin_package}.{pascal_name}ServiceHandler
import io.actor_rtc.actr.ActrId
import io.actor_rtc.actr.ActrType
import io.actor_rtc.actr.ContextBridge
import io.actor_rtc.actr.Realm
import io.actor_rtc.actr.RpcEnvelopeBridge
import io.actor_rtc.actr.WorkloadBridge

/**
 * Workload for {pascal_name}Service
 *
 * Usage:
 * ```kotlin
 * val handler = My{pascal_name}Service()
 * val workload = {pascal_name}ServiceWorkload(handler)
 * val system = createActrSystem(configPath)
 * val node = system.attach(workload)
 * val actrRef = node.start()
 * ```
 */
class {pascal_name}ServiceWorkload(
    private val handler: {pascal_name}ServiceHandler,
    private val realmId: UInt = 2281844430u
) : WorkloadBridge {{

    companion object {{
        private const val TAG = "{pascal_name}ServiceWorkload"
    }}

    private val selfId = ActrId(
        realm = Realm(realmId = realmId),
        serialNumber = System.currentTimeMillis().toULong(),
        type = ActrType(manufacturer = "acme", name = "{pascal_name}Service")
    )

    override suspend fun onStart(ctx: ContextBridge) {{
        Log.i(TAG, "{pascal_name}ServiceWorkload.onStart")
        // Initialize resources, discover remote services, etc.
    }}

    override suspend fun onStop(ctx: ContextBridge) {{
        Log.i(TAG, "{pascal_name}ServiceWorkload.onStop")
        // Cleanup resources
    }}

    /**
     * Dispatch RPC requests to the handler
     *
     * Uses the generated Dispatcher to route requests to the appropriate handler method
     */
    override suspend fun dispatch(ctx: ContextBridge, envelope: RpcEnvelopeBridge): ByteArray {{
        Log.i(TAG, "🔀 dispatch() called")
        Log.i(TAG, "   route_key: ${{envelope.routeKey}}")
        Log.i(TAG, "   request_id: ${{envelope.requestId}}")
        Log.i(TAG, "   payload size: ${{envelope.payload.size}} bytes")

        return {pascal_name}ServiceDispatcher.dispatch(handler, ctx, envelope)
    }}
}}
"#
    )
}
