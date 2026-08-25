//! SSL 证书申请：Let's Encrypt 手动 DNS-01 流程。
//!
//! 应用负责创建 ACME 订单并生成每个域名的 `_acme-challenge` TXT 记录，用户到
//! DNS 控制台手动添加；本模块提供多公共解析器的解析测试，以及验证/签发/导出。

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use certon::acme_client::{
    key_authorization, AcmeClient, AcmeOrder, LETS_ENCRYPT_PRODUCTION, LETS_ENCRYPT_STAGING,
};
use certon::crypto::{
    decode_private_key_pem, encode_private_key_pem, generate_csr, generate_private_key, KeyType,
};
use certon::dns_util::{challenge_record_name, challenge_record_value, is_valid_domain};
use hickory_resolver::config::{NameServerConfig, ResolverConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::TokioResolver;
use serde::{Deserialize, Serialize};
use tauri::Manager;

/// 测试解析使用的公共递归 DNS（含国内与国外，便于覆盖不同传播路径）。
const RESOLVERS: &[&str] = &[
    "223.5.5.5",
    "119.29.29.29",
    "114.114.114.114",
    "8.8.8.8",
    "8.8.4.4",
    "1.1.1.1",
    "1.0.0.1",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainRow {
    domain: String,
    wildcard: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcmeStartRequest {
    ca: String,
    email: String,
    domains: Vec<DomainRow>,
    key_type: String,
    export_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeRecord {
    pub id: String,
    pub record_name: String,
    pub record_value: String,
    pub domain: String,
    pub wildcard: bool,
    pub display: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcmeStartResponse {
    pub order_id: String,
    pub ca_label: String,
    pub expires: Option<String>,
    pub san: Vec<String>,
    pub records: Vec<ChallengeRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsCheckResult {
    pub record_id: String,
    pub domain: String,
    pub resolver: String,
    pub found: bool,
    pub matched: bool,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsCheckSummary {
    pub record_id: String,
    pub domain: String,
    pub status: String,
    pub matched_count: usize,
    pub resolver_count: usize,
    pub details: Vec<DnsCheckResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedCertInfo {
    pub order_id: String,
    pub ca: String,
    pub san: Vec<String>,
    pub export_dir: String,
    pub cert_path: String,
    pub key_path: String,
    pub combined_path: String,
    pub issued_at: u64,
}

#[derive(Clone)]
struct PendingAuthz {
    authz_url: String,
    challenge_url: String,
}

#[derive(Clone)]
struct PendingOrder {
    ca_label: String,
    directory_url: String,
    account_key_pem: String,
    account_url: String,
    order: AcmeOrder,
    order_url: String,
    records: Vec<ChallengeRecord>,
    authz: HashMap<String, PendingAuthz>,
    san: Vec<String>,
    key_type: KeyType,
    export_dir: String,
}

#[derive(Clone)]
struct AccountInfo {
    key_pem: String,
    url: String,
}

/// Tauri 托管状态：pending 订单保存在内存（重启需重新生成挑战记录），
/// ACME 账户按 CA 缓存，账户私钥持久化到应用数据目录。
pub struct AcmeState {
    pending: Mutex<HashMap<String, PendingOrder>>,
    accounts: Mutex<HashMap<String, AccountInfo>>,
}

impl Default for AcmeState {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            accounts: Mutex::new(HashMap::new()),
        }
    }
}

#[tauri::command]
pub async fn acme_start_order(
    app: tauri::AppHandle,
    state: tauri::State<'_, AcmeState>,
    request: AcmeStartRequest,
) -> Result<AcmeStartResponse, String> {
    let san = build_san(&request.domains)?;
    let (ca_key, ca_label, directory_url) = match request.ca.as_str() {
        "letsencrypt_prod" => ("letsencrypt-prod", "Let's Encrypt 正式", LETS_ENCRYPT_PRODUCTION),
        "letsencrypt_staging" => (
            "letsencrypt-staging",
            "Let's Encrypt 测试",
            LETS_ENCRYPT_STAGING,
        ),
        other => return Err(format!("不支持的证书颁发机构: {other}")),
    };

    let account = ensure_account(&app, &state, ca_key, directory_url, &request.email).await?;
    let account_key = decode_private_key_pem(&account.key_pem)
        .map_err(|e| format!("ACME 账户私钥解析失败: {e}"))?;
    let client = AcmeClient::new(directory_url)
        .await
        .map_err(|e| format!("连接 ACME 服务失败: {e}"))?;

    let (order, order_url) = client
        .new_order(&account_key, &account.url, &san)
        .await
        .map_err(|e| format!("创建 ACME 订单失败: {e}"))?;

    let mut records = Vec::new();
    let mut authz = HashMap::new();
    for (idx, authz_url) in order.authorizations.iter().enumerate() {
        let a = client
            .get_authorization(&account_key, &account.url, authz_url)
            .await
            .map_err(|e| format!("读取域名授权失败: {e}"))?;
        // 最近已通过验证的授权（LE 最长复用 30 天）不需要再添加 TXT 记录。
        if a.status == "valid" {
            continue;
        }
        let challenge = a
            .challenges
            .iter()
            .find(|c| c.challenge_type == "dns-01")
            .ok_or_else(|| format!("{} 没有可用的 dns-01 挑战", a.identifier.value))?;
        let key_auth = key_authorization(&challenge.token, &account_key)
            .map_err(|e| format!("计算挑战值失败: {e}"))?;

        let base = a.identifier.value.trim_end_matches('.').to_string();
        let wildcard = a.wildcard.unwrap_or(false);
        let display = if wildcard {
            format!("*.{base}")
        } else {
            base.clone()
        };
        let name = challenge_record_name(&base);
        let record_id = format!("rec-{idx}");

        records.push(ChallengeRecord {
            id: record_id.clone(),
            record_name: name.trim_end_matches('.').to_string(),
            record_value: challenge_record_value(&key_auth),
            domain: base,
            wildcard,
            display,
        });
        authz.insert(
            record_id,
            PendingAuthz {
                authz_url: authz_url.clone(),
                challenge_url: challenge.url.clone(),
            },
        );
    }

    let key_type = parse_key_type(&request.key_type)?;
    let order_id = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let response = AcmeStartResponse {
        order_id: order_id.clone(),
        ca_label: ca_label.to_string(),
        expires: order.expires.clone(),
        san: san.clone(),
        records: records.clone(),
    };
    let pending = PendingOrder {
        ca_label: ca_label.to_string(),
        directory_url: directory_url.to_string(),
        account_key_pem: account.key_pem,
        account_url: account.url,
        order,
        order_url,
        records,
        authz,
        san,
        key_type,
        export_dir: request.export_dir,
    };
    {
        let mut map = state.pending.lock().map_err(|e| e.to_string())?;
        if map.len() >= 10 {
            if let Some(oldest) = map.keys().next().cloned() {
                map.remove(&oldest);
            }
        }
        map.insert(order_id.clone(), pending);
    }
    Ok(response)
}

#[tauri::command]
pub async fn acme_check_dns(
    state: tauri::State<'_, AcmeState>,
    order_id: String,
) -> Result<Vec<DnsCheckSummary>, String> {
    let records = state
        .pending
        .lock()
        .map_err(|e| e.to_string())?
        .get(&order_id)
        .map(|p| p.records.clone())
        .ok_or_else(|| "订单不存在或已过期，请重新生成挑战记录".to_string())?;

    let mut summaries = Vec::new();
    for record in records {
        // 同一域名的多个公共解析器并行查询，避免逐个等待超时。
        let mut handles = Vec::new();
        for resolver in RESOLVERS {
            let record_id = record.id.clone();
            let domain = record.domain.clone();
            let record_name = record.record_name.clone();
            let record_value = record.record_value.clone();
            handles.push(tokio::spawn(async move {
                query_txt(resolver, &record_id, &domain, &record_name, &record_value).await
            }));
        }
        let mut details = Vec::new();
        for handle in handles {
            if let Ok(detail) = handle.await {
                details.push(detail);
            }
        }
        let matched_count = details.iter().filter(|d| d.matched).count();
        let status = if matched_count > 0 {
            "matched"
        } else if details.iter().any(|d| d.found) {
            "mismatch"
        } else {
            "not_found"
        };
        summaries.push(DnsCheckSummary {
            record_id: record.id,
            domain: record.domain,
            status: status.to_string(),
            matched_count,
            resolver_count: details.len(),
            details,
        });
    }
    Ok(summaries)
}

#[tauri::command]
pub async fn acme_finalize_order(
    app: tauri::AppHandle,
    state: tauri::State<'_, AcmeState>,
    order_id: String,
) -> Result<IssuedCertInfo, String> {
    let pending = {
        let map = state.pending.lock().map_err(|e| e.to_string())?;
        map.get(&order_id)
            .cloned()
            .ok_or_else(|| "订单不存在或已过期，请重新生成挑战记录".to_string())?
    };

    let account_key = decode_private_key_pem(&pending.account_key_pem)
        .map_err(|e| format!("ACME 账户私钥解析失败: {e}"))?;
    let client = AcmeClient::new(&pending.directory_url)
        .await
        .map_err(|e| format!("连接 ACME 服务失败: {e}"))?;

    for record in &pending.records {
        let authz = pending
            .authz
            .get(&record.id)
            .ok_or_else(|| format!("订单缺少 {} 的授权信息", record.display))?;
        client
            .accept_challenge(&account_key, &pending.account_url, &authz.challenge_url)
            .await
            .map_err(|e| format!("提交 {} 验证失败: {e}", record.display))?;
        client
            .poll_authorization(
                &account_key,
                &pending.account_url,
                &authz.authz_url,
                Duration::from_secs(300),
            )
            .await
            .map_err(|e| format!("等待 {} 验证失败: {e}", record.display))?;
    }

    let cert_key = generate_private_key(pending.key_type)
        .map_err(|e| format!("生成证书私钥失败: {e}"))?;
    let csr = generate_csr(&cert_key, &pending.san, false)
        .map_err(|e| format!("生成 CSR 失败: {e}"))?;
    let _ = client
        .finalize_order(
            &account_key,
            &pending.account_url,
            &pending.order.finalize,
            &csr,
        )
        .await
        .map_err(|e| format!("提交证书请求失败: {e}"))?;
    let order = client
        .poll_order(
            &account_key,
            &pending.account_url,
            &pending.order_url,
            Duration::from_secs(300),
        )
        .await
        .map_err(|e| format!("等待证书签发失败: {e}"))?;
    let cert_url = order
        .certificate
        .as_deref()
        .ok_or_else(|| "CA 未返回证书下载地址".to_string())?;
    let chain_pem = client
        .download_certificate(&account_key, &pending.account_url, cert_url)
        .await
        .map_err(|e| format!("下载证书失败: {e}"))?;
    let key_pem = encode_private_key_pem(&cert_key)
        .map_err(|e| format!("编码证书私钥失败: {e}"))?;

    let export_dir = expand_home(&pending.export_dir);
    fs::create_dir_all(&export_dir).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let cert_path = export_dir.join("fullchain.pem");
    let key_path = export_dir.join("privkey.pem");
    let combined_path = export_dir.join("combined.pem");
    fs::write(&cert_path, chain_pem.as_bytes())
        .map_err(|e| format!("写入证书文件失败: {e}"))?;
    write_private(&key_path, &key_pem)?;
    let mut combined = chain_pem;
    combined.push('\n');
    combined.push_str(&key_pem);
    fs::write(&combined_path, combined.as_bytes())
        .map_err(|e| format!("写入合并文件失败: {e}"))?;

    let info = IssuedCertInfo {
        order_id: order_id.clone(),
        ca: pending.ca_label.clone(),
        san: pending.san.clone(),
        export_dir: export_dir.to_string_lossy().into_owned(),
        cert_path: cert_path.to_string_lossy().into_owned(),
        key_path: key_path.to_string_lossy().into_owned(),
        combined_path: combined_path.to_string_lossy().into_owned(),
        issued_at: unix_secs(),
    };
    append_cert_meta(&app, &info)?;
    state
        .pending
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&order_id);
    Ok(info)
}

#[tauri::command]
pub async fn acme_list_certificates(
    app: tauri::AppHandle,
) -> Result<Vec<IssuedCertInfo>, String> {
    load_cert_meta(&app)
}

async fn ensure_account(
    app: &tauri::AppHandle,
    state: &AcmeState,
    ca_key: &str,
    directory_url: &str,
    email: &str,
) -> Result<AccountInfo, String> {
    if let Some(account) = state
        .accounts
        .lock()
        .map_err(|e| e.to_string())?
        .get(ca_key)
        .cloned()
    {
        return Ok(account);
    }

    let dir = ssl_data_dir(app)?.join("accounts").join(ca_key);
    fs::create_dir_all(&dir).map_err(|e| format!("创建账户目录失败: {e}"))?;
    let key_path = dir.join("account-key.pem");
    let key_pem = if key_path.exists() {
        fs::read_to_string(&key_path).map_err(|e| format!("读取账户私钥失败: {e}"))?
    } else {
        let key = generate_private_key(KeyType::EcdsaP256)
            .map_err(|e| format!("生成账户私钥失败: {e}"))?;
        let pem = encode_private_key_pem(&key).map_err(|e| format!("编码账户私钥失败: {e}"))?;
        write_private(&key_path, &pem)?;
        pem
    };
    let account_key = decode_private_key_pem(&key_pem)
        .map_err(|e| format!("ACME 账户私钥解析失败: {e}"))?;
    let client = AcmeClient::new(directory_url)
        .await
        .map_err(|e| format!("连接 ACME 服务失败: {e}"))?;

    let url = match client
        .find_account(&account_key)
        .await
        .map_err(|e| format!("查询 ACME 账户失败: {e}"))?
    {
        Some((_, url)) => url,
        None => {
            let contact: Vec<String> = if email.trim().is_empty() {
                Vec::new()
            } else {
                vec![format!("mailto:{}", email.trim())]
            };
            let (_, url) = client
                .new_account(&account_key, &contact, true, None)
                .await
                .map_err(|e| format!("注册 ACME 账户失败: {e}"))?;
            url
        }
    };

    let account = AccountInfo {
        key_pem,
        url,
    };
    state
        .accounts
        .lock()
        .map_err(|e| e.to_string())?
        .insert(ca_key.to_string(), account.clone());
    Ok(account)
}

async fn query_txt(
    resolver_ip: &str,
    record_id: &str,
    domain: &str,
    record_name: &str,
    expected: &str,
) -> DnsCheckResult {
    let ip: IpAddr = match resolver_ip.parse() {
        Ok(ip) => ip,
        Err(_) => {
            return DnsCheckResult {
                record_id: record_id.to_string(),
                domain: domain.to_string(),
                resolver: resolver_ip.to_string(),
                found: false,
                matched: false,
                values: Vec::new(),
            };
        }
    };

    let config = ResolverConfig::from_parts(None, Vec::new(), vec![NameServerConfig::udp(ip)]);
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(4);
    opts.attempts = 1;
    opts.cache_size = 0;
    let resolver = match TokioResolver::builder_with_config(
        config,
        TokioRuntimeProvider::default(),
    )
    .with_options(opts)
    .build()
    {
        Ok(resolver) => resolver,
        Err(_) => {
            return DnsCheckResult {
                record_id: record_id.to_string(),
                domain: domain.to_string(),
                resolver: resolver_ip.to_string(),
                found: false,
                matched: false,
                values: Vec::new(),
            };
        }
    };

    let (found, values) = match resolver.txt_lookup(record_name).await {
        Ok(response) => {
            let values: Vec<String> = response
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    hickory_resolver::proto::rr::RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .collect();
            (true, values)
        }
        Err(_) => (false, Vec::new()),
    };
    let matched = values
        .iter()
        .any(|value| value.trim_matches('"') == expected);
    DnsCheckResult {
        record_id: record_id.to_string(),
        domain: domain.to_string(),
        resolver: resolver_ip.to_string(),
        found,
        matched,
        values,
    }
}

fn build_san(rows: &[DomainRow]) -> Result<Vec<String>, String> {
    let mut san: Vec<String> = Vec::new();
    for row in rows {
        let raw = row.domain.trim().trim_end_matches('.');
        if raw.is_empty() {
            continue;
        }
        let (base, wildcard) = match raw.strip_prefix("*.") {
            Some(rest) => (rest.to_string(), true),
            None => (raw.to_string(), row.wildcard),
        };
        let base = base.to_lowercase();
        if !is_valid_domain(&base) {
            return Err(format!("域名无效: {raw}"));
        }
        if !san.contains(&base) {
            san.push(base.clone());
        }
        if wildcard {
            let wildcard_domain = format!("*.{base}");
            if !san.contains(&wildcard_domain) {
                san.push(wildcard_domain);
            }
        }
    }
    if san.is_empty() {
        return Err("请至少填写一个域名".to_string());
    }
    if san.len() > 100 {
        return Err("单张证书最多包含 100 个域名".to_string());
    }
    Ok(san)
}

fn parse_key_type(value: &str) -> Result<KeyType, String> {
    match value {
        "ecdsa_p256" => Ok(KeyType::EcdsaP256),
        "rsa2048" => Ok(KeyType::Rsa2048),
        other => Err(format!("不支持的密钥类型: {other}")),
    }
}

fn ssl_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("ssl"))
        .map_err(|e| format!("无法定位应用数据目录: {e}"))
}

fn cert_meta_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ssl_data_dir(app)?.join("certs.json"))
}

fn load_cert_meta(app: &tauri::AppHandle) -> Result<Vec<IssuedCertInfo>, String> {
    let path = cert_meta_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("读取证书记录失败: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析证书记录失败: {e}"))
}

