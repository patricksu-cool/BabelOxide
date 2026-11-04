//! Background-only PDF transfers. Public URLs never use the provider transport:
//! its ordinary redirects, proxy and cookie behavior suit authenticated APIs.

use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{Url, header};
use std::{
    collections::HashSet,
    fs::File,
    future::Future,
    io::{BufWriter, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tempfile::{NamedTempFile, TempDir};
use tokio_util::sync::CancellationToken;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MAX_PDF_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;
pub const PDF_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
pub const PDF_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
const PDF_TAIL_BYTES: usize = 8192;

/// Keep this owner in the background until TaskManager::shutdown has completed.
/// Only the path is sent to the GUI; a path alone does not keep the PDF alive.
#[derive(Debug)]
pub struct DownloadedPdf {
    pub directory: TempDir,
    pub path: PathBuf,
}

pub async fn download_pdf(input: &str, cancel: &CancellationToken) -> Result<DownloadedPdf> {
    download_with(
        input,
        cancel,
        &PublicNetwork,
        MAX_PDF_DOWNLOAD_BYTES,
        PDF_DOWNLOAD_TIMEOUT,
    )
    .await
}

// These private seams exercise the actual URL/DNS/redirect/body policy without
// making public-network requests or adding a production allow-private option.
trait DownloadBody: Send {
    fn next(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send;
}

struct Reply<B> {
    status: u16,
    location: Option<String>,
    length: Option<u64>,
    content_type: Option<String>,
    encoding: Option<String>,
    body: B,
}

trait DownloadNetwork: Sync {
    type Body: DownloadBody;
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> impl Future<Output = Result<Vec<SocketAddr>>> + Send;
    fn get(
        &self,
        url: &Url,
        addresses: &[SocketAddr],
    ) -> impl Future<Output = Result<Reply<Self::Body>>> + Send;
}

struct PublicNetwork;

/// A missed DNS override fails closed rather than resolving again inside the
/// HTTP client. This also prevents an accidental future fallback from rebinding.
struct RejectUnpinnedDns;
impl reqwest::dns::Resolve for RejectUnpinnedDns {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Unpinned PDF download host",
            )
            .into())
        })
    }
}

fn pinned_download_client(url: &Url, addresses: &[SocketAddr]) -> Result<reqwest::Client> {
    // URL download can be the first network action in a fresh GUI process.
    // reqwest's rustls-no-provider feature requires explicit initialization;
    // do not rely on a translation transport having installed it beforehand.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let host = url.host_str().context("PDF 連結缺少伺服器名稱")?;
    // A new client on every hop has no cookies, credentials, connection pool
    // or translation-provider state. Only the checked sockets can be used.
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .referer(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .http1_only()
        .connect_timeout(PDF_CONNECT_TIMEOUT)
        .timeout(PDF_DOWNLOAD_TIMEOUT)
        .dns_resolver(Arc::new(RejectUnpinnedDns))
        .resolve_to_addrs(host, addresses)
        .build()
        .map_err(|_| anyhow!("無法建立 PDF 下載連線"))
}

impl DownloadNetwork for PublicNetwork {
    type Body = reqwest::Response;

    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        tokio::net::lookup_host((host, port))
            .await
            .map(|addresses| addresses.collect())
            .map_err(|_| anyhow!("無法解析 PDF 伺服器位址"))
    }

    async fn get(&self, url: &Url, addresses: &[SocketAddr]) -> Result<Reply<Self::Body>> {
        let client = pinned_download_client(url, addresses)?;
        let response = client
            .get(url.clone())
            .header(
                header::ACCEPT,
                "application/pdf, application/octet-stream;q=0.9, */*;q=0.1",
            )
            .header(header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            // reqwest errors can contain signed URLs; do not attach them.
            .map_err(|_| anyhow!("無法連線至 PDF 伺服器，請檢查連結或稍後再試"))?;
        ensure!(
            response
                .remote_addr()
                .is_some_and(|peer| addresses.contains(&peer)),
            "PDF 伺服器的連線位址與核准位址不符"
        );
        let length = response_header(response.headers(), header::CONTENT_LENGTH)?
            .map(|value| {
                ensure!(
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
                    "PDF 伺服器提供了無效的檔案大小"
                );
                value
                    .parse::<u64>()
                    .map_err(|_| anyhow!("PDF 伺服器提供了無效的檔案大小"))
            })
            .transpose()?;
        Ok(Reply {
            status: response.status().as_u16(),
            location: response_header(response.headers(), header::LOCATION)?,
            length,
            content_type: response_header(response.headers(), header::CONTENT_TYPE)?,
            encoding: response_header(response.headers(), header::CONTENT_ENCODING)?,
            body: response,
        })
    }
}

fn response_header(
    headers: &header::HeaderMap,
    name: header::HeaderName,
) -> Result<Option<String>> {
    let all = headers.get_all(name);
    let mut values = all.iter();
    let value = values.next();
    ensure!(values.next().is_none(), "PDF 伺服器提供了重複的回應標頭");
    value
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| anyhow!("PDF 伺服器提供了無效的回應標頭"))
        })
        .transpose()
}

