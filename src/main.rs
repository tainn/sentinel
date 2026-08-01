use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct WebhookPayload {
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub content: Option<String>,
    pub tts: Option<bool>,
    pub embeds: Vec<Embed>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Embed {
    pub author: Option<EmbedAuthor>,
    pub color: Option<u32>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub fields: Vec<EmbedField>,
    pub thumbnail: Option<EmbedThumbnail>,
    pub image: Option<EmbedImage>,
    pub footer: Option<EmbedFooter>,
    pub timestamp: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct EmbedAuthor {
    pub name: Option<String>,
    pub url: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct EmbedField {
    pub name: Option<String>,
    pub value: Option<String>,
    pub inline: Option<bool>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct EmbedThumbnail {
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct EmbedImage {
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct EmbedFooter {
    pub text: Option<String>,
    pub icon_url: Option<String>,
}

struct ScrapedData {
    author_name: String,
    author_url: String,
    last_post_url: String,
    last_post_id: String,
    thread_name: String,
    thread_url: String,
}

#[tokio::main]
async fn main() {
    println!("running sentinel...");

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    loop {
        if let Err(e) = run_monitor(&client).await {
            eprintln!("global exception caught: {e}");
            let retry_interval = env::var("ERR_RETRY_INTERVAL")
                .unwrap_or_else(|_| String::from("3600"))
                .parse::<f64>()
                .unwrap_or(3600.0);
            sleep(Duration::from_secs_f64(retry_interval)).await;
        } else {
            let monitor_interval = env::var("MONITOR_INTERVAL")
                .unwrap_or_else(|_| String::from("60"))
                .parse::<f64>()
                .unwrap_or(60.0);
            sleep(Duration::from_secs_f64(monitor_interval)).await;
        }
    }
}

async fn run_monitor(client: &Client) -> Result<()> {
    let domain = env::var("DOMAIN").unwrap_or_else(|_| String::from("https://forums.bzflag.org"));
    let url = format!("{domain}/search.php?search_id=active_topics");

    let res = client.get(&url).send().await?.text().await?;
    let document = Html::parse_document(&res);

    let list_inner_sel = Selector::parse("div.list-inner")
        .map_err(|_| anyhow!("failed to parse list-inner selector"))?;
    let responsive_show_sel = Selector::parse("div.responsive-show")
        .map_err(|_| anyhow!("failed to parse responsive-show selector"))?;
    let a_sel = Selector::parse("a").map_err(|_| anyhow!("failed to parse anchor selector"))?;
    let topictitle_sel = Selector::parse("a.topictitle")
        .map_err(|_| anyhow!("failed to parse topictitle selector"))?;

    let persistence_path = Path::new("/data/persistence.json");

    if !persistence_path.exists() {
        fs::write(persistence_path, "[]").await?;
    }

    let persist_content = fs::read_to_string(persistence_path).await?;
    let mut persist: Vec<String> = serde_json::from_str(&persist_content)?;

    for entry in document.select(&list_inner_sel) {
        let auth_lp_forum = match entry.select(&responsive_show_sel).next() {
            Some(node) => node,
            None => continue,
        };

        let auth_lp_forum_data: Vec<_> = auth_lp_forum.select(&a_sel).collect();
        if auth_lp_forum_data.len() < 3 {
            continue;
        }

        let thread = match entry.select(&topictitle_sel).next() {
            Some(node) => node,
            None => continue,
        };

        let author_name = auth_lp_forum_data[0].text().collect::<String>();
        let author_href = auth_lp_forum_data[0].value().attr("href").unwrap_or("");

        let last_post_href = auth_lp_forum_data[1].value().attr("href").unwrap_or("");

        let thread_name = thread.text().collect::<String>();
        let thread_href = thread.value().attr("href").unwrap_or("");

        let author_url = parse_url(&domain, author_href, false);
        let last_post_url = parse_url(&domain, last_post_href, true);
        let last_post_id = last_post_url
            .split('#')
            .next_back()
            .unwrap_or("")
            .to_string();
        let thread_url = parse_url(&domain, thread_href, false);

        let data = ScrapedData {
            author_name,
            author_url,
            last_post_url,
            last_post_id: last_post_id.clone(),
            thread_name,
            thread_url,
        };

        if persist.contains(&data.last_post_id) {
            continue;
        }

        discord_webhook(client, &data).await?;

        println!("new post collected and webhook sent: {}", data.last_post_id);
        persist.push(data.last_post_id.clone());

        let persist_quantity = env::var("PERSIST_QUANTITY")
            .unwrap_or_else(|_| String::from("200"))
            .parse::<usize>()
            .unwrap_or(200);

        if persist.len() > persist_quantity {
            let excess = persist.len() - persist_quantity;
            persist.drain(0..excess);
        }

        let updated_json = serde_json::to_string_pretty(&persist)?;
        fs::write(persistence_path, updated_json).await?;
    }

    Ok(())
}

fn parse_url(domain: &str, raw_url: &str, post: bool) -> String {
    let mut clean_url = raw_url
        .split("sid=")
        .next()
        .unwrap_or("")
        .replace("amp;", "");

    if clean_url.ends_with('&') {
        clean_url.pop();
    }

    if post {
        let post_id = clean_url.split('=').next_back().unwrap_or("");
        clean_url = format!("{clean_url}#p{post_id}");
    }

    let sliced = if clean_url.chars().count() >= 2 {
        let idx = clean_url
            .char_indices()
            .nth(2)
            .map(|(i, _)| i)
            .unwrap_or(clean_url.len());
        &clean_url[idx..]
    } else {
        ""
    };

    format!("{domain}/{sliced}")
}

async fn discord_webhook(client: &Client, ec: &ScrapedData) -> Result<()> {
    let author_name = &ec.author_name;
    let author_url = &ec.author_url;
    let thread_name = &ec.thread_name;
    let thread_url = &ec.thread_url;
    let last_post_url = &ec.last_post_url;

    let description = format!(
        "new [**post**]({last_post_url}) by [{author_name}]({author_url}) in [{thread_name}]({thread_url})"
    );

    let mut payload = WebhookPayload {
        username: Some(env::var("WEBHOOK_USERNAME").unwrap_or_else(|_| String::from("sentinel"))),
        avatar_url: Some(
            env::var("WEBHOOK_AVATAR")
                .unwrap_or_else(|_| String::from("https://i.imgur.com/FHLEi2t.png")),
        ),
        ..Default::default()
    };

    let embed = Embed {
        description: Some(description),
        color: Some(0),
        ..Default::default()
    };

    payload.embeds.push(embed);

    let webhook_channels_env = env::var("WEBHOOK_CHANNELS").context("missing WEBHOOK_CHANNELS")?;
    let hooks: Vec<String> = serde_json::from_str(&webhook_channels_env)?;

    for hook in hooks {
        client.post(&hook).json(&payload).send().await?;
    }

    Ok(())
}
