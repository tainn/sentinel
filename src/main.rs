use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Url};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;

const PERSISTENCE_PATH: &str = "/data/persistence.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (BZFlag forums Discord notifier)"
);

#[derive(Serialize, Default, Debug)]
pub struct WebhookPayload<'a> {
    pub username: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub content: Option<&'a str>,
    pub tts: Option<bool>,
    pub embeds: Vec<Embed<'a>>,
}

#[derive(Serialize, Default, Debug)]
pub struct Embed<'a> {
    pub author: Option<EmbedAuthor<'a>>,
    pub color: Option<u32>,
    pub title: Option<&'a str>,
    pub url: Option<&'a str>,
    pub description: Option<&'a str>,
    pub fields: Vec<EmbedField<'a>>,
    pub thumbnail: Option<EmbedThumbnail<'a>>,
    pub image: Option<EmbedImage<'a>>,
    pub footer: Option<EmbedFooter<'a>>,
    pub timestamp: Option<&'a str>,
}

#[derive(Serialize, Default, Debug)]
pub struct EmbedAuthor<'a> {
    pub name: Option<&'a str>,
    pub url: Option<&'a str>,
    pub icon_url: Option<&'a str>,
}

#[derive(Serialize, Default, Debug)]
pub struct EmbedField<'a> {
    pub name: Option<&'a str>,
    pub value: Option<&'a str>,
    pub inline: Option<bool>,
}

#[derive(Serialize, Default, Debug)]
pub struct EmbedThumbnail<'a> {
    pub url: Option<&'a str>,
}

#[derive(Serialize, Default, Debug)]
pub struct EmbedImage<'a> {
    pub url: Option<&'a str>,
}

#[derive(Serialize, Default, Debug)]
pub struct EmbedFooter<'a> {
    pub text: Option<&'a str>,
    pub icon_url: Option<&'a str>,
}

struct ScrapedData {
    author_name: String,
    author_url: Url,
    last_post_url: Url,
    last_post_id: String,
    thread_name: String,
    thread_url: Url,
}

struct Config {
    search_url: Url,
    persist_quantity: usize,
    monitor_interval: Duration,
    err_retry_interval: Duration,
    webhook_username: String,
    webhook_avatar: String,
    webhook_channels: Vec<String>,
}

struct Selectors {
    list_inner: Selector,
    responsive_show: Selector,
    anchor: Selector,
    topic_title: Selector,
}

impl Config {
    fn from_env() -> Result<Self> {
        Ok(Self {
            search_url: search_url()?,
            persist_quantity: env_parse("PERSIST_QUANTITY", 200),
            monitor_interval: env_duration("MONITOR_INTERVAL", Duration::from_secs(60)),
            err_retry_interval: env_duration("ERR_RETRY_INTERVAL", Duration::from_secs(3600)),
            webhook_username: env_string("WEBHOOK_USERNAME", "sentinel"),
            webhook_avatar: env_string("WEBHOOK_AVATAR", "https://i.imgur.com/FHLEi2t.png"),
            webhook_channels: webhook_channels()?,
        })
    }
}

