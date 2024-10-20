
use teloxide::{prelude::*, types::Update};
use teloxide::types::{Message, UpdateKind};
use teloxide::Bot;
use regex::Regex;
use uuid::Uuid;
use core::str;
use std::process::Command;
use std::{env, process};
use std::error::Error;
use warp::Filter;
use url::Url;

use bytes::Bytes;

mod extract_json; 
mod rate_limiter;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "3030".to_string()).parse().expect("Invalid PORT number");
    
    match port {
        0 => {
            log::error!("Try something different than 0 for PORT");
            process::exit(1);
        }
        _ => {}
    }
    
    match env::var("TELOXIDE_TOKEN") {
        Ok(token) if token.is_empty() => {
            log::error!("TELOXIDE_TOKEN env is set but empty");
            process::exit(1);
        }
        Err(env::VarError::NotPresent) => {
            log::error!("TELOXIDE_TOKEN env is not set");
            process::exit(1);
        }
        Ok(_) => {}
        Err(err) => {
            log::error!("Failed to read TELOXIDE_TOKEN env: {}", err);
            process::exit(1);
        }
    }
    
    match env::var("OPENAI_TOKEN") {
        Ok(token) if token.is_empty() => {
            log::error!("OPENAI_TOKEN env is set but empty");
            process::exit(1);
        }
        Err(env::VarError::NotPresent) => {
            log::error!("OPENAI_TOKEN env is not set");
            process::exit(1);
        }
        Ok(_) => {}
        Err(err) => {
            log::error!("Failed to read OPENAI_TOKEN env: {}", err);
            process::exit(1);
        }
    }
    
    log::info!("Starting bot...");
    let bot = Bot::from_env();

    let environment = env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    
    

    if environment == "production" {
        run_webhook(bot, port).await;
    } else {
        run_polling(bot).await;
    }
}

async fn run_webhook(bot: Bot, port: u16) {
    log::info!("Running in webhook mode...");
    let rate_limiter = rate_limiter::RateLimiterWrapper::new(100, 1000, 10000); // 100 RPM, 1000 RPD, 10000 TPM
    let webhook_url: Url = env::var("WEBHOOK_URL")
        .expect("WEBHOOK_URL must be set")
        .parse()
        .expect("Invalid WEBHOOK_URL");
    bot.set_webhook(webhook_url.clone())
        .send()
        .await
        .expect("Failed to set webhook");

    log::info!("Starting server on port {}", port);
    
    let webhook_filter = warp::post()
        .and(warp::path::end())
        .and(warp::body::bytes())
        .map(move |body: Bytes| {
            let bot_clone = bot.clone();
            let update = serde_json::from_slice::<Update>(&body).expect("Failed to parse update");
            let rl_wrap_clone = rate_limiter.clone(); 
            tokio::spawn(async move {
                if let UpdateKind::Message(msg) = update.kind {
                    process_message(&bot_clone, msg, &rl_wrap_clone).await.map_err(|e| {
                        log::error!("Failed to process message: {:?}", e);
                    }).ok(); 
                }
                
            });

            warp::reply::with_status("Webhook received", warp::http::StatusCode::OK)
        });

    warp::serve(webhook_filter).run(([0, 0, 0, 0], port)).await;
    
}

async fn run_polling(bot: Bot) {
    log::info!("Running in polling mode...");
    let rate_limiter = rate_limiter::RateLimiterWrapper::new(100, 1000, 200000); 
    let rl_wrap_clone = rate_limiter.clone();
    teloxide::repl(bot, move |bot_clone: Bot, msg: Message| {
        let rl_wrap_clone = rl_wrap_clone.clone();
        async move {
            process_message(&bot_clone, msg, &rl_wrap_clone).await.map_err(|e| {
                log::error!("Failed to process message: {:?}", e);
            }).ok(); 
            Ok(())
        }
    })
    .await;
}

async fn extract_lang(url: &str) -> Result<String, Box<dyn Error>> {
    let output = Command::new("/usr/local/bin/yt-dlp")
        .arg("--abort-on-error")
        .arg("--print")
        .arg("video:language")
        .arg(url)
        .output()?;
    if !output.status.success() {
        return Err(format!("Failed to extract language. Exit status: {}", output.status).into());
    }
    Ok(str::from_utf8(&output.stdout)?.trim().to_string())
}

async fn download_video(url: &str, lang: &str) -> Result<String, Box<dyn Error>> {
    let file_name = Uuid::new_v4();
    let output = Command::new("/usr/local/bin/yt-dlp")
        .arg("--write-auto-subs")
        .arg("--sub-lang")
        .arg(lang)
        .arg("--skip-download")
        .arg("--no-live-from-start")
        .arg("--convert-subs")
        .arg("srt")
        .arg("-o")
        .arg(format!("./tmp/{}", file_name))
        .arg(url)
        .output()?;
    if !output.status.success() {
        return Err(format!("Failed to download video. Exit status: {}", output.status).into());
    }
    Ok(format!("{}.{}.srt", file_name, lang))
}

async fn process_message(bot: &Bot, msg: Message, rl_wrap: &rate_limiter::RateLimiterWrapper) -> Result<(), Box<dyn Error>>  {
    
        let txt = msg.text().ok_or("No text in message")?;
        let user = msg.from.as_ref().ok_or("No user information in message")?;
        let username = user.username.as_deref().ok_or("No username in user information")?;
        
        log::info!("From sender {} Received message: {}", username ,txt);
        if txt == "/start" {
            bot.send_message(msg.chat.id, "Send me a YouTube link and get a list of the books mentioned in the video. English and Russian are supported. Other languages have yet to be appropriately tested but are available, I guess.")
                .await
                .unwrap();
            return Ok(());
        }
        if txt == "/help" {
            bot.send_message(msg.chat.id, "Send me a YouTube link and get a list of the books mentioned in the video. English and Russian are supported. Other languages have yet to be appropriately tested but are available, I guess.")
                .await
                .unwrap();
            return Ok(());
        }
        let re = Regex::new(r"(?im)^(?:https?:\/\/)?(?:www\.)?(?:youtube\.com\/(?:watch\?v=|embed\/|v\/|shorts\/)|youtu\.be\/)([\w\-]{11})(?:\S*)?")
            .expect("Invalid regular expression");

        if let Some(url) = re.captures(txt) {
            log::info!("Whole match: {}", &url[0]);
            bot.send_message(msg.chat.id, "Valid YouTube link received. Hold the line")
                .await
                .unwrap();
            let lang = extract_lang(&url[0]).await?;
            let file_name = download_video(&url[0], &lang).await?;
            bot.send_message(msg.chat.id, format!("Language: {}", lang))
                .await
                .unwrap();
            let books = match extract_json::extract_json(&file_name, &env::var("OPENAI_TOKEN").unwrap(), rl_wrap).await {
                Ok(books) => books,
                Err(err) => {
                    log::error!("Error extracting JSON: {:?}", err);
                    return Ok(());
                }
            };
            
            if books.len() == 0 {
                bot.send_message(msg.chat.id, "No books or authors found in the video.")
                    .await
                    .unwrap();
            } else {
                let mut message = String::new();
                for r in books {
                    message += &format!("{} \"{}\"\n", r.author, r.title);
                }
                bot.send_message(msg.chat.id, message)
                    .await
                    .unwrap();
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                bot.send_message(msg.chat.id, "That's all I could find. Hope it helps!")
                    .await
                    .unwrap();
            }
            
            
        } else {
            bot.send_message(msg.chat.id, "Does not look like a YouTube link. Try one more time.")
                .await
                .unwrap();
        }
        
        Ok(())
    
}
