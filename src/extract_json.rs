use futures::future::join_all;
use regex::Regex;
use tokio::task;
use openai::{
    chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole}, set_key,
};
use std::{error::Error, fs, str};

use crate::rate_limiter;




#[derive(Debug, serde::Deserialize, Clone,PartialEq)]
pub struct Book {
    pub author: String,
    pub title: String
}




pub async fn extract_json(file_name: &str, oai_key: &str, rate_limiter:&rate_limiter::RateLimiterWrapper) -> Result<Vec<Book>, Box<dyn Error>> {
    // Prepare the prompt template
    let prompt = r#"I will give you a paragraph of text. Read it and find the mentioned books and their authors.
    Please return a JSON response in the following format:
    [{"author": "string","title": "string"}]
    Make sure the response is only valid JSON with no additional formatting like code blocks, special chars like new lines.
    If nothing is found, then give me an empty array like []. Keep the original language for the book titles and authors."#;
    
    set_key(oai_key.to_owned());
    
    let mut content = fs::read_to_string(format!("./tmp/{}", file_name))?;
    fs::remove_file(format!("./tmp/{}", file_name))?;
    
    // Regex replacements to clean up the content
    let re_newline = Regex::new(r"\r\n")?;
    let re_timestamps = Regex::new(r"^[\d:,\s\->]+$")?;
    let re_empty_lines = Regex::new(r"^\s*$[\r\n]")?;
    
    content = re_newline.replace_all(&content, "\n").into_owned();
    content = re_timestamps.replace_all(&content, "").into_owned();
    content = re_empty_lines.replace_all(&content, "").into_owned();
    content = re_newline.replace_all(&content, "\n").into_owned();

    let lines: Vec<&str> = content.split('\n').collect();
    let mut new_lines: Vec<String> = Vec::new();
    let mut i: isize = -1;
    
    for mut line in lines {
        line = line.trim();
        if line.is_empty() {
            continue;
        }
        if i == -1 {
            new_lines.push(line.to_string());
            i += 1;
            continue;
        }
        if new_lines[i as usize] == line {
            continue;
        }
        new_lines.push(line.to_string());
        i += 1;
    }

    let mut bucket = new_lines.join("\n");
    let mut remaining = bucket.chars().count();
    let win_size = 16000;

    let mut i_chunk = 0;
    let mut tasks = vec![];  // Store all async tasks for parallel execution
    log::info!("Total chars.count: {}", remaining);
    while remaining > 0 {
  
        let mut chunk = bucket.chars().take(win_size.min(remaining)).collect::<String>();
        bucket = bucket.chars().skip(chunk.chars().count()).collect::<String>();
        remaining = bucket.chars().count();
        log::info!("After bucketskip Remaining chars.count: {}", remaining);
        

        
        if remaining > 0 {
        // Ensure the chunk ends with space or newline
            if  ![32, 10].contains(chunk.as_bytes().last().unwrap()) && ![32,10].contains(bucket.char_indices().nth(0).map(|(_, ch)| ch.to_string()).unwrap().as_bytes().first().unwrap()) {
                if let Some(space_index) = chunk.rfind(' ') {
                    if space_index + 1 < chunk.len() {
                        // Safely update bucket and chunk
                        bucket = format!("{}{}", &chunk[space_index..], bucket);
                        chunk = chunk[..space_index].to_string();
                        
                    }
                }
            }
        }
        remaining = bucket.chars().count();
        log::info!("Space-adjusted bucket remaining chars.count: {}", remaining);
        log::info!("Space-adjusted Chunk {}: chars.count {}\n\n", i_chunk, chunk.chars().count());

        
        // Define the messages variable
        let messages = [
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(prompt.to_string()),
                name: None,
                function_call: None,
            },
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::User,
                content: Some(chunk),
                name: None,
                function_call: None,
            },
        ];
        
        let rate_limiter_clone = rate_limiter.clone(); // Clone the Arc
        let task  = task::spawn(async move {
            rate_limiter_clone.is_allowed().await;
            match ChatCompletion::builder("gpt-4o-mini", messages.clone())
                .temperature(0.7)
                .create()
                .await
            {
                Ok(chat_completion) => {
                    
                    
                    Ok::<_, Box<dyn Error + Send>>(chat_completion)
                },
                Err(e) => Err(Box::new(e) as Box<dyn Error + Send>)
            }
        });

        tasks.push(task);  // Collect the task
        i_chunk += 1;
    }

    let responses = join_all(tasks).await;
    let mut books: Vec<Vec<Book>> = Vec::new();
    for res in responses {
        for choice in res?.unwrap().choices {
            if choice.message.role == ChatCompletionMessageRole::Assistant && choice.message.content.is_some() {
                let content = choice.message.content.clone().unwrap();
                log::info!("Raw response: {}", content);
    
                let trimmed_content = content.trim();
                match serde_json::from_str::<Vec<Book>>(trimmed_content) {
                    Ok(parsed_books) => {
                        if !parsed_books.is_empty() {
                            books.push(parsed_books);
                        }
                    }
                    Err(err) => {
                        log::error!("Failed to parse JSON: {}", err);
                    }
                }
            }
        }
    }
    
    
    let mut res: Vec<Book> = books
    .into_iter()
    .filter(|x| !x.is_empty())
    .flatten()
    .collect::<Vec<Book>>();

    res.sort_by(|a, b| a.title.cmp(&b.title));
    log::info!("Books found: {:#?}", res);

    let res_clone = res.clone();
    let res_clone2 = res.clone();

    // Deduplicate the books
    res = res_clone2
        .into_iter()
        .fold(Vec::new(), |mut acc, x| {
            if !acc.contains(&x) {
                acc.push(x);
            }
            acc
        })
        .into_iter()
        .map(|book| {
            let combined_authors = res_clone
                .iter()
                .filter(|t| t.title == book.title)
                .map(|m| m.author.clone())
                .collect::<Vec<String>>()
                .join(", ");
            Book {
                author: combined_authors,
                title: book.title.clone(),
            }
        })
        .collect::<Vec<Book>>()
        .into_iter()
        .filter(|x| x.author != x.title) // Ensure final filtering happens here
        .collect::<Vec<Book>>();

    log::info!("Final books: {:#?}", res);

    //todo revalidate the books with chatgpt
    Ok(res)
}