impl Selectors {
    fn new() -> Result<Self> {
        Ok(Self {
            list_inner: parse_selector("div.list-inner")?,
            responsive_show: parse_selector("div.responsive-show")?,
            anchor: parse_selector("a")?,
            topic_title: parse_selector("a.topictitle")?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("running sentinel...");

    let config = Config::from_env()?;
    let client = build_client()?;
    let selectors = Selectors::new()?;

    loop {
        let delay = match run_monitor(&client, &selectors, &config).await {
            Ok(()) => config.monitor_interval,
            Err(e) => {
                eprintln!("global exception caught: {e:#}");
                config.err_retry_interval
            }
        };

        sleep(delay).await;
    }
}

async fn run_monitor(client: &Client, selectors: &Selectors, config: &Config) -> Result<()> {
    let search_url = &config.search_url;
    let page = fetch_page(client, search_url).await?;
    let topics = scrape_topics(&page, selectors, search_url);

    if topics.is_empty() {
        eprintln!(
            "warning: no topics found at {search_url}, access may be denied or the page layout may have changed"
        );
        return Ok(());
    }

    notify_new_topics(client, config, topics).await
}

async fn fetch_page(client: &Client, url: &Url) -> Result<String> {
    Ok(client.get(url.as_str()).send().await?.text().await?)
}

fn scrape_topics(page: &str, selectors: &Selectors, base: &Url) -> Vec<ScrapedData> {
    let document = Html::parse_document(page);

    document
        .select(&selectors.list_inner)
        .filter_map(|entry| scrape_topic(entry, selectors, base))
        .collect()
}

fn scrape_topic(entry: ElementRef, selectors: &Selectors, base: &Url) -> Option<ScrapedData> {
    let details = entry.select(&selectors.responsive_show).next()?;
    let mut anchors = details.select(&selectors.anchor);
    let (Some(author), Some(last_post), Some(_forum)) =
        (anchors.next(), anchors.next(), anchors.next())
    else {
        return None;
    };
    let thread = entry.select(&selectors.topic_title).next()?;

    let mut last_post_url = resolve_href(base, last_post)?;
    let last_post_id = post_id(&last_post_url)?;
    last_post_url.set_fragment(Some(&last_post_id));

    Some(ScrapedData {
        author_name: author.text().collect(),
        author_url: resolve_href(base, author)?,
        last_post_url,
        last_post_id,
        thread_name: thread.text().collect(),
        thread_url: resolve_href(base, thread)?,
    })
}

fn resolve_href(base: &Url, element: ElementRef) -> Option<Url> {
    let href = element.value().attr("href")?;
    let mut url = base.join(href).ok()?;
    strip_sid(&mut url);
    Some(url)
}

fn strip_sid(url: &mut Url) {
    let Some(query) = url.query() else {
        return;
    };

    let query = query
        .split('&')
        .filter(|param| !param.starts_with("sid="))
        .collect::<Vec<_>>()
        .join("&");

    url.set_query((!query.is_empty()).then_some(query.as_str()));
}

fn post_id(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == "p")
        .map(|(_, value)| format!("p{value}"))
}

async fn notify_new_topics(
    client: &Client,
    config: &Config,
    topics: Vec<ScrapedData>,
) -> Result<()> {
    let mut persisted = load_persisted().await?;

    for topic in topics {
        if persisted.contains(&topic.last_post_id) {
            continue;
        }

        discord_webhook(client, config, &topic).await?;

        let last_post_id = topic.last_post_id;
        println!("new post collected and webhook sent: {last_post_id}");

        remember_post(&mut persisted, last_post_id, config.persist_quantity);
        save_persisted(&persisted).await?;
    }

    Ok(())
}

fn remember_post(persisted: &mut Vec<String>, post_id: String, quantity: usize) {
    persisted.push(post_id);

    let excess = persisted.len().saturating_sub(quantity);
    persisted.drain(..excess);
}

async fn load_persisted() -> Result<Vec<String>> {
    if !fs::try_exists(PERSISTENCE_PATH).await? {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(PERSISTENCE_PATH)
        .await
        .with_context(|| format!("failed to read {PERSISTENCE_PATH}"))?;

    serde_json::from_str(&content).with_context(|| format!("failed to parse {PERSISTENCE_PATH}"))
}

async fn save_persisted(persisted: &[String]) -> Result<()> {
    let json = serde_json::to_string_pretty(persisted)?;

    fs::write(PERSISTENCE_PATH, json)
        .await
        .with_context(|| format!("failed to write {PERSISTENCE_PATH}"))
}

async fn discord_webhook(client: &Client, config: &Config, topic: &ScrapedData) -> Result<()> {
    let description = describe(topic);
    let payload = build_payload(config, &description);

    for hook in &config.webhook_channels {
        client.post(hook).json(&payload).send().await?;
    }

    Ok(())
}

fn describe(topic: &ScrapedData) -> String {
    let ScrapedData {
        author_name,
        author_url,
        last_post_url,
        thread_name,
        thread_url,
        ..
    } = topic;

    format!(
        "new [**post**]({last_post_url}) by [{author_name}]({author_url}) in [{thread_name}]({thread_url})"
    )
}

fn build_payload<'a>(config: &'a Config, description: &'a str) -> WebhookPayload<'a> {
    WebhookPayload {
        username: Some(&config.webhook_username),
        avatar_url: Some(&config.webhook_avatar),
        embeds: vec![Embed {
            description: Some(description),
            color: Some(0),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn build_client() -> Result<Client> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .cookie_store(true)
        .build()
        .context("failed to build http client")
}

fn parse_selector(selector: &str) -> Result<Selector> {
    Selector::parse(selector).map_err(|e| anyhow!("failed to parse selector {selector}: {e}"))
}

fn search_url() -> Result<Url> {
    let domain = env_string("DOMAIN", "https://forums.bzflag.org");
    let base = domain.trim_end_matches('/');

    Url::parse(&format!("{base}/search.php?search_id=active_topics"))
        .with_context(|| format!("invalid DOMAIN: {domain}"))
}

fn webhook_channels() -> Result<Vec<String>> {
    let channels = env_var("WEBHOOK_CHANNELS").context("missing WEBHOOK_CHANNELS")?;

    serde_json::from_str(&channels).context("WEBHOOK_CHANNELS must be a JSON array of strings")
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}

fn env_string(key: &str, default: &str) -> String {
    env_var(key).unwrap_or_else(|| default.to_owned())
}

fn env_parse<T: FromStr>(key: &str, default: T) -> T {
    env_var(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_duration(key: &str, default: Duration) -> Duration {
    env_var(key)
        .and_then(|value| value.parse().ok())
        .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
        .unwrap_or(default)
}
