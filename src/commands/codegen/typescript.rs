use crate::commands::codegen::{GenContext, LanguageGenerator};
use crate::error::{ActrCliError, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

// Required tools for TypeScript codegen
const PROTOC: &str = "protoc";
const PROTOC_GEN_TS_PROTO: &str = "protoc-gen-ts_proto";

pub struct TypescriptGenerator;

#[async_trait]
impl LanguageGenerator for TypescriptGenerator {
    async fn generate_infrastructure(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        info!("🚀 生成 TypeScript 代码...");

        // 确保必需的工具可用
        self.ensure_required_tools()?;

        // 创建输出目录
        std::fs::create_dir_all(&context.output).map_err(|e| {
            ActrCliError::command_error(format!("Failed to create output directory: {}", e))
        })?;

        let proto_root = if context.input_path.is_file() {
            context
                .input_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
        } else {
            context.input_path.as_path()
        };

        // Step 1: 使用 ts-proto 生成基本的 TypeScript 类型和编解码函数
        // 从当前工作目录（Actr.toml 所在目录）开始查找 node_modules
        let cwd = std::env::current_dir().unwrap_or_default();
        let ts_proto_path = self.find_ts_proto_plugin_from(&cwd)?;

        info!("使用 ts-proto 插件: {}", ts_proto_path.display());

        let mut cmd = Command::new(PROTOC);
        cmd.arg(format!("--proto_path={}", proto_root.display()))
            .arg(format!(
                "--plugin=protoc-gen-ts_proto={}",
                ts_proto_path.display()
            ))
            .arg(format!("--ts_proto_out={}", context.output.display()))
            // ts-proto options: 生成 encode/decode 方法，ESM 兼容
            .arg("--ts_proto_opt=esModuleInterop=true")
            .arg("--ts_proto_opt=outputEncodeMethods=true")
            .arg("--ts_proto_opt=outputJsonMethods=true")
            .arg("--ts_proto_opt=outputClientImpl=false") // 我们用自己的 ActorRef
            .arg("--ts_proto_opt=outputServices=false"); // 服务由 actr framework 生成

        for proto_file in &context.proto_files {
            cmd.arg(proto_file);
        }

        debug!("执行 protoc (ts-proto): {:?}", cmd);
        let output = cmd.output().map_err(|e| {
            ActrCliError::command_error(format!("Failed to execute protoc (ts-proto): {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ActrCliError::command_error(format!(
                "protoc (ts-proto) failed: {}",
                stderr
            )));
        }

        info!("✅ ts-proto 类型生成完成");

        // Step 2: 生成 ActorRef 包装类（使用自定义插件或模板）
        let actor_ref_files = self.generate_actor_refs(context)?;

        // Step 3: 生成配置文件
        let config_file = self.generate_config_file(context)?;

        // Step 4: 生成 index.ts
        let index_file = self.generate_index_file(context)?;

        // 收集所有生成的文件
        let mut generated_files = vec![config_file, index_file];
        generated_files.extend(actor_ref_files);

        // 收集 ts-proto 生成的文件
        if let Ok(entries) = std::fs::read_dir(&context.output) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("ts") {
                    if !generated_files.contains(&path) {
                        generated_files.push(path);
                    }
                }
            }
        }

        info!("✅ TypeScript 代码生成完成");
        Ok(generated_files)
    }

    async fn generate_scaffold(&self, _context: &GenContext) -> Result<Vec<PathBuf>> {
        // TypeScript 不需要生成 scaffold
        Ok(vec![])
    }

    async fn format_code(&self, context: &GenContext, files: &[PathBuf]) -> Result<()> {
        if context.no_format {
            return Ok(());
        }

        info!("🎨 格式化 TypeScript 代码...");

        // 尝试使用 prettier
        for file in files {
            if file.extension().and_then(|s| s.to_str()) == Some("ts") {
                let output = Command::new("npx")
                    .args(["prettier", "--write", file.to_str().unwrap()])
                    .output();

                match output {
                    Ok(output) if output.status.success() => {
                        debug!("✅ 格式化: {}", file.display());
                    }
                    _ => {
                        warn!("⚠️  prettier 不可用，跳过格式化");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn validate_code(&self, _context: &GenContext) -> Result<()> {
        // TypeScript 验证可以通过 tsc 完成，但这里暂时跳过
        Ok(())
    }

    fn print_next_steps(&self, _context: &GenContext) {
        println!("\n📝 下一步:");
        println!("  1. 在你的 TypeScript 项目中导入生成的代码:");
        println!("     import {{ actrConfig }} from './generated/actr-config';");
        println!("     import {{ EchoServiceActorRef, EchoRequest }} from './generated/index';",);
        println!("  2. 创建 ActorClient 并使用生成的 ActorRef:");
        println!("     const client = await createActorClient(actrConfig);");
        println!("     const ref = new EchoServiceActorRef(client);");
        println!("     const response = await ref.echo({{ message: 'Hello' }});");
    }
}

impl TypescriptGenerator {
    /// 确保必需的工具可用
    fn ensure_required_tools(&self) -> Result<()> {
        // 检查 protoc
        let output = Command::new(PROTOC).arg("--version").output();
        if output.is_err() || !output.unwrap().status.success() {
            return Err(ActrCliError::command_error(
                "protoc 未安装。请安装 Protocol Buffers 编译器: brew install protobuf",
            ));
        }

        Ok(())
    }

    /// 查找 ts-proto 插件路径，从指定的基础路径开始查找
    fn find_ts_proto_plugin_from(&self, base_path: &Path) -> Result<PathBuf> {
        // 首先检查 PATH 中
        if let Ok(output) = Command::new("which").arg(PROTOC_GEN_TS_PROTO).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        // 检查 npm global
        if let Ok(output) = Command::new("npm").args(["bin", "-g"]).output() {
            if output.status.success() {
                let npm_bin = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let plugin_path = PathBuf::from(&npm_bin).join(PROTOC_GEN_TS_PROTO);
                if plugin_path.exists() {
                    return Ok(plugin_path);
                }
            }
        }

        // 从基础路径向上查找 node_modules
        let mut current = base_path.to_path_buf();
        loop {
            let local_path = current.join("node_modules/.bin").join(PROTOC_GEN_TS_PROTO);
            if local_path.exists() {
                return Ok(local_path);
            }

            if !current.pop() {
                break;
            }
        }

        // 检查当前工作目录
        let cwd_path = PathBuf::from("node_modules/.bin").join(PROTOC_GEN_TS_PROTO);
        if cwd_path.exists() {
            return Ok(cwd_path.canonicalize().unwrap_or(cwd_path));
        }

        Err(ActrCliError::command_error(
            "找不到 protoc-gen-ts_proto。请运行: npm install ts-proto 或 pnpm add ts-proto",
        ))
    }

    /// 查找 ts-proto 插件路径
    fn find_ts_proto_plugin(&self) -> Result<PathBuf> {
        self.find_ts_proto_plugin_from(&std::env::current_dir().unwrap_or_default())
    }

    /// 生成 ActorRef 包装类
    fn generate_actor_refs(&self, context: &GenContext) -> Result<Vec<PathBuf>> {
        let mut generated_files = Vec::new();

        let proto_root = if context.input_path.is_file() {
            context
                .input_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
        } else {
            context.input_path.as_path()
        };

        // 解析 proto 文件，提取 service 信息
        for proto_file in &context.proto_files {
            let content = std::fs::read_to_string(proto_file).map_err(|e| {
                ActrCliError::command_error(format!("Failed to read proto file: {}", e))
            })?;

            let services = self.parse_services(&content)?;
            let package_name = self.extract_package_name(&content);

            // 计算相对于 proto_root 的路径，用于找到 ts-proto 生成的文件
            let relative_proto_path = proto_file
                .strip_prefix(proto_root)
                .unwrap_or(proto_file.as_path());

            // ts-proto 生成的文件与 proto 文件路径相同，但扩展名是 .ts
            let ts_proto_relative = relative_proto_path.with_extension(""); // 去掉 .proto

            for service in services {
                let actor_ref_code = self.generate_actor_ref_code(
                    &service,
                    &package_name,
                    &ts_proto_relative,
                    context,
                )?;
                let file_name = format!("{}.actorref.ts", to_kebab_case(&service.name));
                let file_path = context.output.join(&file_name);

                std::fs::write(&file_path, actor_ref_code).map_err(|e| {
                    ActrCliError::command_error(format!("Failed to write ActorRef file: {}", e))
                })?;

                info!("📄 生成 ActorRef: {}", file_path.display());
                generated_files.push(file_path);
            }
        }

        Ok(generated_files)
    }

    /// 从 proto 内容中解析 service 定义
    fn parse_services(&self, content: &str) -> Result<Vec<ServiceDef>> {
        let mut services = Vec::new();
        let service_re = regex::Regex::new(r"service\s+(\w+)\s*\{([^}]*)\}").unwrap();
        let rpc_re =
            regex::Regex::new(r"rpc\s+(\w+)\s*\(\s*(\w+)\s*\)\s*returns\s*\(\s*(\w+)\s*\)")
                .unwrap();

        for cap in service_re.captures_iter(content) {
            let service_name = cap[1].to_string();
            let service_body = &cap[2];

            let mut methods = Vec::new();
            for rpc_cap in rpc_re.captures_iter(service_body) {
                methods.push(MethodDef {
                    name: rpc_cap[1].to_string(),
                    input_type: rpc_cap[2].to_string(),
                    output_type: rpc_cap[3].to_string(),
                });
            }

            services.push(ServiceDef {
                name: service_name,
                methods,
            });
        }

        Ok(services)
    }

    /// 提取 package 名称
    fn extract_package_name(&self, content: &str) -> Option<String> {
        let package_re = regex::Regex::new(r"package\s+(\w+(?:\.\w+)*)\s*;").unwrap();
        package_re.captures(content).map(|cap| cap[1].to_string())
    }

    /// 生成 ActorRef 代码
    fn generate_actor_ref_code(
        &self,
        service: &ServiceDef,
        _package_name: &Option<String>,
        ts_proto_relative_path: &Path,
        context: &GenContext,
    ) -> Result<String> {
        let service_name = &service.name;
        let actor_ref_name = format!("{}ActorRef", service_name);

        // 构建导入语句 - 从 ts-proto 生成的文件导入
        // ts-proto 生成的文件路径与 proto 文件路径相同
        let ts_proto_file = format!("./{}", ts_proto_relative_path.display());

        // 收集所有需要的类型
        let mut imports: Vec<String> = Vec::new();
        for method in &service.methods {
            if !imports.contains(&method.input_type) {
                imports.push(method.input_type.clone());
            }
            if !imports.contains(&method.output_type) {
                imports.push(method.output_type.clone());
            }
        }

        // 生成方法
        let methods_code: Vec<String> = service
            .methods
            .iter()
            .map(|method| {
                let method_name = to_camel_case(&method.name);
                let input_type = &method.input_type;
                let output_type = &method.output_type;

                format!(
                    r#"  /**
   * 调用 {} RPC 方法
   */
  async {}(request: {}): Promise<{}> {{
    const encoded = {}.encode(request).finish();
    const responseData = await this.client.callRaw('{}', encoded);
    return {}.decode(responseData);
  }}"#,
                    method.name,
                    method_name,
                    input_type,
                    output_type,
                    input_type,
                    method.name,
                    output_type,
                )
            })
            .collect();

        let actr_type = &context.config.package.actr_type;

        let code = format!(
            r#"/**
 * 自动生成的 ActorRef
 * 服务: {}
 *
 * ⚠️  请勿手动编辑此文件
 */

import type {{ ActorClient }} from '@actr/web';
import {{ {} }} from '{}';

/**
 * ActrType 定义
 */
export const {}ActrType = {{
  manufacturer: '{}',
  name: '{}',
}};

/**
 * {} 的 ActorRef 包装
 * 提供类型安全的 RPC 调用方法
 */
export class {} {{
  private client: ActorClient;

  constructor(client: ActorClient) {{
    this.client = client;
  }}

{}
}}
"#,
            service_name,
            imports.join(", "),
            ts_proto_file,
            service_name,
            actr_type.manufacturer,
            actr_type.name,
            service_name,
            actor_ref_name,
            methods_code.join("\n\n"),
        );

        Ok(code)
    }

    /// 从 Actr.toml 生成 TypeScript 配置文件
    fn generate_config_file(&self, context: &GenContext) -> Result<PathBuf> {
        let config = &context.config;

        // 提取配置值
        let signaling_url = config.signaling_url.as_str();
        let realm_id = config.realm.realm_id;

        // 构建 iceServers
        let mut ice_servers = Vec::new();

        for ice_server in &config.webrtc.ice_servers {
            for url in &ice_server.urls {
                if let (Some(username), Some(credential)) =
                    (&ice_server.username, &ice_server.credential)
                {
                    ice_servers.push(format!(
                        "    {{ urls: '{}', username: '{}', credential: '{}' }}",
                        url, username, credential
                    ));
                } else {
                    ice_servers.push(format!("    {{ urls: '{}' }}", url));
                }
            }
        }

        let ice_servers_str = if ice_servers.is_empty() {
            "    { urls: 'stun:stun.l.google.com:19302' }".to_string()
        } else {
            ice_servers.join(",\n")
        };

        let content = format!(
            r#"/**
 * 自动生成的 Actr 配置
 * 来源: Actr.toml
 *
 * ⚠️  请勿手动编辑此文件
 */

import type {{ ActorClientConfig }} from '@actr/web';

/**
 * Actor 客户端配置
 */
export const actrConfig: ActorClientConfig = {{
  signalingUrl: '{}',
  realm: '{}',
  iceServers: [
{}
  ],
  serviceWorkerPath: '/actor.sw.js',
  autoReconnect: true,
  debug: false,
}};

/**
 * 包名称
 */
export const packageName = '{}';

/**
 * ActrType
 */
export const actrType = {{
  manufacturer: '{}',
  name: '{}',
}};
"#,
            signaling_url,
            realm_id,
            ice_servers_str,
            config.package.name,
            config.package.actr_type.manufacturer,
            config.package.actr_type.name,
        );

        let file_path = context.output.join("actr-config.ts");
        std::fs::write(&file_path, content).map_err(|e| {
            ActrCliError::command_error(format!("Failed to write config file: {}", e))
        })?;

        info!("📄 生成配置文件: {}", file_path.display());

        Ok(file_path)
    }

    /// 生成 index.ts 汇总文件
    fn generate_index_file(&self, context: &GenContext) -> Result<PathBuf> {
        let mut exports = Vec::new();

        // 导出配置
        exports.push("export * from './actr-config';".to_string());

        // 递归收集所有 .ts 文件
        fn collect_ts_files(dir: &Path, base: &Path, exports: &mut Vec<String>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // 递归扫描子目录
                        collect_ts_files(&path, base, exports);
                    } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.ends_with(".ts") && name != "index.ts" && name != "actr-config.ts" {
                            // 计算相对于 base 的路径
                            if let Ok(relative) = path.strip_prefix(base) {
                                let module_path = relative.with_extension("");
                                let module_str = module_path.display().to_string();
                                exports.push(format!("export * from './{}';", module_str));
                            }
                        }
                    }
                }
            }
        }

        collect_ts_files(&context.output, &context.output, &mut exports);

        let content = format!(
            r#"/**
 * 自动生成的 Actr 代码入口
 *
 * ⚠️  请勿手动编辑此文件
 */

{}
"#,
            exports.join("\n")
        );

        let file_path = context.output.join("index.ts");
        std::fs::write(&file_path, content).map_err(|e| {
            ActrCliError::command_error(format!("Failed to write index file: {}", e))
        })?;

        info!("📄 生成入口文件: {}", file_path.display());

        Ok(file_path)
    }
}

/// Service 定义
struct ServiceDef {
    name: String,
    methods: Vec<MethodDef>,
}

/// Method 定义
struct MethodDef {
    name: String,
    input_type: String,
    output_type: String,
}

/// 转换为 camelCase
fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for (i, c) in s.chars().enumerate() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else if i == 0 {
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

/// 转换为 kebab-case
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();

    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}