fn append_cert_meta(app: &tauri::AppHandle, info: &IssuedCertInfo) -> Result<(), String> {
    let mut list = load_cert_meta(app)?;
    list.retain(|cert| cert.order_id != info.order_id);
    list.insert(0, info.clone());
    let path = cert_meta_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建记录目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&list)
        .map_err(|e| format!("序列化证书记录失败: {e}"))?;
    fs::write(path, json).map_err(|e| format!("写入证书记录失败: {e}"))
}

fn write_private(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn expand_home(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        if !home.is_empty() {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_san_with_wildcard_and_dedupe() {
        let rows = vec![
            DomainRow {
                domain: "Example.com".into(),
                wildcard: true,
            },
            DomainRow {
                domain: "example.com".into(),
                wildcard: false,
            },
            DomainRow {
                domain: "*.api.example.com".into(),
                wildcard: false,
            },
        ];
        assert_eq!(
            build_san(&rows).unwrap(),
            vec![
                "example.com",
                "*.example.com",
                "api.example.com",
                "*.api.example.com"
            ]
        );
    }

    #[test]
    fn rejects_invalid_domain() {
        let rows = vec![DomainRow {
            domain: "bad domain!".into(),
            wildcard: false,
        }];
        assert!(build_san(&rows).is_err());
    }

    #[test]
    fn rejects_empty_domain_list() {
        assert!(build_san(&[]).is_err());
    }

    #[test]
    fn keeps_absolute_path_unchanged() {
        #[cfg(windows)]
        let path = expand_home("C:\\certs");
        #[cfg(not(windows))]
        let path = expand_home("/tmp/certs");
        #[cfg(windows)]
        assert_eq!(path, PathBuf::from("C:\\certs"));
        #[cfg(not(windows))]
        assert_eq!(path, PathBuf::from("/tmp/certs"));
    }

    #[test]
    fn parses_key_types() {
        assert_eq!(parse_key_type("ecdsa_p256").unwrap(), KeyType::EcdsaP256);
        assert_eq!(parse_key_type("rsa2048").unwrap(), KeyType::Rsa2048);
        assert!(parse_key_type("dsa").is_err());
    }
}