impl DownloadBody for reqwest::Response {
    async fn next(&mut self) -> Result<Option<Vec<u8>>> {
        self.chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|_| anyhow!("PDF 下載中斷，請檢查連線後重試"))
    }
}

async fn download_with<N: DownloadNetwork>(
    input: &str,
    cancel: &CancellationToken,
    network: &N,
    max_bytes: u64,
    timeout: Duration,
) -> Result<DownloadedPdf> {
    // This one deadline covers DNS, every redirect, headers and all body reads.
    // Dropping the future cancels each asynchronous network stage immediately.
    tokio::select! {
        biased;
        _ = cancel.cancelled() => bail!("已取消 PDF 下載"),
        result = tokio::time::timeout(timeout, download_inner(input, network, max_bytes)) => {
            result.map_err(|_| anyhow!("PDF 下載逾時，請稍後再試"))?
        }
    }
}

async fn download_inner<N: DownloadNetwork>(
    input: &str,
    network: &N,
    max_bytes: u64,
) -> Result<DownloadedPdf> {
    let mut url = checked_url(input.trim(), None)?;
    for redirects in 0..=MAX_REDIRECTS {
        let host = url.host_str().context("PDF 連結缺少伺服器名稱")?;
        let port = url
            .port_or_known_default()
            .context("PDF 連結的連接埠無效")?;
        let literal = host.trim_matches(['[', ']']).parse::<IpAddr>().ok();
        let mut addresses = if let Some(ip) = literal {
            vec![SocketAddr::new(ip, port)]
        } else {
            network
                .resolve(host, port)
                .await
                .map_err(|_| anyhow!("無法解析 PDF 伺服器位址"))?
        };
        ensure!(
            !addresses.is_empty() && addresses.len() <= 64,
            "PDF 伺服器的 DNS 回應無效"
        );
        // Reject the entire DNS answer if any record is unsafe. Never silently
        // filter a private address and allow a later connection to select it.
        ensure!(
            addresses
                .iter()
                .all(|address| address.port() == port && is_public_ip(address.ip())),
            "PDF 連結不可存取本機、內網或保留位址"
        );
        addresses.sort_unstable();
        addresses.dedup();
        let mut reply = network
            .get(&url, &addresses)
            .await
            .map_err(|_| anyhow!("無法下載 PDF，請檢查連結或稍後再試"))?;
        if matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            ensure!(redirects < MAX_REDIRECTS, "PDF 連結的重新導向次數過多");
            let location = reply
                .location
                .as_deref()
                .context("PDF 重新導向缺少目的位址")?;
            let next = checked_url(location, Some(&url))?;
            ensure!(
                url.scheme() != "https" || next.scheme() == "https",
                "PDF 連結不可重新導向至未加密的 HTTP"
            );
            url = next;
            continue;
        }
        ensure!(
            reply.status == 200,
            "PDF 伺服器回應錯誤（HTTP {}）",
            reply.status
        );
        ensure!(
            reply.length.is_none_or(|length| length <= max_bytes),
            "PDF 大小超過下載上限（100 MiB）"
        );
        ensure!(
            reply
                .encoding
                .as_deref()
                .is_none_or(|value| value.eq_ignore_ascii_case("identity")),
            "PDF 伺服器回傳了不支援的傳輸編碼"
        );
        ensure!(
            reply.content_type.as_deref().is_none_or(|value| !matches!(
                value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
                    .as_str(),
                "text/html" | "application/xhtml+xml"
            )),
            "連結回傳的是網頁，請提供可直接下載的 PDF 連結"
        );
        let directory = tempfile::Builder::new()
            .prefix("babeloxide-download-")
            .tempdir()
            .context("無法建立 PDF 下載暫存資料夾")?;
        // A random basename also prevents different URL inputs from publishing
        // translations with the same name into a shared output directory.
        let unique_name = directory
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .context("無法建立 PDF 暫存檔名")?;
        let path = directory.path().join(format!("{unique_name}.pdf"));
        // These bounded writes run only on the background runtime. Synchronous
        // ownership makes cancellation close the file before deleting TempDir,
        // including on Windows; there is no detached disk operation to outlive it.
        let mut output = BufWriter::new(File::create(&path).context("無法建立 PDF 暫存檔")?);
        let mut pdf = PdfInspector::default();
        while let Some(chunk) = reply
            .body
            .next()
            .await
            .map_err(|_| anyhow!("PDF 下載中斷，請檢查連線後重試"))?
        {
            ensure!(
                chunk.len() as u64 <= max_bytes.saturating_sub(pdf.size),
                "PDF 大小超過下載上限（100 MiB）"
            );
            pdf.observe(&chunk)?;
            ensure!(
                reply.length.is_none_or(|length| pdf.size <= length),
                "PDF 的實際大小與伺服器回應不符"
            );
            output.write_all(&chunk).context("無法寫入 PDF 暫存檔")?;
            // Also gives cancellation/deadline a turn when buffered body frames
            // are immediately ready, rather than monopolizing the runtime.
            tokio::task::yield_now().await;
        }
        ensure!(
            reply.length.is_none_or(|length| pdf.size == length),
            "PDF 下載不完整，實際大小與伺服器回應不符"
        );
        pdf.finish()?;
        output.flush().context("無法完成 PDF 暫存檔")?;
        tokio::task::yield_now().await;
        drop(output);
        return Ok(DownloadedPdf { directory, path });
    }
    bail!("PDF 連結的重新導向次數過多")
}

