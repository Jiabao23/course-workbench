use anyhow::{anyhow, bail, ensure, Context, Result};
use course_core::{subtitles::parse_subtitles, Segment};
use reqwest::{
    blocking::Client,
    header::{HeaderValue, COOKIE},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, time::Duration};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub language: String,
    pub label: String,
    pub url: String,
    pub format: String,
    #[serde(default)]
    pub automatic: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePart {
    pub page: u32,
    pub cid: Option<u64>,
    pub title: String,
    pub duration_ms: u64,
    pub subtitle_status: String,
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePreview {
    pub source: String,
    pub title: String,
    pub source_kind: String,
    pub bvid: Option<String>,
    pub parts: Vec<SourcePart>,
    pub warnings: Vec<String>,
}

pub trait SourceProvider: Send + Sync {
    fn preview(&self, source: &str) -> Result<SourcePreview>;
    fn inspect_part(&self, bvid: &str, part: &mut SourcePart) -> Result<()>;
    fn subtitles(&self, source: &str, track: &SubtitleTrack) -> Result<Vec<Segment>>;
}

pub fn is_bilibili_source(source: &str) -> bool {
    let source = source.trim();
    if source.starts_with("BV") && !source.contains(['/', '\\']) {
        return true;
    }
    Url::parse(source).ok().is_some_and(|url| {
        url.host_str()
            .is_some_and(|host| host == "bilibili.com" || host.ends_with(".bilibili.com"))
    })
}

pub fn extract_bvid(source: &str) -> Result<String> {
    let input = source.trim();
    let valid = |value: &str| {
        value.len() == 12
            && value.starts_with("BV")
            && value.bytes().all(|b| b.is_ascii_alphanumeric())
    };
    if valid(input) {
        return Ok(input.into());
    }
    let url = Url::parse(input).context("请输入 B 站单视频链接、含 bvid 的收藏链接，或 BV 号")?;
    let host = url.host_str().unwrap_or("");
    ensure!(
        url.scheme() == "https" || url.scheme() == "http",
        "链接协议不受支持"
    );
    ensure!(
        host == "bilibili.com" || host.ends_with(".bilibili.com"),
        "首版仅支持 bilibili.com 视频链接"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "链接不能包含登录凭据"
    );
    for (key, value) in url.query_pairs() {
        if key == "bvid" && valid(&value) {
            return Ok(value.into_owned());
        }
    }
    for segment in url.path_segments().into_iter().flatten() {
        if valid(segment) {
            return Ok(segment.into());
        }
    }
    bail!("链接中没有 BV 号；请复制视频页的完整链接")
}

pub fn classify_subtitles(value: &Value) -> (String, Vec<SubtitleTrack>) {
    let code = value.get("code").and_then(Value::as_i64);
    if code == Some(-101) {
        return ("loginRequired".into(), vec![]);
    }
    if code != Some(0) {
        return ("failed".into(), vec![]);
    }
    let Some(data) = value.get("data").filter(|data| data.is_object()) else {
        return ("failed".into(), vec![]);
    };
    let tracks: Vec<_> = data
        .pointer("/subtitle/subtitles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let raw = track.get("subtitle_url")?.as_str()?;
            if raw.is_empty() {
                return None;
            }
            Some(SubtitleTrack {
                language: track["lan"].as_str().unwrap_or("unknown").into(),
                label: track["lan_doc"].as_str().unwrap_or("字幕").into(),
                url: if raw.starts_with("//") {
                    format!("https:{raw}")
                } else {
                    raw.into()
                },
                format: "json".into(),
                automatic: false,
            })
        })
        .collect();
    if !tracks.is_empty() {
        ("available".into(), tracks)
    } else if data["need_login_subtitle"].as_bool().unwrap_or(false)
        || data
            .pointer("/subtitle/need_login_subtitle")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        ("loginRequired".into(), tracks)
    } else if data
        .pointer("/subtitle/subtitles")
        .is_some_and(Value::is_array)
    {
        ("absent".into(), tracks)
    } else {
        ("failed".into(), tracks)
    }
}

pub fn parse_view(value: &Value, bvid: &str) -> Result<SourcePreview> {
    ensure!(
        value["code"].as_i64() == Some(0),
        "B 站视频信息请求失败（代码 {}）",
        value["code"]
    );
    let data = &value["data"];
    let title = data["title"]
        .as_str()
        .ok_or_else(|| anyhow!("视频信息缺少标题"))?;
    let pages = data["pages"]
        .as_array()
        .ok_or_else(|| anyhow!("视频信息缺少分 P 列表"))?;
    ensure!(!pages.is_empty(), "该视频没有可处理的分 P");
    let parts: Result<Vec<_>> = pages
        .iter()
        .map(|part| {
            Ok(SourcePart {
                page: u32::try_from(part["page"].as_u64().context("分 P 编号无效")?)?,
                cid: part["cid"].as_u64(),
                title: part["part"].as_str().unwrap_or(title).into(),
                duration_ms: part["duration"].as_u64().unwrap_or(0).saturating_mul(1000),
                subtitle_status: "unchecked".into(),
                subtitles: vec![],
            })
        })
        .collect();
    Ok(SourcePreview {
        source: format!("https://www.bilibili.com/video/{bvid}"),
        title: title.into(),
        source_kind: "bilibili".into(),
        bvid: Some(bvid.into()),
        parts: parts?,
        warnings: vec![],
    })
}

pub struct BilibiliProvider {
    client: Client,
    cookie: Option<HeaderValue>,
}

impl BilibiliProvider {
    pub fn new(cookie_file: &str) -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(25)).connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36")
            .build()?;
        let cookie = if cookie_file.trim().is_empty() {
            None
        } else {
            read_cookie_header(Path::new(cookie_file))?
        };
        Ok(Self { client, cookie })
    }
    fn api(&self, endpoint: &str, params: &[(&str, String)]) -> Result<Value> {
        let mut request = self
            .client
            .get(format!("https://api.bilibili.com/{endpoint}"))
            .query(params)
            .header("Referer", "https://www.bilibili.com/");
        if let Some(cookie) = &self.cookie {
            request = request.header(COOKIE, cookie.clone());
        }
        let response = request.send().context("无法连接 B 站，请检查网络后重试")?;
        ensure!(
            response.status().is_success(),
            "B 站请求失败（HTTP {}），请稍后重试",
            response.status().as_u16()
        );
        response.json().context("B 站返回了无法识别的响应")
    }
}

impl SourceProvider for BilibiliProvider {
    fn preview(&self, source: &str) -> Result<SourcePreview> {
        let bvid = extract_bvid(source)?;
        let data = self.api("x/web-interface/view", &[("bvid", bvid.clone())])?;
        let mut preview = parse_view(&data, &bvid)?;
        // Inspect only the first part; other parts are checked when selected for
        // processing. Avoid 69 unsolicited subtitle requests for a long course.
        if let Some(first) = preview.parts.first_mut() {
            if let Err(error) = self.inspect_part(&bvid, first) {
                first.subtitle_status = "failed".into();
                preview.warnings.push(error.to_string());
            }
        }
        if preview.parts.len() > 1 {
            preview
                .warnings
                .push("仅处理勾选的分 P，其余分 P 的字幕将在处理前检查。".into());
        }
        Ok(preview)
    }
    fn inspect_part(&self, bvid: &str, part: &mut SourcePart) -> Result<()> {
        let data = self.api(
            "x/player/v2",
            &[
                ("bvid", bvid.into()),
                ("cid", part.cid.context("缺少分 P cid")?.to_string()),
            ],
        )?;
        (part.subtitle_status, part.subtitles) = classify_subtitles(&data);
        Ok(())
    }
    fn subtitles(&self, _source: &str, track: &SubtitleTrack) -> Result<Vec<Segment>> {
        let url = Url::parse(&track.url)?;
        let host = url.host_str().unwrap_or("");
        ensure!(
            url.scheme() == "https"
                && (host == "bilibili.com"
                    || host.ends_with(".bilibili.com")
                    || host == "hdslb.com"
                    || host.ends_with(".hdslb.com")),
            "字幕地址不属于 B 站字幕服务"
        );
        // CDN subtitle fetch never receives the user's Bilibili Cookie.
        let response = self
            .client
            .get(url)
            .header("Referer", "https://www.bilibili.com/")
            .send()?;
        ensure!(
            response.status().is_success(),
            "字幕下载失败（HTTP {}）",
            response.status().as_u16()
        );
        ensure!(
            response.content_length().unwrap_or(0) < 32 * 1024 * 1024,
            "字幕文件过大"
        );
        let text = response.text()?;
        ensure!(text.len() < 32 * 1024 * 1024, "字幕文件过大");
        parse_subtitles(&text, &track.format)
    }
}