fn checked_url(input: &str, base: Option<&Url>) -> Result<Url> {
    ensure!(
        !input.is_empty()
            && input.len() <= 8192
            && !input
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '\\' | '#')),
        "請輸入完整的 HTTP 或 HTTPS PDF 連結，勿包含空白、反斜線或片段（#）"
    );
    // Require the normal authority spelling and catch even empty userinfo (@),
    // which URL normalization would otherwise remove before validation.
    let authority = if let Some(authority) = input.strip_prefix("//") {
        Some(authority)
    } else if let Some((scheme, rest)) = input.split_once(':').filter(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    }) {
        ensure!(
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"),
            "僅支援 HTTP 或 HTTPS PDF 連結"
        );
        Some(rest.strip_prefix("//").context("PDF 連結的格式無效")?)
    } else {
        None
    };
    ensure!(
        authority.is_none_or(|authority| !authority
            .split(['/', '?'])
            .next()
            .unwrap_or("")
            .contains('@')),
        "PDF 連結不可包含帳號或密碼"
    );
    let url = match base {
        Some(base) => base.join(input),
        None => Url::parse(input),
    }
    .map_err(|_| anyhow!("PDF 連結的格式無效"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "僅支援 HTTP 或 HTTPS PDF 連結"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "PDF 連結不可包含帳號、密碼或片段（#）"
    );
    let expected_port = if url.scheme() == "https" { 443 } else { 80 };
    ensure!(
        url.port_or_known_default() == Some(expected_port),
        "PDF 連結僅支援 HTTP 80 或 HTTPS 443 連接埠"
    );
    let host = url.host_str().context("PDF 連結缺少伺服器名稱")?;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        ensure!(is_public_ip(ip), "PDF 連結不可存取本機、內網或保留位址");
    } else {
        let name = host.to_ascii_lowercase();
        ensure!(
            name.contains('.')
                && !name.ends_with('.')
                && !name.split('.').any(str::is_empty)
                && ![
                    "localhost",
                    "local",
                    "internal",
                    "lan",
                    "home",
                    "home.arpa",
                    "onion"
                ]
                .iter()
                .any(|suffix| name == *suffix || name.ends_with(&format!(".{suffix}"))),
            "PDF 連結不可使用本機或內網伺服器名稱"
        );
    }
    Ok(url)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    // IANA IPv4 Special-Purpose Address Registry. Conservatively reject whole
    // special-use blocks, including the public protocol-anycast exceptions.
    !(matches!(a, 0 | 10 | 127 | 224..=255)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && ((b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99) || b == 168))
        || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let parts = ip.segments();
    // Conservative IANA allocated global-unicast ranges (registry 2025-10-10):
    // https://www.iana.org/assignments/ipv6-unicast-address-assignments
    // Unlike a bare 2000::/3 check, this excludes unallocated/reserved space.
    // Mapped/compatible IPv4, NAT64, Teredo, 6to4, local, multicast and protocol
    // assignments are deliberately not allowed to tunnel around IPv4 checks.
    match parts[0] {
        0x2001 => {
            matches!(parts[1], 0x0200..=0x0fff | 0x1200..=0x4dff | 0x5000..=0x5fff | 0x8000..=0xbfff)
                && parts[1] != 0x0db8
        }
        0x2003 => parts[1] < 0x4000,
        0x2400..=0x241f
        | 0x2600..=0x260f
        | 0x2630..=0x263f
        | 0x2800..=0x280f
        | 0x2a00..=0x2a1f
        | 0x2c00..=0x2c0f => true,
        0x2610 | 0x2620 => parts[1] < 0x0200,
        _ => false,
    }
}

#[derive(Default)]
struct PdfInspector {
    head: Vec<u8>,
    tail: Vec<u8>,
    size: u64,
}

impl PdfInspector {
    fn observe(&mut self, bytes: &[u8]) -> Result<()> {
        self.size = self
            .size
            .checked_add(bytes.len() as u64)
            .context("PDF 檔案大小無效")?;
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(16 - self.head.len())]);
        if bytes.len() >= PDF_TAIL_BYTES {
            self.tail.clear();
            self.tail
                .extend_from_slice(&bytes[bytes.len() - PDF_TAIL_BYTES..]);
        } else {
            let discard = (self.tail.len() + bytes.len()).saturating_sub(PDF_TAIL_BYTES);
            self.tail.drain(..discard);
            self.tail.extend_from_slice(bytes);
        }
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        ensure!(
            self.head.len() >= 9
                && self.head.starts_with(b"%PDF-")
                && matches!(self.head[5], b'1' | b'2')
                && self.head[6] == b'.'
                && self.head[7].is_ascii_digit()
                && pdf_space(self.head[8]),
            "檔案不是有效的 PDF（缺少 PDF 檔頭）"
        );
        let tail = trim_pdf_space(&self.tail);
        let trailer = tail
            .strip_suffix(b"%%EOF")
            .context("PDF 下載不完整或檔案已損壞（缺少結尾標記）")?;
        let marker = trailer
            .windows(9)
            .rposition(|bytes| bytes == b"startxref")
            .context("PDF 下載不完整或檔案已損壞（缺少索引）")?;
        ensure!(
            marker == 0 || pdf_space(trailer[marker - 1]),
            "PDF 索引格式無效"
        );
        let offset = &trailer[marker + 9..];
        ensure!(
            offset.first().copied().is_some_and(pdf_space),
            "PDF 索引格式無效"
        );
        let offset = trim_pdf_space(offset);
        ensure!(
            !offset.is_empty() && offset.iter().all(u8::is_ascii_digit),
            "PDF 索引格式無效"
        );
        let offset = std::str::from_utf8(offset)
            .ok()
            .and_then(|offset| offset.parse::<u64>().ok())
            .context("PDF 索引格式無效")?;
        ensure!(
            offset >= 9 && offset < self.size,
            "PDF 索引位置無效，檔案可能不完整"
        );
        Ok(())
    }
}