fn read_cookie_header(path: &Path) -> Result<Option<HeaderValue>> {
    ensure!(
        fs::metadata(path).context("无法读取 Cookie 文件")?.len() < 5 * 1024 * 1024,
        "Cookie 文件过大"
    );
    let contents = fs::read_to_string(path)?;
    let now = chrono::Utc::now().timestamp();
    let mut values = Vec::new();
    for raw in contents.lines() {
        let line = raw.strip_prefix("#HttpOnly_").unwrap_or(raw);
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 7 {
            continue;
        }
        let host = fields[0].trim_start_matches('.').to_ascii_lowercase();
        let includes_subdomains = fields[1].eq_ignore_ascii_case("true");
        if host != "api.bilibili.com" && !(host == "bilibili.com" && includes_subdomains) {
            continue;
        }
        let expiry: i64 = fields[4].parse().unwrap_or(0);
        if expiry != 0 && expiry < now {
            continue;
        }
        if !fields[5]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            continue;
        }
        if fields[6].contains(['\r', '\n', ';']) {
            continue;
        }
        values.push(format!("{}={}", fields[5], fields[6]));
    }
    if values.is_empty() {
        return Ok(None);
    }
    let mut header = HeaderValue::from_str(&values.join("; "))?;
    header.set_sensitive(true);
    Ok(Some(header))
}

pub fn local_preview(source: &str) -> Result<SourcePreview> {
    let path = dunce::canonicalize(Path::new(source.trim().trim_matches('"')))
        .context("找不到本地文件")?;
    ensure!(path.is_file(), "请选择文件");
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let subtitle = ["srt", "vtt", "json"].contains(&format.as_str());
    ensure!(
        subtitle
            || [
                "mp3", "wav", "m4a", "mp4", "mkv", "flac", "ogg", "aac", "opus", "webm", "mov",
                "wma"
            ]
            .contains(&format.as_str()),
        "不支持此文件类型；请选择音视频、SRT、VTT 或 B 站字幕 JSON"
    );
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("本地资料")
        .to_owned();
    let duration_ms = if subtitle {
        let metadata = fs::metadata(&path)?;
        ensure!(metadata.len() < 32 * 1024 * 1024, "字幕文件过大");
        let segments = parse_subtitles(&fs::read_to_string(&path)?, &format)?;
        segments.iter().map(|s| s.end_ms).max().unwrap_or(0)
    } else {
        0
    };
    Ok(SourcePreview {
        source: path.to_string_lossy().into_owned(),
        title: title.clone(),
        source_kind: if subtitle { "subtitle" } else { "localMedia" }.into(),
        bvid: None,
        parts: vec![SourcePart {
            page: 1,
            cid: None,
            title,
            duration_ms,
            subtitle_status: if subtitle { "available" } else { "absent" }.into(),
            subtitles: vec![],
        }],
        warnings: vec![],
    })
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn unrelated_website_cookies_allow_anonymous_bilibili_access() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cookies.txt");
        fs::write(
            &path,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tsession\tOTHER_SITE\n",
        )
        .unwrap();
        let provider = BilibiliProvider::new(path.to_str().unwrap()).unwrap();
        assert!(provider.cookie.is_none());
    }

    #[test]
    fn only_unexpired_cookies_scoped_to_the_bilibili_api_are_sent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cookies.txt");
        fs::write(
            &path,
            concat!(
                "# Netscape HTTP Cookie File\n",
                ".youtube.com\tTRUE\t/\tTRUE\t0\tsession\tOTHER_SITE\n",
                ".bilibili.com.evil.example\tTRUE\t/\tTRUE\t0\tspoof\tSPOOF\n",
                "www.bilibili.com\tFALSE\t/\tTRUE\t0\tweb_only\tWEB_ONLY\n",
                "bilibili.com\tFALSE\t/\tTRUE\t0\troot_only\tROOT_ONLY\n",
                ".bilibili.com\tTRUE\t/\tTRUE\t1\texpired\tEXPIRED\n",
                "#HttpOnly_.bilibili.com\tTRUE\t/\tTRUE\t0\tSESSDATA\tTEST_SESSION\n",
                "api.bilibili.com\tFALSE\t/\tTRUE\t0\tapi_cookie\tTEST_API\n",
            ),
        )
        .unwrap();
        let provider = BilibiliProvider::new(path.to_str().unwrap()).unwrap();
        let header = provider.cookie.unwrap();
        assert_eq!(
            header.to_str().unwrap(),
            "SESSDATA=TEST_SESSION; api_cookie=TEST_API"
        );
        assert!(header.is_sensitive());
    }
}