fn pdf_space(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

fn trim_pdf_space(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().copied().is_some_and(pdf_space) {
        bytes = &bytes[1..];
    }
    while bytes.last().copied().is_some_and(pdf_space) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// Call on a blocking background worker. Every PDF is fully read and checked;
/// the old destination survives every failure before the final atomic replace.
pub fn export_zip(sources: &[PathBuf], destination: &Path) -> Result<()> {
    let sources = zip_sources(sources, destination)?;
    let parent = destination.parent().context("請選擇 ZIP 儲存資料夾")?;
    let mut temporary = NamedTempFile::new_in(parent).context("無法建立 ZIP 暫存檔")?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        let mut buffer = [0_u8; 64 * 1024];
        for source in &sources {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(0o600)
                .large_file(source.length > u32::MAX as u64);
            archive
                .start_file(source.name.as_str(), options)
                .context("無法新增 ZIP 項目")?;
            let mut input = File::open(&source.path).context("無法讀取 PDF 匯出來源")?;
            let mut pdf = PdfInspector::default();
            loop {
                let count = input
                    .read(&mut buffer)
                    .context("無法完整讀取 PDF 匯出來源")?;
                if count == 0 {
                    break;
                }
                pdf.observe(&buffer[..count])?;
                ensure!(
                    pdf.size <= source.length,
                    "PDF 來源在匯出時發生變動，請重試"
                );
                archive
                    .write_all(&buffer[..count])
                    .context("無法寫入 ZIP 項目")?;
            }
            ensure!(
                pdf.size == source.length,
                "PDF 來源在匯出時發生變動，請重試"
            );
            pdf.finish()?;
        }
        archive.finish().context("無法完成 ZIP 檔案")?;
    }
    temporary.flush().context("無法完成 ZIP 暫存檔")?;
    temporary
        .as_file()
        .sync_all()
        .context("無法同步 ZIP 暫存檔")?;
    verify_archive(temporary.path(), &sources)?;
    // Never delete the destination or fall back to a non-atomic copy. Tempfile
    // uses same-filesystem rename / MoveFileExW(REPLACE_EXISTING) on Windows.
    temporary
        .persist(destination)
        .map_err(|_| anyhow!("無法原子儲存 ZIP；原有檔案保持不變"))?;
    Ok(())
}

struct ZipSource {
    path: PathBuf,
    name: String,
    length: u64,
}

fn zip_sources(sources: &[PathBuf], destination: &Path) -> Result<Vec<ZipSource>> {
    ensure!(!sources.is_empty(), "目前沒有可匯出的 PDF 結果");
    ensure!(
        destination.is_absolute()
            && destination
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip")),
        "請選擇完整的 ZIP 儲存路徑（.zip）"
    );
    let parent = destination.parent().context("請選擇 ZIP 儲存資料夾")?;
    ensure!(parent.is_dir(), "ZIP 儲存資料夾不存在");
    let existing = match destination.symlink_metadata() {
        Ok(_) => {
            ensure!(destination.is_file(), "ZIP 儲存路徑不是一般檔案");
            Some(
                destination
                    .canonicalize()
                    .context("無法確認 ZIP 儲存路徑")?,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => bail!("無法確認 ZIP 儲存路徑"),
    };
    let mut names = HashSet::new();
    sources
        .iter()
        .map(|source| {
            ensure!(
                source.is_absolute()
                    && source
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")),
                "ZIP 匯出來源必須是完整的 PDF 路徑"
            );
            let path = source.canonicalize().context("PDF 匯出來源已不存在")?;
            let metadata = path.metadata().context("無法讀取 PDF 匯出來源資訊")?;
            ensure!(metadata.is_file(), "PDF 匯出來源不是一般檔案");
            ensure!(existing.as_ref() != Some(&path), "ZIP 不可覆寫 PDF 來源");
            let stem = safe_stem(source);
            let mut name = format!("{stem}.pdf");
            let mut duplicate = 2;
            // Uppercase is conservative for Windows case-insensitive matching,
            // including Unicode case pairs. Sanitization removes trailing dots/spaces.
            while !names.insert(name.to_uppercase()) {
                name = format!("{stem} ({duplicate}).pdf");
                duplicate += 1;
            }
            Ok(ZipSource {
                path,
                name,
                length: metadata.len(),
            })
        })
        .collect()
}

fn safe_stem(source: &Path) -> String {
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let mut stem: String = stem
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .take(120)
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_owned();
    if stem.is_empty() {
        stem = "document".into();
    }
    let device = stem.split('.').next().unwrap_or("").to_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].into_iter().any(|prefix| {
        device.strip_prefix(prefix).is_some_and(|number| {
            number.len() == 1 && number.as_bytes()[0].is_ascii_digit()
                || matches!(number, "¹" | "²" | "³")
        })
    }) {
        stem.insert(0, '_');
    }
    stem
}

fn verify_archive(path: &Path, sources: &[ZipSource]) -> Result<()> {
    let mut archive = ZipArchive::new(File::open(path).context("無法重新讀取 ZIP 暫存檔")?)
        .context("ZIP 完整性檢查失敗")?;
    ensure!(archive.len() == sources.len(), "ZIP 項目數量不完整");
    let mut buffer = [0_u8; 64 * 1024];
    let mut expected = [0_u8; 64 * 1024];
    for (index, source) in sources.iter().enumerate() {
        let mut entry = archive.by_index(index).context("無法重新讀取 ZIP 項目")?;
        ensure!(
            entry.name_raw() == source.name.as_bytes()
                && entry.size() == source.length
                && entry.is_file(),
            "ZIP 項目資訊不完整"
        );
        let mut input = File::open(&source.path).context("無法重新讀取 PDF 匯出來源")?;
        let mut read = 0_u64;
        loop {
            // Reading to EOF also makes zip verify each entry's CRC32. Compare
            // every byte with the source rather than trusting ZIP metadata alone.
            let count = entry.read(&mut buffer).context("ZIP 項目完整性檢查失敗")?;
            if count == 0 {
                break;
            }
            input
                .read_exact(&mut expected[..count])
                .context("PDF 來源在匯出時發生變動，請重試")?;
            ensure!(
                buffer[..count] == expected[..count],
                "ZIP 內容與 PDF 來源不符，請重試"
            );
            read += count as u64;
        }
        ensure!(
            read == source.length
                && input
                    .read(&mut expected[..1])
                    .context("無法確認 PDF 匯出來源大小")?
                    == 0,
            "PDF 來源在匯出時發生變動，請重試"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{Seek, SeekFrom},
        sync::Mutex,
    };
    use tokio::sync::Notify;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Stage {
        Dns,
        Headers,
        Body,
    }

    #[derive(Default)]
    struct MockBody {
        chunks: VecDeque<Vec<u8>>,
        blocked: Option<Arc<Notify>>,
    }

    impl DownloadBody for MockBody {
        async fn next(&mut self) -> Result<Option<Vec<u8>>> {
            if let Some(entered) = &self.blocked {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(self.chunks.pop_front())
        }
    }

    #[derive(Default)]
    struct MockNetwork {
        resolutions: Mutex<VecDeque<Vec<IpAddr>>>,
        replies: Mutex<VecDeque<Reply<MockBody>>>,
        sent: Mutex<Vec<(String, Vec<SocketAddr>)>>,
        blocked: Option<Stage>,
        entered: Arc<Notify>,
    }

    impl MockNetwork {
        fn new(replies: Vec<Reply<MockBody>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                ..Self::default()
            }
        }
    }

    impl DownloadNetwork for MockNetwork {
        type Body = MockBody;

        async fn resolve(&self, _: &str, port: u16) -> Result<Vec<SocketAddr>> {
            if self.blocked == Some(Stage::Dns) {
                self.entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(self
                .resolutions
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| vec![public_ip()])
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect())
        }

        async fn get(&self, url: &Url, addresses: &[SocketAddr]) -> Result<Reply<Self::Body>> {
            self.sent
                .lock()
                .unwrap()
                .push((url.to_string(), addresses.to_vec()));
            if self.blocked == Some(Stage::Headers) {
                self.entered.notify_one();
                std::future::pending::<()>().await;
            }
            let mut reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .context("Missing test reply")?;
            if self.blocked == Some(Stage::Body) {
                reply.body.blocked = Some(self.entered.clone());
            }
            Ok(reply)
        }
    }

    fn public_ip() -> IpAddr {
        "93.184.216.34".parse().unwrap()
    }

    fn url_with_userinfo(userinfo: &str) -> String {
        // Synthetic rejection cases are assembled without storing a complete
        // credential-bearing URL in the source distributed for review.
        ["https://", userinfo, "@", "files.example/a.pdf"].concat()
    }

    fn pdf_bytes(label: &str) -> Vec<u8> {
        let mut bytes = format!("%PDF-1.7\n%{label}\n").into_bytes();
        let first = bytes.len();
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        let second = bytes.len();
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
        let xref = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 3\n0000000000 65535 f \n{first:010} 00000 n \n{second:010} 00000 n \ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        bytes
    }

    fn ok_reply(bytes: &[u8]) -> Reply<MockBody> {
        Reply {
            status: 200,
            location: None,
            length: Some(bytes.len() as u64),
            content_type: Some("application/pdf".into()),
            encoding: None,
            body: MockBody {
                chunks: bytes.chunks(7).map(<[u8]>::to_vec).collect(),
                blocked: None,
            },
        }
    }

    fn redirect(location: &str) -> Reply<MockBody> {
        Reply {
            status: 302,
            location: Some(location.into()),
            length: None,
            content_type: None,
            encoding: None,
            body: MockBody::default(),
        }
    }

    async fn run(network: &MockNetwork) -> Result<DownloadedPdf> {
        download_with(
            "https://files.example/paper.pdf?token=secret",
            &CancellationToken::new(),
            network,
            MAX_PDF_DOWNLOAD_BYTES,
            PDF_DOWNLOAD_TIMEOUT,
        )
        .await
    }

    #[test]
    fn non_public_ipv4_and_ipv6_are_rejected() {
        for address in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.9",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "::8.8.8.8",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "100::1",
            "2000::1",
            "2001::1",
            "2001:2::1",
            "2001:db8::1",
            "2002:a00:1::1",
            "2003:4000::1",
            "2001:1000::1",
            "2001:c000::1",
            "2500::1",
            "3000::1",
            "3ffe::1",
            "3fff::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(
                !is_public_ip(address.parse().unwrap()),
                "unsafe address: {address}"
            );
        }
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "100.128.0.1",
            "172.32.0.1",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
            "2a00:1450::1",
            "2410::1",
        ] {
            assert!(
                is_public_ip(address.parse().unwrap()),
                "public address: {address}"
            );
        }
    }

    #[test]
    fn url_policy_rejects_credentials_fragments_ports_and_obfuscated_local_hosts() {
        let credentials = url_with_userinfo("user:secret");
        let empty_userinfo = url_with_userinfo("");
        let backslash_authority = ["https://", "files.example\\", "@127.0.0.1/a.pdf"].concat();
        for input in [
            "file:///C:/secret.pdf",
            "ftp://files.example/a.pdf",
            "data:application/pdf;base64,AA",
            "http:/files.example/a.pdf",
            credentials.as_str(),
            empty_userinfo.as_str(),
            "https://files.example/a.pdf#",
            "https://files.example/a.pdf#page=2",
            "http://files.example:8080/a.pdf",
            "https://files.example:80/a.pdf",
            "https://files.example:8443/a.pdf",
            "http://localhost/a.pdf",
            "http://router.local/a.pdf",
            "http://host.home.arpa/a.pdf",
            "https://files.example./a.pdf",
            "http://2130706433/a.pdf",
            "http://0177.0.0.1/a.pdf",
            "http://0x7f000001/a.pdf",
            "http://[::ffff:127.0.0.1]/a.pdf",
            backslash_authority.as_str(),
            "https://files.example/\r\nsecret",
            "https://files.example/a b.pdf",
        ] {
            let error = checked_url(input, None).unwrap_err();
            assert!(
                !format!("{error:#}").contains(input),
                "must not disclose input"
            );
        }
        assert!(checked_url("https://files.example/a.pdf?token=secret%23value", None).is_ok());
        assert!(checked_url("http://8.8.8.8:80/a.pdf", None).is_ok());
    }

    #[tokio::test]
    async fn validated_dns_sockets_are_passed_to_the_executor_and_temp_owner_keeps_pdf_alive() {
        let bytes = pdf_bytes("download");
        let network = MockNetwork::new(vec![ok_reply(&bytes)]);
        let addresses = vec![public_ip(), "1.1.1.1".parse().unwrap()];
        network
            .resolutions
            .lock()
            .unwrap()
            .push_back(addresses.clone());
        let downloaded = run(&network).await.unwrap();
        assert!(downloaded.path.starts_with(downloaded.directory.path()));
        assert_eq!(std::fs::read(&downloaded.path).unwrap(), bytes);
        let mut expected: Vec<_> = addresses
            .into_iter()
            .map(|ip| SocketAddr::new(ip, 443))
            .collect();
        expected.sort_unstable();
        assert_eq!(network.sent.lock().unwrap()[0].1, expected);
        let path = downloaded.path.clone();
        drop(downloaded);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn mixed_private_dns_answer_is_rejected_before_any_request() {
        let network = MockNetwork::default();
        network
            .resolutions
            .lock()
            .unwrap()
            .push_back(vec![public_ip(), "10.0.0.1".parse().unwrap()]);
        assert!(run(&network).await.is_err());
        assert!(network.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn every_redirect_is_revalidated_including_rebinding_the_same_hostname() {
        let network = MockNetwork::new(vec![redirect("/second.pdf"), redirect("/third.pdf")]);
        network.resolutions.lock().unwrap().extend([
            vec![public_ip()],
            vec![public_ip()],
            vec!["127.0.0.1".parse().unwrap()],
        ]);
        assert!(run(&network).await.is_err());
        assert_eq!(network.sent.lock().unwrap().len(), 2);
        let credentials = url_with_userinfo("user:secret");
        for target in [
            "http://files.example/a.pdf",
            "https://10.0.0.1/a.pdf",
            "https://[::1]/a.pdf",
            "file:///a.pdf",
            credentials.as_str(),
            "https://files.example/a.pdf#frag",
            "https://files.example:8080/a.pdf",
        ] {
            let network = MockNetwork::new(vec![redirect("/second.pdf"), redirect(target)]);
            assert!(run(&network).await.is_err());
            assert_eq!(
                network.sent.lock().unwrap().len(),
                2,
                "bad redirect reached executor"
            );
        }
    }

    #[tokio::test]
    async fn valid_relative_redirect_works_and_redirect_loops_are_bounded() {
        let bytes = pdf_bytes("redirect");
        let network = MockNetwork::new(vec![redirect("../final.pdf"), ok_reply(&bytes)]);
        let downloaded = run(&network).await.unwrap();
        assert_eq!(std::fs::read(downloaded.path).unwrap(), bytes);
        assert_eq!(
            network.sent.lock().unwrap()[1].0,
            "https://files.example/final.pdf"
        );
        let network = MockNetwork::new(
            (0..=MAX_REDIRECTS)
                .map(|_| redirect("/again.pdf"))
                .collect(),
        );
        assert!(run(&network).await.is_err());
        assert_eq!(network.sent.lock().unwrap().len(), MAX_REDIRECTS + 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_dns_headers_and_body() {
        for stage in [Stage::Dns, Stage::Headers, Stage::Body] {
            let network = Arc::new(MockNetwork {
                blocked: Some(stage),
                ..MockNetwork::new(vec![ok_reply(&pdf_bytes("cancel"))])
            });
            let token = CancellationToken::new();
            let task_network = network.clone();
            let task_token = token.clone();
            let task = tokio::spawn(async move {
                download_with(
                    "https://files.example/a.pdf",
                    &task_token,
                    task_network.as_ref(),
                    MAX_PDF_DOWNLOAD_BYTES,
                    PDF_DOWNLOAD_TIMEOUT,
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(2), network.entered.notified())
                .await
                .unwrap();
            token.cancel();
            let result = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
            assert!(result.unwrap_err().to_string().contains("取消"));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_also_covers_dns_before_connecting() {
        let network = MockNetwork {
            blocked: Some(Stage::Dns),
            ..MockNetwork::default()
        };
        let error = download_with(
            "https://files.example/a.pdf",
            &CancellationToken::new(),
            &network,
            MAX_PDF_DOWNLOAD_BYTES,
            Duration::from_secs(2),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("逾時"));
        assert!(network.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn size_caps_apply_to_declared_and_streamed_bytes_and_require_complete_length() {
        let bytes = pdf_bytes("size");
        for declared in [Some(129), None] {
            let mut reply = ok_reply(&bytes);
            reply.length = declared;
            let network = MockNetwork::new(vec![reply]);
            let error = download_with(
                "https://files.example/a.pdf",
                &CancellationToken::new(),
                &network,
                128,
                PDF_DOWNLOAD_TIMEOUT,
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("上限"));
        }
        for length in [bytes.len() as u64 - 1, bytes.len() as u64 + 1] {
            let mut reply = ok_reply(&bytes);
            reply.length = Some(length);
            assert!(
                run(&MockNetwork::new(vec![reply]))
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("大小")
            );
        }
    }

    #[tokio::test]
    async fn non_pdf_truncated_html_error_status_and_encoded_bodies_are_rejected_without_url_leaks()
    {
        let bytes = pdf_bytes("valid");
        let mut html = ok_reply(&bytes);
        html.content_type = Some("text/html; charset=UTF-8".into());
        let mut status = ok_reply(&bytes);
        status.status = 403;
        let mut encoded = ok_reply(&bytes);
        encoded.encoding = Some("gzip".into());
        for reply in [
            ok_reply(b"<html>not a PDF</html>"),
            ok_reply(b"%PDF-1.7\ntruncated"),
            ok_reply(b"%PDF-1.7\nstartxref\n999999999\n%%EOF"),
            html,
            status,
            encoded,
        ] {
            let error = run(&MockNetwork::new(vec![reply])).await.unwrap_err();
            let message = format!("{error:#}");
            assert!(
                !message.contains("secret")
                    && !message.contains("files.example")
                    && !message.contains("https://")
            );
        }
    }

    #[tokio::test]
    async fn unpinned_http_dns_fallback_fails_closed() {
        use reqwest::dns::Resolve;
        assert!(
            RejectUnpinnedDns
                .resolve("files.example".parse().unwrap())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cold_start_download_client_initializes_tls() {
        const CHILD_FLAG: &str = "BABELOXIDE_TEST_COLD_DOWNLOAD_TLS_CHILD";
        const TEST_NAME: &str = "transfers::tests::cold_start_download_client_initializes_tls";
        if std::env::var_os(CHILD_FLAG).is_some() {
            assert!(
                rustls::crypto::CryptoProvider::get_default().is_none(),
                "fresh subprocess must not inherit an initialized TLS provider"
            );
            let url = checked_url("https://files.example/a.pdf", None).unwrap();
            let addresses = [SocketAddr::new(public_ip(), 443)];
            // Building clients is entirely offline: no DNS lookup or request.
            let _first = pinned_download_client(&url, &addresses).unwrap();
            assert!(rustls::crypto::CryptoProvider::get_default().is_some());
            let _second = pinned_download_client(&url, &addresses).unwrap();
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_FLAG, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains(TEST_NAME),
            "cold-start regression subprocess failed or did not run its test:\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn zip_roundtrip_preserves_all_bytes_and_deduplicates_windows_case_names() {
        let directory = tempfile::tempdir().unwrap();
        let mut sources = Vec::new();
        for (index, name) in ["report.PDF", "REPORT.pdf", "Report (2).pdf"]
            .iter()
            .enumerate()
        {
            let parent = directory.path().join(index.to_string());
            std::fs::create_dir(&parent).unwrap();
            let path = parent.join(name);
            std::fs::write(&path, pdf_bytes(&index.to_string())).unwrap();
            sources.push(path);
        }
        let destination = directory.path().join("results.zip");
        export_zip(&sources, &destination).unwrap();
        let mut archive = ZipArchive::new(File::open(destination).unwrap()).unwrap();
        assert_eq!(archive.len(), sources.len());
        let mut names = HashSet::new();
        for (index, source) in sources.iter().enumerate() {
            let mut entry = archive.by_index(index).unwrap();
            let name = String::from_utf8(entry.name_raw().to_vec()).unwrap();
            assert!(names.insert(name.to_uppercase()));
            assert!(!name.contains(['/', '\\', ':']));
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, std::fs::read(source).unwrap());
        }
    }

    #[test]
    fn archive_names_remove_windows_devices_illegal_characters_and_trailing_dots() {
        assert_eq!(safe_stem(Path::new("CON.pdf")), "_CON");
        assert_eq!(safe_stem(Path::new("com1.more.pdf")), "_com1.more");
        assert_eq!(safe_stem(Path::new("LPT¹.pdf")), "_LPT¹");
        assert_eq!(safe_stem(Path::new("a?b:*.pdf")), "a_b__");
        assert_eq!(safe_stem(Path::new(" .pdf")), "document");
        assert_eq!(safe_stem(Path::new("paper. .pdf")), "paper");
    }

    #[test]
    fn invalid_later_source_preserves_old_zip_and_source_pdf() {
        let directory = tempfile::tempdir().unwrap();
        let good = directory.path().join("good.pdf");
        let bad = directory.path().join("bad.pdf");
        let destination = directory.path().join("results.zip");
        let bytes = pdf_bytes("source");
        std::fs::write(&good, &bytes).unwrap();
        std::fs::write(&bad, b"%PDF-1.7\ncorrupt").unwrap();
        export_zip(std::slice::from_ref(&good), &destination).unwrap();
        let old = std::fs::read(&destination).unwrap();
        assert!(export_zip(&[good.clone(), bad], &destination).is_err());
        assert!(export_zip(&[], &destination).is_err());
        assert!(export_zip(std::slice::from_ref(&good), &good).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), old);
        assert_eq!(std::fs::read(&good).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    #[test]
    fn readback_rejects_corrupt_stored_data_and_changed_source() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("paper.pdf");
        let destination = directory.path().join("result.zip");
        std::fs::write(&source, pdf_bytes("original")).unwrap();
        export_zip(std::slice::from_ref(&source), &destination).unwrap();
        let sources = zip_sources(std::slice::from_ref(&source), &destination).unwrap();
        let start = {
            let mut archive = ZipArchive::new(File::open(&destination).unwrap()).unwrap();
            archive.by_index(0).unwrap().data_start().unwrap()
        };
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .open(&destination)
            .unwrap();
        output.seek(SeekFrom::Start(start + 12)).unwrap();
        output.write_all(b"!").unwrap();
        drop(output);
        assert!(verify_archive(&destination, &sources).is_err());
        export_zip(std::slice::from_ref(&source), &destination).unwrap();
        std::fs::write(&source, pdf_bytes("modified")).unwrap();
        assert!(verify_archive(&destination, &sources).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn failed_atomic_publish_keeps_existing_zip() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("paper.pdf");
        let destination = directory.path().join("result.zip");
        std::fs::write(&source, pdf_bytes("source")).unwrap();
        export_zip(std::slice::from_ref(&source), &destination).unwrap();
        let old = std::fs::read(&destination).unwrap();
        let original_permissions = destination.metadata().unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&destination, readonly_permissions).unwrap();
        let result = export_zip(std::slice::from_ref(&source), &destination);
        std::fs::set_permissions(&destination, original_permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), old);
    }
}
